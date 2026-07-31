//! Shared direct-HTTP model download transport.
//!
//! Both legacy URL models and Hugging Face mirror fallbacks use this module;
//! source-specific orchestration and finalization remain in the parent module.

use super::{DownloadProgress, ModelManager};
use anyhow::Result;
use futures_util::StreamExt;
use hf_hub::api::tokio::CancellationToken;
use log::{info, warn};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::Emitter;

/// Bound on connection setup for direct HTTP downloads (mirror + URL models).
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// No headers, body bytes, or hf-hub progress for this long means the transfer
/// is wedged, not slow: direct downloads error out (keeping the partial for
/// resume) and HF attempts are cancelled by a watchdog — either way the retry
/// loop and mirror fallback take over instead of hanging forever.
pub(super) const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Start offset of a `Content-Range: bytes <start>-<end>/<total>` header.
fn content_range_start(value: &str) -> Option<u64> {
    let range = value.trim().strip_prefix("bytes")?.trim_start();
    range.split('-').next()?.trim().parse().ok()
}

/// How a [`ModelManager::download_http_resumable`] call ended, cancellation
/// being an outcome (partial kept, no error surfaced) rather than a failure.
#[derive(Debug)]
pub(super) enum HttpDownloadOutcome {
    Completed,
    Cancelled,
}

/// Side-channel notifications from the resumable HTTP downloader, decoupled
/// from Tauri (the production wrapper maps them onto app events) so the
/// transport logic is testable without an `AppHandle`.
enum HttpDownloadEvent<'a> {
    Progress(&'a DownloadProgress),
    VerificationStarted,
    VerificationCompleted,
}

impl ModelManager {
    /// Verifies the SHA256 of `path` against `expected_sha256` (if provided).
    /// On mismatch or read error the partial file is deleted and an error is returned,
    /// so the next download attempt always starts from a clean state.
    /// When `expected_sha256` is `None` (custom user models) verification is skipped.
    fn verify_sha256(path: &Path, expected_sha256: Option<&str>, model_id: &str) -> Result<()> {
        let Some(expected) = expected_sha256 else {
            return Ok(());
        };
        match Self::compute_sha256(path) {
            Ok(actual) if actual == expected => {
                info!("SHA256 verified for model {}", model_id);
                Ok(())
            }
            Ok(actual) => {
                warn!(
                    "SHA256 mismatch for model {}: expected {}, got {}",
                    model_id, expected, actual
                );
                let _ = fs::remove_file(path);
                Err(anyhow::anyhow!(
                    "Download verification failed for model {}: file is corrupt. Please retry.",
                    model_id
                ))
            }
            Err(e) => {
                let _ = fs::remove_file(path);
                Err(anyhow::anyhow!(
                    "Failed to verify download for model {}: {}. Please retry.",
                    model_id,
                    e
                ))
            }
        }
    }

