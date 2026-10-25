import { create } from "zustand";
import { toast } from "sonner";
import i18n from "@/i18n";
import { commands, events, type TranscriptionJobStatus } from "@/bindings";

export type ActiveJobStatus = Extract<
  TranscriptionJobStatus,
  { state: "queued" } | { state: "running" }
>;

// Background transcriptions (files, and History entries transcribed again),
// keyed by History entry id. Only unfinished jobs are kept.
interface TranscriptionJobsStore {
  jobs: Record<number, ActiveJobStatus>;
  initialized: boolean;
  initialize: () => Promise<void>;
}

export const useTranscriptionJobsStore = create<TranscriptionJobsStore>()(
  (set, get) => ({
    jobs: {},
    initialized: false,

    initialize: async () => {
      if (get().initialized) return;
      set({ initialized: true });

      // Jobs an event has reported on. The list fetched below may be older
      // than an event that arrived first, so it never overrides one: a job
      // that just finished would otherwise come back and stay forever.
      const reported = new Set<number>();
      await events.transcriptionJob.listen(({ payload }) => {
        const { entry_id: entryId, status } = payload;
        reported.add(entryId);
        set((state) => {
          const jobs = { ...state.jobs };
          if (status.state === "queued" || status.state === "running") {
            jobs[entryId] = status;
          } else {
            delete jobs[entryId];
          }
          return { jobs };
        });
        if (status.state === "failed") {
          console.error(`Transcription of entry ${entryId} failed:`, status);
          toast.error(i18n.t("settings.history.transcribeFailed"));
        }
      });

      const jobs = await commands.getTranscriptionJobs();
      set((state) => {
        const merged = { ...state.jobs };
        for (const job of jobs) {
          if (reported.has(job.entry_id)) continue;
          if (job.status.state === "queued" || job.status.state === "running") {
            merged[job.entry_id] = job.status;
          }
        }
        return { jobs: merged };
      });
    },
  }),
);

const hasJobs = (state: TranscriptionJobsStore) =>
  Object.keys(state.jobs).length > 0;

// While a background transcription runs, the model can't be switched,
// unloaded or deleted, so one transcript is never written by two models.
export const useModelsLocked = () => useTranscriptionJobsStore(hasJobs);

// The same, read once outside React.
export const areModelsLocked = () =>
  hasJobs(useTranscriptionJobsStore.getState());
