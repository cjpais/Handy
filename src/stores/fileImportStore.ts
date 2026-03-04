import { create } from "zustand";
import { produce } from "immer";

export type FileImportProgress = {
  stage: string;
  message: string;
  percent: number;
  done: boolean;
  source_sample_rate?: number;
  sample_rate?: number;
  channels?: number;
  duration_sec?: number;
  source_bitrate_kbps?: number;
};

/** Maps backend progress stage names to i18n keys for toast/banner messages. */
export const STAGE_I18N_KEYS: Record<string, string> = {
  starting: "toasts.fileImport.preparing",
  loading_model: "toasts.fileImport.loadingModel",
  decoding: "toasts.fileImport.decoding",
  transcribing: "toasts.fileImport.transcribing",
  saving: "toasts.fileImport.saving",
  finalizing: "toasts.fileImport.finalizing",
};

interface FileImportStore {
  isRunning: boolean;
  sourcePath: string | null;
  fileName: string | null;
  stage: string | null;
  message: string | null;
  percent: number;
  error: string | null;
  sourceSampleRate: number | null;
  sampleRate: number | null;
  channels: number | null;
  durationSec: number | null;
  bitrateKbps: number | null;
  startedAt: number | null;
  completedAt: number | null;
  start: (path: string) => void;
  updateFromProgress: (progress: FileImportProgress) => void;
  finishSuccess: () => void;
  finishError: (message: string) => void;
  clearError: () => void;
  reset: () => void;
}

const basenameFromPath = (path: string): string => {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/");
  return parts[parts.length - 1] || path;
};

export const useFileImportStore = create<FileImportStore>()((set) => ({
  isRunning: false,
  sourcePath: null,
  fileName: null,
  stage: null,
  message: null,
  percent: 0,
  error: null,
  sourceSampleRate: null,
  sampleRate: null,
  channels: null,
  durationSec: null,
  bitrateKbps: null,
  startedAt: null,
  completedAt: null,

  start: (path) =>
    set(
      produce((state: FileImportStore) => {
        state.isRunning = true;
        state.sourcePath = path;
        state.fileName = basenameFromPath(path);
        state.stage = "starting";
        state.message = null;
        state.percent = 0;
        state.error = null;
        state.sourceSampleRate = null;
        state.sampleRate = null;
        state.channels = null;
        state.durationSec = null;
        state.bitrateKbps = null;
        state.startedAt = Date.now();
        state.completedAt = null;
      }),
    ),

  updateFromProgress: (progress) =>
    set(
      produce((state: FileImportStore) => {
        state.stage = progress.stage;
        state.message = progress.message;
        state.percent = Math.max(
          0,
          Math.min(100, Math.round(progress.percent)),
        );
        if (progress.source_sample_rate != null) {
          state.sourceSampleRate = progress.source_sample_rate;
        }
        if (progress.sample_rate != null) {
          state.sampleRate = progress.sample_rate;
        }
        if (progress.channels != null) {
          state.channels = progress.channels;
        }
        if (progress.duration_sec != null) {
          state.durationSec = progress.duration_sec;
        }
        if (progress.source_bitrate_kbps != null) {
          state.bitrateKbps = progress.source_bitrate_kbps;
        }
        if (progress.done) {
          state.isRunning = false;
          state.completedAt = Date.now();
          if (progress.stage === "failed") {
            state.error = progress.message || null;
          }
        }
      }),
    ),

  finishSuccess: () =>
    set(
      produce((state: FileImportStore) => {
        state.isRunning = false;
        state.stage = "completed";
        state.message = null;
        state.percent = 100;
        state.error = null;
        state.completedAt = Date.now();
      }),
    ),

  finishError: (message) =>
    set(
      produce((state: FileImportStore) => {
        state.isRunning = false;
        state.stage = "failed";
        state.message = message;
        state.error = message;
        state.completedAt = Date.now();
      }),
    ),

  clearError: () =>
    set(
      produce((state: FileImportStore) => {
        state.error = null;
      }),
    ),

  reset: () =>
    set({
      isRunning: false,
      sourcePath: null,
      fileName: null,
      stage: null,
      message: null,
      percent: 0,
      error: null,
      sourceSampleRate: null,
      sampleRate: null,
      channels: null,
      durationSec: null,
      bitrateKbps: null,
      startedAt: null,
      completedAt: null,
    }),
}));
