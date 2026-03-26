import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { studioApi } from "@/lib/studioApi";
import type {
  StartStudioJobConfig,
  StudioFormat,
  StudioJob,
  StudioJobPreviewEvent,
  StudioJobProgressEvent,
} from "@/lib/types/studio";

const LAST_OUTPUT_FOLDER_KEY = "studio:last-output-folder";
const LAST_FORMATS_KEY = "studio:last-formats";

const DEFAULT_FORMATS: StudioFormat[] = ["txt", "srt"];

const readStoredFormats = (): StudioFormat[] => {
  const raw = localStorage.getItem(LAST_FORMATS_KEY);
  if (!raw) return DEFAULT_FORMATS;
  try {
    const value = JSON.parse(raw);
    if (!Array.isArray(value)) return DEFAULT_FORMATS;
    const formats = value.filter((item): item is StudioFormat =>
      ["txt", "srt", "vtt"].includes(item),
    );
    return formats.length > 0 ? formats : DEFAULT_FORMATS;
  } catch {
    return DEFAULT_FORMATS;
  }
};

type StudioStage =
  | "idle"
  | "ready"
  | "preparing_audio"
  | "transcribing"
  | "writing_output_files"
  | "paused"
  | "done"
  | "error";

interface StudioStore {
  initialized: boolean;
  isLoading: boolean;
  isPreparing: boolean;
  isStarting: boolean;
  recentJobs: StudioJob[];
  preparedJob: StudioJob | null;
  activeJob: StudioJob | null;
  statusMessage: string | null;
  currentStage: StudioStage;
  error: string | null;
  selectedFormats: StudioFormat[];
  lastOutputFolder: string;
  initialize: () => Promise<void>;
  refreshHome: () => Promise<void>;
  prepareFile: (filePath: string) => Promise<StudioJob>;
  startPreparedJob: (config: StartStudioJobConfig) => Promise<void>;
  cancelActiveJob: () => Promise<void>;
  openOutputFolder: (jobId: string) => Promise<void>;
  deleteJob: (jobId: string) => Promise<void>;
  retryJob: (jobId: string) => Promise<void>;
  clearPreparedJob: () => void;
  setSelectedFormats: (formats: StudioFormat[]) => void;
  setLastOutputFolder: (folder: string) => void;
}

const upsertJob = (jobs: StudioJob[], nextJob: StudioJob): StudioJob[] => {
  const remaining = jobs.filter((job) => job.id !== nextJob.id);
  return [nextJob, ...remaining].sort((a, b) => b.created_at - a.created_at);
};

