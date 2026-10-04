# Local text formatting

Advanced → Transcription → Text formatting enables deterministic formatting without an AI provider or external paste script. The defaults enable spoken punctuation, keep only explicitly spoken sentence periods, and capitalize at line starts and after sentence-ending punctuation. Explicitly saved preferences remain configurable.

When enabled:

- Spoken punctuation replaces editable literal phrases, ignoring case and matching whole words. Rules run in one pass per stage, with longer phrases matched first. Commands such as “exclamation point” produce symbols; “new paragraph” inserts two newlines. Matching words are treated as commands even when spoken literally, so remove ambiguous entries such as “period” if needed.
- First letter can keep the recognizer’s output, become lowercase, or become uppercase. The default “Capitalize at line starts and sentence boundaries” reads the text before the insertion point using macOS Accessibility. Empty or whitespace-only line prefixes and prefixes ending in a period, question mark, or exclamation point capitalize the opening; unfinished sentences use lowercase. Dictated line breaks also capitalize the next word. Editors that do not expose a text value and selection range fall back to tracking boundaries across dictations in the same focused editor. Switching to another identifiable editor starts with a capital letter instead of inheriting an unfinished sentence from another app. Without readable cursor context, manual edits or moving to a new line within the same editor cannot be detected reliably. Range-based text access is also supported for rich text editors that do not expose a plain text value. Editor contents are neither logged nor persisted. Internal names retain their case.
- Sentence periods can remain, be removed only at the end of a dictation, or be removed at sentence boundaries. Decimals remain. The default spoken-only mode removes automatic ellipses too. Removing periods is a character heuristic and may also remove abbreviation periods.

Use Add replacement, Remove, and Save replacements to edit the list. Enter `\n` in a replacement for a newline. Up to 100 unique phrases are supported; phrases may contain up to 200 UTF-8 bytes and replacements up to 100 bytes. Empty replacements are allowed.

Spoken replacements run before optional AI processing, then final replacements, capitalization and period handling run afterward. Plain dictation, AI dictation, and retrying a history recording share this path. The original recognition text remains in history; when AI processing succeeds, the processed history text includes the final formatting. Without AI processing, raw history remains raw. Copying an existing raw history entry does not rerun formatting.

Explicit spoken periods are protected through formatting. In spoken-only mode, if an AI rewrite loses the protected sentence boundaries, Handy falls back to the local text for that dictation.

Turn off Text formatting to revert to the existing processing behavior. Settings persist in Handy’s normal settings store. No transcript is sent to a server by this feature; optional AI processing still uses the provider already configured by the user.

For a read-only native check, run the installed Handy executable with `--format-preview "hello period next sentence"` while the target editor has focus. With Handy already running, this forwards the check to the authorized GUI app and saves the capitalization decision and formatted sample in `format-preview.json` in Handy’s app data directory, without pasting, recording, or exposing editor contents. The preview uses the built-in formatting defaults.
