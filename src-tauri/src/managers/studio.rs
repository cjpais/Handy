use crate::audio_toolkit::read_wav_samples;
use crate::exporters::{srt, txt, vtt};
use crate::managers::model::{EngineType, ModelManager};
use crate::managers::transcription::TranscriptionManager;
use crate::media::decode;
use crate::settings::{get_settings, AppSettings, WhisperAcceleratorSetting};
use anyhow::{anyhow, bail, Result};
use chrono::{Local, TimeZone};
use log::{error, info, warn};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

const CHUNK_MS: i64 = 10_000;
const CHUNK_SAMPLE_RATE: usize = 16_000;
const CHUNK_OVERLAP_MS: i64 = 750;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

static MIGRATIONS: &[M] = &[M::up(
    "CREATE TABLE IF NOT EXISTS studio_jobs (
        id TEXT PRIMARY KEY,
        source_path TEXT NOT NULL,
        source_name TEXT NOT NULL,
        working_wav_path TEXT,
        media_duration_ms INTEGER NOT NULL,
        file_size_bytes INTEGER NOT NULL DEFAULT 0,
        container_format TEXT,
        audio_codec TEXT,
        audio_sample_rate_hz INTEGER,
        status TEXT NOT NULL,
        model_id TEXT NOT NULL,
        language TEXT NOT NULL,
        output_folder TEXT,
        output_formats TEXT NOT NULL DEFAULT '[]',
        settings_fingerprint TEXT NOT NULL DEFAULT '',
        chunk_count INTEGER NOT NULL DEFAULT 0,
        chunks_completed INTEGER NOT NULL DEFAULT 0,
        transcript_text TEXT NOT NULL DEFAULT '',
        error_message TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        completed_at INTEGER
    );

    CREATE TABLE IF NOT EXISTS studio_chunks (
        id TEXT PRIMARY KEY,
        job_id TEXT NOT NULL,
        chunk_index INTEGER NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        text TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        FOREIGN KEY(job_id) REFERENCES studio_jobs(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS studio_exports (
        id TEXT PRIMARY KEY,
        job_id TEXT NOT NULL,
        format TEXT NOT NULL,
        output_path TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        FOREIGN KEY(job_id) REFERENCES studio_jobs(id) ON DELETE CASCADE
    );",
)];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StudioJobStatus {
    Pending,
    Running,
    Paused,
    Done,
    Error,
    Cancelled,
}

impl StudioJobStatus {
    fn as_str(self) -> &'static str {
        match self {
            StudioJobStatus::Pending => "pending",
            StudioJobStatus::Running => "running",
            StudioJobStatus::Paused => "paused",
            StudioJobStatus::Done => "done",
            StudioJobStatus::Error => "error",
            StudioJobStatus::Cancelled => "cancelled",
        }
    }

    fn from_db(value: String) -> Self {
        match value.as_str() {
            "running" => StudioJobStatus::Running,
            "paused" => StudioJobStatus::Paused,
            "done" => StudioJobStatus::Done,
            "error" => StudioJobStatus::Error,
            "cancelled" => StudioJobStatus::Cancelled,
            _ => StudioJobStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StudioOutputFile {
    pub format: String,
    pub output_path: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StudioJob {
    pub id: String,
    pub source_path: String,
    pub source_name: String,
    pub source_dir: Option<String>,
    pub working_wav_path: Option<String>,
    pub media_duration_ms: i64,
    pub file_size_bytes: u64,
    pub container_format: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_sample_rate_hz: Option<u32>,
    pub status: StudioJobStatus,
    pub model_id: String,
    pub language: String,
    pub output_folder: Option<String>,
    pub output_formats: Vec<String>,
    pub settings_fingerprint: String,
    pub chunk_count: i64,
    pub chunks_completed: i64,
    pub transcript_text: String,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub output_files: Vec<StudioOutputFile>,
    pub estimate_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StudioHomeData {
    pub jobs: Vec<StudioJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StartStudioJobConfig {
    pub output_folder: String,
    pub output_formats: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StudioProgressEvent {
    job_id: String,
    chunks_completed: i64,
    chunk_count: i64,
    stage: String,
    message: String,
    preparation_progress: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct StudioPreviewEvent {
    job_id: String,
    appended_text: String,
}

#[derive(Debug, Clone)]
struct ChunkRow {
    chunk_index: i64,
    start_ms: i64,
    end_ms: i64,
    text: String,
    status: String,
}

#[derive(Clone)]
pub struct StudioManager {
    app_handle: AppHandle,
    db_path: PathBuf,
    jobs_dir: PathBuf,
    active_job_id: Arc<Mutex<Option<String>>>,
    cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

struct ActiveJobCleanup {
    active_job_id: Arc<Mutex<Option<String>>>,
    cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    job_id: String,
    armed: bool,
}

impl ActiveJobCleanup {
    fn new(manager: &StudioManager, job_id: impl Into<String>) -> Self {
        Self::from_state(
            manager.active_job_id.clone(),
            manager.cancel_flags.clone(),
            job_id,
        )
    }

    fn from_state(
        active_job_id: Arc<Mutex<Option<String>>>,
        cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
        job_id: impl Into<String>,
    ) -> Self {
        Self {
            active_job_id,
            cancel_flags,
            job_id: job_id.into(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ActiveJobCleanup {
    fn drop(&mut self) {
        if self.armed {
            clear_active_job_state(&self.active_job_id, &self.cancel_flags, &self.job_id);
        }
    }
}

impl StudioManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let studio_dir = crate::portable::app_data_dir(app_handle)?.join("studio");
        let jobs_dir = studio_dir.join("jobs");
        let db_path = studio_dir.join("studio.db");
        fs::create_dir_all(&jobs_dir)?;

        let manager = Self {
            app_handle: app_handle.clone(),
            db_path,
            jobs_dir,
            active_job_id: Arc::new(Mutex::new(None)),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
        };

        manager.init_database()?;
        manager.recover_interrupted_jobs()?;
        Ok(manager)
    }

    fn init_database(&self) -> Result<()> {
        let mut conn = Connection::open(&self.db_path)?;
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        #[cfg(debug_assertions)]
        migrations.validate().expect("Invalid Studio migrations");
        migrations.to_latest(&mut conn)?;
        Ok(())
    }

    fn recover_interrupted_jobs(&self) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute(
            "UPDATE studio_jobs
             SET status = ?1,
                 error_message = CASE
                     WHEN error_message IS NULL OR error_message = '' THEN ?2
                     ELSE error_message
                 END,
                 updated_at = ?3
             WHERE status IN ('running', 'paused')",
            params![
                StudioJobStatus::Cancelled.as_str(),
                "Studio stopped before this job finished. Retry to continue.",
                Self::now_ms()
            ],
        )?;
        Ok(())
    }

    fn get_connection(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    fn create_id(prefix: &str) -> String {
        let suffix = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}-{}-{suffix}", Self::now_ms())
    }

    fn map_job(&self, row: &rusqlite::Row<'_>) -> rusqlite::Result<StudioJob> {
        let id: String = row.get("id")?;
        let source_path: String = row.get("source_path")?;
        Ok(StudioJob {
            id: id.clone(),
            source_path: source_path.clone(),
            source_name: row.get("source_name")?,
            source_dir: Path::new(&source_path)
                .parent()
                .map(|parent| parent.to_string_lossy().to_string()),
            working_wav_path: row.get("working_wav_path")?,
            media_duration_ms: row.get("media_duration_ms")?,
            file_size_bytes: row.get::<_, i64>("file_size_bytes")?.max(0) as u64,
            container_format: row.get("container_format")?,
            audio_codec: row.get("audio_codec")?,
            audio_sample_rate_hz: row
                .get::<_, Option<i64>>("audio_sample_rate_hz")?
                .map(|value| value.max(0) as u32),
            status: StudioJobStatus::from_db(row.get("status")?),
            model_id: row.get("model_id")?,
            language: row.get("language")?,
            output_folder: row.get("output_folder")?,
            output_formats: serde_json::from_str::<Vec<String>>(
                &row.get::<_, String>("output_formats")?,
            )
            .unwrap_or_default(),
            settings_fingerprint: row.get("settings_fingerprint")?,
            chunk_count: row.get("chunk_count")?,
            chunks_completed: row.get("chunks_completed")?,
            transcript_text: row.get("transcript_text")?,
            error_message: row.get("error_message")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            completed_at: row.get("completed_at")?,
            output_files: self.get_output_files(&id).unwrap_or_default(),
            estimate_text: None,
        })
    }

    fn get_output_files(&self, job_id: &str) -> Result<Vec<StudioOutputFile>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT format, output_path FROM studio_exports WHERE job_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![job_id], |row| {
            let output_path: String = row.get("output_path")?;
            let file_name = Path::new(&output_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string();
            Ok(StudioOutputFile {
                format: row.get("format")?,
                output_path,
                file_name,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn list_jobs(&self) -> Result<StudioHomeData> {
        let conn = self.get_connection()?;
        let mut stmt =
            conn.prepare("SELECT * FROM studio_jobs ORDER BY created_at DESC LIMIT 20")?;
        let rows = stmt.query_map([], |row| self.map_job(row))?;
        let mut jobs = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        for job in &mut jobs {
            job.estimate_text = Some(self.estimate_for_job(job));
        }
        Ok(StudioHomeData { jobs })
    }

    pub fn get_job(&self, job_id: &str) -> Result<Option<StudioJob>> {
        let conn = self.get_connection()?;
        let mut job = conn
            .query_row(
                "SELECT * FROM studio_jobs WHERE id = ?1",
                params![job_id],
                |row| self.map_job(row),
            )
            .optional()?;
        if let Some(job_ref) = job.as_mut() {
            job_ref.estimate_text = Some(self.estimate_for_job(job_ref));
        }
        Ok(job)
    }

    pub fn prepare_job(&self, file_path: &str) -> Result<StudioJob> {
        let input_path = PathBuf::from(file_path);
        if !input_path.exists() {
            bail!("The selected file does not exist");
        }
        if !input_path.is_file() {
            bail!("The selected path is not a file");
        }

        let metadata = decode::probe(&input_path)?;
        let settings = get_settings(&self.app_handle);
        let job_id = Self::create_id("job");
        let now = Self::now_ms();
        let source_name = input_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("Could not read file name"))?
            .to_string();

        let conn = self.get_connection()?;
        conn.execute(
            "INSERT INTO studio_jobs (
                id, source_path, source_name, working_wav_path, media_duration_ms,
                file_size_bytes, container_format, audio_codec, audio_sample_rate_hz,
                status, model_id, language, output_folder, output_formats,
                settings_fingerprint, chunk_count, chunks_completed, transcript_text,
                error_message, created_at, updated_at, completed_at
            ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, '[]', '', 0, 0, '', NULL, ?12, ?12, NULL)",
            params![
                &job_id,
                input_path.to_string_lossy().to_string(),
                &source_name,
                metadata.duration_ms,
                metadata.file_size_bytes as i64,
                metadata.container_format,
                metadata.audio_codec,
                metadata.audio_sample_rate_hz.map(|value| value as i64),
                StudioJobStatus::Pending.as_str(),
                settings.selected_model,
                settings.selected_language,
                now,
            ],
        )?;

        let mut job = self
            .get_job(&job_id)?
            .ok_or_else(|| anyhow!("Failed to load prepared Studio job"))?;
        job.estimate_text = Some(self.estimate_for_job(&job));

        let _ = self.app_handle.emit(
            "studio-job-created",
            serde_json::json!({
                "job_id": job.id,
                "file_name": job.source_name,
                "duration_ms": job.media_duration_ms,
                "estimate_text": job.estimate_text,
            }),
        );

        Ok(job)
    }

    pub fn start_job(&self, job_id: &str, config: StartStudioJobConfig) -> Result<()> {
        self.start_or_retry_job(job_id, config, false)
    }

    pub fn retry_job(&self, job_id: &str) -> Result<()> {
        let job = self
            .get_job(job_id)?
            .ok_or_else(|| anyhow!("Studio job not found"))?;
        if !can_retry_with_available_source(
            job.working_wav_path.as_deref(),
            Path::new(&job.source_path),
        ) {
            bail!("The original source file is missing. Re-select the file to start again.");
        }
        let output_folder = job
            .output_folder
            .clone()
            .ok_or_else(|| anyhow!("This Studio job has no output folder saved"))?;
        self.start_or_retry_job(
            job_id,
            StartStudioJobConfig {
                output_folder,
                output_formats: job.output_formats,
            },
            true,
        )
    }

    pub fn cancel_job(&self, job_id: &str) -> Result<()> {
        let flags = self.cancel_flags.lock().unwrap();
        let flag = flags
            .get(job_id)
            .ok_or_else(|| anyhow!("Studio job is not currently running"))?;
        flag.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn delete_job(&self, job_id: &str) -> Result<()> {
        if self.active_job_id.lock().unwrap().as_deref() == Some(job_id) {
            bail!("Cancel the active Studio job before deleting it");
        }

        let conn = self.get_connection()?;
        conn.execute(
            "DELETE FROM studio_exports WHERE job_id = ?1",
            params![job_id],
        )?;
        conn.execute(
            "DELETE FROM studio_chunks WHERE job_id = ?1",
            params![job_id],
        )?;
        conn.execute("DELETE FROM studio_jobs WHERE id = ?1", params![job_id])?;

        let work_dir = self.jobs_dir.join(job_id);
        if work_dir.exists() {
            fs::remove_dir_all(work_dir)?;
        }

        Ok(())
    }

    pub fn open_output_folder(&self, job_id: &str) -> Result<()> {
        let job = self
            .get_job(job_id)?
            .ok_or_else(|| anyhow!("Studio job not found"))?;
        let output_folder = job
            .output_folder
            .clone()
            .ok_or_else(|| anyhow!("Studio job has no output folder"))?;

        self.app_handle
            .opener()
            .open_path(output_folder, None::<String>)
            .map_err(|error| anyhow!("Failed to open Studio output folder: {}", error))
    }

    fn estimate_for_job(&self, job: &StudioJob) -> String {
        let settings = get_settings(&self.app_handle);
        let mut factor = match job.model_id.as_str() {
            "large" | "breeze-asr" => 0.24,
            "medium" | "turbo" => 0.18,
            "small" => 0.14,
            _ => match self
                .app_handle
                .state::<Arc<ModelManager>>()
                .get_model_info(&job.model_id)
                .map(|model| model.engine_type)
            {
                Some(EngineType::Parakeet)
                | Some(EngineType::Moonshine)
                | Some(EngineType::MoonshineStreaming)
                | Some(EngineType::SenseVoice) => 0.12,
                Some(EngineType::Canary) => 0.16,
                Some(EngineType::GigaAM) => 0.18,
                _ => 0.20,
            },
        };

        if settings.whisper_accelerator == WhisperAcceleratorSetting::Cpu {
            factor *= 2.0;
        }

        let seconds = ((job.media_duration_ms.max(0) as f64) / 1000.0) * factor + 15.0;
        let min = (seconds / 60.0).floor().max(1.0);
        let max = (min + 2.0).ceil();
        format!("About {:.0} to {:.0} minutes", min, max)
    }

    fn set_active_job(&self, job_id: &str) -> Result<Arc<AtomicBool>> {
        let mut active = self.active_job_id.lock().unwrap();
        if active.as_deref().is_some() && active.as_deref() != Some(job_id) {
            bail!("Studio already has an active job");
        }

        active.replace(job_id.to_string());
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel_flags
            .lock()
            .unwrap()
            .insert(job_id.to_string(), flag.clone());
        Ok(flag)
    }

    fn clear_active_job(&self, job_id: &str) {
        clear_active_job_state(&self.active_job_id, &self.cancel_flags, job_id);
    }

    fn start_or_retry_job(
        &self,
        job_id: &str,
        config: StartStudioJobConfig,
        resume_requested: bool,
    ) -> Result<()> {
        if config.output_formats.is_empty() {
            bail!("Select at least one output format");
        }

        let cancel_flag = self.set_active_job(job_id)?;
        let mut setup_cleanup = ActiveJobCleanup::new(self, job_id);
        let setup_result = (|| -> Result<()> {
            let now = Self::now_ms();
            let output_formats_json = serde_json::to_string(&config.output_formats)?;

            let conn = self.get_connection()?;
            conn.execute(
                "UPDATE studio_jobs
                 SET output_folder = ?1, output_formats = ?2, status = ?3, error_message = NULL, updated_at = ?4
                 WHERE id = ?5",
                params![
                    &config.output_folder,
                    output_formats_json,
                    StudioJobStatus::Running.as_str(),
                    now,
                    job_id
                ],
            )?;

            Ok(())
        })();

        if let Err(error) = setup_result {
            return Err(error);
        }
        setup_cleanup.disarm();

        let manager = self.clone();
        let job_id = job_id.to_string();
        std::thread::spawn(move || {
            let thread_cleanup = ActiveJobCleanup::new(&manager, job_id.clone());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                manager.run_job(&job_id, resume_requested, cancel_flag.clone())
            }));

            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    manager.report_job_failure(&job_id, error.to_string());
                }
                Err(payload) => {
                    manager.report_job_failure(&job_id, panic_payload_to_message(payload));
                }
            }
            drop(thread_cleanup);
        });

        Ok(())
    }

    fn run_job(
        &self,
        job_id: &str,
        resume_requested: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let settings_snapshot = get_settings(&self.app_handle);
        if settings_snapshot.selected_model.trim().is_empty() {
            bail!("Select a model before starting Studio");
        }

        let current_fingerprint = settings_fingerprint(&settings_snapshot)?;
        let job = self
            .get_job(job_id)?
            .ok_or_else(|| anyhow!("Studio job not found"))?;

        self.ensure_model_ready(&settings_snapshot)?;

        let work_dir = self.jobs_dir.join(job_id);
        let working_wav_path = work_dir.join("normalized.wav");
        let should_resume = resume_requested
            && !job.settings_fingerprint.is_empty()
            && job.settings_fingerprint == current_fingerprint
            && working_wav_path.exists()
            && job.chunk_count > 0;

        if !should_resume {
            self.reset_job_storage(job_id, &work_dir)?;
            self.emit_progress(job_id, 0, 0, "preparing_audio", "Preparing audio", Some(0));
            let normalization_result = decode::normalize_to_wav_with_progress(
                Path::new(&job.source_path),
                &working_wav_path,
                |stage, message, progress| {
                    self.emit_progress(job_id, 0, 0, stage, message, progress.map(i64::from))
                },
                || cancel_flag.load(Ordering::Relaxed),
            );
            if let Err(error) = normalization_result {
                if decode::is_cancelled(&error) {
                    self.finish_cancelled_job(job_id)?;
                    return Ok(());
                }
                return Err(error);
            }

            let samples = read_wav_samples(&working_wav_path)?;
            if cancel_flag.load(Ordering::Relaxed) {
                self.finish_cancelled_job(job_id)?;
                return Ok(());
            }

            self.emit_progress(
                job_id,
                0,
                0,
                "building_chunks",
                "Building chunks",
                Some(100),
            );
            let chunks = build_chunks(samples.len());
            self.store_chunks(job_id, &chunks)?;
            self.update_job_for_start(
                job_id,
                &working_wav_path,
                chunks.len() as i64,
                &current_fingerprint,
                &settings_snapshot,
            )?;
            self.emit_progress(
                job_id,
                0,
                chunks.len() as i64,
                "transcribing",
                &format!("Ready to transcribe {} chunks", chunks.len()),
                None,
            );
            let all_samples = samples;
            return self.run_transcription(job_id, cancel_flag, settings_snapshot, all_samples);
        } else {
            self.set_job_status(job_id, StudioJobStatus::Running, None)?;
        }

        let all_samples = read_wav_samples(&working_wav_path)?;
        self.run_transcription(job_id, cancel_flag, settings_snapshot, all_samples)
    }

    fn run_transcription(
        &self,
        job_id: &str,
        cancel_flag: Arc<AtomicBool>,
        settings_snapshot: AppSettings,
        all_samples: Vec<f32>,
    ) -> Result<()> {
        let chunks = self.load_chunks(job_id)?;
        let total_chunks = chunks.len() as i64;
        let tm = self.app_handle.state::<Arc<TranscriptionManager>>();

        let mut paused = false;
        for chunk in chunks {
            if chunk.status == "done" {
                continue;
            }

            if cancel_flag.load(Ordering::Relaxed) {
                self.finish_cancelled_job(job_id)?;
                return Ok(());
            }

            if tm.is_dictation_active() {
                paused = true;
                self.set_job_status(job_id, StudioJobStatus::Paused, None)?;
                let _ = self.app_handle.emit(
                    "studio-job-paused",
                    serde_json::json!({ "job_id": job_id, "reason": "dictation" }),
                );
                tm.wait_for_dictation_idle();
            }

            if paused {
                paused = false;
                self.set_job_status(job_id, StudioJobStatus::Running, None)?;
                let _ = self.app_handle.emit(
                    "studio-job-resumed",
                    serde_json::json!({ "job_id": job_id }),
                );
            }

            let start_sample = ((chunk.start_ms * CHUNK_SAMPLE_RATE as i64) / 1000) as usize;
            let end_sample = ((chunk.end_ms * CHUNK_SAMPLE_RATE as i64) / 1000) as usize;
            let overlap_samples = ((CHUNK_OVERLAP_MS * CHUNK_SAMPLE_RATE as i64) / 1000) as usize;
            let slice_start = if chunk.chunk_index > 0 {
                start_sample.saturating_sub(overlap_samples)
            } else {
                start_sample
            };
            let slice = all_samples
                [slice_start.min(all_samples.len())..end_sample.min(all_samples.len())]
                .to_vec();

            self.emit_progress(
                job_id,
                chunk.chunk_index,
                total_chunks,
                "transcribing",
                &format!(
                    "Transcribing chunk {} of {}",
                    chunk.chunk_index + 1,
                    total_chunks
                ),
                None,
            );

            let text = tm.transcribe_with_settings(slice, settings_snapshot.clone())?;
            let cleaned_text = self.complete_chunk(job_id, chunk.chunk_index, &text)?;

            if !cleaned_text.is_empty() {
                let _ = self.app_handle.emit(
                    "studio-job-preview",
                    StudioPreviewEvent {
                        job_id: job_id.to_string(),
                        appended_text: cleaned_text,
                    },
                );
            }
        }

        self.emit_progress(
            job_id,
            total_chunks,
            total_chunks,
            "writing_output_files",
            "Writing output files",
            None,
        );
        let output_files = self.export_job(job_id)?;
        self.complete_job(job_id)?;

        let _ = self.app_handle.emit(
            "studio-job-completed",
            serde_json::json!({
                "job_id": job_id,
                "output_files": output_files,
            }),
        );

        self.clear_active_job(job_id);
        info!("Studio job {} completed", job_id);
        Ok(())
    }

    fn ensure_model_ready(&self, settings: &AppSettings) -> Result<()> {
        let tm = self.app_handle.state::<Arc<TranscriptionManager>>();
        if tm.get_current_model().as_deref() == Some(settings.selected_model.as_str()) {
            return Ok(());
        }

        tm.load_model(&settings.selected_model)
            .map_err(|error| anyhow!("Failed to load Studio model: {}", error))
    }

    fn reset_job_storage(&self, job_id: &str, work_dir: &Path) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute(
            "DELETE FROM studio_chunks WHERE job_id = ?1",
            params![job_id],
        )?;
        conn.execute(
            "DELETE FROM studio_exports WHERE job_id = ?1",
            params![job_id],
        )?;
        if work_dir.exists() {
            fs::remove_dir_all(work_dir)?;
        }
        fs::create_dir_all(work_dir.join("staged"))?;
        Ok(())
    }

    fn update_job_for_start(
        &self,
        job_id: &str,
        working_wav_path: &Path,
        chunk_count: i64,
        settings_fingerprint: &str,
        settings: &AppSettings,
    ) -> Result<()> {
        let now = Self::now_ms();
        let conn = self.get_connection()?;
        conn.execute(
            "UPDATE studio_jobs
             SET working_wav_path = ?1,
                 status = ?2,
                 model_id = ?3,
                 language = ?4,
                 settings_fingerprint = ?5,
                 chunk_count = ?6,
                 chunks_completed = 0,
                 transcript_text = '',
                 error_message = NULL,
                 completed_at = NULL,
                 updated_at = ?7
             WHERE id = ?8",
            params![
                working_wav_path.to_string_lossy().to_string(),
                StudioJobStatus::Running.as_str(),
                settings.selected_model,
                settings.selected_language,
                settings_fingerprint,
                chunk_count,
                now,
                job_id,
            ],
        )?;
        Ok(())
    }

    fn store_chunks(&self, job_id: &str, chunks: &[(i64, i64)]) -> Result<()> {
        let conn = self.get_connection()?;
        let now = Self::now_ms();
        for (index, (start_ms, end_ms)) in chunks.iter().enumerate() {
            conn.execute(
                "INSERT INTO studio_chunks (id, job_id, chunk_index, start_ms, end_ms, text, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, '', 'pending', ?6, ?6)",
                params![
                    Self::create_id("chunk"),
                    job_id,
                    index as i64,
                    start_ms,
                    end_ms,
                    now,
                ],
            )?;
        }
        Ok(())
    }

    fn load_chunks(&self, job_id: &str) -> Result<Vec<ChunkRow>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT chunk_index, start_ms, end_ms, text, status
             FROM studio_chunks
             WHERE job_id = ?1
             ORDER BY chunk_index ASC",
        )?;
        let rows = stmt.query_map(params![job_id], |row| {
            Ok(ChunkRow {
                chunk_index: row.get("chunk_index")?,
                start_ms: row.get("start_ms")?,
                end_ms: row.get("end_ms")?,
                text: row.get("text")?,
                status: row.get("status")?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn complete_chunk(&self, job_id: &str, chunk_index: i64, text: &str) -> Result<String> {
        let conn = self.get_connection()?;
        let now = Self::now_ms();
        let cleaned = self.trim_chunk_text(job_id, chunk_index, text)?;

        conn.execute(
            "UPDATE studio_chunks SET text = ?1, status = 'done', updated_at = ?2 WHERE job_id = ?3 AND chunk_index = ?4",
            params![cleaned, now, job_id, chunk_index],
        )?;

        let transcript_text = self.build_transcript(job_id)?;
        let completed = self.count_completed_chunks(job_id)?;
        let total = self.count_chunks(job_id)?;
        conn.execute(
            "UPDATE studio_jobs
             SET chunks_completed = ?1,
                 transcript_text = ?2,
                 status = ?3,
                 updated_at = ?4
             WHERE id = ?5",
            params![
                completed,
                transcript_text,
                StudioJobStatus::Running.as_str(),
                now,
                job_id
            ],
        )?;

        self.emit_progress(
            job_id,
            completed,
            total,
            "transcribing",
            &format!("Transcribing chunk {} of {}", completed.min(total), total),
            None,
        );
        Ok(cleaned)
    }

    fn trim_chunk_text(&self, job_id: &str, chunk_index: i64, text: &str) -> Result<String> {
        let cleaned = text.trim().to_string();
        if chunk_index <= 0 || cleaned.is_empty() {
            return Ok(cleaned);
        }

        let conn = self.get_connection()?;
        let previous_text = conn
            .query_row(
                "SELECT text FROM studio_chunks WHERE job_id = ?1 AND chunk_index = ?2",
                params![job_id, chunk_index - 1],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_default();

        Ok(trim_overlap_prefix(&previous_text, &cleaned))
    }

    fn count_completed_chunks(&self, job_id: &str) -> Result<i64> {
        let conn = self.get_connection()?;
        conn.query_row(
            "SELECT COUNT(*) FROM studio_chunks WHERE job_id = ?1 AND status = 'done'",
            params![job_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
    }

    fn count_chunks(&self, job_id: &str) -> Result<i64> {
        let conn = self.get_connection()?;
        conn.query_row(
            "SELECT COUNT(*) FROM studio_chunks WHERE job_id = ?1",
            params![job_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
    }

    fn build_transcript(&self, job_id: &str) -> Result<String> {
        let chunks = self.load_chunks(job_id)?;
        Ok(chunks
            .into_iter()
            .filter_map(|chunk| {
                let text = chunk.text.trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"))
    }

    fn export_job(&self, job_id: &str) -> Result<Vec<StudioOutputFile>> {
        let job = self
            .get_job(job_id)?
            .ok_or_else(|| anyhow!("Studio job not found"))?;
        let output_folder = job
            .output_folder
            .clone()
            .ok_or_else(|| anyhow!("Studio job has no output folder"))?;
        let staged_dir = self.jobs_dir.join(job_id).join("staged");
        fs::create_dir_all(&staged_dir)?;
        fs::create_dir_all(&output_folder)?;

        let chunk_rows = self.load_chunks(job_id)?;
        let subtitle_chunks = chunk_rows
            .into_iter()
            .filter(|chunk| chunk.status == "done")
            .map(|chunk| srt::SubtitleChunk {
                start_ms: chunk.start_ms,
                end_ms: chunk.end_ms,
                text: chunk.text,
            })
            .collect::<Vec<_>>();

        let base_name = Path::new(&job.source_name)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("transcript")
            .to_string();
        let export_timestamp_ms = Self::now_ms();

        let mut staged_files = Vec::new();
        for format in &job.output_formats {
            let staged_path = staged_dir.join(format!("{base_name}.{format}"));
            match format.as_str() {
                "txt" => txt::write(&staged_path, &job.transcript_text)?,
                "srt" => srt::write(&staged_path, &subtitle_chunks)?,
                "vtt" => vtt::write(&staged_path, &subtitle_chunks)?,
                other => warn!("Skipping unsupported Studio export format: {}", other),
            }
            if staged_path.exists() {
                staged_files.push((format.clone(), staged_path));
            }
        }

        let mut export_plan = Vec::new();
        for (format, staged_path) in staged_files {
            let final_destination = resolve_export_destination(
                Path::new(&output_folder),
                &base_name,
                &format,
                export_timestamp_ms,
            );
            let file_name = final_destination
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow!("Invalid Studio export file name"))?
                .to_string();
            let temporary_destination =
                Path::new(&output_folder).join(format!("{file_name}.handy-partial"));
            if temporary_destination.exists() {
                fs::remove_file(&temporary_destination)?;
            }
            export_plan.push((
                format,
                staged_path,
                temporary_destination,
                final_destination,
            ));
        }

        let conn = self.get_connection()?;
        conn.execute(
            "DELETE FROM studio_exports WHERE job_id = ?1",
            params![job_id],
        )?;

        let mut temporary_paths = Vec::new();
        for (_, staged_path, temporary_destination, _) in &export_plan {
            if let Err(error) = move_file(staged_path, temporary_destination) {
                for temp_path in &temporary_paths {
                    let _ = fs::remove_file(temp_path);
                }
                return Err(error);
            }
            temporary_paths.push(temporary_destination.clone());
        }

        let created_at = Self::now_ms();
        let mut finalized_paths = Vec::new();
        let mut output_files = Vec::new();
        for (format, _, temporary_destination, final_destination) in export_plan {
            if let Err(error) = fs::rename(&temporary_destination, &final_destination) {
                for path in &finalized_paths {
                    let _ = fs::remove_file(path);
                }
                let _ = fs::remove_file(&temporary_destination);
                for temp_path in &temporary_paths {
                    if temp_path != &temporary_destination {
                        let _ = fs::remove_file(temp_path);
                    }
                }
                return Err(error.into());
            }

            let output_path = final_destination.to_string_lossy().to_string();
            conn.execute(
                "INSERT INTO studio_exports (id, job_id, format, output_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    Self::create_id("export"),
                    job_id,
                    &format,
                    &output_path,
                    created_at
                ],
            )?;
            finalized_paths.push(final_destination.clone());
            output_files.push(StudioOutputFile {
                format,
                file_name: final_destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_string(),
                output_path,
            });
        }

        Ok(output_files)
    }

    fn complete_job(&self, job_id: &str) -> Result<()> {
        let now = Self::now_ms();
        let conn = self.get_connection()?;
        conn.execute(
            "UPDATE studio_jobs SET status = ?1, updated_at = ?2, completed_at = ?2 WHERE id = ?3",
            params![StudioJobStatus::Done.as_str(), now, job_id],
        )?;
        Ok(())
    }

    fn set_job_status(
        &self,
        job_id: &str,
        status: StudioJobStatus,
        error_message: Option<&str>,
    ) -> Result<()> {
        let now = Self::now_ms();
        let conn = self.get_connection()?;
        conn.execute(
            "UPDATE studio_jobs SET status = ?1, error_message = ?2, updated_at = ?3 WHERE id = ?4",
            params![status.as_str(), error_message, now, job_id],
        )?;
        Ok(())
    }

    fn cancel_running_job(&self, job_id: &str) -> Result<()> {
        self.set_job_status(job_id, StudioJobStatus::Cancelled, None)
    }

    fn fail_job(&self, job_id: &str, error_message: &str) -> Result<()> {
        self.set_job_status(job_id, StudioJobStatus::Error, Some(error_message))
    }

    fn finish_cancelled_job(&self, job_id: &str) -> Result<()> {
        self.cancel_running_job(job_id)?;
        self.clear_active_job(job_id);
        let _ = self.app_handle.emit(
            "studio-job-cancelled",
            serde_json::json!({ "job_id": job_id }),
        );
        Ok(())
    }

    fn report_job_failure(&self, job_id: &str, error_message: String) {
        error!("Studio job {} failed: {}", job_id, error_message);
        if let Err(update_error) = self.fail_job(job_id, &error_message) {
            error!(
                "Failed to persist Studio failure for {}: {}",
                job_id, update_error
            );
        }
        let _ = self.app_handle.emit(
            "studio-job-failed",
            serde_json::json!({
                "job_id": job_id,
                "error": error_message,
            }),
        );
    }

    fn emit_progress(
        &self,
        job_id: &str,
        chunks_completed: i64,
        chunk_count: i64,
        stage: &str,
        message: &str,
        preparation_progress: Option<i64>,
    ) {
        let _ = self.app_handle.emit(
            "studio-job-progress",
            StudioProgressEvent {
                job_id: job_id.to_string(),
                chunks_completed,
                chunk_count,
                stage: stage.to_string(),
                message: message.to_string(),
                preparation_progress,
            },
        );
    }
}

fn build_chunks(sample_count: usize) -> Vec<(i64, i64)> {
    let chunk_samples = (CHUNK_MS as usize * CHUNK_SAMPLE_RATE) / 1000;
    if sample_count == 0 {
        return vec![(0, CHUNK_MS)];
    }

    let mut chunks = Vec::new();
    let mut start_sample = 0usize;
    while start_sample < sample_count {
        let end_sample = (start_sample + chunk_samples).min(sample_count);
        let start_ms = ((start_sample as f64 / CHUNK_SAMPLE_RATE as f64) * 1000.0).round() as i64;
        let end_ms = ((end_sample as f64 / CHUNK_SAMPLE_RATE as f64) * 1000.0).round() as i64;
        chunks.push((start_ms, end_ms.max(start_ms + 1000)));
        start_sample = end_sample;
    }
    chunks
}

fn trim_overlap_prefix(previous_text: &str, current_text: &str) -> String {
    let previous_words = previous_text
        .split_whitespace()
        .map(normalize_word)
        .collect::<Vec<_>>();
    let current_original_words = current_text.split_whitespace().collect::<Vec<_>>();
    let current_words = current_original_words
        .iter()
        .map(|word| normalize_word(word))
        .collect::<Vec<_>>();

    let max_overlap = previous_words.len().min(current_words.len());
    for overlap in (3..=max_overlap).rev() {
        if previous_words[previous_words.len() - overlap..] == current_words[..overlap] {
            return current_original_words[overlap..]
                .join(" ")
                .trim()
                .to_string();
        }
    }

    current_text.trim().to_string()
}

fn normalize_word(word: &str) -> String {
    word.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn can_retry_with_available_source(working_wav_path: Option<&str>, source_path: &Path) -> bool {
    working_wav_path.map(Path::new).is_some_and(Path::exists) || source_path.exists()
}

fn settings_fingerprint(settings: &AppSettings) -> Result<String> {
    Ok(serde_json::json!({
        "model_id": settings.selected_model,
        "language": settings.selected_language,
        "translate_to_english": settings.translate_to_english,
    })
    .to_string())
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }

    if to.exists() {
        fs::remove_file(to)?;
    }

    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(from, to)?;
            fs::remove_file(from)?;
            Ok(())
        }
    }
}

fn clear_active_job_state(
    active_job_id: &Arc<Mutex<Option<String>>>,
    cancel_flags: &Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    job_id: &str,
) {
    let mut active = active_job_id.lock().unwrap();
    if active.as_deref() == Some(job_id) {
        active.take();
    }
    cancel_flags.lock().unwrap().remove(job_id);
}

fn panic_payload_to_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        format!("Studio job panicked: {}", message)
    } else if let Some(message) = payload.downcast_ref::<String>() {
        format!("Studio job panicked: {}", message)
    } else {
        "Studio job panicked unexpectedly".to_string()
    }
}

fn export_collision_suffix(timestamp_ms: i64) -> String {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d %H-%M-%S")
        .to_string()
}

fn resolve_export_destination(
    output_folder: &Path,
    base_name: &str,
    format: &str,
    timestamp_ms: i64,
) -> PathBuf {
    let initial = output_folder.join(format!("{base_name}.{format}"));
    if !initial.exists() {
        return initial;
    }

    let suffix = export_collision_suffix(timestamp_ms);
    for attempt in 0.. {
        let file_name = if attempt == 0 {
            format!("{base_name} ({suffix}).{format}")
        } else {
            format!("{base_name} ({suffix}) ({}).{format}", attempt + 1)
        };
        let candidate = output_folder.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("collision resolution loop should always return")
}

#[cfg(test)]
mod tests {
    use super::{
        build_chunks, can_retry_with_available_source, export_collision_suffix,
        panic_payload_to_message, resolve_export_destination, trim_overlap_prefix,
        ActiveJobCleanup, CHUNK_MS, CHUNK_SAMPLE_RATE,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    #[test]
    fn active_job_cleanup_clears_state_on_drop() {
        let active_job_id = Arc::new(Mutex::new(Some("job-123".to_string())));
        let cancel_flags = Arc::new(Mutex::new(HashMap::from([(
            "job-123".to_string(),
            Arc::new(AtomicBool::new(false)),
        )])));

        {
            let _cleanup = ActiveJobCleanup::from_state(
                active_job_id.clone(),
                cancel_flags.clone(),
                "job-123",
            );
        }

        assert!(active_job_id.lock().unwrap().is_none());
        assert!(!cancel_flags.lock().unwrap().contains_key("job-123"));
    }

    #[test]
    fn active_job_cleanup_can_be_disarmed() {
        let active_job_id = Arc::new(Mutex::new(Some("job-123".to_string())));
        let cancel_flags = Arc::new(Mutex::new(HashMap::from([(
            "job-123".to_string(),
            Arc::new(AtomicBool::new(false)),
        )])));

        {
            let mut cleanup = ActiveJobCleanup::from_state(
                active_job_id.clone(),
                cancel_flags.clone(),
                "job-123",
            );
            cleanup.disarm();
        }

        assert_eq!(active_job_id.lock().unwrap().as_deref(), Some("job-123"));
        assert!(cancel_flags.lock().unwrap().contains_key("job-123"));
    }

    #[test]
    fn panic_payload_to_message_handles_string_payloads() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        let message = panic_payload_to_message(payload);
        assert_eq!(message, "Studio job panicked: boom");
    }

    #[test]
    fn trim_overlap_prefix_removes_repeated_words() {
        let trimmed = trim_overlap_prefix(
            "this is the end of the sentence",
            "end of the sentence and here is more",
        );

        assert_eq!(trimmed, "and here is more");
    }

    #[test]
    fn trim_overlap_prefix_removes_long_repeated_prefixes() {
        let previous =
            "one two three four five six seven eight nine ten eleven twelve thirteen fourteen";
        let current = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen and new words";

        let trimmed = trim_overlap_prefix(previous, current);

        assert_eq!(trimmed, "and new words");
    }

    #[test]
    fn build_chunks_uses_ten_second_windows() {
        let twenty_five_seconds_of_samples = CHUNK_SAMPLE_RATE * 25;
        let chunks = build_chunks(twenty_five_seconds_of_samples);

        assert_eq!(CHUNK_MS, 10_000);
        assert_eq!(
            chunks,
            vec![(0, 10_000), (10_000, 20_000), (20_000, 25_000)]
        );
    }

    #[test]
    fn retry_accepts_existing_working_wav_even_if_source_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let working_wav = temp_dir.path().join("normalized.wav");
        std::fs::write(&working_wav, b"wav").expect("working wav");
        let missing_source = temp_dir.path().join("missing.mp3");

        assert!(can_retry_with_available_source(
            Some(working_wav.to_string_lossy().as_ref()),
            &missing_source,
        ));
    }

    #[test]
    fn retry_requires_source_when_working_wav_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let missing_wav = PathBuf::from(temp_dir.path().join("missing.wav"));
        let missing_source = temp_dir.path().join("missing.mp3");

        assert!(!can_retry_with_available_source(
            Some(missing_wav.to_string_lossy().as_ref()),
            &missing_source,
        ));
    }

    #[test]
    fn export_collision_suffix_uses_safe_timestamp_format() {
        let suffix = export_collision_suffix(1_743_076_979_000);
        assert!(suffix.contains('-'));
        assert!(!suffix.contains(':'));
    }

    #[test]
    fn resolve_export_destination_keeps_original_name_when_available() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let output_path =
            resolve_export_destination(temp_dir.path(), "Let It Be", "srt", 1_743_076_979_000);

        assert_eq!(
            output_path.file_name().and_then(|name| name.to_str()),
            Some("Let It Be.srt")
        );
    }

    #[test]
    fn resolve_export_destination_adds_timestamp_on_collision() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let original = temp_dir.path().join("Let It Be.srt");
        fs::write(&original, "existing").expect("write existing file");

        let output_path =
            resolve_export_destination(temp_dir.path(), "Let It Be", "srt", 1_743_076_979_000);

        assert_ne!(output_path, original);
        assert!(output_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .starts_with("Let It Be ("));
        assert_eq!(
            output_path.extension().and_then(|ext| ext.to_str()),
            Some("srt")
        );
    }

    #[test]
    fn resolve_export_destination_adds_counter_when_timestamped_name_exists() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let original = temp_dir.path().join("Let It Be.srt");
        fs::write(&original, "existing").expect("write existing file");

        let timestamped =
            resolve_export_destination(temp_dir.path(), "Let It Be", "srt", 1_743_076_979_000);
        fs::write(&timestamped, "timestamped").expect("write timestamped file");

        let next =
            resolve_export_destination(temp_dir.path(), "Let It Be", "srt", 1_743_076_979_000);

        assert_ne!(next, original);
        assert_ne!(next, timestamped);
        assert!(next
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .contains(" (2).srt"));
    }
}
