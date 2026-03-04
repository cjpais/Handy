use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

pub const FILE_TRANSCRIPTION_PROGRESS_EVENT: &str = "file-transcription-progress";

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct FileTranscriptionProgress {
    pub stage: String,
    pub message: String,
    pub percent: u8,
    pub done: bool,
    pub source_sample_rate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub duration_sec: Option<f32>,
    pub source_bitrate_kbps: Option<u32>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type)]
pub struct FileTranscriptionAudioMeta {
    pub source_sample_rate: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_sec: f32,
    pub source_bitrate_kbps: Option<u32>,
}

pub fn emit_file_transcription_progress(
    app_handle: &AppHandle,
    stage: &str,
    message: &str,
    percent: u8,
    done: bool,
) {
    let payload = FileTranscriptionProgress {
        stage: stage.to_string(),
        message: message.to_string(),
        percent,
        done,
        source_sample_rate: None,
        sample_rate: None,
        channels: None,
        duration_sec: None,
        source_bitrate_kbps: None,
    };
    let _ = app_handle.emit(FILE_TRANSCRIPTION_PROGRESS_EVENT, payload);
}

pub fn emit_file_transcription_progress_with_audio_meta(
    app_handle: &AppHandle,
    stage: &str,
    message: &str,
    percent: u8,
    done: bool,
    audio_meta: FileTranscriptionAudioMeta,
) {
    let payload = FileTranscriptionProgress {
        stage: stage.to_string(),
        message: message.to_string(),
        percent,
        done,
        source_sample_rate: Some(audio_meta.source_sample_rate),
        sample_rate: Some(audio_meta.sample_rate),
        channels: Some(audio_meta.channels),
        duration_sec: Some(audio_meta.duration_sec),
        source_bitrate_kbps: audio_meta.source_bitrate_kbps,
    };
    let _ = app_handle.emit(FILE_TRANSCRIPTION_PROGRESS_EVENT, payload);
}
