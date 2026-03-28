export type StudioFormat = "txt" | "srt" | "vtt";
export type StudioJobStatus =
  | "pending"
  | "running"
  | "paused"
  | "done"
  | "error"
  | "cancelled";

export interface StudioOutputFile {
  format: StudioFormat | string;
  output_path: string;
  file_name: string;
}

export interface StudioJob {
  id: string;
  source_path: string;
  source_name: string;
  source_dir: string | null;
  working_wav_path: string | null;
  media_duration_ms: number;
  file_size_bytes: number;
  container_format: string | null;
  audio_codec: string | null;
  audio_sample_rate_hz: number | null;
  status: StudioJobStatus;
  model_id: string;
  language: string;
  output_folder: string | null;
  output_formats: string[];
  settings_fingerprint: string;
  chunk_count: number;
  chunks_completed: number;
  transcript_text: string;
  error_message: string | null;
  created_at: number;
  updated_at: number;
  completed_at: number | null;
  output_files: StudioOutputFile[];
  estimate_min_minutes: number | null;
  estimate_max_minutes: number | null;
}

export interface StudioHomeData {
  jobs: StudioJob[];
}

export interface StartStudioJobConfig {
  output_folder: string;
  output_formats: StudioFormat[];
}

export interface StudioJobProgressEvent {
  job_id: string;
  chunks_completed: number;
  chunk_count: number;
  stage: string;
  message: string;
  preparation_progress?: number | null;
}

export interface StudioJobPreviewEvent {
  job_id: string;
  appended_text: string;
}