    /// Computes the SHA256 hex digest of a file, reading in 64KB chunks to handle large models.
    fn compute_sha256(path: &Path) -> Result<String> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Emit verification events around a blocking sha256 check of `path`.
    /// On mismatch `verify_sha256` deletes the file, so the next attempt (or
    /// next source) starts clean. A `None` hash skips checking (custom models).
    async fn verify_file_with_events(
        model_id: &str,
        path: &Path,
        expected_sha256: Option<&str>,
        emit: &(dyn Fn(HttpDownloadEvent<'_>) + Send + Sync),
    ) -> Result<()> {
        emit(HttpDownloadEvent::VerificationStarted);
        let path = path.to_path_buf();
        let expected = expected_sha256.map(str::to_string);
        let id = model_id.to_string();
        tokio::task::spawn_blocking(move || Self::verify_sha256(&path, expected.as_deref(), &id))
            .await
            .map_err(|e| anyhow::anyhow!("SHA256 task panicked: {}", e))??;
        emit(HttpDownloadEvent::VerificationCompleted);
        Ok(())
    }

    /// [`Self::download_http_resumable_with_events`] wired to the Tauri event
    /// bus — the production entry point.
    pub(super) async fn download_http_resumable(
        &self,
        model_id: &str,
        url: &str,
        partial_path: &Path,
        expected_size: Option<u64>,
        expected_sha256: Option<&str>,
        cancel_token: &CancellationToken,
    ) -> Result<HttpDownloadOutcome> {
        let app_handle = self.app_handle.clone();
        let id = model_id.to_string();
        Self::download_http_resumable_with_events(
            model_id,
            url,
            partial_path,
            expected_size,
            expected_sha256,
            cancel_token,
            &move |event| {
                let _ = match event {
                    HttpDownloadEvent::Progress(progress) => {
                        app_handle.emit("model-download-progress", progress)
                    }
                    HttpDownloadEvent::VerificationStarted => {
                        app_handle.emit("model-verification-started", &id)
                    }
                    HttpDownloadEvent::VerificationCompleted => {
                        app_handle.emit("model-verification-completed", &id)
                    }
                };
            },
        )
        .await
    }

    /// The one resumable HTTP downloader, shared by the mirror fallback and
    /// URL-sourced models: fetch `url` into `partial_path`, resuming what's
    /// already there, and leave verified bytes in `partial_path` on success —
    /// finalizing (rename / extract) is the caller's job. Takes progress and
    /// verification notifications as a callback instead of touching Tauri, so
    /// the failure-mode behavior below is exercised by tests against a local
    /// socket server.
    ///
    /// Robustness properties, in the order the failure modes appear:
    /// - a partial already at the expected size (crash between completion and
    ///   finalize) is verified and accepted instead of asking the server for
    ///   `Range: bytes=<EOF>-` and looping on 416 forever; an oversized one is
    ///   deleted; a live 416 finishes the partial only when a hash can prove
    ///   it, and otherwise clears it
    /// - connection setup and every body chunk are bounded by
    ///   [`HTTP_CONNECT_TIMEOUT`] / [`DOWNLOAD_STALL_TIMEOUT`] and race the
    ///   cancel token, so a wedged transfer can neither hang the download
    ///   forever nor ignore a cancel
    /// - a 200 to a Range request (server ignored it) restarts from zero
    ///   rather than appending the whole file to the partial; a 206 must start
    ///   exactly at our offset or the partial is discarded
    /// - a server claiming or sending more than the expected size is cut off
    ///   at the first excess byte, not trusted until it closes the stream
    /// - the final bytes are checked against `expected_size` (catalog, or
    ///   content-length when unknown) and `expected_sha256` before returning
    async fn download_http_resumable_with_events(
        model_id: &str,
        url: &str,
        partial_path: &Path,
        expected_size: Option<u64>,
        expected_sha256: Option<&str>,
        cancel_token: &CancellationToken,
        emit: &(dyn Fn(HttpDownloadEvent<'_>) + Send + Sync),
    ) -> Result<HttpDownloadOutcome> {
        let mut resume_from = partial_path.metadata().map(|m| m.len()).unwrap_or(0);

        if let Some(expected) = expected_size {
            if resume_from > expected {
                let _ = fs::remove_file(partial_path);
                resume_from = 0;
            } else if resume_from == expected && expected > 0 {
                info!(
                    "Partial download of {} is already full-size; verifying",
                    model_id
                );
                Self::verify_file_with_events(model_id, partial_path, expected_sha256, emit)
                    .await?;
                return Ok(HttpDownloadOutcome::Completed);
            }
        }

        if resume_from > 0 {
            info!(
                "Resuming download of {} from byte {}",
                model_id, resume_from
            );
        } else {
            info!("Starting fresh download of {} from {}", model_id, url);
        }

        let client = reqwest::Client::builder()
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .build()?;
        let mut request = client.get(url);
        if resume_from > 0 {
            request = request.header("Range", format!("bytes={}-", resume_from));
        }
        let response = tokio::select! {
            r = tokio::time::timeout(DOWNLOAD_STALL_TIMEOUT, request.send()) => r
                .map_err(|_| anyhow::anyhow!(
                    "no response within {}s from {}",
                    DOWNLOAD_STALL_TIMEOUT.as_secs(), url
                ))??,
            _ = cancel_token.cancelled() => return Ok(HttpDownloadOutcome::Cancelled),
        };

        // 416 to our Range request means its start is at or past the object's
        // end. With a catalog size in hand that can only mean the server's
        // object is *smaller* than expected (a full-size partial never issues
        // a request — handled above), and with no hash there is no trusted
        // signal to bless the partial: both restart clean. Only a hash can
        // genuinely finish a partial here. Without a Range in flight a 416 is
        // just a broken server, which the generic status check below rejects.
        if resume_from > 0 && response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            if expected_size.is_some() || expected_sha256.is_none() {
                let _ = fs::remove_file(partial_path);
                return Err(anyhow::anyhow!(
                    "server object ends before the expected size (HTTP 416)"
                ));
            }
            Self::verify_file_with_events(model_id, partial_path, expected_sha256, emit).await?;
            return Ok(HttpDownloadOutcome::Completed);
        }
        // A 200 to a Range request means the server ignored it and is sending
        // the whole file; appending it to the partial would corrupt the model.
        if resume_from > 0 && response.status() == reqwest::StatusCode::OK {
            let _ = fs::remove_file(partial_path);
            resume_from = 0;
        }
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "server returned HTTP {}",
                response.status()
            ));
        }
        // On a 206, trust but verify the offset: a reply starting anywhere but
        // exactly our partial's end would silently corrupt the file on append.
        if resume_from > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            let starts_at = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(content_range_start);
            if starts_at != Some(resume_from) {
                let _ = fs::remove_file(partial_path);
                return Err(anyhow::anyhow!(
                    "server returned Content-Range starting at {:?}, expected {}",
                    starts_at,
                    resume_from
                ));
            }
        }
        // When the catalog pins the size, a server advertising a different
        // total is already misbehaving — reject before writing anything.
        if let (Some(expected), Some(len)) = (expected_size, response.content_length()) {
            if resume_from + len != expected {
                return Err(anyhow::anyhow!(
                    "server advertises {} bytes, expected {}",
                    resume_from + len,
                    expected
                ));
            }
        }

