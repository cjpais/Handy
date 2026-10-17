# Building Voxa on a Mac

CI (`.github/workflows/voxa-macos.yml`) builds the same artifact. This is the
local route, for an Apple Silicon Mac.

## One command

```bash
bash scripts/voxa/build-mac.sh
```

It installs the Rust target and frontend dependencies, runs `tauri build` for
`aarch64-apple-darwin`, signs the bundle ad-hoc (`codesign --sign -`, with the
microphone entitlement) and writes `dist-artifact/Voxa-macos-arm64.zip`.

Prerequisites: Xcode command line tools (`xcode-select --install`),
[Rust](https://rustup.rs), [Bun](https://bun.sh), CMake (`brew install cmake`).
The first build compiles ggml with Metal and takes a while; later builds are
incremental.

## Development run

```bash
bun install
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

## After installing a fresh build

```bash
xattr -cr /Applications/Voxa.app                  # remove the download quarantine
tccutil reset Accessibility com.daniyar.voxa      # only if shortcuts stopped working
tccutil reset ListenEvent com.daniyar.voxa
```

then grant Accessibility and Input Monitoring to Voxa again in System Settings.

## Checks that run without a Mac

```bash
bun run lint && bunx tsc --noEmit && bun run test:unit && bun run check:translations
(cd src-tauri && cargo fmt -- --check && cargo test)
bun run screenshots          # needs a Chromium: PLAYWRIGHT_CHROMIUM_PATH=/path/to/chrome
```
