//! Signal-level helpers for captured PCM audio.
//!
//! Used to tell *digital silence* — a dead, muted, or wrong input device that
//! delivers an essentially all-zero signal — apart from an ordinary "no speech
//! was recognized". Without this distinction a misrouted input (for example a
//! monitor loopback selected instead of a microphone) produces an empty
//! transcription with nothing in the logs to explain why.
//!
//! See <https://github.com/cjpais/Handy/issues/1899>.

/// Peak level of `samples` in dBFS (decibels relative to full scale, where full
/// scale is amplitude `1.0`). Returns [`f32::NEG_INFINITY`] for an all-zero or
/// empty buffer.
///
/// `dBFS = 20 * log10(peak_amplitude)`. `0` dBFS is a full-scale signal and
/// quieter signals are negative: room tone sits roughly around -60..-45 dBFS,
/// dictation roughly -35..-25 dBFS, and a dead input at or below about
/// -90 dBFS.
pub fn peak_dbfs(samples: &[f32]) -> f32 {
    let peak = samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);

    if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        f32::NEG_INFINITY
    }
}

/// Default threshold below which captured audio is treated as effectively
/// silent. Set well under real room tone (~-45 dBFS) so that only a dead or
/// muted input trips it, never a quiet-but-live microphone.
pub const SILENCE_THRESHOLD_DBFS: f32 = -80.0;

/// Whether `samples` are effectively silent — i.e. their peak level is below
/// `threshold_dbfs`. See [`SILENCE_THRESHOLD_DBFS`] for the default.
pub fn is_effectively_silent(samples: &[f32], threshold_dbfs: f32) -> bool {
    peak_dbfs(samples) < threshold_dbfs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_is_silent() {
        assert_eq!(peak_dbfs(&[]), f32::NEG_INFINITY);
        assert!(is_effectively_silent(&[], SILENCE_THRESHOLD_DBFS));
    }

    #[test]
    fn all_zero_buffer_is_silent() {
        let buf = vec![0.0_f32; 16_000];
        assert_eq!(peak_dbfs(&buf), f32::NEG_INFINITY);
        assert!(is_effectively_silent(&buf, SILENCE_THRESHOLD_DBFS));
    }

    #[test]
    fn full_scale_is_zero_dbfs() {
        let buf = [1.0_f32, -1.0, 0.5, -0.25];
        // Peak amplitude 1.0 -> 0 dBFS.
        assert!((peak_dbfs(&buf) - 0.0).abs() < 1e-4);
        assert!(!is_effectively_silent(&buf, SILENCE_THRESHOLD_DBFS));
    }

    #[test]
    fn half_scale_is_about_minus_six_dbfs() {
        let buf = [0.5_f32, -0.5];
        // 20 * log10(0.5) = -6.0206 dBFS.
        assert!((peak_dbfs(&buf) - (-6.0206)).abs() < 1e-2);
        assert!(!is_effectively_silent(&buf, SILENCE_THRESHOLD_DBFS));
    }

    #[test]
    fn quiet_room_tone_is_not_silent() {
        // ~-54 dBFS, comfortably above the -80 dBFS floor: a live microphone.
        let buf = [0.002_f32, -0.002, 0.0015];
        assert!(peak_dbfs(&buf) > SILENCE_THRESHOLD_DBFS);
        assert!(!is_effectively_silent(&buf, SILENCE_THRESHOLD_DBFS));
    }

    #[test]
    fn dead_input_below_threshold_is_silent() {
        // ~-91 dBFS, matching the dead-input level reported in the bug.
        let buf = [2.8e-5_f32, -2.7e-5];
        assert!(peak_dbfs(&buf) < SILENCE_THRESHOLD_DBFS);
        assert!(is_effectively_silent(&buf, SILENCE_THRESHOLD_DBFS));
    }

    #[test]
    fn threshold_is_respected() {
        let buf = [0.01_f32]; // -40 dBFS
        assert!(is_effectively_silent(&buf, -20.0));
        assert!(!is_effectively_silent(&buf, -60.0));
    }
}
