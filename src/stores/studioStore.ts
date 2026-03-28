import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import i18n from "@/i18n";
import { studioApi } from "@/lib/studioApi";
import type {
  StartStudioJobConfig,
  StudioFormat,
  StudioJob,
  StudioJobPreviewEvent,
  StudioJobProgressEvent,
} from "@/lib/types/studio";

// Studio preferences are stored in localStorage rather than tauri-plugin-store
// because they are lightweight, non-critical defaults (last output folder and
// format selection) that don't need to survive a WebView cache clear.
const LAST_OUTPUT_FOLDER_KEY = "studio:last-output-folder";
const LAST_FORMATS_KEY = "studio:last-formats";

const DEFAULT_FORMATS: StudioFormat[] = ["txt", "srt"];
const FALLBACK_EXTENSIONS = ["mp3", "wav", "m4a", "flac", "ogg"];
let studioInitPromise: Promise<void> | null = null;
let listenersRegistered = false;

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
  | "opening_file"
  | "decoding_audio"
  | "resampling_audio"
  | "writing_normalized_audio"
  | "building_chunks"
  | "transcribing"
  | "writing_output_files"
  | "stopping"
  | "paused"
  | "done"
  | "error";

const mapStage = (stage: string): StudioStage => {
  switch (stage) {
    case "opening_file":
    case "decoding_audio":
    case "resampling_audio":
    case "writing_normalized_audio":
    case "building_chunks":
    case "preparing_audio":
    case "transcribing":
    case "writing_output_files":
    case "stopping":
    case "paused":
    case "done":
    case "error":
      return stage;
    default:
      return "transcribing";
  }
};

interface StudioStore {
  initialized: boolean;
  isLoading: boolean;
  isPreparing: boolean;
  isStarting: boolean;
  supportedExtensions: string[];
  recentJobs: StudioJob[];
  preparedJob: StudioJob | null;
  activeJob: StudioJob | null;
  statusMessage: string | null;
  currentStage: StudioStage;
  preparationProgress: number | null;
  error: string | null;
  preparedJobOrigin: "recent" | "new" | null;
  selectedRecentJobId: string | null;
  preparedJobSelectionToken: number;
  selectedJob: StudioJob | null;
  selectedFormats: StudioFormat[];
  lastOutputFolder: string;
  initialize: () => Promise<void>;
  refreshHome: () => Promise<void>;
  prepareFile: (filePath: string) => Promise<StudioJob>;
  startPreparedJob: (config: StartStudioJobConfig) => Promise<void>;
  cancelActiveJob: () => Promise<void>;
  openOutputFolder: (jobId: string) => Promise<void>;
  deleteJob: (jobId: string) => Promise<void>;
  clearRecentJobs: (jobIds: string[]) => Promise<void>;
  retryJob: (jobId: string) => Promise<void>;
  selectRecentJob: (jobId: string) => Promise<void>;
  returnToActiveJob: () => void;
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
  supportedExtensions: FALLBACK_EXTENSIONS,
  recentJobs: [],
  preparedJob: null,
  activeJob: null,
  statusMessage: null,
  currentStage: "idle",
  preparationProgress: null,
  error: null,
  preparedJobOrigin: null,
  selectedRecentJobId: null,
  preparedJobSelectionToken: 0,
  selectedJob: null,
  selectedFormats: readStoredFormats(),
  lastOutputFolder: localStorage.getItem(LAST_OUTPUT_FOLDER_KEY) ?? "",

