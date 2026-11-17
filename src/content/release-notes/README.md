# Release Notes

Add user-facing release notes as Markdown files named by app version:

```text
src/content/release-notes/0.8.4.md
```

To show a note on one platform only, add the platform after the version, for
example `0.8.4.linux.md` (also `macos` or `windows`). On that platform it is
shown after the shared `0.8.4.md`, if there is one.

The update modal shows the highest bundled release note newer than the
persisted `whats_new_last_seen_version` and not newer than the running app
version.

Keep these files focused on headline user-facing changes. Release notes support
paragraphs, headings, lists, links, code, quotes, separators, hard line breaks,
and local images under `/release-notes/...`. Raw HTML is ignored before
rendering.

Place image assets in `public/release-notes/{version}/` and reference them from
Markdown with absolute paths:

```md
![Streaming transcription preview](/release-notes/0.9.0/streaming-transcription.webp)
```
