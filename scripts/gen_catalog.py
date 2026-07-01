# /// script
# requires-python = ">=3.10"
# dependencies = ["huggingface_hub", "fsspec"]
# ///
"""
Handy model catalog generator.

Merges three sources into one catalog.json:
  1. HF card `transcribe_cpp` block  -> capabilities + benchmarks (canonical)
  2. a tiny GGUF header range-read    -> display labels only
  3. local CURATION (this file)       -> recommended set, editorial descriptions

Emits catalog.json to be committed and `include_str!`'d into the Rust binary.
Run:  HF_TOKEN=$(hf auth token) uv run gen_catalog.py [out_path]
"""
import json, os, re, sys, math, struct, datetime
from concurrent.futures import ThreadPoolExecutor, as_completed
from huggingface_hub import HfApi, HfFileSystem

ORG = "handy-computer"
CATALOG_VERSION = 1

# ───────────────────────── scoring (one constant each) ──────────────────────
SPEED_SCALE = 8.0    # speed = 100·(1 − e^(−rtf/8))     grows toward 100
ACC_SCALE   = 15.0   # accuracy = 100·e^(−wer/15)        decays from 100
SPEED_FLOOR = 10     # no reference-machine rtf (too heavy to benchmark) → floor, it's slow
def speed_from_rtf(rtf):
    return SPEED_FLOOR if not rtf or rtf <= 0 else round(100 * (1 - math.exp(-rtf / SPEED_SCALE)))
def acc_from_wer(wer):
    return None if wer is None else round(100 * math.exp(-wer / ACC_SCALE))

# ───────────────────────── curation (CJ owns; editorial, not derivable) ──────
# slug -> {rank?, rec?, desc?, use_case?, default_quant?, hidden?}.  All optional.
# desc is hand-written UI copy; models without one use factual generated summaries.
# rank = editorial sort position (broad ordering). rec = the small "Recommended"
# badge / onboarding subset — independent of rank, so a model can rank high
# without carrying the recommended tag.
CURATION = {
    "parakeet-unified-en-0.6b":        {"rank": 1, "rec": True, "desc": "Fast, accurate live English transcription"},
    "nemotron-3.5-asr-streaming-0.6b": {"rank": 2, "rec": True, "desc": "Live multilingual transcription across 28 languages"},
    "canary-180m-flash":               {"rank": 3, "rec": True, "desc": "Tiny and instant, runs well on any hardware"},
    "cohere-transcribe-03-2026":       {"rank": 4, "rec": True, "desc": "Highest accuracy, 14 languages, slower"},
    "whisper-medium":                  {"rank": 5, "rec": True, "desc": "Broadest language, but may run a bit slow"},
    # ranked (sorted high) but NOT tagged recommended
    "Voxtral-Mini-4B-Realtime-2602":   {"rank": 6, "desc": "Live multilingual, excellent on powerful machines"},
    "parakeet-tdt-0.6b-v3":            {"rank": 7, "desc": "Fast and accurate. Supports 25 European languages"},
    "parakeet-tdt-0.6b-v2":            {"rank": 8, "desc": "English only. The best model for English speakers"},
    "Qwen3-ASR-0.6B":                  {"rank": 9, "desc": "Excellent multilingual model"},
    "Fun-ASR-MLT-Nano-2512":           {"rank": 10, "desc": "A tiny multilingual model"},
    # description-only (unranked, not recommended) — carried over from the legacy .bin entry
    "Breeze-ASR-25":                   {"desc": "Optimized for Taiwanese Mandarin. Code-switching support."},
}
# temporary capability corrections pending a card re-push (remove once cards fixed)
OVERRIDES = {
    "granite-4.0-1b-speech": {"timestamps": "none"},
    "granite-speech-4.1-2b": {"timestamps": "none"},
}

# ───────────────────────── helpers ──────────────────────────────────────────
ARCH = ["whisper","moonshine-streaming","moonshine","parakeet","canary-qwen","canary","voxtral",
        "granite-speech","granite","qwen3","gigaam","sensevoice","cohere","fun-asr","nemotron","medasr"]
ACR = {"asr":"ASR","ctc":"CTC","rnnt":"RNNT","tdt":"TDT","nar":"NAR","mlt":"MLT"}
SCALAR = {0:("<B",1),1:("<b",1),2:("<H",2),3:("<h",2),4:("<I",4),5:("<i",4),
          6:("<f",4),7:("<?",1),10:("<Q",8),11:("<q",8),12:("<d",8)}

