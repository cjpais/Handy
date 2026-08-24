# Data on disk

## Summary

Everything Handy keeps between launches lives in one folder, the app data directory, plus two places it shares with the rest of the system: the Hugging Face cache for catalog models and the system log folder for `handy.log`. The app data directory holds the settings file, the history database, the recordings folder, the models folder, and an optional pair of custom chime files. The user meets these places through three buttons — "App Data Directory" and "Log Directory" on the About section, "Open Recordings Folder" on the History section — through the Models page's "Rescan", and through the README's instructions for installing a model by hand. Apart from the browser engine's own small cache (the theme, see Tray and overlay below), nothing is written anywhere else, and nothing is ever sent off the machine. This document says what each file is, when it is written, what removes it, and what happens when it is missing, read-only, or edited while Handy is running. How settings behave is owned by [The settings model](../foundations/the-settings-model.md); model states by [Models](../foundations/models.md); history retention by [The history page](../history/the-history-page.md).

## The simple case

The user installs Handy, finishes onboarding, and dictates a few times. Afterwards, `~/Library/Application Support/com.pais.handy` contains `settings_store.json`, `history.db`, a `recordings` folder with one `handy-<seconds>.wav` per dictation (up to five, the default history limit), and an empty `models` folder, because the model they chose in onboarding went to `~/.cache/huggingface/hub` instead. `~/Library/Logs/com.pais.handy/handy.log` has been collecting a debug-level log since the first launch. The user never has to look at any of this; the only time they are pointed at it is when support asks for the log, when they want to install a model by hand, or when they want to hear or back up a recording.

