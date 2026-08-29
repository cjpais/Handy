//! Native S1-mini text-normalization runtime and artifact lifecycle.
//!
//! S1-mini is deliberately isolated from the ASR model catalog: it has its own
//! pinned artifacts, status/event namespace, cancellation, and storage folder.

use crate::managers::model::download::{HttpDownloadEvent, HttpDownloadOutcome};
use crate::managers::model::ModelManager;
use crate::managers::s1_qwen3::ModelWeights as Qwen3;
use crate::settings::{S1Context, S1Structure, S1Styling, S1_MINI_LABEL};
use anyhow::{anyhow, Context, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Emitter};
use tokenizers::Tokenizer;

pub const MODEL_REPO: &str = "superwhisper/s1-mini-GGUF";
pub const MODEL_REVISION: &str = "34add00a48a2e5d24e5a4ee5405a99620a3a240c";
pub const MODEL_FILENAME: &str = "s1-mini-q4_k_m.gguf";
pub const MODEL_SIZE: u64 = 484_219_808;
pub const MODEL_SHA256: &str = "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634";

pub const TOKENIZER_REPO: &str = "superwhisper/s1-mini";
pub const TOKENIZER_REVISION: &str = "88f6b15896c73bbb13a3b596e0afe8ea0d5150b4";
pub const TOKENIZER_FILENAME: &str = "tokenizer.json";
pub const TOKENIZER_SIZE: u64 = 11_422_654;
pub const TOKENIZER_SHA256: &str =
    "aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4";

pub const LICENSE_FILENAME: &str = "LICENSE";
pub const LICENSE_SIZE: u64 = 12_033;
pub const LICENSE_SHA256: &str = "f715982df6ce767ae64864d74b95644e5d658aab54717dbceeb252eb3cbcb421";
pub const NOTICE_FILENAME: &str = "NOTICE";
pub const NOTICE_SIZE: u64 = 470;
pub const NOTICE_SHA256: &str = "4feae786f1766dc58e807bcae1e7bdd06bf3610a03a23d0621b6c6c4d05f2980";