def slug(repo): return repo.split("/")[1].replace("-gguf", "")
def family(s, tags):
    for f in ARCH:
        if f in s.lower(): return "moonshine" if f.startswith("moonshine") else f.split("-")[0]
    for t in tags or []:
        if t in ("whisper","moonshine","parakeet","canary","voxtral","granite","qwen3","gigaam","sensevoice","cohere"):
            return t
    return "other"
def pretty(s):
    return " ".join(ACR.get(p.lower(), p if (p.isupper() or any(c.isdigit() for c in p)) else p.capitalize())
                    for p in s.split("-"))
def quant_of(fn):
    m = re.search(r"-(F32|F16|BF16|Q\d[\w_]*?)\.gguf$", fn);  return m.group(1) if m else "?"
def parse_params(size_label):
    """`general.size_label` ("0.6B" / "1.7B" / "62M") -> count in billions, or None."""
    m = re.match(r"([\d.]+)\s*([BM])", str(size_label or "").strip(), re.I)
    if not m: return None
    v = float(m.group(1));  return v if m.group(2).upper() == "B" else v / 1000
def pick_wer(b):
    if "wer_librispeech_test_clean" in b: return "librispeech", b["wer_librispeech_test_clean"]
    if "wer_fleurs_en" in b: return "fleurs_en", b["wer_fleurs_en"]
    fl = [k for k in b if k.startswith("wer_fleurs")]
    return (fl[0].replace("wer_", ""), b[fl[0]]) if fl else (None, {})
def headline_wer(d):
    for q in ("q8_0","f16","q5_k_m","q6_k","q4_k_m","f32","bf16"):
        if isinstance(d, dict) and q in d: return d[q], q
    return None, None
def auto_desc(langn, caps):
    feats = []
    if caps["translate"]:   feats.append("translation")
    if caps["lang_detect"]: feats.append("auto language detection")
    if caps["streaming"]:   feats.append("streaming")
    if caps["timestamps"] != "none": feats.append(f"{caps['timestamps']}-level timestamps")
    base = f"{langn}-language speech-to-text" if langn > 1 else "English speech-to-text"
    return base + (" with " + ", ".join(feats) + "." if feats else ".")

api = HfApi(token=os.environ.get("HF_TOKEN"))
fs  = HfFileSystem(token=os.environ.get("HF_TOKEN"))

GGUF_WANT = {"general.architecture", "general.name", "general.basename", "general.size_label"}
def probe_header(repo, filename, nbytes=65536):
    """Range-read only friendly general.* labels (no tensors, no capabilities).

    Capabilities come from HF card data here and from Rust's GGUF probe at
    runtime. Keep this as a narrow display-label reader so the catalog generator
    does not become a second source of truth for model behavior.
    """
    out = {}
    try:
        with fs.open(f"{repo}/{filename}", "rb", block_size=nbytes) as f:
            buf = f.read(nbytes)
    except Exception:
        return out
    if buf[:4] != b"GGUF": return out
    u32 = lambda o: struct.unpack_from("<I", buf, o)[0]
    u64 = lambda o: struct.unpack_from("<Q", buf, o)[0]
    def rstr(o): n = u64(o); return buf[o+8:o+8+n].decode("utf-8","replace"), o+8+n
    def skip(o, vt):
        if vt in SCALAR: return o + SCALAR[vt][1]
        if vt == 8: return o + 8 + u64(o)
        if vt == 9:
            et = u32(o); cnt = u64(o+4); o += 12
            if et in SCALAR: return o + SCALAR[et][1]*cnt
            if et == 8:
                for _ in range(cnt): o += 8 + u64(o)
                return o
        raise ValueError
    off = 24                      # magic+ver+n_tensors+n_kv
    n_kv = u64(16)
    try:
        for _ in range(n_kv):
            key, off = rstr(off); vt = u32(off); off += 4
            if key in GGUF_WANT and vt == 8:
                out[key], off = rstr(off)
            elif key in GGUF_WANT and vt in SCALAR:
                fmt, sz = SCALAR[vt]; out[key] = struct.unpack_from(fmt, buf, off)[0]; off += sz
            else:
                off = skip(off, vt)
            if GGUF_WANT.issubset(out): break
    except Exception:
        pass                      # ran past the buffer (hit tokenizer) — keep what we got
    return out