  initialize: async () => {
    if (get().initialized) return;
    if (studioInitPromise) return studioInitPromise;

    studioInitPromise = (async () => {
      await get().refreshHome();

      studioApi
        .getSupportedExtensions()
        .then((extensions) => {
          if (extensions.length > 0) set({ supportedExtensions: extensions });
        })
        .catch(() => {});

      if (listenersRegistered) {
        set({ initialized: true });
        return;
      }

      await Promise.all([
        listen<StudioJobProgressEvent>("studio-job-progress", (event) => {
          const payload = event.payload;
          const stage = mapStage(payload.stage);
          set((state) => {
            const nextStatus =
              stage === "paused"
                ? "paused"
                : stage === "done"
                  ? "done"
                  : "running";
            const updateJob = (job: StudioJob): StudioJob => ({
              ...job,
              chunk_count:
                payload.chunk_count > 0 ? payload.chunk_count : job.chunk_count,
              chunks_completed:
                payload.chunk_count > 0
                  ? payload.chunks_completed
                  : job.chunks_completed,
              status: nextStatus,
            });

            if (!state.activeJob || state.activeJob.id !== payload.job_id) {
              return {
                currentStage: stage,
                statusMessage: payload.message,
                preparationProgress:
                  payload.preparation_progress ??
                  (stage === "transcribing" || stage === "writing_output_files"
                    ? null
                    : 0),
                recentJobs: state.recentJobs.map((job) =>
                  job.id === payload.job_id ? updateJob(job) : job,
                ),
              };
            }

            return {
              activeJob: updateJob(state.activeJob),
              selectedJob:
                state.selectedJob?.id === payload.job_id
                  ? updateJob(state.selectedJob)
                  : state.selectedJob,
              recentJobs: state.recentJobs.map((job) =>
                job.id === payload.job_id ? updateJob(job) : job,
              ),
              currentStage: stage,
              statusMessage: payload.message,
              preparationProgress:
                payload.preparation_progress ??
                (stage === "transcribing" || stage === "writing_output_files"
                  ? null
                  : 0),
            };
          });
        }),

        listen<StudioJobPreviewEvent>("studio-job-preview", (event) => {
          const payload = event.payload;
          set((state) => {
            if (!state.activeJob || state.activeJob.id !== payload.job_id) {
              return {};
            }

            const transcript_text = state.activeJob.transcript_text.trim()
              .length
              ? `${state.activeJob.transcript_text}\n\n${payload.appended_text}`
              : payload.appended_text;

            return {
              activeJob: {
                ...state.activeJob,
                transcript_text,
              },
              selectedJob:
                state.selectedJob?.id === payload.job_id
                  ? {
                      ...state.selectedJob,
                      transcript_text,
                    }
                  : state.selectedJob,
            };
          });
        }),

        listen<{ job_id: string; reason: string }>(
          "studio-job-paused",
          (event) => {
            set((state) => ({
              currentStage: "paused",
              statusMessage: i18n.t("studio.job.status.pausedForDictation", {
                defaultValue: "Paused while dictation is running",
              }),
              preparationProgress: null,
              activeJob:
                state.activeJob?.id === event.payload.job_id
                  ? { ...state.activeJob, status: "paused" }
                  : state.activeJob,
              selectedJob:
                state.selectedJob?.id === event.payload.job_id
                  ? { ...state.selectedJob, status: "paused" }
                  : state.selectedJob,
            }));
          },
        ),

        listen<{ job_id: string }>("studio-job-resumed", (event) => {
          set((state) => ({
            currentStage: "transcribing",
            statusMessage:
              state.statusMessage ===
              i18n.t("studio.job.status.pausedForDictation", {
                defaultValue: "Paused while dictation is running",
              })
                ? i18n.t("studio.job.status.resuming", {
                    defaultValue: "Resuming Studio",
                  })
                : state.statusMessage,
            preparationProgress: null,
            activeJob:
              state.activeJob?.id === event.payload.job_id
                ? { ...state.activeJob, status: "running" }
                : state.activeJob,
            selectedJob:
              state.selectedJob?.id === event.payload.job_id
                ? { ...state.selectedJob, status: "running" }
                : state.selectedJob,
          }));
        }),

        listen<{ job_id: string; output_files: StudioJob["output_files"] }>(
          "studio-job-completed",
          async (event) => {
            const job = await studioApi.getJob(event.payload.job_id);
            if (!job) return;
            set((state) => ({
              activeJob: job,
              selectedJob:
                state.selectedJob?.id === event.payload.job_id
                  ? job
                  : state.selectedJob,
              preparedJob: null,
              currentStage: "done",
              statusMessage: i18n.t("studio.statuses.done", {
                defaultValue: "Done",
              }),
              preparationProgress: null,
              recentJobs: upsertJob(state.recentJobs, job),
            }));
          },
        ),

        listen<{ job_id: string; error: string }>(
          "studio-job-failed",
          async (event) => {
            const job = await studioApi.getJob(event.payload.job_id);
            set((state) => ({
              activeJob: job ?? state.activeJob,
              selectedJob:
                state.selectedJob?.id === event.payload.job_id
                  ? (job ?? state.selectedJob)
                  : state.selectedJob,
              preparedJob: null,
              currentStage: "error",
              error: event.payload.error,
              statusMessage: i18n.t("studio.job.processError", {
                defaultValue: "This file could not be processed",
              }),
              preparationProgress: null,
              recentJobs: job
                ? upsertJob(state.recentJobs, job)
                : state.recentJobs,
            }));
          },
        ),

        listen<{ job_id: string }>("studio-job-cancelled", async (event) => {
          const job = await studioApi.getJob(event.payload.job_id);
          set((state) => ({
            activeJob: job ?? state.activeJob,
            selectedJob:
              state.selectedJob?.id === event.payload.job_id
                ? (job ?? state.selectedJob)
                : state.selectedJob,
            preparedJob: null,
            currentStage: "idle",
            statusMessage: null,
            preparationProgress: null,
            recentJobs: job
              ? upsertJob(state.recentJobs, job)
              : state.recentJobs,
          }));
        }),
      ]);

      listenersRegistered = true;
      set({ initialized: true });
    })().catch((error) => {
      studioInitPromise = null;
      listenersRegistered = false;
      set({ initialized: false });
      throw error;
    });

    return studioInitPromise;
  },

