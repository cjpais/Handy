# /// script
# requires-python = ">=3.10"
# dependencies = ["boto3", "huggingface_hub[hf_xet]"]
# ///
"""
Mirror the catalog's GGUF files to S3-compatible blob storage (Cloudflare R2).

Object keys are `{repo_id}/{revision}/{filename}` — the same three values that
form the HF resolve URL, so the app's mirror template is plain substitution.
A revision pins content, making keys immutable: objects are served with an
immutable cache policy, and existence implies correctness.

The bucket is the only state. Each run HEADs every expected key and uploads
what's missing, so runs are idempotent and machine-independent; concurrent
runs from different machines at worst duplicate work with identical bytes.
Every file is hash-verified against the catalog's `sha256` between download
and upload — that verification is what lets everything downstream trust bare
key existence. When a repo's revision moves but a file's bytes didn't change,
the object is server-side copied from the old revision's key instead of
re-transferred.

Modes:
  (default)   dry run — print the plan; read-only HEAD/LIST when credentials
              are present, offline otherwise. Writes nothing.
  --execute   perform the plan. Downloads (hf_xet, parallel chunks) and
              uploads (one worker thread) are pipelined, so wall time is
              roughly max(total download, total upload). Ctrl-C is safe at any
              point: a key only appears once its multipart upload completes
              (an interrupted upload is invisible and auto-aborted by R2's
              lifecycle rule), the in-flight upload finishes before exit, and
              a rerun skips everything already mirrored.
  --verify    audit the mirror: stream every expected object back from the
              bucket and hash it against the catalog. No disk writes; R2
              egress is free, so a full audit costs only time.

Only each model's default quant is mirrored — the one quant the app offers
for download; --all-quants widens to every listed quant.

Env:  R2_ENDPOINT (https://<account>.r2.cloudflarestorage.com)
      R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY, R2_BUCKET
Run:  uv run scripts/mirror_models.py [--execute | --verify] [--only SUBSTR]
          [--all-quants] [--tmpdir DIR] [catalog_path]
"""
import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
import time
from collections import deque
from concurrent.futures import ThreadPoolExecutor

import boto3
from boto3.s3.transfer import TransferConfig
from huggingface_hub import hf_hub_download
from tqdm import tqdm

CACHE_CONTROL = "public, max-age=31536000, immutable"
DOWNLOAD_ATTEMPTS = 3
CHUNK = 8 * 1024 * 1024
TRANSFER = TransferConfig(max_concurrency=16, multipart_chunksize=32 * 1024 * 1024)


def say(msg):
    """All script output funnels through tqdm.write so lines land cleanly
    above the active download bar instead of tearing through it."""
    tqdm.write(msg)


