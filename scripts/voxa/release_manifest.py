#!/usr/bin/env python3
"""Release helpers for .github/workflows/voxa-release.yml.

  release_manifest.py version  <run_number>                 print the version
  release_manifest.py notes    <previous_tag|-> [--repo DIR]  print the changelog
  release_manifest.py manifest --version V --notes FILE --signature FILE
                               --url URL --out latest.json

The manifest is the `latest.json` of the Tauri updater (static JSON format):
version, notes, pub_date and one entry per platform with the signature (the
contents of the `.sig` file) and the download URL.
"""

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

# Versions are 0.MINOR.<workflow run number>. MINOR sits above the 0.9.x of the
# Handy fork the project started from, so every Voxa release is newer than the
# CI test builds that still carry 0.9.8. The run number only grows, which is
# what the updater needs (docs/DECISIONS.md, D-18).
MAJOR = 0
MINOR = 10

PLATFORMS = ("darwin-aarch64", "darwin-aarch64-app")
MAX_NOTE_LINES = 40


def make_version(run_number: int) -> str:
    if run_number < 1:
        raise ValueError("run number must be positive")
    return f"{MAJOR}.{MINOR}.{run_number}"


def format_changelog(subjects: list[str]) -> str:
    """Bullet list from commit subjects; blank lines dropped, long lists cut."""
    lines = [s.strip() for s in subjects if s.strip()]
    if not lines:
        return "No user-visible changes."
    shown = lines[:MAX_NOTE_LINES]
    out = [f"- {s}" for s in shown]
    if len(lines) > len(shown):
        out.append(f"- ...and {len(lines) - len(shown)} more")
    return "\n".join(out)


def commit_subjects(previous_tag: str | None, repo: str = ".") -> list[str]:
    rev = f"{previous_tag}..HEAD" if previous_tag else "HEAD"
    out = subprocess.run(
        ["git", "-C", repo, "log", "--no-merges", "--pretty=%s", rev],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return out.splitlines()


def build_manifest(
    version: str, notes: str, signature: str, url: str, now: datetime | None = None
) -> dict:
    now = now or datetime.now(timezone.utc)
    entry = {"signature": signature.strip(), "url": url}
    return {
        "version": version,
        "notes": notes,
        "pub_date": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {name: dict(entry) for name in PLATFORMS},
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("version")
    p.add_argument("run_number", type=int)

    p = sub.add_parser("notes")
    p.add_argument("previous_tag")
    p.add_argument("--repo", default=".")

    p = sub.add_parser("manifest")
    p.add_argument("--version", required=True)
    p.add_argument("--notes", required=True, help="file with the changelog")
    p.add_argument("--signature", required=True, help="the .sig file")
    p.add_argument("--url", required=True)
    p.add_argument("--out", required=True)

    args = parser.parse_args(argv)
    if args.cmd == "version":
        print(make_version(args.run_number))
    elif args.cmd == "notes":
        tag = None if args.previous_tag in ("", "-") else args.previous_tag
        print(format_changelog(commit_subjects(tag, args.repo)))
    else:
        manifest = build_manifest(
            args.version,
            Path(args.notes).read_text(encoding="utf-8").strip(),
            Path(args.signature).read_text(encoding="utf-8"),
            args.url,
        )
        Path(args.out).write_text(
            json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