export const useStudioStore = create<StudioStore>((set, get) => ({
  initialized: false,
  isLoading: true,
  isPreparing: false,
  isStarting: false,
  recentJobs: [],
  preparedJob: null,
  activeJob: null,
  statusMessage: null,
  currentStage: "idle",
  error: null,
  selectedFormats: readStoredFormats(),
  lastOutputFolder: localStorage.getItem(LAST_OUTPUT_FOLDER_KEY) ?? "",

  initialize: async () => {
    if (get().initialized) return;

    await get().refreshHome();

    listen<StudioJobProgressEvent>("studio-job-progress", async (event) => {
      const payload = event.payload;
      set((state) => {
        if (!state.activeJob || state.activeJob.id !== payload.job_id) {
          return {
            currentStage:
              payload.stage === "writing_output_files"
                ? "writing_output_files"
                : "transcribing",
            statusMessage: payload.message,
          };
        }

        return {
          activeJob: {
            ...state.activeJob,
            chunk_count: payload.chunk_count,
            chunks_completed: payload.chunks_completed,
            status:
              payload.stage === "writing_output_files" ? "running" : "running",
          },
          currentStage:
            payload.stage === "preparing_audio"
              ? "preparing_audio"
              : payload.stage === "writing_output_files"
                ? "writing_output_files"
                : "transcribing",
          statusMessage: payload.message,
        };
      });

      const job = await studioApi.getJob(payload.job_id);
      if (job) {
        set((state) => ({
          activeJob:
            state.activeJob?.id === payload.job_id ? job : state.activeJob,
          recentJobs: upsertJob(state.recentJobs, job),
        }));
      }
    });

    listen<StudioJobPreviewEvent>("studio-job-preview", (event) => {
      const payload = event.payload;
      set((state) => {
        if (!state.activeJob || state.activeJob.id !== payload.job_id) {
          return {};
        }

        const transcript_text = state.activeJob.transcript_text.trim().length
          ? `${state.activeJob.transcript_text}\n\n${payload.appended_text}`
          : payload.appended_text;

        return {
          activeJob: {
            ...state.activeJob,
            transcript_text,
          },
        };
      });
    });

    listen<{ job_id: string; reason: string }>("studio-job-paused", (event) => {
      set((state) => ({
        currentStage: "paused",
        statusMessage: "Paused while dictation is running",
        activeJob:
          state.activeJob?.id === event.payload.job_id
            ? { ...state.activeJob, status: "paused" }
            : state.activeJob,
      }));
    });

    listen<{ job_id: string }>("studio-job-resumed", (event) => {
      set((state) => ({
        currentStage: "transcribing",
        statusMessage:
          state.statusMessage === "Paused while dictation is running"
            ? "Resuming Studio"
            : state.statusMessage,
        activeJob:
          state.activeJob?.id === event.payload.job_id
            ? { ...state.activeJob, status: "running" }
            : state.activeJob,
      }));
    });

    listen<{ job_id: string; output_files: StudioJob["output_files"] }>(
      "studio-job-completed",
      async (event) => {
        const job = await studioApi.getJob(event.payload.job_id);
        if (!job) return;
        set((state) => ({
          activeJob: job,
          preparedJob: null,
          currentStage: "done",
          statusMessage: "Done",
          recentJobs: upsertJob(state.recentJobs, job),
        }));
      },
    );

    listen<{ job_id: string; error: string }>("studio-job-failed", async (event) => {
      const job = await studioApi.getJob(event.payload.job_id);
      set((state) => ({
        activeJob: job ?? state.activeJob,
        preparedJob: null,
        currentStage: "error",
        error: event.payload.error,
        statusMessage: "This file could not be processed",
        recentJobs: job ? upsertJob(state.recentJobs, job) : state.recentJobs,
      }));
    });

    listen<{ job_id: string }>("studio-job-cancelled", async (event) => {
      const job = await studioApi.getJob(event.payload.job_id);
      set((state) => ({
        activeJob: job ?? state.activeJob,
        preparedJob: null,
        currentStage: "idle",
        statusMessage: null,
        recentJobs: job ? upsertJob(state.recentJobs, job) : state.recentJobs,
      }));
    });

    set({ initialized: true });
  },

  refreshHome: async () => {
    const home = await studioApi.listJobs();
    const activeJob =
      home.jobs.find((job) => job.status === "running" || job.status === "paused") ??
      null;
    set({
      isLoading: false,
      recentJobs: home.jobs,
      activeJob,
    });
  },

  prepareFile: async (filePath: string) => {
    set({
      isPreparing: true,
      error: null,
      statusMessage: null,
      currentStage: "idle",
    });
    try {
      const job = await studioApi.prepareJob(filePath);
      set((state) => ({
        preparedJob: job,
        activeJob: null,
        currentStage: "ready",
        recentJobs: upsertJob(state.recentJobs, job),
      }));
      return job;
    } finally {
      set({ isPreparing: false });
    }
  },

  startPreparedJob: async (config: StartStudioJobConfig) => {
    const preparedJob = get().preparedJob;
    if (!preparedJob) {
      throw new Error("No Studio job is ready to start");
    }

    set({
      isStarting: true,
      error: null,
      currentStage: "preparing_audio",
      statusMessage: "Preparing audio",
    });

    try {
      await studioApi.startJob(preparedJob.id, config);
      localStorage.setItem(LAST_OUTPUT_FOLDER_KEY, config.output_folder);
      localStorage.setItem(LAST_FORMATS_KEY, JSON.stringify(config.output_formats));
      set((state) => ({
        isStarting: false,
        preparedJob: null,
        activeJob: {
          ...preparedJob,
          output_folder: config.output_folder,
          output_formats: config.output_formats,
          status: "running",
        },
        selectedFormats: config.output_formats,
        lastOutputFolder: config.output_folder,
        recentJobs: upsertJob(state.recentJobs, {
          ...preparedJob,
          output_folder: config.output_folder,
          output_formats: config.output_formats,
          status: "running",
        }),
      }));
    } catch (error) {
      set({
        isStarting: false,
        currentStage: "error",
        error: error instanceof Error ? error.message : String(error),
      });
      throw error;
    }
  },

  cancelActiveJob: async () => {
    const activeJob = get().activeJob;
    if (!activeJob) return;
    await studioApi.cancelJob(activeJob.id);
  },

  openOutputFolder: async (jobId: string) => {
    await studioApi.openOutputFolder(jobId);
  },

  deleteJob: async (jobId: string) => {
    await studioApi.deleteJob(jobId);
    set((state) => ({
      recentJobs: state.recentJobs.filter((job) => job.id !== jobId),
      preparedJob: state.preparedJob?.id === jobId ? null : state.preparedJob,
      activeJob: state.activeJob?.id === jobId ? null : state.activeJob,
    }));
  },

  retryJob: async (jobId: string) => {
    set({
      error: null,
      currentStage: "preparing_audio",
      statusMessage: "Preparing audio",
    });
    await studioApi.retryJob(jobId);
    const job = await studioApi.getJob(jobId);
    set((state) => ({
      activeJob: job ?? state.activeJob,
      preparedJob: null,
      recentJobs: job ? upsertJob(state.recentJobs, job) : state.recentJobs,
    }));
  },

  clearPreparedJob: () => set({ preparedJob: null, currentStage: "idle", error: null }),

  setSelectedFormats: (formats) => set({ selectedFormats: formats }),

  setLastOutputFolder: (folder) => set({ lastOutputFolder: folder }),
}));