def gguf_files(repo, siblings):
    """Return GGUF files with mandatory size metadata.

    `QuantFile.size_bytes` is a non-null `u64` in Rust. Failing generation here
    keeps a transient/malformed HF listing from producing a catalog that panics
    at app startup when deserialized by `include_str!("catalog.json")`.
    """
    files = []
    invalid = []
    for x in siblings:
        if not x.rfilename.endswith(".gguf"):
            continue
        if type(x.size) is not int or x.size <= 0:
            invalid.append(x.rfilename)
            continue
        files.append({
            "filename": x.rfilename,
            "quant": quant_of(x.rfilename),
            "size_bytes": x.size,
        })
    if invalid:
        raise ValueError(f"{repo}: missing/invalid size metadata for {', '.join(invalid)}")
    return sorted(files, key=lambda f: f["size_bytes"])

def build(repo):
    info = api.model_info(repo, files_metadata=True)
    cd = info.card_data.to_dict() if info.card_data else {}
    s = slug(repo)
    cur = CURATION.get(s, {})
    b = dict(cd.get("transcribe_cpp") or {});  b.update(OVERRIDES.get(s, {}))
    langs = cd.get("language") or []

    files = gguf_files(repo, info.siblings)
    q8 = next((f for f in files if "Q8" in f["quant"]), files[-1] if files else None)
    gg = probe_header(repo, q8["filename"]) if q8 else {}

    caps = {"streaming": bool(b.get("streaming")), "translate": bool(b.get("translate")),
            "lang_detect": bool(b.get("lang_detect")), "timestamps": b.get("timestamps", "none")}
    ryz = b.get("rtf_ryzen_4750u") or {};  rtf = ryz.get("vulkan", ryz.get("cpu"))
    eval_set, werd = pick_wer(b);  hw, hwq = headline_wer(werd)

    # Default-quant policy by model size (params from general.size_label):
    #   >=1B  -> Q5_K_M   (large models tolerate it; ~30% less download/RAM)
    #   <1B   -> Q8_0     (already small; keep reference quality)
    # A per-model CURATION `default_quant` overrides the policy. If the policy
    # quant is absent for a model, fall back: Q8_0 → any Q8 → Q5_K_M → smallest.
    params = parse_params(gg.get("general.size_label"))
    policy = "Q5_K_M" if (params is not None and params >= 1.0) else "Q8_0"
    have = lambda q: next((f["quant"] for f in files if f["quant"] == q), None)
    default_quant = (cur.get("default_quant")
                     or have(policy)
                     or have("Q8_0")
                     or next((f["quant"] for f in files if "Q8" in f["quant"]), None)
                     or have("Q5_K_M")
                     or (files[0]["quant"] if files else None))

    return {
        "id": repo,
        "slug": s,
        "name": gg.get("general.name") or pretty(s),         # friendly name (from GGUF)
        "architecture": gg.get("general.architecture"),
        "family": family(s, info.tags),
        "parameters": gg.get("general.size_label"),          # "0.6B" / "1.7B" / "62M"
        "description": cur.get("desc") or auto_desc(len(langs), caps),
        "base_model": cd.get("base_model"),
        "license": cd.get("license"),
        "language_count": len(langs),
        "languages": langs,
        "capabilities": caps,
        "speed_score": speed_from_rtf(rtf),
        "accuracy_score": acc_from_wer(hw),
        "files": files,
        "default_quant": default_quant,
        "recommended": bool(cur.get("rec")),         # small badge/onboarding subset
        "recommended_rank": cur.get("rank"),          # editorial sort position (independent)
    }

def main():
    repos = [m.id for m in api.list_models(author=ORG, limit=500)]
    models = []
    failures = []
    with ThreadPoolExecutor(max_workers=10) as ex:
        futs = {ex.submit(build, r): r for r in repos}
        for f in as_completed(futs):
            try:
                m = f.result()
                if not CURATION.get(m["slug"], {}).get("hidden"): models.append(m)
            except Exception as e:
                failures.append((futs[f], e))
                print(f"!! {futs[f]}: {e}", file=sys.stderr)
    if failures:
        print(f"catalog generation failed for {len(failures)} repo(s)", file=sys.stderr)
        raise SystemExit(1)
    models.sort(key=lambda m: (not m["recommended"], m["recommended_rank"] or 1e9,
                               m["family"], -(m["speed_score"] or 0)))
    catalog = {
        "catalog_version": CATALOG_VERSION,
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "models": models,
    }
    text = json.dumps(catalog, indent=2, ensure_ascii=False)
    # print each `languages` array on a single line for readability
    text = re.sub(r'"languages": \[(.*?)\]',
                  lambda m: '"languages": [' + ", ".join(re.findall(r'"[^"]*"', m.group(1))) + ']',
                  text, flags=re.S)
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "catalog.json")
    open(out, "w").write(text)
    print(f"wrote {out}: {len(models)} models, {os.path.getsize(out)/1024:.1f} KB", file=sys.stderr)

if __name__ == "__main__":
    main()
