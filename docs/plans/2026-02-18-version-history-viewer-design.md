# Version History Viewer — Design Document

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow users to view the full timeline of post-processing enhancements for a history entry and restore any previous version.

**Architecture:** Expandable accordion UI within each history card, backed by the existing `transcription_versions` table and a new `restore_version` command.

**Tech Stack:** Rust/Tauri (backend command), React/TypeScript (frontend component), SQLite (existing schema)

---

## Context

The post-process feature lets users enhance transcription text via an LLM. Each enhancement is recorded in the `transcription_versions` table with the result text, the prompt used, and a timestamp. The `get_transcription_versions` command already exists and returns versions ordered by timestamp ASC.

Currently, users can toggle between "Show Original" and "Show Enhanced" but cannot see or restore intermediate versions. This design adds a version timeline with restore capability.

## Design Decisions

- **Original transcription is not stored as version 0.** The original lives in `transcription_text` on the history entry (immutable). The UI renders it as the bottom entry in the timeline. This avoids data duplication.
- **Restore does not create new version rows.** It only updates `post_processed_text` and `post_process_prompt` on the history entry. The version timeline is append-only — nothing is ever deleted or modified in `transcription_versions`.
- **"Show Original" toggle is kept.** It serves as a quick-peek shortcut for A/B comparison. The version history is for deeper exploration and restore.
- **Versions are fetched lazily.** Only when the accordion is first expanded, not on every history page load.

---

## 1. Backend Changes

### New Tauri Command: `restore_version`

```rust
#[tauri::command]
#[specta::specta]
pub async fn restore_version(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    entry_id: i64,
    version_id: Option<i64>,
) -> Result<(), String>
```

**Behavior:**

- Enforces three-level feature gate (`experimental_enabled`, `post_process_enabled`, `history_post_process_enabled`). Returns `HISTORY_POST_PROCESS_DISABLED` if not met.
- If `version_id` is `Some(id)`: looks up the version in `transcription_versions`, sets `post_processed_text` and `post_process_prompt` on the history entry to match.
- If `version_id` is `None`: sets `post_processed_text = NULL` and `post_process_prompt = NULL` (restoring to original).
- Returns `VERSION_NOT_FOUND` if the specified version doesn't exist.
- Emits `history-updated` event on success.

### New HistoryManager Method: `restore_version`

```rust
pub fn restore_version(&self, entry_id: i64, version_id: Option<i64>) -> Result<()>
```

- If `version_id` is `None`: `UPDATE transcription_history SET post_processed_text = NULL, post_process_prompt = NULL WHERE id = ?`
- If `version_id` is `Some(id)`: query the version row, then update the history entry with the version's text and prompt.
- No transaction needed — single UPDATE statement.

### No New Migrations

The existing schema supports everything. No changes to `transcription_versions` or `transcription_history`.

---

## 2. Frontend: VersionHistory Component

### Location

New component: `src/components/settings/history/VersionHistory.tsx`

Rendered inside `HistoryEntryComponent`, between the transcription text and the audio player.

### Render Conditions

Only renders when `entry.post_processed_text != null` (at least one enhancement has been made).

### Structure

```
[Version History Toggle Bar]
  - Clock icon (pink accent)
  - "Version History" label (pink accent)
  - "(N versions)" count (muted)
  - Chevron up/down (pink accent)

[Expanded Timeline] (when open)
  - Version N (Current) — pink border, "Active" badge, text, prompt
  - Version N-1 — muted border, "Restore" button, text, prompt
  - ...
  - Original — muted border, mic icon, "Original transcription" label, "Restore" button
```

### State Management

- `isExpanded: boolean` — controls accordion open/close
- `versions: TranscriptionVersion[] | null` — null until first fetch
- `loadingVersions: boolean` — loading state for fetch
- Versions fetched via `commands.getTranscriptionVersions(entry.id)` on first expand
- Cached in state; refreshed when `history-updated` event fires and accordion is open

### Active Version Detection

- Compare `entry.post_processed_text` against each version's `text`
- If `entry.post_processed_text` is null, the original is active
- The active version shows a pink "Active" pill badge instead of a "Restore" button

---

## 3. Restore Interaction Flow

1. User clicks "Restore" on a version card
2. Button text changes to "Confirm restore?" (pink accent, outlined button style)
3. A 3-second timer starts
4. **If clicked again within 3 seconds:**
   - Calls `commands.restoreVersion(entry.id, version.id)` (or `null` for original)
   - Shows spinner while in flight
   - On success: `history-updated` event fires, parent refreshes, timeline updates
   - On error: toast with i18n error message, button reverts to "Restore"
5. **If 3 seconds pass:** button reverts to "Restore"

### Post-Restore Behavior

- Main transcription text at top of card updates (parent re-renders from event)
- "Show Original" toggle reacts correctly (reads from `entry.post_processed_text`)
- Timeline "Active" badge moves to the restored version
- If original is restored, `post_processed_text` becomes null — but the version history toggle stays visible because versions still exist in the table. The "Show Original" toggle in the header disappears (no enhanced text to compare against).

**Edge case — restore original when versions exist:** The accordion stays visible (versions still exist), but the "Show Original" toggle in the header disappears since `post_processed_text` is null. The user can still re-enhance via the sparkle button or restore a previous version from the timeline.

---

## 4. i18n Keys

```json
{
  "settings": {
    "history": {
      "versionHistory": "Version History",
      "versionCount": "({{count}} versions)",
      "activeVersion": "Active",
      "originalVersion": "Original transcription",
      "restore": "Restore",
      "confirmRestore": "Confirm restore?",
      "restoreError": "Failed to restore version",
      "versionNotFound": "Version not found"
    }
  }
}
```

### Error Code Mapping

| Backend Error Code              | i18n Key                                          |
| ------------------------------- | ------------------------------------------------- |
| `HISTORY_POST_PROCESS_DISABLED` | `settings.history.postProcessDisabled` (existing) |
| `VERSION_NOT_FOUND`             | `settings.history.versionNotFound` (new)          |

---

## 5. Testing

### Backend (Rust)

- `restore_to_specific_version`: insert entry, create 2 versions, restore to version 1 — verify `post_processed_text` matches version 1's text
- `restore_to_original`: insert entry, create version, restore to original (None) — verify `post_processed_text` is NULL
- `versions_preserved_after_restore`: restore to any version — verify `transcription_versions` table is unchanged (same row count, same data)
- `restore_nonexistent_version`: attempt to restore to a version ID that doesn't exist — verify error

### Frontend (Manual)

- Expand timeline on an entry with 2+ enhancements — verify all versions display with correct timestamps and prompts
- Verify "Active" badge is on the correct version
- Click "Restore" — verify inline confirmation appears
- Wait 3 seconds — verify button reverts
- Click "Restore" then "Confirm restore?" — verify card text updates, Active badge moves
- Restore to original — verify "Show Original" toggle disappears from header, timeline stays visible
- Re-enhance after restoring original — verify new version appears in timeline

---

## 6. Wireframe Reference

See `version-history.pen` in the repository root for the visual design created in Pencil. Key visual elements:

- Version timeline uses vertical connector lines between version cards
- Active version: pink accent border + background, "Active" pill badge
- Previous versions: subtle border, muted text, "Restore" outlined button
- Original: mic icon label, same restore button pattern
- Toggle bar: pink accent text with clock icon and chevron