const TOTAL_DOWNLOAD_SIZE: u64 = MODEL_SIZE + TOKENIZER_SIZE + LICENSE_SIZE + NOTICE_SIZE;
const MANIFEST_FILENAME: &str = "verified-v1";
const MAX_TRANSCRIPT_TOKENS: usize = 1_000;
const MAX_CONTEXT_TOKENS: usize = 4_096;
const PREFILL_CHUNK_TOKENS: usize = 128;
const EOS_IM_END: u32 = 151_645;
const EOS_END_OF_TEXT: u32 = 151_643;
const OPERATION_IDLE: u8 = 0;
const OPERATION_DOWNLOAD: u8 = 1;
const OPERATION_GENERATION: u8 = 2;
const OPERATION_MUTATION: u8 = 3;
const SYSTEM_PROMPT: &str = "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.";
const RESERVED_INPUT_TOKENS: [&str; 5] = [
    "<|im_start|>",
    "<|im_end|>",
    "<|endoftext|>",
    "<think>",
    "</think>",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum S1MiniStatus {
    NotDownloaded {
        downloaded_bytes: u64,
        total_bytes: u64,
        progress: f64,
    },
    Downloading {
        downloaded_bytes: u64,
        total_bytes: u64,
        progress: f64,
    },
    Ready {
        downloaded_bytes: u64,
        total_bytes: u64,
        progress: f64,
    },
    Error {
        downloaded_bytes: u64,
        total_bytes: u64,
        progress: f64,
        error: String,
    },
}

impl S1MiniStatus {
    fn progress(downloaded_bytes: u64) -> f64 {
        (downloaded_bytes.min(TOTAL_DOWNLOAD_SIZE) as f64 / TOTAL_DOWNLOAD_SIZE as f64) * 100.0
    }

    fn not_downloaded(downloaded_bytes: u64) -> Self {
        Self::NotDownloaded {
            downloaded_bytes,
            total_bytes: TOTAL_DOWNLOAD_SIZE,
            progress: Self::progress(downloaded_bytes),
        }
    }

    fn downloading(downloaded_bytes: u64) -> Self {
        Self::Downloading {
            downloaded_bytes,
            total_bytes: TOTAL_DOWNLOAD_SIZE,
            progress: Self::progress(downloaded_bytes),
        }
    }

    fn ready() -> Self {
        Self::Ready {
            downloaded_bytes: TOTAL_DOWNLOAD_SIZE,
            total_bytes: TOTAL_DOWNLOAD_SIZE,
            progress: 100.0,
        }
    }

    fn error(downloaded_bytes: u64, error: String) -> Self {
        Self::Error {
            downloaded_bytes,
            total_bytes: TOTAL_DOWNLOAD_SIZE,
            progress: Self::progress(downloaded_bytes),
            error,
        }
    }
}

#[derive(Default)]
struct DownloadState {
    downloading: bool,
    downloaded_bytes: u64,
    error: Option<String>,
    cancel: Option<hf_hub::api::tokio::CancellationToken>,
}

pub struct S1MiniManager {
    app_handle: AppHandle,
    model_dir: PathBuf,
    download_state: Mutex<DownloadState>,
    operation: AtomicU8,
    artifacts_verified: AtomicBool,
    cancellation_generation: AtomicU64,
    runtime: Mutex<Option<S1MiniRuntime>>,
}

impl S1MiniManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let model_dir = crate::portable::app_data_dir(app_handle)
            .map_err(|error| anyhow!("Failed to get app data directory: {error}"))?
            .join("post-processing")
            .join("s1-mini");
        fs::create_dir_all(&model_dir)?;
        let downloaded_bytes = downloaded_bytes_on_disk(&model_dir);
        Ok(Self {
            app_handle: app_handle.clone(),
            model_dir,
            download_state: Mutex::new(DownloadState {
                downloaded_bytes,
                ..DownloadState::default()
            }),
            operation: AtomicU8::new(OPERATION_IDLE),
            artifacts_verified: AtomicBool::new(false),
            cancellation_generation: AtomicU64::new(0),
            runtime: Mutex::new(None),
        })
    }

    pub fn status(&self) -> S1MiniStatus {
        let state = self
            .download_state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if state.downloading {
            S1MiniStatus::downloading(state.downloaded_bytes)
        } else if artifacts_ready(&self.model_dir) {
            S1MiniStatus::ready()
        } else if let Some(error) = &state.error {
            S1MiniStatus::error(state.downloaded_bytes, error.clone())
        } else {
            S1MiniStatus::not_downloaded(downloaded_bytes_on_disk(&self.model_dir))
        }
    }

    fn emit_status(&self) {
        let _ = self.app_handle.emit("s1-mini-status", self.status());
    }

    pub async fn download(&self) -> Result<()> {
        let _operation = OperationGuard::claim(
            &self.operation,
            OPERATION_DOWNLOAD,
            "S1-mini is already busy",
        )?;
        self.artifacts_verified.store(false, Ordering::Release);
        let cancel = hf_hub::api::tokio::CancellationToken::new();
        {
            let mut state = self
                .download_state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if state.downloading {
                return Err(anyhow!("S1-mini download is already in progress"));
            }
            state.downloading = true;
            state.downloaded_bytes = downloaded_bytes_on_disk(&self.model_dir);
            state.error = None;
            state.cancel = Some(cancel.clone());
        }
        self.emit_status();

        let result = self.download_artifacts(&cancel).await;
        {
            let mut state = self
                .download_state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            state.downloading = false;
            state.cancel = None;
            state.downloaded_bytes = downloaded_bytes_on_disk(&self.model_dir);
            state.error = result.as_ref().err().map(ToString::to_string);
        }
        self.emit_status();

        match result {
            Ok(true) => Ok(()),
            Ok(false) => {
                debug!("S1-mini download cancelled; partial files kept for resume");
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn download_artifacts(
        &self,
        cancel: &hf_hub::api::tokio::CancellationToken,
    ) -> Result<bool> {
        let model = Artifact {
            repo: MODEL_REPO,
            revision: MODEL_REVISION,
            filename: MODEL_FILENAME,
            size: MODEL_SIZE,
            sha256: MODEL_SHA256,
            progress_base: 0,
        };
        if !self.download_artifact(model, cancel).await? {
            return Ok(false);
        }

        let tokenizer = Artifact {
            repo: TOKENIZER_REPO,
            revision: TOKENIZER_REVISION,
            filename: TOKENIZER_FILENAME,
            size: TOKENIZER_SIZE,
            sha256: TOKENIZER_SHA256,
            progress_base: MODEL_SIZE,
        };
        if !self.download_artifact(tokenizer, cancel).await? {
            return Ok(false);
        }

        let license = Artifact {
            repo: TOKENIZER_REPO,
            revision: TOKENIZER_REVISION,
            filename: LICENSE_FILENAME,
            size: LICENSE_SIZE,
            sha256: LICENSE_SHA256,
            progress_base: MODEL_SIZE + TOKENIZER_SIZE,
        };
        if !self.download_artifact(license, cancel).await? {
            return Ok(false);
        }

        let notice = Artifact {
            repo: TOKENIZER_REPO,
            revision: TOKENIZER_REVISION,
            filename: NOTICE_FILENAME,
            size: NOTICE_SIZE,
            sha256: NOTICE_SHA256,
            progress_base: MODEL_SIZE + TOKENIZER_SIZE + LICENSE_SIZE,
        };
        if !self.download_artifact(notice, cancel).await? {
            return Ok(false);
        }

        fs::write(self.model_dir.join(MANIFEST_FILENAME), manifest_contents())?;
        self.artifacts_verified.store(true, Ordering::Release);
        info!("S1-mini artifacts downloaded and verified");
        Ok(true)
    }

    async fn download_artifact(
        &self,
        artifact: Artifact,
        cancel: &hf_hub::api::tokio::CancellationToken,
    ) -> Result<bool> {
        let destination = self.model_dir.join(artifact.filename);
        if verified_file(&destination, artifact.size, artifact.sha256).await? {
            self.update_download_progress(artifact.progress_base + artifact.size);
            return Ok(true);
        }
        if destination.exists() {
            fs::remove_file(&destination)?;
        }

        let partial = self
            .model_dir
            .join(format!("{}.partial", artifact.filename));
        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}?download=true",
            artifact.repo, artifact.revision, artifact.filename
        );
        let progress_base = artifact.progress_base;
        let outcome = ModelManager::download_http_resumable_with_events(
            S1_MINI_LABEL,
            &url,
            &partial,
            Some(artifact.size),
            Some(artifact.sha256),
            cancel,
            &|event| {
                if let HttpDownloadEvent::Progress(progress) = event {
                    self.update_download_progress(progress_base + progress.downloaded);
                }
            },
        )
        .await?;

        match outcome {
            HttpDownloadOutcome::Cancelled => Ok(false),
            HttpDownloadOutcome::Completed => {
                fs::rename(&partial, &destination)?;
                self.update_download_progress(progress_base + artifact.size);
                Ok(true)
            }
        }
    }

    fn update_download_progress(&self, downloaded_bytes: u64) {
        {
            let mut state = self
                .download_state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            state.downloaded_bytes = downloaded_bytes.min(TOTAL_DOWNLOAD_SIZE);
        }
        self.emit_status();
    }

    pub fn cancel_download(&self) -> Result<()> {
        let cancel = self
            .download_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel
            .clone()
            .ok_or_else(|| anyhow!("No S1-mini download is in progress"))?;
        cancel.cancel();
        Ok(())
    }

    pub fn cancellation_generation(&self) -> u64 {
        self.cancellation_generation.load(Ordering::Acquire)
    }

    pub fn cancel_generation(&self) {
        self.cancellation_generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn process(
        &self,
        transcript: &str,
        styling: S1Styling,
        structure: S1Structure,
        context: S1Context,
        expected_cancellation_generation: u64,
    ) -> Result<String> {
        let _operation = OperationGuard::claim(
            &self.operation,
            OPERATION_GENERATION,
            "S1-mini is already busy",
        )?;
        if !artifacts_ready(&self.model_dir) {
            return Err(anyhow!("S1-mini is not downloaded"));
        }
        self.verify_artifacts_once()?;
        self.ensure_not_cancelled(expected_cancellation_generation)?;

        let mut runtime_slot = self.runtime_lock()?;
        *runtime_slot = Some(S1MiniRuntime::load(
            &self.model_dir.join(MODEL_FILENAME),
            &self.model_dir.join(TOKENIZER_FILENAME),
            transcript,
            styling,
            structure,
            context,
        )?);
        let result = runtime_slot
            .as_mut()
            .expect("runtime inserted above")
            .generate(transcript, styling, structure, context, || {
                self.cancellation_generation.load(Ordering::Acquire)
                    != expected_cancellation_generation
            });

        // Qwen3's CPU KV cache is large. Clear it on every exit and drop the
        // complete runtime after each inference so the memory is transient.
        if let Some(runtime) = runtime_slot.as_mut() {
            runtime.clear_kv_cache();
        }
        *runtime_slot = None;
        result
    }

    fn ensure_not_cancelled(&self, expected: u64) -> Result<()> {
        if self.cancellation_generation.load(Ordering::Acquire) != expected {
            Err(anyhow!("S1-mini generation cancelled"))
        } else {
            Ok(())
        }
    }

    fn runtime_lock(&self) -> Result<MutexGuard<'_, Option<S1MiniRuntime>>> {
        self.runtime
            .lock()
            .map_err(|_| anyhow!("S1-mini runtime lock is poisoned"))
    }

    fn verify_artifacts_once(&self) -> Result<()> {
        if self.artifacts_verified.load(Ordering::Acquire) {
            return Ok(());
        }
        for (filename, size, sha256) in artifact_specs() {
            if !verified_file_sync(&self.model_dir.join(filename), size, sha256)? {
                self.artifacts_verified.store(false, Ordering::Release);
                let _ = fs::remove_file(self.model_dir.join(MANIFEST_FILENAME));
                let error = format!(
                    "S1-mini artifact '{filename}' failed integrity verification; download it again"
                );
                self.download_state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .error = Some(error.clone());
                self.emit_status();
                return Err(anyhow!(error));
            }
        }
        self.artifacts_verified.store(true, Ordering::Release);
        Ok(())
    }

    pub fn unload(&self) -> Result<()> {
        let _operation = OperationGuard::claim(
            &self.operation,
            OPERATION_MUTATION,
            "S1-mini is currently busy",
        )?;
        self.unload_runtime()
    }

    fn unload_runtime(&self) -> Result<()> {
        let mut runtime = self.runtime_lock()?;
        if let Some(runtime) = runtime.as_mut() {
            runtime.clear_kv_cache();
        }
        *runtime = None;
        Ok(())
    }

    pub fn delete(&self) -> Result<()> {
        let _operation = OperationGuard::claim(
            &self.operation,
            OPERATION_MUTATION,
            "Cancel the active S1-mini operation before deleting it",
        )?;
        self.unload_runtime()?;
        for filename in [
            MODEL_FILENAME,
            TOKENIZER_FILENAME,
            LICENSE_FILENAME,
            NOTICE_FILENAME,
            MANIFEST_FILENAME,
            &format!("{MODEL_FILENAME}.partial"),
            &format!("{TOKENIZER_FILENAME}.partial"),
            &format!("{LICENSE_FILENAME}.partial"),
            &format!("{NOTICE_FILENAME}.partial"),
        ] {
            let path = self.model_dir.join(filename);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        {
            let mut state = self
                .download_state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            state.downloaded_bytes = 0;
            state.error = None;
        }
        self.artifacts_verified.store(false, Ordering::Release);
        self.emit_status();
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Artifact {
    repo: &'static str,
    revision: &'static str,
    filename: &'static str,
    size: u64,
    sha256: &'static str,
    progress_base: u64,
}

struct OperationGuard<'a>(&'a AtomicU8);

impl<'a> OperationGuard<'a> {
    fn claim(operation: &'a AtomicU8, next: u8, busy_message: &str) -> Result<Self> {
        operation
            .compare_exchange(OPERATION_IDLE, next, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self(operation))
            .map_err(|_| anyhow!(busy_message.to_string()))
    }
}

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        self.0.store(OPERATION_IDLE, Ordering::Release);
    }
}

