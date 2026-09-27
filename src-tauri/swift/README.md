# `src-tauri/swift/` — Apple Intelligence bridge

A C-ABI bridge that lets the Rust backend call Apple's on-device LLM
(`FoundationModels`) for transcript post-processing. Used only on
**macOS aarch64**; everywhere else `apple_intelligence.rs` compiles to a stub that
reports unavailable.

Compilation is driven from the **Cargo build script**
(`src-tauri/build.rs`, `build_apple_intelligence_bridge()`), not by Xcode. The
script picks a Swift file, compiles it to a static library at build time, and links
it into the crate.

---

## Files

| File | Purpose |
| --- | --- |
| `apple_intelligence.swift` | Real implementation. `@Generable struct CleanedTranscript` and `@available(macOS 26.0, *)` |
| `apple_intelligence_stub.swift` | Fallback when `FoundationModels` is unavailable. Reports `is_apple_intelligence_available() == 0` |
| `apple_intelligence_bridge.h` | The C-compatible interface both implementations satisfy |

### The bridge contract

`apple_intelligence_bridge.h` declares three C functions:

```c
int is_apple_intelligence_available(void);
AppleLLMResponse* process_text_with_system_prompt_apple(
    const char* system_prompt, const char* user_content, int max_tokens);
void free_apple_llm_response(AppleLLMResponse* response);
```

`AppleLLMResponse` carries `{ char* response; int success; char* error_message; }`,
with `error_message` valid only when `success == 0`.

> **Memory:** strings returned across the bridge are allocated with `strdup`. Rust
> must call `free_apple_llm_response()` for every response, including failures.
> Leaking here leaks into a long-lived process.

---

## How the build selects an implementation

`build.rs` checks two things before choosing the real implementation:

1. Whether the active macOS SDK contains `FoundationModels.framework`.
2. Whether the toolchain is **Command Line Tools only**. CLT ships a `swiftc`
   *without* the `FoundationModelsMacros` plugin, so the macro-using Swift file
   cannot compile even when the framework exists.

If either check fails, the stub is compiled and a warning is emitted. This is why
the same source tree can build successfully with either toolchain — and why
"Apple Intelligence is unavailable" may mean "you do not have full Xcode", not
"your OS is too old".

`swiftc` is located via the `SWIFTC` environment variable when set, otherwise
`xcrun --find swiftc`.

Linking details handled by `build.rs`: `FoundationModels` is linked **weakly** so
the app still launches on systems without the framework, and
`-Wl,-rpath,/usr/lib/swift` is added along with the toolchain and SDK Swift runtime
library paths.

---

## Notes and traps

- **Do not add a `.swift` file and expect it to be picked up.** The build script
  names the two source files explicitly. A third file is ignored unless
  `build.rs` is updated.
- `FoundationModels` requires macOS 26.0+ and Apple silicon. The
  `is_apple_intelligence_available()` check gates the provider in the UI
  (`commands::check_apple_intelligence_available`), so the frontend never offers a
  provider that cannot work.
- Because linking is weak, a missing framework is a **runtime** concern, not a link
  error. Always check availability before calling.
- This is a **post-processing** provider only. It is not wired into the dictation
  path's core transcription and has nothing to do with the planned conversation
  pipeline.
- The bridge is not covered by the Rust unit tests — it requires a macOS host with
  full Xcode to exercise.
