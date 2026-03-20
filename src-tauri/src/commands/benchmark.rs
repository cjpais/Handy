use crate::audio_toolkit::{load_wav_samples, wav_duration_secs};
use crate::managers::history::HistoryManager;
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use log::{info, warn};
use serde::Serialize;
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use transcribe_rs::accel;

/// Newtype wrapper for the benchmark cancellation flag to avoid Tauri state collisions.
pub struct BenchmarkCancelFlag(pub Arc<AtomicBool>);

/// Cancel a running benchmark.
#[tauri::command]
#[specta::specta]
pub fn cancel_benchmark(cancel_flag: State<'_, BenchmarkCancelFlag>) {
    cancel_flag.0.store(true, Ordering::Relaxed);
    info!("benchmark: cancellation requested");
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct BenchmarkProgress {
    pub trial: u32,
    pub total: u32,
    pub thread_count: u8,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct BenchmarkTiming {
    pub thread_count: u8,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct BenchmarkResult {
    pub best_thread_count: u8,
    pub best_time_ms: u64,
    pub audio_duration_ms: u64,
    pub model_name: String,
    /// All trial results sorted by thread count, for display in the results UI.
    pub all_timings: Vec<BenchmarkTiming>,
    /// True if the benchmark was cancelled before completion.
    pub cancelled: bool,
}

/// RAII guard that restores the original ORT thread count on drop.
/// Ensures restore happens even on panic.
struct ThreadCountGuard {
    original: u8,
}

impl Drop for ThreadCountGuard {
    fn drop(&mut self) {
        accel::set_ort_intra_threads(self.original);
        info!(
            "benchmark: restored ORT thread count to {} (0=auto)",
            self.original
        );
    }
}

/// Coarse grid of thread counts to test. Filtered to ≤ max_cores at runtime.
const THREAD_GRID: &[u8] = &[1, 2, 4, 6, 8, 12, 16, 20, 24, 32];

/// Run a single timed transcription trial with the given thread count.
/// If `reload` is true, unloads and reloads the model to apply the new thread count.
/// If false, uses the model as-is (for the first trial when it's already loaded).
/// Returns elapsed milliseconds, or None if the trial failed.
fn run_trial(
    transcription_manager: &TranscriptionManager,
    model_id: &str,
    samples: &[f32],
    thread_count: u8,
    reload: bool,
) -> Option<u64> {
    if reload {
        accel::set_ort_intra_threads(thread_count);

        if let Err(e) = transcription_manager.unload_model() {
            warn!(
                "benchmark: failed to unload model for thread_count={}: {}",
                thread_count, e
            );
            return None;
        }

        if let Err(e) = transcription_manager.load_model(model_id) {
            warn!(
                "benchmark: failed to load model for thread_count={}: {}",
                thread_count, e
            );
            return None;
        }
    }

    let start = std::time::Instant::now();
    match transcription_manager.transcribe(samples.to_vec()) {
        Ok(_) => Some(start.elapsed().as_millis() as u64),
        Err(e) => {
            warn!(
                "benchmark: transcription failed for thread_count={}: {}",
                thread_count, e
            );
            None
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn benchmark_ort_thread_count(
    app: AppHandle,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    history_manager: State<'_, Arc<HistoryManager>>,
    model_manager: State<'_, Arc<ModelManager>>,
    cancel_flag: State<'_, BenchmarkCancelFlag>,
) -> Result<BenchmarkResult, String> {
    // Reset cancellation flag
    cancel_flag.0.store(false, Ordering::Relaxed);

    // Check that nothing else is currently loading
    if !transcription_manager.is_loading_idle() {
        return Err(
            "A model is currently loading. Wait for it to complete before running the benchmark."
                .to_string(),
        );
    }

    let settings = crate::settings::get_settings(&app);
    let original_thread_count = settings.ort_thread_count;

    // Determine which model to benchmark — use currently loaded, or load the selected model
    let model_id = match transcription_manager.get_current_model() {
        Some(id) => id,
        None => {
            // Model not loaded (e.g. idle-unloaded). Load the user's selected model.
            let selected = settings.selected_model.clone();
            if selected.is_empty() {
                return Err(
                    "No model selected. Select a model before running the benchmark.".to_string(),
                );
            }
            info!(
                "benchmark: no model loaded, loading selected model '{}'",
                selected
            );
            transcription_manager
                .load_model(&selected)
                .map_err(|e| format!("Failed to load model for benchmark: {}", e))?;
            selected
        }
    };

    // Get model display name from model manager (not from settings, which can diverge)
    let model_name = model_manager
        .get_model_info(&model_id)
        .map(|info| info.name)
        .unwrap_or_else(|| model_id.clone());

    // Query recordings from history
    let entries = history_manager
        .get_history_entries()
        .await
        .map_err(|e| format!("Failed to query history: {}", e))?;

    if entries.is_empty() {
        return Err("No recordings available. Record some audio first.".to_string());
    }

    // Gather WAV paths and their durations, sorted longest-first
    let mut recordings_with_duration: Vec<(std::path::PathBuf, f32)> = entries
        .iter()
        .filter_map(|e| {
            let path = history_manager.get_audio_file_path(&e.file_name);
            if path.exists() {
                wav_duration_secs(&path).ok().map(|d| (path, d))
            } else {
                None
            }
        })
        .collect();

    if recordings_with_duration.is_empty() {
        return Err("No recordings available. Record some audio first.".to_string());
    }

    recordings_with_duration
        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Select recordings totalling at least 15 seconds, preferring fewer longer ones
    const TARGET_SECS: f32 = 15.0;
    let mut total_secs = 0.0f32;
    let mut selected_paths: Vec<std::path::PathBuf> = Vec::new();
    for (path, dur) in &recordings_with_duration {
        selected_paths.push(path.clone());
        total_secs += dur;
        if total_secs >= TARGET_SECS {
            break;
        }
    }
    let audio_duration_ms = (total_secs * 1000.0) as u64;

    // Load and concatenate audio samples
    let samples: Vec<f32> = {
        let mut all_samples = Vec::new();
        for path in &selected_paths {
            match load_wav_samples(path) {
                Ok(s) => all_samples.extend(s),
                Err(e) => warn!("benchmark: failed to load {}: {}", path.display(), e),
            }
        }
        all_samples
    };

    if samples.is_empty() {
        return Err("Failed to load audio samples for benchmark.".to_string());
    }

    let max_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8)
        .min(32) as u8;

    // Build the grid of thread counts to test, excluding the current setting
    // (we'll test that first without reloading).
    // Order: mid-range first (fast feedback), then max, then 1, then fill remaining.
    let grid: Vec<u8> = THREAD_GRID
        .iter()
        .copied()
        .filter(|&t| t <= max_threads && t != original_thread_count)
        .collect();

    let mut candidates: Vec<u8> = Vec::with_capacity(grid.len());
    let mid = max_threads / 2;
    // Find the grid value closest to mid and add it first
    if let Some(&mid_val) = grid
        .iter()
        .min_by_key(|&&t| (t as i16 - mid as i16).unsigned_abs())
    {
        candidates.push(mid_val);
    }
    // Then max, then 1
    if let Some(&max_val) = grid.last() {
        if !candidates.contains(&max_val) {
            candidates.push(max_val);
        }
    }
    if let Some(&min_val) = grid.first() {
        if !candidates.contains(&min_val) {
            candidates.push(min_val);
        }
    }
    // Then fill remaining in sorted order
    for &t in &grid {
        if !candidates.contains(&t) {
            candidates.push(t);
        }
    }

    // Total = grid candidates + 1 for the current setting tested first
    let total_trials = (candidates.len() + 1) as u32;

    let transcription_manager_clone = transcription_manager.inner().clone();
    let model_id_clone = model_id.clone();
    let model_name_clone = model_name.clone();
    let cancel_clone = cancel_flag.0.clone();
    // Use the actual current thread count (0 = auto/all cores)
    let current_tc = original_thread_count;

    let result = tokio::task::spawn_blocking(move || {
        // RAII guard ensures thread count is restored even on panic
        let _tc_guard = ThreadCountGuard {
            original: original_thread_count,
        };

        let mut timings: Vec<(u8, u64)> = Vec::new();

        info!(
            "benchmark: testing {} thread counts up to {}, audio={}ms",
            total_trials, max_threads, audio_duration_ms
        );

        // Trial 1: test with the currently-loaded model (no reload needed)
        info!(
            "benchmark: trial 1/{} — {} threads (current, no reload)",
            total_trials, current_tc
        );
        if let Some(ms) = run_trial(
            &transcription_manager_clone,
            &model_id_clone,
            &samples,
            current_tc,
            false,
        ) {
            timings.push((current_tc, ms));
            let _ = app.emit(
                "ort-benchmark-progress",
                BenchmarkProgress {
                    trial: 1,
                    total: total_trials,
                    thread_count: current_tc,
                    elapsed_ms: ms,
                },
            );
            info!(
                "benchmark: thread_count={} (current) => {}ms",
                current_tc, ms
            );
        }

        // Remaining trials: reload model with each candidate thread count
        for (i, &tc) in candidates.iter().enumerate() {
            // Check cancellation between trials
            if cancel_clone.load(Ordering::Relaxed) {
                info!("benchmark: cancelled by user after {} trials", i + 1);
                let _ = transcription_manager_clone.unload_model();
                let _ = transcription_manager_clone.load_model(&model_id_clone);
                return Ok(BenchmarkResult {
                    best_thread_count: 0,
                    best_time_ms: 0,
                    audio_duration_ms,
                    model_name: model_name_clone,
                    all_timings: vec![],
                    cancelled: true,
                });
            }

            let trial_num = (i + 2) as u32; // +2 because trial 1 was the current setting
            info!(
                "benchmark: trial {}/{} — {} threads",
                trial_num, total_trials, tc
            );

            let elapsed = run_trial(
                &transcription_manager_clone,
                &model_id_clone,
                &samples,
                tc,
                true,
            );

            if let Some(ms) = elapsed {
                timings.push((tc, ms));
                let _ = app.emit(
                    "ort-benchmark-progress",
                    BenchmarkProgress {
                        trial: trial_num,
                        total: total_trials,
                        thread_count: tc,
                        elapsed_ms: ms,
                    },
                );
                info!("benchmark: thread_count={} => {}ms", tc, ms);
            } else {
                warn!("benchmark: thread_count={} trial failed, skipping", tc);
            }
        }

        if timings.is_empty() {
            // Restore model before returning (guard restores thread count)
            let _ = transcription_manager_clone.unload_model();
            let _ = transcription_manager_clone.load_model(&model_id_clone);
            return Err("All benchmark trials failed.".to_string());
        }

        let (best_thread_count, best_time_ms) =
            timings.iter().min_by_key(|(_, ms)| *ms).copied().unwrap();

        // Sort timings by thread count for display
        timings.sort_by_key(|(tc, _)| *tc);
        let all_timings = timings
            .iter()
            .map(|&(thread_count, elapsed_ms)| BenchmarkTiming {
                thread_count,
                elapsed_ms,
            })
            .collect();

        info!(
            "benchmark: best_thread_count={} best_time_ms={} audio_duration_ms={}",
            best_thread_count, best_time_ms, audio_duration_ms
        );

        // Restore model with original settings (guard restores thread count on drop)
        let _ = transcription_manager_clone.unload_model();
        let _ = transcription_manager_clone.load_model(&model_id_clone);

        Ok(BenchmarkResult {
            best_thread_count,
            best_time_ms,
            audio_duration_ms,
            model_name: model_name_clone,
            all_timings,
            cancelled: false,
        })
    })
    .await
    .map_err(|e| format!("Benchmark task panicked: {}", e))??;

    Ok(result)
}