struct S1MiniRuntime {
    model: Qwen3,
    tokenizer: Tokenizer,
    device: Device,
}

impl S1MiniRuntime {
    fn load(
        model_path: &Path,
        tokenizer_path: &Path,
        transcript: &str,
        styling: S1Styling,
        structure: S1Structure,
        context: S1Context,
    ) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(anyhow::Error::msg)?;
        validate_special_tokens(&tokenizer)?;
        let context_capacity = {
            prepare_generations(&tokenizer, transcript, styling, structure, context)?
                .into_iter()
                .map(|(prompt_tokens, max_new_tokens)| prompt_tokens.len() + max_new_tokens)
                .max()
                .ok_or_else(|| anyhow!("S1-mini transcript is empty"))?
        };
        let device = Device::Cpu;
        let mut file = File::open(model_path)?;
        let content = gguf_file::Content::read(&mut file).map_err(|e| e.with_path(model_path))?;
        let model = Qwen3::from_gguf(content, &mut file, &device, context_capacity)?;
        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    fn generate(
        &mut self,
        transcript: &str,
        styling: S1Styling,
        structure: S1Structure,
        context: S1Context,
        is_cancelled: impl Fn() -> bool + Sync,
    ) -> Result<String> {
        let prepared =
            prepare_generations(&self.tokenizer, transcript, styling, structure, context)?;
        let mut outputs = Vec::with_capacity(prepared.len());
        for (prompt_tokens, max_new_tokens) in prepared {
            self.model.clear_kv_cache();
            let output = self.generate_one(&prompt_tokens, max_new_tokens, &is_cancelled)?;
            if !output.is_empty() {
                outputs.push(output);
            }
        }
        Ok(outputs.join("\n\n"))
    }

