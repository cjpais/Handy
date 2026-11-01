// Browser component fixture only. Audio, ASR merging, IPC, target-window focus,
// clipboard and native paste are mocked; this does not prove native integration.
import React from "react";
import { createRoot } from "react-dom/client";
import type { OverlayEditSnapshot } from "../../src/bindings";

type DraftWrite = {
  sessionId: number;
  userRevision: number;
  baseAsr: string;
  text: string;
};
const callbacks = new Map<number, (value: unknown) => void>();
const listeners = new Map<number, { event: string; callback: number }>();
let nextId = 1;
let snapshot: OverlayEditSnapshot = {
  session_id: 0,
  phase: "inactive",
  text: "",
  asr_text: "",
  revision: 0,
  user_revision: 0,
  supported: true,
  conflict: false,
  paste_requested: false,
};
let preview = "Hello world";
const dispatch = (event: string, payload: unknown) => {
  for (const [id, listener] of listeners) {
    if (listener.event === event) {
      callbacks.get(listener.callback)?.({ event, id, payload });
    }
  }
};
const publish = (patch: Partial<OverlayEditSnapshot>) => {
  snapshot = { ...snapshot, revision: snapshot.revision + 1, ...patch };
  dispatch("overlay-edit-state", { ...snapshot });
};
const acceptDraft = (draft: DraftWrite) => {
  if (draft.userRevision < snapshot.user_revision) return;
  // A deliberately small mocked merge: these tests cover the React protocol,
  // while Rust tests own real recognition/user-edit conflict resolution.
  const appended = snapshot.asr_text.startsWith(draft.baseAsr)
    ? snapshot.asr_text.slice(draft.baseAsr.length)
    : "";
  publish({ text: draft.text + appended, user_revision: draft.userRevision });
};

const fixture = {
  ready: false,
  holdDrafts: false,
  failNextPaste: false,
  drafts: [] as DraftWrite[],
  submissions: [] as DraftWrite[],
  pasted: [] as string[],
  get snapshot() {
    return { ...snapshot };
  },
  emitSnapshot: publish,
  emitStream(committed: string, tentative = "") {
    preview = [committed, tentative].filter(Boolean).join(" ");
    dispatch("stream-text-event", { committed, tentative });
  },
  appendRecognition(asr: string) {
    const appended = asr.startsWith(snapshot.asr_text)
      ? asr.slice(snapshot.asr_text.length)
      : "";
    publish({ asr_text: asr, text: snapshot.text + appended });
  },
  flushDrafts() {
    fixture.holdDrafts = false;
    const last = fixture.drafts.at(-1);
    if (last) acceptDraft(last);
  },
  finishRecognition(asr = snapshot.asr_text) {
    fixture.appendRecognition(asr);
    publish({ phase: "review", paste_requested: false });
  },
};
export type LiveEditorFixture = typeof fixture;
declare global {
  interface Window {
    editorFixture: LiveEditorFixture;
  }
}
window.editorFixture = fixture;

Object.assign(window, {
  __TAURI_EVENT_PLUGIN_INTERNALS__: {
    unregisterListener: (_event: string, id: number) => listeners.delete(id),
  },
  __TAURI_INTERNALS__: {
    transformCallback(callback: (value: unknown) => void) {
      const id = nextId++;
      callbacks.set(id, callback);
      return id;
    },
    async invoke(command: string, args: Record<string, unknown> = {}) {
      if (command === "plugin:event|listen") {
        const id = nextId++;
        listeners.set(id, {
          event: String(args.event),
          callback: Number(args.handler),
        });
        return id;
      }
      if (command === "plugin:event|unlisten") {
        listeners.delete(Number(args.eventId));
        return null;
      }
      if (
        command === "plugin:event|emit" ||
        command === "plugin:event|emit_to"
      ) {
        dispatch(String(args.event), args.payload);
        return null;
      }
      if (command === "get_app_settings")
        return { app_language: "en", overlay_position: "bottom" };
      if (command === "get_overlay_edit_state") {
        if (!fixture.ready) {
          fixture.ready = true;
          setTimeout(() => {
            dispatch("show-overlay", "streaming");
            dispatch("recording-ready", null);
            fixture.emitStream(preview);
          }, 0);
        }
        return { ...snapshot };
      }
      if (command === "begin_overlay_edit") {
        publish({
          session_id: snapshot.session_id + 1,
          phase: "recording",
          text: preview,
          asr_text: preview,
          user_revision: 0,
          conflict: false,
          paste_requested: false,
        });
        dispatch("show-overlay", "editing");
        return { ...snapshot };
      }
      if (command === "update_overlay_edit_draft") {
        const draft = args as unknown as DraftWrite;
        fixture.drafts.push({ ...draft });
        if (!fixture.holdDrafts) acceptDraft(draft);
        return null;
      }
      if (command === "submit_overlay_edit") {
        const draft = args as unknown as DraftWrite;
        fixture.submissions.push({ ...draft });
        acceptDraft(draft);
        if (snapshot.phase === "recording") {
          publish({ phase: "finalizing", paste_requested: true });
        } else if (fixture.failNextPaste) {
          fixture.failNextPaste = false;
          publish({ phase: "review", paste_requested: false });
          throw "Simulated target-window activation failure";
        } else {
          publish({ phase: "pasting", paste_requested: true });
          fixture.pasted.push(snapshot.text);
          publish({ phase: "inactive", paste_requested: false });
          dispatch("hide-overlay", null);
        }
        return null;
      }
      if (command === "cancel_operation") {
        publish({ phase: "inactive", paste_requested: false });
        dispatch("hide-overlay", null);
        return null;
      }
      throw new Error(`Unexpected fixture IPC command: ${command}`);
    },
  },
});

// Install the bridge before importing the component and its i18n/settings graph.
const { default: RecordingOverlay } = await import(
  "../../src/overlay/RecordingOverlay"
);
const { default: i18n } = await import("../../src/i18n");
await i18n.changeLanguage("en");
document.body.style.margin = "0";
createRoot(document.getElementById("root")!).render(<RecordingOverlay />);
