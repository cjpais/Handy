import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { produce } from "immer";
import { save } from "@tauri-apps/plugin-dialog";
import { commands, events } from "@/bindings";

export type FileJobStatus = "queued" | "transcribing" | "done" | "failed";

export interface FileJob {
  /** Absolute path on disk. Also the identity of the job. */
  path: string;
  /** Basename, shown in the UI and used as the archive entry name. */
  name: string;
  status: FileJobStatus;
  text?: string;
  error?: string;
}

interface FileTranscriptionStore {
  jobs: FileJob[];
  running: boolean;
  postProcess: boolean;
  initialized: boolean;

  // Actions
  initialize: () => Promise<void>;
  addFiles: (paths: string[]) => void;
  removeFile: (path: string) => void;
  clearAll: () => void;
  setPostProcess: (postProcess: boolean) => void;
  start: () => Promise<void>;
  cancel: () => Promise<void>;
  /** Returns false when the user dismissed the save dialog. */
  exportZip: () => Promise<boolean>;
}

const basename = (path: string): string => {
  const segments = path.split(/[/\\]/);
  return segments[segments.length - 1] || path;
};

/**
 * Batch file-transcription queue.
 *
 * State lives here rather than in the tab component because switching tabs
 * swaps the component at that position in the tree, tearing down local state —
 * a batch has to survive that. For the same reason `initialize()` is called
 * once from `main.tsx`, not from the component: events fired while the tab is
 * unmounted would otherwise be dropped.
 */
export const useFileTranscriptionStore = create<FileTranscriptionStore>()(
  subscribeWithSelector((set, get) => ({
    jobs: [],
    running: false,
    postProcess: false,
    initialized: false,

    addFiles: (paths) =>
      set(
        produce((state: FileTranscriptionStore) => {
          for (const path of paths) {
            if (state.jobs.some((job) => job.path === path)) continue;
            state.jobs.push({
              path,
              name: basename(path),
              status: "queued",
            });
          }
        }),
      ),

    removeFile: (path) =>
      set(
        produce((state: FileTranscriptionStore) => {
          state.jobs = state.jobs.filter((job) => job.path !== path);
        }),
      ),

    clearAll: () => set({ jobs: [] }),

    setPostProcess: (postProcess) => set({ postProcess }),

    start: async () => {
      if (get().running) return;

      const targets = get().jobs.filter((job) => job.status !== "done");
      if (targets.length === 0) return;

      // Flip to running before awaiting so a double-click cannot start twice;
      // the backend rejects a concurrent batch anyway, but this avoids the
      // pointless error toast.
      set(
        produce((state: FileTranscriptionStore) => {
          state.running = true;
          for (const job of state.jobs) {
            if (job.status === "done") continue;
            job.status = "queued";
            job.error = undefined;
          }
        }),
      );

      const result = await commands.transcribeAudioFiles(
        targets.map((job) => job.path),
        get().postProcess,
      );

      if (result.status !== "ok") {
        set({ running: false });
        throw new Error(String(result.error));
      }
    },

    cancel: async () => {
      await commands.cancelFileTranscription();
    },

    exportZip: async () => {
      const completed = get().jobs.filter(
        (job) => job.status === "done" && job.text,
      );
      if (completed.length === 0) return false;

      const dest = await save({
        defaultPath: "handy-transcripts.zip",
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      if (!dest) return false;

      const result = await commands.exportTranscriptsZip(
        dest,
        completed.map((job) => ({ name: job.name, text: job.text ?? "" })),
      );
      if (result.status !== "ok") {
        throw new Error(String(result.error));
      }
      return true;
    },

    initialize: async () => {
      if (get().initialized) return;

      events.fileTranscriptionEvent.listen((event) => {
        const payload = event.payload;

        set(
          produce((state: FileTranscriptionStore) => {
            if (payload.status === "batchFinished") {
              state.running = false;
              return;
            }

            const job = state.jobs.find((entry) => entry.path === payload.path);
            if (!job) return;

            if (payload.status === "started") {
              job.status = "transcribing";
              job.error = undefined;
            } else if (payload.status === "completed") {
              job.status = "done";
              job.text = payload.text;
            } else {
              job.status = "failed";
              job.error = payload.error;
            }
          }),
        );
      });

      set({ initialized: true });
    },
  })),
);