    fn generate_one(
        &mut self,
        prompt_tokens: &[u32],
        max_new_tokens: usize,
        is_cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<String> {
        if is_cancelled() {
            return Err(anyhow!("S1-mini generation cancelled"));
        }
        let mut logits_processor = LogitsProcessor::from_sampling(0, Sampling::ArgMax);
        let generated = self.device.with_context(|| -> Result<Vec<u32>> {
            let mut next_token = None;
            let mut offset = 0;
            for chunk in prompt_tokens.chunks(PREFILL_CHUNK_TOKENS) {
                if is_cancelled() {
                    return Err(anyhow!("S1-mini generation cancelled"));
                }
                let input = Tensor::new(chunk, &self.device)?.unsqueeze(0)?;
                let logits = self.model.forward(&input, offset)?.squeeze(0)?;
                next_token = Some(logits_processor.sample(&logits)?);
                offset += chunk.len();
            }
            let mut next_token = next_token.ok_or_else(|| anyhow!("S1-mini prompt is empty"))?;
            let mut output = Vec::with_capacity(max_new_tokens);

            for index in 0..max_new_tokens {
                if is_cancelled() {
                    return Err(anyhow!("S1-mini generation cancelled"));
                }
                if is_eos(next_token) {
                    break;
                }
                output.push(next_token);
                if index + 1 == max_new_tokens {
                    break;
                }
                let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;
                let logits = self
                    .model
                    .forward(&input, prompt_tokens.len() + index)?
                    .squeeze(0)?;
                next_token = logits_processor.sample(&logits)?;
            }
            Ok(output)
        })?;

        self.tokenizer
            .decode(&generated, true)
            .map(|output| output.trim().to_string())
            .map_err(anyhow::Error::msg)
    }

    fn clear_kv_cache(&mut self) {
        self.model.clear_kv_cache();
    }
}

impl Drop for S1MiniRuntime {
    fn drop(&mut self) {
        self.clear_kv_cache();
    }
}

pub fn control_line(styling: S1Styling, structure: S1Structure, context: S1Context) -> String {
    format!(
        "[Styling: {}] [Structure: {}] [Context: {}]",
        styling.as_control_value(),
        structure.as_control_value(),
        context.as_control_value()
    )
}

fn build_prompt(
    transcript: &str,
    styling: S1Styling,
    structure: S1Structure,
    context: S1Context,
) -> String {
    format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n{}\n{transcript}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
        control_line(styling, structure, context)
    )
}

