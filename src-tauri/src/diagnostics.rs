//! Background probes that make silent failures visible in logs.
//!
//! The post-sleep "stuck transcribing" class of bugs (#1213) produces no log
//! lines through normal event-driven logging — a wedged pipeline looks
//! identical to a healthy idle one. These probes supply the missing ground
//! truth: when the system slept, and whether the async runtime is still
//! polling tasks. Both are cheap enough to run unconditionally.

use log::{debug, warn};
use std::time::{Duration, Instant, SystemTime};

/// Detects system suspend/resume (and hard process stalls) by watching for
/// jumps in wall-clock and monotonic time across a 1s sleep. The wall clock
/// always advances through a suspend; whether the monotonic clock does is
/// platform-dependent — logging both shows which, and brackets every
/// post-resume failure report with a definitive "the system slept here"
/// marker.
pub fn spawn_suspend_watcher() {
    const TICK: Duration = Duration::from_secs(1);
    const REPORT_GAP: Duration = Duration::from_secs(5);
    std::thread::spawn(move || {
        let mut last_mono = Instant::now();
        let mut last_wall = SystemTime::now();
        loop {
            std::thread::sleep(TICK);
            let mono_gap = last_mono.elapsed();
            let wall_gap = last_wall.elapsed().unwrap_or(Duration::ZERO);
            if wall_gap > REPORT_GAP || mono_gap > REPORT_GAP {
                warn!(
                    "system suspend/stall detected: wall clock advanced {wall_gap:.1?} and \
                     monotonic clock {mono_gap:.1?} across a ~1s sleep — audio devices, \
                     Bluetooth links, and keyboard hooks may have been reset"
                );
            }
            last_mono = Instant::now();
            last_wall = SystemTime::now();
        }
    });
}

/// Ticks on the shared tokio runtime every 30s. If heartbeats stop appearing
/// in a debug log (or arrive late), the runtime's workers are blocked — the
/// signature of the "spawned transcription task never started" variant of the
/// post-sleep wedge.
pub fn spawn_async_runtime_heartbeat() {
    const INTERVAL: Duration = Duration::from_secs(30);
    tauri::async_runtime::spawn(async move {
        let mut tick: u64 = 0;
        let mut last = Instant::now();
        loop {
            tokio::time::sleep(INTERVAL).await;
            tick += 1;
            let gap = last.elapsed();
            last = Instant::now();
            if gap > INTERVAL + Duration::from_secs(5) {
                warn!(
                    "async-runtime heartbeat #{tick} arrived {gap:.1?} after the previous \
                     one (expected ~{INTERVAL:?}) — the runtime or system was stalled"
                );
            } else {
                debug!("async-runtime heartbeat #{tick}");
            }
        }
    });
}