  refreshHome: async () => {
    const home = await studioApi.listJobs();
    const activeJob =
      home.jobs.find(
        (job) => job.status === "running" || job.status === "paused",
      ) ?? null;
    set({
      isLoading: false,
      recentJobs: home.jobs,
      activeJob,
      preparationProgress: null,
    });
  },

  prepareFile: async (filePath: string) => {
    set({
      isPreparing: true,
      error: null,
      statusMessage: null,
      currentStage: "idle",
      preparationProgress: null,
    });
    try {
      const job = await studioApi.prepareJob(filePath);
      set((state) => ({
        preparedJob: job,
        activeJob: state.activeJob,
        selectedJob: null,
        currentStage: "ready",
        preparedJobOrigin: "new",
        selectedRecentJobId: null,
        preparedJobSelectionToken: Date.now(),
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
      throw new Error(
        i18n.t("studio.errors.noPreparedJob", {
          defaultValue: "No Studio job is ready to start",
        }),
      );
    }

    set({
      isStarting: true,
      error: null,
      currentStage: "preparing_audio",
      statusMessage: i18n.t("studio.job.stage.preparingAudio", {
        defaultValue: "Preparing audio",
      }),
      preparationProgress: 0,
    });

    try {
      await studioApi.startJob(preparedJob.id, config);
      localStorage.setItem(LAST_OUTPUT_FOLDER_KEY, config.output_folder);
      localStorage.setItem(
        LAST_FORMATS_KEY,
        JSON.stringify(config.output_formats),
      );
      set((state) => ({
        isStarting: false,
        preparedJob: null,
        activeJob: {
          ...preparedJob,
          output_folder: config.output_folder,
          output_formats: config.output_formats,
          status: "running",
        },
        selectedJob: null,
        preparedJobOrigin: null,
        selectedRecentJobId: null,
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
        preparationProgress: null,
      });
      throw error;
    }
  },

  cancelActiveJob: async () => {
    const activeJob = get().activeJob;
    if (!activeJob) return;
    set({
      currentStage: "stopping",
      statusMessage: i18n.t("studio.job.status.stopping", {
        defaultValue: "Stopping...",
      }),
    });
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
      selectedJob: state.selectedJob?.id === jobId ? null : state.selectedJob,
      preparedJobOrigin:
        state.preparedJob?.id === jobId ? null : state.preparedJobOrigin,
      selectedRecentJobId:
        state.selectedRecentJobId === jobId ? null : state.selectedRecentJobId,
    }));
  },

  clearRecentJobs: async (jobIds: string[]) => {
    if (jobIds.length === 0) return;

    const results = await Promise.allSettled(
      jobIds.map(async (jobId) => {
        await studioApi.deleteJob(jobId);
        return jobId;
      }),
    );
    const deletedJobIds = results.flatMap((result) =>
      result.status === "fulfilled" ? [result.value] : [],
    );

    set((state) => ({
      recentJobs: state.recentJobs.filter(
        (job) => !deletedJobIds.includes(job.id),
      ),
      preparedJob:
        state.preparedJob && deletedJobIds.includes(state.preparedJob.id)
          ? null
          : state.preparedJob,
      activeJob:
        state.activeJob && deletedJobIds.includes(state.activeJob.id)
          ? null
          : state.activeJob,
      selectedJob:
        state.selectedJob && deletedJobIds.includes(state.selectedJob.id)
          ? null
          : state.selectedJob,
      preparedJobOrigin:
        state.preparedJob && deletedJobIds.includes(state.preparedJob.id)
          ? null
          : state.preparedJobOrigin,
      selectedRecentJobId:
        state.selectedRecentJobId &&
        deletedJobIds.includes(state.selectedRecentJobId)
          ? null
          : state.selectedRecentJobId,
    }));

    const failedCount = results.length - deletedJobIds.length;
    if (failedCount > 0) {
      throw new Error(
        failedCount === 1
          ? i18n.t("studio.recent.deleteOneFailed", {
              defaultValue: "One recent job could not be deleted.",
            })
          : i18n.t("studio.recent.deleteManyFailed", {
              defaultValue: "{{count}} recent jobs could not be deleted.",
              count: failedCount,
            }),
      );
    }
  },

  retryJob: async (jobId: string) => {
    set({
      error: null,
      currentStage: "preparing_audio",
      statusMessage: i18n.t("studio.job.stage.preparingAudio", {
        defaultValue: "Preparing audio",
      }),
      preparationProgress: 0,
    });
    await studioApi.retryJob(jobId);
    const job = await studioApi.getJob(jobId);
    set((state) => ({
      activeJob: job ?? state.activeJob,
      preparedJob: null,
      selectedJob: null,
      preparedJobOrigin: null,
      selectedRecentJobId: null,
      recentJobs: job ? upsertJob(state.recentJobs, job) : state.recentJobs,
    }));
  },

  selectRecentJob: async (jobId: string) => {
    const job = await studioApi.getJob(jobId);
    if (!job) {
      throw new Error(
        i18n.t("studio.errors.jobNotFound", {
          defaultValue: "Studio job not found",
        }),
      );
    }

    if (job.status === "pending") {
      set((state) => ({
        preparedJob: job,
        activeJob: state.activeJob,
        selectedJob: null,
        currentStage: "ready",
        statusMessage: null,
        preparationProgress: null,
        error: null,
        preparedJobOrigin: "recent",
        selectedRecentJobId: job.id,
        preparedJobSelectionToken: Date.now(),
        recentJobs: upsertJob(state.recentJobs, job),
      }));
      return;
    }

    set((state) => ({
      preparedJob: null,
      selectedJob: job,
      currentStage:
        job.status === "error"
          ? "error"
          : job.status === "cancelled"
            ? "error"
            : job.status === "done"
              ? "done"
              : job.status === "paused"
                ? "paused"
                : "transcribing",
      statusMessage: null,
      preparationProgress: null,
      error: null,
      preparedJobOrigin: null,
      selectedRecentJobId: job.id,
      preparedJobSelectionToken: Date.now(),
      recentJobs: upsertJob(state.recentJobs, job),
    }));
  },

  returnToActiveJob: () =>
    set((state) => ({
      preparedJob: null,
      selectedJob: null,
      preparedJobOrigin: null,
      selectedRecentJobId: state.activeJob?.id ?? null,
      preparedJobSelectionToken: Date.now(),
      currentStage:
        state.activeJob?.status === "paused"
          ? "paused"
          : state.activeJob?.status === "done"
            ? "done"
            : state.activeJob?.status === "error"
              ? "error"
              : "transcribing",
      error: null,
      statusMessage:
        state.activeJob?.status === "paused"
          ? i18n.t("studio.job.status.pausedForDictation", {
              defaultValue: "Paused while dictation is running",
            })
          : state.statusMessage,
      preparationProgress: null,
    })),

  clearPreparedJob: () =>
    set((state) => ({
      preparedJob: null,
      currentStage:
        state.activeJob?.status === "paused"
          ? "paused"
          : state.activeJob?.status === "running"
            ? "transcribing"
            : "idle",
      preparationProgress: null,
      error: null,
      selectedJob: null,
      preparedJobOrigin: null,
      selectedRecentJobId: null,
    })),

  setSelectedFormats: (formats) => set({ selectedFormats: formats }),

  setLastOutputFolder: (folder) => set({ lastOutputFolder: folder }),
}));