fn prepare_generation(
    tokenizer: &Tokenizer,
    transcript: &str,
    styling: S1Styling,
    structure: S1Structure,
    context: S1Context,
) -> Result<(Vec<u32>, usize)> {
    reject_reserved_tokens(transcript)?;
    let transcript_tokens = tokenizer
        .encode(transcript, false)
        .map_err(anyhow::Error::msg)?
        .len();
    validate_transcript_token_count(transcript_tokens)?;
    let prompt_tokens = tokenizer
        // The complete manual chat template already includes every required
        // special token; no tokenizer-added wrapper is wanted.
        .encode(build_prompt(transcript, styling, structure, context), false)
        .map_err(anyhow::Error::msg)?
        .get_ids()
        .to_vec();
    let max_new_tokens = max_new_tokens(transcript_tokens);
    if prompt_tokens.len() + max_new_tokens > MAX_CONTEXT_TOKENS {
        return Err(anyhow!("S1-mini input exceeds its context window"));
    }
    Ok((prompt_tokens, max_new_tokens))
}

fn prepare_generations(
    tokenizer: &Tokenizer,
    transcript: &str,
    styling: S1Styling,
    structure: S1Structure,
    context: S1Context,
) -> Result<Vec<(Vec<u32>, usize)>> {
    split_transcript_chunks(tokenizer, transcript)?
        .into_iter()
        .map(|chunk| prepare_generation(tokenizer, &chunk, styling, structure, context))
        .collect()
}

