# `public/` — static frontend assets

Files here are copied verbatim into the frontend build output by Vite and served
at the web root, so a file at `public/release-notes/0.9.7/x.png` is fetched as
`/release-notes/0.9.7/x.png`.

Because they bypass the bundler, files here are **not** hashed, transformed,
type-checked, or tree-shaken. Keep this directory small — everything in it ships.

---

## Contents

`release-notes/` — images referenced by the Markdown release notes in
[`../src/content/release-notes/`](../src/content/release-notes/). One subdirectory
per version, matching the note filename (`0.9.7.md` → `release-notes/0.9.7/`).

---

## How release notes and their images work together

Two locations are involved, and the split matters:

| Location | Role |
| --- | --- |
| `src/content/release-notes/*.md` | The note text, imported as a module by `releaseNotes.ts` |
| `public/release-notes/<version>/` | Images referenced from that Markdown |

The Markdown is bundled and rendered by `react-markdown`
(`components/whats-new/MarkdownContent.tsx`), shown via `WhatsNewGate` /
`WhatsNewModal` when the "What's New" gate opens after an update. Because the
Markdown is bundled, a syntax error is a build failure; because the images are
served statically, a **wrong image path fails silently at runtime** as a broken
image.

Reference images with a root-absolute path so it resolves identically in dev and in
the packaged app:

```markdown
![Shortcut behaviour](/release-notes/0.9.7/shortcut-behavior.png)
```

---

## Notes

- Do not import from `public/` in TypeScript. Anything the bundler should process
  belongs in `src/` instead — importing from `public/` gives you a URL string, not
  a module, and breaks hashing and type safety.
- Upstream also keeps a `src-tauri/resources/` directory for assets the **Rust**
  side needs at runtime. Those are separate: `public/` is frontend-only, and
  `resources/` is resolved through Tauri's `BaseDirectory::Resource`.
- Release-note text is versioned content and must be translated like any other
  user-facing string where applicable; the surrounding UI strings live in the i18n
  locale files.
