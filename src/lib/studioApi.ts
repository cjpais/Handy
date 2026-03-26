import { invoke } from "@tauri-apps/api/core";
import type {
  StartStudioJobConfig,
  StudioHomeData,
  StudioJob,
} from "./types/studio";

export const studioApi = {
  prepareJob(filePath: string) {
    return invoke<StudioJob>("prepare_studio_job", { filePath });
  },
  startJob(jobId: string, config: StartStudioJobConfig) {
    return invoke<void>("start_studio_job", { jobId, config });
  },
  cancelJob(jobId: string) {
    return invoke<void>("cancel_studio_job", { jobId });
  },
  getJob(jobId: string) {
    return invoke<StudioJob | null>("get_studio_job", { jobId });
  },
  listJobs() {
    return invoke<StudioHomeData>("list_studio_jobs");
  },
  deleteJob(jobId: string) {
    return invoke<void>("delete_studio_job", { jobId });
  },
  openOutputFolder(jobId: string) {
    return invoke<void>("open_studio_output_folder", { jobId });
  },
  retryJob(jobId: string) {
    return invoke<void>("retry_studio_job", { jobId });
  },
};