fn split_transcript_chunks(tokenizer: &Tokenizer, transcript: &str) -> Result<Vec<String>> {
    let full_encoding = tokenizer
        .encode(transcript, false)
        .map_err(anyhow::Error::msg)?;
    if full_encoding.len() <= MAX_TRANSCRIPT_TOKENS {
        return Ok(vec![transcript.to_string()]);
    }

    let mut remaining = transcript;
    let mut chunks = Vec::new();
    loop {
        let encoding = tokenizer
            .encode(remaining, false)
            .map_err(anyhow::Error::msg)?;
        if encoding.len() <= MAX_TRANSCRIPT_TOKENS {
            let tail = remaining.trim();
            if !tail.is_empty() {
                chunks.push(tail.to_string());
            }
            break;
        }

        let offsets = encoding.get_offsets();
        let final_quarter_start = MAX_TRANSCRIPT_TOKENS * 3 / 4;
        let min_byte = offsets
            .get(final_quarter_start - 1)
            .map(|offset| offset.1)
            .unwrap_or(0);
        let max_byte = offsets
            .get(MAX_TRANSCRIPT_TOKENS - 1)
            .map(|offset| offset.1)
            .unwrap_or(remaining.len());
        let fallback = char_boundary_at_or_before(remaining, max_byte);
        let boundary = select_chunk_boundary(remaining, min_byte, max_byte).unwrap_or(fallback);
        if boundary == 0 || boundary >= remaining.len() {
            return Err(anyhow!(
                "S1-mini could not find a safe boundary for a long transcript"
            ));
        }
        let (head, tail) = remaining.split_at(boundary);
        let head = head.trim();
        if head.is_empty() {
            return Err(anyhow!("S1-mini produced an empty transcript chunk"));
        }
        chunks.push(head.to_string());
        remaining = tail.trim_start();
    }
    Ok(chunks)
}

fn select_chunk_boundary(text: &str, min_byte: usize, max_byte: usize) -> Option<usize> {
    let mut paragraph = None;
    let mut sentence = None;
    let mut whitespace = None;
    let mut previous_newline_end = None;

    for (index, character) in text.char_indices() {
        let end = index + character.len_utf8();
        let within_window = end >= min_byte && end <= max_byte;
        if character == '\n' {
            if let Some(previous_end) = previous_newline_end {
                if within_window && text[previous_end..index].chars().all(char::is_whitespace) {
                    paragraph = Some(end);
                }
            }
            previous_newline_end = Some(end);
        } else if !character.is_whitespace() {
            previous_newline_end = None;
        }

        if within_window
            && matches!(character, '.' | '!' | '?')
            && text[end..]
                .chars()
                .next()
                .map(|next| next.is_whitespace())
                .unwrap_or(true)
        {
            sentence = Some(end);
        }
        if character.is_whitespace() && index >= min_byte && index <= max_byte {
            whitespace = Some(index);
        }
    }
    paragraph.or(sentence).or(whitespace)
}

fn char_boundary_at_or_before(text: &str, index: usize) -> usize {
    let mut boundary = index.min(text.len());
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn max_new_tokens(transcript_tokens: usize) -> usize {
    (transcript_tokens * 13).div_ceil(10) + 32
}

fn validate_transcript_token_count(transcript_tokens: usize) -> Result<()> {
    if transcript_tokens > MAX_TRANSCRIPT_TOKENS {
        Err(anyhow!(
            "S1-mini accepts at most {MAX_TRANSCRIPT_TOKENS} transcript tokens; got {transcript_tokens}"
        ))
    } else {
        Ok(())
    }
}

fn is_eos(token: u32) -> bool {
    token == EOS_IM_END || token == EOS_END_OF_TEXT
}

fn reject_reserved_tokens(transcript: &str) -> Result<()> {
    if let Some(token) = RESERVED_INPUT_TOKENS
        .iter()
        .find(|token| transcript.contains(**token))
    {
        Err(anyhow!(
            "S1-mini transcript contains reserved control token '{token}'"
        ))
    } else {
        Ok(())
    }
}

fn validate_special_tokens(tokenizer: &Tokenizer) -> Result<()> {
    for (token, expected) in [
        ("<|im_end|>", EOS_IM_END),
        ("<|endoftext|>", EOS_END_OF_TEXT),
    ] {
        let actual = tokenizer
            .token_to_id(token)
            .ok_or_else(|| anyhow!("S1-mini tokenizer is missing {token}"))?;
        if actual != expected {
            return Err(anyhow!(
                "S1-mini tokenizer maps {token} to {actual}, expected {expected}"
            ));
        }
    }
    Ok(())
}

fn manifest_contents() -> String {
    format!(
        "{MODEL_REVISION}\n{MODEL_SHA256}\n{TOKENIZER_REVISION}\n{TOKENIZER_SHA256}\n{LICENSE_SHA256}\n{NOTICE_SHA256}\n"
    )
}

fn artifact_specs() -> [(&'static str, u64, &'static str); 4] {
    [
        (MODEL_FILENAME, MODEL_SIZE, MODEL_SHA256),
        (TOKENIZER_FILENAME, TOKENIZER_SIZE, TOKENIZER_SHA256),
        (LICENSE_FILENAME, LICENSE_SIZE, LICENSE_SHA256),
        (NOTICE_FILENAME, NOTICE_SIZE, NOTICE_SHA256),
    ]
}

fn artifacts_ready(model_dir: &Path) -> bool {
    fs::read_to_string(model_dir.join(MANIFEST_FILENAME))
        .is_ok_and(|manifest| manifest == manifest_contents())
        && artifact_specs()
            .into_iter()
            .all(|(filename, size, _)| file_has_size(&model_dir.join(filename), size))
}

fn downloaded_bytes_on_disk(model_dir: &Path) -> u64 {
    artifact_specs()
        .into_iter()
        .map(|(filename, size, _)| artifact_downloaded_bytes(model_dir, filename, size))
        .sum()
}

fn artifact_downloaded_bytes(model_dir: &Path, filename: &str, expected: u64) -> u64 {
    let complete = model_dir.join(filename);
    if file_has_size(&complete, expected) {
        return expected;
    }
    model_dir
        .join(format!("{filename}.partial"))
        .metadata()
        .map(|metadata| metadata.len().min(expected))
        .unwrap_or(0)
}

fn file_has_size(path: &Path, expected: u64) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.len() == expected)
}