> Technical note: the folder name is Handy's bundle identifier, `com.pais.handy`, from `src-tauri/tauri.conf.json`. Tauri derives the app data directory as the platform's user data directory plus that identifier: `~/Library/Application Support/com.pais.handy` on macOS, `%APPDATA%\com.pais.handy` (`C:\Users\<user>\AppData\Roaming\com.pais.handy`) on Windows, `~/.local/share/com.pais.handy` on Linux. The log directory is `~/Library/Logs/com.pais.handy` on macOS and `<local data>\com.pais.handy\logs` (`%LOCALAPPDATA%` on Windows, `~/.local/share` on Linux) elsewhere. Every path in this document is resolved through one portable-aware helper, so in [portable mode](#portable-mode) they all move together.

## The app data directory

The directory is created on first launch. Handy creates `models` and `recordings` inside it at startup and opens `history.db`; it does not check the directory again until a file is needed.

### The settings file

One JSON file holding a single `settings` object with every setting Handy has. It is the only place settings live; there are no hidden preference files.

- **Written** on every change: the whole object is rewritten, not the one field. Writes are coalesced — a burst of changes within 100 ms lands as one write — and a pending write is flushed when Handy quits cleanly.
- **Written on read** too: the first read after an upgrade fills in missing fields, runs migrations (the overlay-style migration, the GPU-device reset, the What's New marker, the onboarding marker) and adds any binding that did not exist, then writes the result back. Reading an old file therefore changes it.
- **Damaged file:** if the file cannot be parsed as a whole, Handy keeps every field that is valid on its own and resets only the broken ones; if it is not a JSON object at all, every setting resets. The rules and the list of migrations are in [The settings model](../foundations/the-settings-model.md#damaged-and-out-of-date-settings-files).
- **Secrets:** post-processing API keys are stored in this file in plain text alongside everything else.
- **Hand edits while running:** Handy reads settings from memory and only re-reads the file at launch, so an edit made while it runs is overwritten by the next change the user makes in the window. Deleting the file while running has no visible effect until the next change, which recreates it in full.

### The history database

A SQLite database with one table of history entries: the recording's file name, timestamp, saved flag, title, transcript, and the post-processed text and prompt when post-processing ran. It is created and migrated at startup. Every dictation that captured sound adds a row, including failed transcriptions (empty text so the user can retry); [History Limit and Auto-Delete Recordings](../history/the-history-page.md) delete rows and their files together. Deleting `history.db` while Handy runs is not recovered: the next dictation recreates an empty file without the table, the history entry cannot be saved (logged, not shown), and the History section fails to load until Handy is relaunched, when the table is created again.

### The recordings folder

One file per dictation, `handy-<unix seconds>.wav`: 16 kHz, mono, 16-bit, holding exactly the capture — after voice activity detection, so silence is already removed, and padded to 1.25 s when the speech was shorter than a second (see [Audio capture](../foundations/audio-capture.md)). The file is written at the stop, in parallel with transcription, and verified before the history entry is created. Two dictations stopped within the same second would get the same name; the second overwrites the first.

Files and entries are deleted together by the History section's delete button and by retention. Files without an entry are never touched, so they accumulate as orphans:

- a cancel during processing that lands after the file is written but before the entry is saved (the file stays; see [Cancelling](../dictation/cancelling.md));
- a history save that fails (database missing or locked);
- Handy quitting or crashing between the write and the save.

The "Open Recordings Folder" button at the top of the History section opens this folder in Finder. If the folder was deleted while Handy runs, every later dictation fails to write its file ("Failed to save WAV file" in the log), no history entry is created, and the text is still pasted; the folder is created again at the next launch. Suspected gap: nothing tells the user their history stopped recording.

### The models folder

Handy's own models folder, created at startup. What lands here:

- **Legacy models** downloaded from Handy's own server: single files (`ggml-small.bin`, `whisper-medium-q4_1.bin`, `ggml-large-v3-turbo.bin`, `ggml-large-v3-q5_0.bin`, and the ONNX families) and directories (`parakeet-tdt-0.6b-v2-int8`, `parakeet-tdt-0.6b-v3-int8`, `giga-am-v3-int8`, and similar) extracted from archives.
- **Catalog models that fell back to a mirror.** When the Hugging Face download fails after four attempts, the mirror copy is saved here instead of in the cache.
- **Custom models** the user drops in: any `.bin` or `.gguf` whose name is not a known download, badged "Custom" on the Models page. Hidden files, non-model files, and `.partial` files are ignored. The README's manual-install instructions target this folder.
- **`<filename>.partial`** — a download in progress or one that was cancelled or interrupted. It is kept on cancel so the next "Download" resumes from where it stopped (the Models page shows the partial size). A completed file is renamed into place only after its checksum is verified; a checksum mismatch deletes the partial so the next attempt starts over.
- **`<filename>.extracting`** — a temporary directory while an archive is unpacked; it becomes the model directory when extraction succeeds. A leftover from an interrupted extraction is removed the next time the model list is refreshed.

Deleting a legacy or custom model from the Models page removes its file or directory and any partial next to it. A custom model disappears from the list for good, because there is nothing to re-download. "Rescan" re-reads this folder and the cache so a file dropped in by hand appears without a relaunch; a file removed by hand disappears at the next refresh, and if it was the active model the selection is cleared (see [Models](../foundations/models.md)).

> Technical note: at startup Handy also performs two one-time moves: a bundled `ggml-small.bin`, if the app bundle ships one (current builds do not), is copied here; and an old single-file GigaAM download (`giga-am-v3.int8.onnx`) is moved into the `giga-am-v3-int8` directory layout the current engine expects.

### The custom chime files

Two optional files in the root of the app data directory. When both exist, the Sound Theme dropdown in the Debug section gains a third option, "Custom", next to "Marimba" and "Pop", and selecting it plays these files as the start and stop chimes (at the Volume setting, through the chosen Output Device). The check runs when the settings window loads, so files added while the window is open need the window reopened (or Handy relaunched) before "Custom" appears. If the theme is "Custom" and one file is later removed, that chime is silently skipped and an error goes to the log; the dropdown keeps showing "Custom" until the window is reloaded, after which it shows the stored value with no matching option.

## The shared Hugging Face cache

Catalog models — everything under "Available to Download" — are downloaded into the Hugging Face cache, `~/.cache/huggingface/hub` (or `$HF_HOME/hub` when `HF_HOME` is set), so a model downloaded by another tool is reused and vice versa. Inside, each model repository is a folder `models--<org>--<name>` holding `blobs` (the data), `refs` (which commit is current), and `snapshots/<commit>/<file>` (the names Handy opens). A download in progress keeps a partial file in the cache that a later attempt resumes; cancelling "Download" leaves it there.

What the Models page sees here:

- A catalog model is "downloaded" when its file is in the cache or a copy is in the models folder.
- "Rescan" also lists any `.gguf` found in any snapshot in the cache that is not already a catalog entry, including alternate quantizations of catalog models (shown as, for example, "Whisper Medium (Q4_K_M)") and models from other tools, as long as their headers can be read.

What "Delete" removes:

- For a catalog model's default file, the **entire `models--<org>--<name>` folder** — blobs, refs, and every snapshot, including any other quantizations or files of that repository that another tool may depend on. This is deliberate (the code calls it a product decision) and is not warned about beyond the usual "Are you sure you want to delete {{modelName}}? You will need to download it again to use it." dialog.
- For an alternate quantization discovered in the cache, only that file's snapshot entry and its blob; the default file survives.
- In both cases, also any copy and `.partial` of the same file in the models folder.

## Logs

`handy.log` in the log directory, started at launch. The file is limited to 500 KB; when it reaches that size it is **deleted and started again**, so at most one file exists and the oldest lines are gone the moment it rolls over. A bug report taken after a long session may therefore contain only the last few minutes.

The amount written is the "Log Level" setting on the Debug section (Error, Warn, Info, Debug, Trace; default Debug), applied immediately and saved. `handy --debug` uses Trace for that run without saving. At the default level the log includes every transcript's text ("Transcription completed in …: '…'"), the paths Handy opened, device names, and model names — worth knowing before sharing the file. The "Live Logs" panel in the Debug section shows the same stream without opening the file.

The About section shows the log directory path in a "Log Directory" row with an "Open" button that reveals the folder in Finder. (The row uses the Debug section's strings but is placed on About.) If the folder is deleted while Handy runs, logging stops until the next launch, which creates it again.

## Bundled resources

Inside the app bundle (`Handy.app/Contents/Resources/resources/`), read-only: the Silero voice-activity model `models/silero_vad_v4.onnx`, the "Marimba" and "Pop" chimes, the tray and overlay icons, a vocabulary file for GigaAM, and the bundled What's New notes. Handy loads the VAD model from here the first time a microphone stream is opened. Nothing is ever written into the bundle, and reinstalling or updating Handy does not touch the app data directory.

## What the buttons open

| Where | Row | What it shows | "Open" does |
| --- | --- | --- | --- |
| About | "App Data Directory" — "Location where Handy stores its data" | The full path in a monospace box (selectable) | Opens the app data directory in Finder |
| About | "Log Directory" — "Location where log files are stored" | The full path | Opens the log directory in Finder |
| History | "Open Recordings Folder" button | — | Opens `recordings` in Finder |

If a path cannot be resolved the row shows "Error loading directory: {{error}}" instead. Opening uses the system's file opener, so on Windows it is Explorer and on Linux whatever handles folders.

## Portable mode

A Windows-only install layout (out of scope for this description, named here so the paths above are complete). The installer offers "Portable Installation" — "Self-contained folder with no registry changes, shortcuts, or uninstaller. Data stored next to the app." — and writes a file named `portable` containing the text `Handy Portable Mode` next to `Handy.exe`. When that marker is present at launch, every path above moves into a `Data` folder beside the executable: `Data\settings_store.json`, `Data\history.db`, `Data\recordings`, `Data\models`, `Data\logs\handy.log`, `Data\huggingface\hub` for catalog models (`HF_HOME` is set for the process), and `Data\webview` for the browser engine's own storage. An empty marker left by version 0.8.0 is upgraded in place when a `Data` folder is already there. The About rows show the `Data` paths. Self-update is refused: "Update available" leads to a "Manual update required" dialog — "Portable installs cannot be updated automatically. Download the installer for your system below, run it, and install to the same folder — your Data/ folder (settings, models, recordings) is kept in place." with "Download installer" (or "Open GitHub Releases" when no matching installer is listed). See [Updates](../integration/updates.md).

## When the directory cannot be written

Handy does not check for write access. Read from the code, not tested:

- **First launch into a read-only location:** creating `recordings` (or `models`) fails and Handy exits during startup with no window and no message beyond the console.
- **Existing directory made read-only:** settings changes appear to apply (the window updates, the running app uses them) but the file write fails silently, so they are gone at the next launch. Dictations still paste; the recording file fails to write and no history entry is created. Downloads fail with the download-failed toast. Nothing in the window explains any of this.
- **Disk full mid-download:** the partial is deleted when verification fails and the download is reported failed; the next attempt starts from zero.

## Modifiers

| Modifier | Set before the start | Changed while active |
| --- | --- | --- |
| Push to talk | No effect on what is written. | No effect. |
| Binding | Transcribe with Post-Processing adds the post-processed text and the prompt to the history row; the recording file is the same. | Fixed at the start. |
| Overlay style | No effect on disk. | No effect. |
| Streaming model | No effect on which files are written; the longer VAD tail makes the recording file a little longer. | Fixed at the trigger. |
| Voice activity detection | On: the recording file holds speech plus run-in and tail only. Off: it holds everything captured, silence included. | Fixed at the trigger. |
| Always-on microphone | No effect on disk. | No effect. |

## Cancel and interrupt

| Event | Before a dictation | During a dictation |
| --- | --- | --- |
| Cancel | Nothing is written, nothing to discard. | While recording: no file, no entry. During processing: the file may already be on disk and stays as an orphan; an entry already saved stays. See [Cancelling](../dictation/cancelling.md). |
| Another trigger | No effect on disk. | No effect on disk. |
| A setting changed mid-way | Every change rewrites `settings_store.json`. Lowering History Limit or shortening Auto-Delete Recordings deletes rows and files at once. Deleting a model removes its files. | Same; a model deleted while recording makes the dictation fail at the stop with an empty-text entry and a file. |
| Microphone lost | Nothing written. | The stop writes whatever was captured, if anything; an empty capture writes nothing. |
| Model or processing failure | A failed download leaves a `.partial` (resumable) or, on a checksum mismatch, nothing. | A failed transcription still writes the file and an entry with empty text so the user can "Re-transcribe". A failed post-processing request writes the plain transcript. |
| The active application changes | No effect. | No effect. |
| Handy quits or the system sleeps | A settings change made in the last 100 ms before a clean quit is flushed; a crash in that window loses it. A download in progress keeps its partial; an interrupted extraction's `.extracting` folder is cleaned at the next launch. | The recording is lost. A file half-written when the process dies fails verification at the next launch only if Handy looks at it — it does not; it stays as a short or corrupt orphan. |
| Keyboard channel changes | No effect on disk, except that a handy_keys startup failure writes `keyboard_implementation: tauri` to settings. | Same. |

## Interactions with other systems

**Permissions.** None of the app data locations needs a macOS permission. The "Open" buttons hand the path to Finder; they do not require Accessibility or Full Disk Access.

**History and recordings.** `history.db` and `recordings/` are this document's files; the History section is the only UI over them. Retention deletes by database row, so orphan files are invisible to it.

**Clipboard.** Nothing on disk is involved in pasting; clipboard contents are never written to a file.

**Model state.** "Downloaded" is a property of the file system: a file present in the models folder or the cache. Deleting, moving, or renaming files outside Handy changes the Models page at the next "Rescan" or launch, and removing the active model's file clears the selection (see [Models](../foundations/models.md)).

**Tray and overlay.** None. (The settings window and the overlay each remember the theme in the browser engine's own local storage so the first paint matches; this is a cache, not a setting, and lives in the engine's data folder, not the app data directory.)

**Sounds and system audio.** Only the two custom chime files; the built-in themes are read from the bundle.

**Settings persistence.** `settings_store.json`, described above and in [The settings model](../foundations/the-settings-model.md). The `log_level` and `sound_theme` settings are the ones that directly name files in this document.

**Platform differences.** Paths: Windows `%APPDATA%\com.pais.handy` with logs in `%LOCALAPPDATA%\com.pais.handy\logs`; Linux `~/.local/share/com.pais.handy` with logs in `~/.local/share/com.pais.handy/logs`; the Hugging Face cache is `~/.cache/huggingface/hub` on every platform (`C:\Users\<user>\.cache\huggingface\hub` on Windows). Portable mode exists only through the Windows installer. On Windows, cache entries may be plain files instead of links, so deleting an alternate quantization removes one file rather than a link and its blob. See [Platform differences](platform-differences.md).

## Edge cases

- The README's manual-install path for Linux (`~/.config/com.pais.handy/`) does not match where Handy actually looks (`~/.local/share/com.pais.handy/`); the About section's "App Data Directory" row is authoritative.
- Two dictations stopped within one second share a file name; the second write replaces the first recording while both history rows point at it, and deleting either row deletes the file for both.
- A `.partial` left by a cancelled download counts toward disk usage but is not shown anywhere except as the resume size on the model card.
- Dropping a catalog model's exact file (for example the Parakeet `.gguf` from the README) into the models folder makes that catalog entry "downloaded" without the cache being involved; "Delete" then removes only the models-folder copy.
- A `.gguf` in the models folder whose header cannot be read is still listed as a custom model; the same file in the Hugging Face cache is ignored, because the cache scan trusts headers only.
- Setting `HF_HOME` for the shell that launches Handy moves the cache; models already downloaded to the default location are no longer seen until the variable is unset.
- Log rollover is by size, not time: a short session with Trace logging can roll over several times and leave a log that starts mid-dictation.

## Open questions and verification

- The log rotation keeps no old file: `RotationStrategy::KeepOne` deletes `handy.log` at 500 KB and starts fresh. Whether this is intended (the name suggests "keep one old copy") was not confirmed; a user sending a log after a long session may send only the tail. Suspected bug or at least a naming trap.
- Transcript text is written to `handy.log` at the default "Debug" level. Whether that is an accepted privacy trade-off was not confirmed; noted here so support requests for logs can warn users.
- The README's Linux data path (`~/.config/com.pais.handy/`) versus Tauri's `~/.local/share/com.pais.handy/` was read from the path library, not checked on a Linux machine. Suspected documentation bug.
- Deleting a catalog model removes the whole Hugging Face repository folder, including other quantizations and files another tool may use, with no extra warning. Read from the code and its comment; worth a product decision on the dialog text.
- A deleted `recordings` folder or `history.db` is not recreated until relaunch, and dictations silently stop being recorded in history. Suspected bug.
- The `DebugPaths` component (showing "%APPDATA%/handy" paths) exists in the source but is not rendered by any section; it is dead code with a wrong folder name and was ignored here.
- Whether the "Custom" sound option really needs a window reload to appear after the files are added (the check runs once at settings-store initialization) was not observed.
- The first-launch behavior in a read-only location (silent exit) and the silent loss of settings in a read-only existing directory were read from the startup code and not reproduced.
- Where the browser engine's local storage (theme cache) lives on macOS (`~/Library/WebKit/com.pais.handy` is the usual place) was not verified and is not cleaned by deleting the app data directory.
- The 100 ms write coalescing and flush-on-quit come from the store plugin's defaults; whether a crash inside that window actually loses the change was not tested.

Verified against Handy commit `af48dd6`.