        let known_total =
            expected_size.or_else(|| response.content_length().map(|l| resume_from + l));
        let total_size = known_total.unwrap_or(0);
        let mut downloaded = resume_from;
        let mut file = if resume_from > 0 {
            std::fs::OpenOptions::new()
                .append(true)
                .open(partial_path)?
        } else {
            std::fs::File::create(partial_path)?
        };

        let emit_progress = |downloaded: u64| {
            emit(HttpDownloadEvent::Progress(&DownloadProgress {
                model_id: model_id.to_string(),
                downloaded,
                total: total_size,
                percentage: if total_size > 0 {
                    (downloaded as f64 / total_size as f64) * 100.0
                } else {
                    0.0
                },
            }));
        };
        emit_progress(downloaded);

        // Throttle progress events to max 10/sec (100ms intervals)
        let mut last_emit = Instant::now();
        let throttle = Duration::from_millis(100);
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                c = tokio::time::timeout(DOWNLOAD_STALL_TIMEOUT, stream.next()) => match c {
                    // Stalled mid-body: keep the partial for resume.
                    Err(_) => return Err(anyhow::anyhow!(
                        "transfer stalled: no data for {}s",
                        DOWNLOAD_STALL_TIMEOUT.as_secs()
                    )),
                    Ok(None) => break,
                    Ok(Some(chunk)) => chunk?,
                },
                _ = cancel_token.cancelled() => {
                    // Keep the partial for resume; caller handles state cleanup.
                    return Ok(HttpDownloadOutcome::Cancelled);
                }
            };
            // An untrusted server must not be able to fill the disk: cut the
            // transfer at the first byte past the known total instead of
            // trusting it to eventually close the stream. Everything written
            // so far is tainted by a provably-misbehaving server — clear it.
            if let Some(cap) = known_total {
                if downloaded + chunk.len() as u64 > cap {
                    drop(file);
                    let _ = fs::remove_file(partial_path);
                    return Err(anyhow::anyhow!(
                        "server sent more than the expected {} bytes",
                        cap
                    ));
                }
            }
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            if last_emit.elapsed() >= throttle {
                emit_progress(downloaded);
                last_emit = Instant::now();
            }
        }
        file.flush()?;
        drop(file);
        emit_progress(downloaded);

        if let Some(expected) = known_total {
            let actual = partial_path.metadata()?.len();
            if actual != expected {
                let _ = fs::remove_file(partial_path);
                return Err(anyhow::anyhow!(
                    "download incomplete: expected {} bytes, got {}",
                    expected,
                    actual
                ));
            }
        }

        // The catalog hash is the trust anchor: for a mirror (an untrusted
        // host) this verification is what makes the fallback safe at all.
        Self::verify_file_with_events(model_id, partial_path, expected_sha256, emit).await?;
        Ok(HttpDownloadOutcome::Completed)
    }
}

#[cfg(test)]
mod tests;