async fn verified_file(path: &Path, expected_size: u64, expected_sha256: &str) -> Result<bool> {
    if !file_has_size(path, expected_size) {
        return Ok(false);
    }
    let path = path.to_path_buf();
    let expected_sha256 = expected_sha256.to_string();
    tokio::task::spawn_blocking(move || verified_file_sync(&path, expected_size, &expected_sha256))
        .await
        .context("S1-mini verification task panicked")?
}

fn verified_file_sync(path: &Path, expected_size: u64, expected_sha256: &str) -> Result<bool> {
    if !file_has_size(path, expected_size) {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()) == expected_sha256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::pre_tokenizers::whitespace::Whitespace;

    #[test]
    fn prompt_matches_the_model_card_contract() {
        assert_eq!(
            build_prompt(
                "so um send the report by uh friday",
                S1Styling::SemiFormal,
                S1Structure::Prose,
                S1Context::General,
            ),
            concat!(
                "<|im_start|>system\n",
                "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.",
                "<|im_end|>\n<|im_start|>user\n",
                "[Styling: semi-formal] [Structure: prose] [Context: general]\n",
                "so um send the report by uh friday",
                "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
            )
        );
    }

    #[test]
    fn all_control_values_match_the_trained_vocabulary() {
        assert_eq!(S1Styling::Casual.as_control_value(), "casual");
        assert_eq!(S1Styling::SemiCasual.as_control_value(), "semi-casual");
        assert_eq!(S1Styling::SemiFormal.as_control_value(), "semi-formal");
        assert_eq!(S1Styling::Formal.as_control_value(), "formal");
        assert_eq!(S1Structure::Prose.as_control_value(), "prose");
        assert_eq!(S1Structure::Lists.as_control_value(), "lists");
        assert_eq!(S1Context::General.as_control_value(), "general");
        assert_eq!(S1Context::Email.as_control_value(), "email");
    }

    #[test]
    fn max_new_token_formula_uses_a_safe_ceiling() {
        assert_eq!(max_new_tokens(0), 32);
        assert_eq!(max_new_tokens(1), 34);
        assert_eq!(max_new_tokens(10), 45);
        assert_eq!(max_new_tokens(1_000), 1_332);
    }

    #[test]
    fn long_chunk_boundary_prefers_paragraph_then_sentence_then_whitespace() {
        let paragraph_text = "start words. middle words\n\nnext paragraph tail";
        assert_eq!(
            select_chunk_boundary(paragraph_text, 6, 35),
            Some(paragraph_text.find("\n\n").unwrap() + 2)
        );

        let sentence_text = "start words middle sentence. next sentence tail";
        assert_eq!(
            select_chunk_boundary(sentence_text, 6, 35),
            Some(sentence_text.find('.').unwrap() + 1)
        );

        let whitespace_text = "start alpha beta gamma delta tail";
        let boundary = select_chunk_boundary(whitespace_text, 10, 27).unwrap();
        assert!((10..=27).contains(&boundary));
        assert!(whitespace_text[boundary..].starts_with(char::is_whitespace));
    }

    #[test]
    fn tokenizer_aware_chunking_keeps_every_chunk_within_the_model_limit() {
        let model = WordLevel::builder()
            .vocab(
                [("<unk>".to_string(), 0), ("word".to_string(), 1)]
                    .into_iter()
                    .collect(),
            )
            .unk_token("<unk>".to_string())
            .build()
            .unwrap();
        let mut tokenizer = Tokenizer::new(model);
        tokenizer.with_pre_tokenizer(Some(Whitespace {}));
        let transcript = format!("{}\n\n{}", "word ".repeat(900), "word ".repeat(1_400));
        let chunks = split_transcript_chunks(&tokenizer, &transcript).unwrap();

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.split_whitespace().count())
                .sum::<usize>(),
            2_300
        );
        assert!(chunks.iter().all(
            |chunk| tokenizer.encode(chunk.as_str(), false).unwrap().len() <= MAX_TRANSCRIPT_TOKENS
        ));
    }

    #[test]
    fn both_documented_eos_tokens_stop_generation() {
        assert!(is_eos(EOS_IM_END));
        assert!(is_eos(EOS_END_OF_TEXT));
        assert!(!is_eos(0));
    }

    #[test]
    fn reserved_control_tokens_are_rejected() {
        for token in RESERVED_INPUT_TOKENS {
            let error = reject_reserved_tokens(&format!("hello {token} goodbye")).unwrap_err();
            assert!(error.to_string().contains(token));
        }
        assert!(reject_reserved_tokens("ordinary dictated text").is_ok());
    }

    #[test]
    fn oversized_transcript_error_is_clear() {
        assert!(validate_transcript_token_count(MAX_TRANSCRIPT_TOKENS).is_ok());
        let error = validate_transcript_token_count(MAX_TRANSCRIPT_TOKENS + 1).unwrap_err();
        assert!(error.to_string().contains("at most 1000 transcript tokens"));
    }

    #[test]
    fn empty_output_is_a_valid_decoded_value() {
        let output = String::new();
        assert_eq!(Some(output), Some(String::new()));
    }

    #[test]
    fn lifecycle_operations_are_mutually_exclusive() {
        let operation = Arc::new(AtomicU8::new(OPERATION_IDLE));
        let generation = OperationGuard::claim(&operation, OPERATION_GENERATION, "busy").unwrap();
        let competing = Arc::clone(&operation);
        assert!(std::thread::spawn(move || {
            OperationGuard::claim(&competing, OPERATION_DOWNLOAD, "busy").is_err()
        })
        .join()
        .unwrap());
        drop(generation);
        assert!(OperationGuard::claim(&operation, OPERATION_MUTATION, "busy").is_ok());
    }

    #[test]
    fn manifest_covers_runtime_and_legal_artifacts() {
        let manifest = manifest_contents();
        for expected in [
            MODEL_SHA256,
            TOKENIZER_SHA256,
            LICENSE_SHA256,
            NOTICE_SHA256,
        ] {
            assert!(manifest.contains(expected));
        }
        assert_eq!(artifact_specs().len(), 4);
    }

    /// Real Q4 integration gate. Place the pinned artifacts in
    /// `<system temp>/s1-mini-runtime` (or set `HANDY_S1_SMOKE_DIR`) and run:
    /// `cargo test official_s1_mini_q4_smoke -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the pinned 484 MB GGUF and tokenizer artifacts"]
    fn official_s1_mini_q4_smoke() {
        let directory = std::env::var_os("HANDY_S1_SMOKE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("s1-mini-runtime"));
        let mut runtime = S1MiniRuntime::load(
            &directory.join(MODEL_FILENAME),
            &directory.join(TOKENIZER_FILENAME),
            "so um i need to like send the the report by uh friday no wait make that thursday",
            S1Styling::SemiFormal,
            S1Structure::Prose,
            S1Context::General,
        )
        .expect("load pinned official S1-mini artifacts");
        let output = runtime
            .generate(
                "so um i need to like send the the report by uh friday no wait make that thursday",
                S1Styling::SemiFormal,
                S1Structure::Prose,
                S1Context::General,
                || false,
            )
            .expect("run greedy S1-mini inference");
        // The published card table says `I need ...`, while the pinned Q4
        // artifact consistently returns this text in Candle and llama.cpp.
        assert_eq!(output, "So I need to send the report by Thursday.");
    }
}
