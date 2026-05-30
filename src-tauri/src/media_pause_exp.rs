use log::debug;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(1);
static CURRENT_RUN_ID: AtomicU64 = AtomicU64::new(0);
static RUN_STARTS: Lazy<Mutex<HashMap<u64, Instant>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn enabled() -> bool {
    matches!(
        std::env::var("HANDY_MEDIA_PAUSE_EXPERIMENT")
            .ok()
            .as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

pub fn begin_run() -> Option<u64> {
    if !enabled() {
        return None;
    }

    let run = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
    CURRENT_RUN_ID.store(run, Ordering::Relaxed);
    RUN_STARTS.lock().unwrap().insert(run, Instant::now());
    log(run, "run_begin", None, None, "result=start");
    Some(run)
}

pub fn current_run() -> Option<u64> {
    let run = CURRENT_RUN_ID.load(Ordering::Relaxed);
    if enabled() && run != 0 {
        Some(run)
    } else {
        None
    }
}

pub fn mark(run: Option<u64>, phase: &str, extra: impl AsRef<str>) {
    if let Some(run) = run {
        log(run, phase, None, None, extra.as_ref());
    }
}

pub fn timed<T>(run: Option<u64>, phase: &str, extra: impl AsRef<str>, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    if let Some(run) = run {
        log(run, phase, Some(start.elapsed()), None, extra.as_ref());
    }
    result
}

pub fn timed_attempt<T>(
    run: Option<u64>,
    phase: &str,
    attempt: usize,
    extra: impl AsRef<str>,
    f: impl FnOnce() -> T,
) -> T {
    let start = Instant::now();
    let result = f();
    if let Some(run) = run {
        log(
            run,
            phase,
            Some(start.elapsed()),
            Some(attempt),
            extra.as_ref(),
        );
    }
    result
}

pub fn env_u64(name: &str, default: u64) -> u64 {
    if !enabled() {
        return default;
    }

    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

pub fn env_usize(name: &str, default: usize) -> usize {
    if !enabled() {
        return default;
    }

    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

pub fn env_bool(name: &str) -> bool {
    enabled()
        && matches!(
            std::env::var(name).ok().as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        )
}

fn log(run: u64, phase: &str, duration: Option<Duration>, attempt: Option<usize>, extra: &str) {
    if !enabled() {
        return;
    }

    let cumulative_ms = RUN_STARTS
        .lock()
        .unwrap()
        .get(&run)
        .map(|start| start.elapsed().as_millis())
        .unwrap_or_default();
    let duration_ms = duration.map(|duration| duration.as_millis()).unwrap_or(0);
    let attempt = attempt
        .map(|attempt| format!(" attempt={attempt}"))
        .unwrap_or_default();

    if extra.is_empty() {
        debug!(
            "media_pause_exp run={run} phase={phase}{attempt} duration_ms={duration_ms} cumulative_ms={cumulative_ms}"
        );
    } else {
        debug!(
            "media_pause_exp run={run} phase={phase}{attempt} duration_ms={duration_ms} cumulative_ms={cumulative_ms} {extra}"
        );
    }
}