def make_s3():
    """R2 client and bucket from env, or (None, None) → offline dry-run."""
    endpoint, key, secret, bucket = (os.environ.get(v) for v in
        ("R2_ENDPOINT", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY", "R2_BUCKET"))
    if not all((endpoint, key, secret, bucket)):
        return None, None
    return boto3.client("s3", endpoint_url=endpoint,
                        aws_access_key_id=key, aws_secret_access_key=secret), bucket


def head_meta(s3, bucket, key):
    """Object's user metadata, or None if the key doesn't exist."""
    try:
        return s3.head_object(Bucket=bucket, Key=key).get("Metadata", {})
    except s3.exceptions.ClientError as e:
        if e.response["Error"]["Code"] in ("404", "NoSuchKey", "NotFound"):
            return None
        raise


def find_copy_source(s3, bucket, repo_id, filename, sha):
    """Key of the same file under an older revision with matching sha256, if any."""
    paginator = s3.get_paginator("list_objects_v2")
    for page in paginator.paginate(Bucket=bucket, Prefix=repo_id + "/"):
        for obj in page.get("Contents", []):
            key = obj["Key"]
            if key.rsplit("/", 1)[-1] != filename:
                continue
            meta = head_meta(s3, bucket, key)
            if meta is not None and meta.get("sha256") == sha:
                return key
    return None


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(CHUNK), b""):
            h.update(chunk)
    return h.hexdigest()


def download_verified(repo_id, revision, f, tmpdir):
    """Fetch at the pinned revision (hf_xet parallel transfer, own progress
    bar), then hash-verify against the catalog.

    Returns (path, scratch_dir); the caller removes scratch_dir after upload.
    Retries transfer errors AND verification failures — a hash mismatch here
    means a corrupted transfer, and uploading it would poison an immutable key.
    """
    last_err = None
    for attempt in range(1, DOWNLOAD_ATTEMPTS + 1):
        scratch = tempfile.mkdtemp(dir=tmpdir, prefix="mirror-")
        try:
            path = hf_hub_download(repo_id, f["filename"], revision=revision,
                                   cache_dir=scratch)
            if (sha256_file(path) == f["sha256"]
                    and os.path.getsize(path) == f["size_bytes"]):
                return path, scratch
            last_err = "verification failed"
        except Exception as e:
            last_err = str(e)
        except BaseException:  # Ctrl-C mid-download: don't leak the scratch dir
            shutil.rmtree(scratch, ignore_errors=True)
            raise
        shutil.rmtree(scratch, ignore_errors=True)
        say(f"  attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed: {last_err}")
    raise RuntimeError(f"download failed for {repo_id}/{f['filename']}@{revision}: {last_err}")


def upload_one(s3, bucket, key, path, scratch, sha, size):
    """Runs on the uploader thread: push to R2, report once, drop the scratch."""
    extra = {"Metadata": {"sha256": sha},
             "CacheControl": CACHE_CONTROL,
             "ContentType": "application/octet-stream"}
    try:
        t0 = time.monotonic()
        s3.upload_file(path, bucket, key, ExtraArgs=extra, Config=TRANSFER)
        rate = size / max(time.monotonic() - t0, 1e-9) / 1e6
        say(f"↑ done  {key}  ({rate:.0f} MB/s)")
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def verify_bucket(s3, bucket, jobs):
    """Stream every expected object and hash it against the catalog."""
    bad = 0
    for key, f in jobs:
        try:
            body = s3.get_object(Bucket=bucket, Key=key)["Body"]
        except s3.exceptions.ClientError as e:
            if e.response["Error"]["Code"] in ("404", "NoSuchKey", "NotFound"):
                say(f"MISSING  {key}")
                bad += 1
                continue
            raise
        h, t0 = hashlib.sha256(), time.monotonic()
        for chunk in iter(lambda: body.read(CHUNK), b""):
            h.update(chunk)
        rate = f["size_bytes"] / max(time.monotonic() - t0, 1e-9) / 1e6
        if h.hexdigest() == f["sha256"]:
            say(f"ok       {key}  ({rate:.0f} MB/s)")
        else:
            say(f"BAD      {key}  object does not match catalog sha256")
            bad += 1
    return bad


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("catalog", nargs="?", default=os.path.join(
        os.path.dirname(__file__), "..", "src-tauri", "src", "catalog", "catalog.json"))
    ap.add_argument("--execute", action="store_true", help="perform the plan (default: dry run)")
    ap.add_argument("--verify", action="store_true", help="stream-hash every mirrored object against the catalog")
    ap.add_argument("--only", help="restrict to models whose id contains this substring")
    ap.add_argument("--all-quants", action="store_true",
                    help="mirror every quant instead of just each model's default")
    ap.add_argument("--tmpdir", default=None,
                    help="scratch dir for downloads (needs room for two files)")
    args = ap.parse_args()

    catalog = json.load(open(args.catalog))
    jobs = []  # (model, file entry, object key) for every file this run covers
    for m in catalog["models"]:
        if args.only and args.only not in m["id"]:
            continue
        for f in m["files"]:
            if args.all_quants or f["quant"] == m["default_quant"]:
                jobs.append((m, f, f'{m["id"]}/{m["revision"]}/{f["filename"]}'))

    s3, bucket = make_s3()
    if args.verify:
        if s3 is None:
            sys.exit("--verify requires R2_ENDPOINT, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY, R2_BUCKET")
        bad = verify_bucket(s3, bucket, [(key, f) for _, f, key in jobs])
        print(f"verified {len(jobs)} object(s), {bad} problem(s)")
        sys.exit(1 if bad else 0)
    if args.execute and s3 is None:
        sys.exit("--execute requires R2_ENDPOINT, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY, R2_BUCKET")
    if s3 is None:
        say("no R2 credentials: offline plan (every file counts as 'upload')")

    uploader = ThreadPoolExecutor(max_workers=1)
    in_flight = deque()

    def drain(limit):
        while len(in_flight) > limit:
            in_flight.popleft().result()  # re-raises upload errors here

    tally = {"skip": 0, "copy": 0, "upload": 0, "mismatch": 0}
    upload_bytes = 0
    for m, f, key in jobs:
        gb = f["size_bytes"] / 1e9
        copy_src = None
        if s3 is not None:
            meta = head_meta(s3, bucket, key)
            if meta is not None:
                # Immutable key already populated. "Existence implies
                # correctness" only covers objects this script wrote (verified,
                # hash recorded) — a disagreeing OR missing hash means some
                # other writer, the one state that must never pass silently.
                recorded = meta.get("sha256")
                if recorded != f["sha256"]:
                    got = f"object sha {recorded[:12]}…" if recorded else "no sha256 metadata"
                    say(f"!! MISMATCH {key}: {got} != catalog {f['sha256'][:12]}…")
                    tally["mismatch"] += 1
                else:
                    tally["skip"] += 1
                continue
            copy_src = find_copy_source(s3, bucket, m["id"], f["filename"], f["sha256"])

        if not args.execute:
            action = "copy" if copy_src else "upload"
            say(f"DRY {action:6} {key}  ({gb:.2f} GB" + (f", from {copy_src}" if copy_src else "") + ")")
            tally[action] += 1
            upload_bytes += 0 if copy_src else f["size_bytes"]
            continue

        if copy_src:
            say(f"copy    {key}  (from {copy_src})")
            extra = {"Metadata": {"sha256": f["sha256"]},
                     "CacheControl": CACHE_CONTROL,
                     "ContentType": "application/octet-stream"}
            # Managed copy: server-side, multipart above the 5 GB single-copy limit.
            s3.copy({"Bucket": bucket, "Key": copy_src}, bucket, key,
                    ExtraArgs={**extra, "MetadataDirective": "REPLACE"})
            tally["copy"] += 1
            continue

        # Pipeline: cap queued uploads at one so at most two scratch dirs
        # exist, then download the next file while the previous one uploads.
        drain(1)
        say(f"↓ {key}  ({gb:.2f} GB)")
        path, scratch = download_verified(m["id"], m["revision"], f, args.tmpdir)
        in_flight.append(uploader.submit(upload_one, s3, bucket, key, path, scratch,
                                         f["sha256"], f["size_bytes"]))
        tally["upload"] += 1
        upload_bytes += f["size_bytes"]

    drain(0)
    uploader.shutdown()
    print(f'{"DRY RUN — nothing written. " if not args.execute else ""}'
          f'skip {tally["skip"]}, copy {tally["copy"]}, upload {tally["upload"]}'
          f' ({upload_bytes/1e9:.1f} GB), mismatch {tally["mismatch"]}')
    if tally["mismatch"]:
        sys.exit(1)


if __name__ == "__main__":
    main()
