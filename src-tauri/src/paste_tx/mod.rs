//! Clipboard paste as a transaction: publish the transcript, send the paste
//! chord, return, and restore the previous clipboard in the background.
//!
//! The paste keystroke is only *enqueued* when the chord is sent — the target
//! application reads the clipboard whenever its event loop gets to it. A
//! restore that comes first makes it paste the user's old clipboard (#502),
//! and nothing tells us when the target is done. So the restore waits out a
//! quiet period (`clipboard_restore_delay_ms`, 1s by default), which costs
//! nothing: the paste has returned long before.
//!
//! What the quiet period counts from depends on how the transcript was
//! published (`Publish`):
//!
//! - `Plain` (macOS default): an ordinary write. Nothing reports a read, so it
//!   counts from the chord.
//! - `Promise` ("reliable paste", debug-gated): the transcript is a *lazy
//!   promise* and the operating system tells us when a consumer reads it — a
//!   "receipt" — so it counts from the last receipt and a slow target is
//!   waited for:
//!   - Windows: delayed rendering (`SetClipboardData(CF_UNICODETEXT, NULL)`),
//!     the owner window receives `WM_RENDERFORMAT` on read.
//!   - macOS: `declareTypes:owner:` with an owner object, the pasteboard calls
//!     `pasteboard:provideDataForType:` on read.
//!
//! A receipt says that *somebody* read the transcript, not that the paste
//! target did: clipboard managers read on every change, and on macOS only the
//! first read is reported at all. The quiet period is what the target gets
//! when another reader comes first. Two rules apply to receipts:
//!
//! 1. Only receipts observed *after* the paste chord was injected count. A
//!    read before that is an eager third party (clipboard manager, antivirus)
//!    reacting to the clipboard change itself.
//! 2. A bounded timeout caps how long the transcript may occupy the clipboard
//!    when nobody reads it.
//!
//! Either way, restoration only happens while the clipboard is still ours
//! (sequence number / changeCount unchanged, no ownership-lost event). If the
//! user copied something else in the meantime, their action wins.
//!
//! Windows still restores on a fixed delay inside the paste itself unless
//! reliable paste is on (`clipboard::paste_via_clipboard`); Linux always does.

// The shared transaction state is compiled on all platforms (for the unit
// tests), but only the macOS/Windows platform modules consume all of it.
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

/// How long after the *last* observed read the auto-submit Enter is sent.
/// Covers applications that read the clipboard several times per paste (e.g.
/// Chromium probe-then-read). Separate from the restore's quiet period so a
/// longer one does not delay the Enter.
pub(crate) const AUTO_SUBMIT_DELAY: Duration = Duration::from_millis(200);

/// How long after the chord a plain paste's auto-submit Enter is sent: what
/// the paste waited before this module took it over (60ms before the restore,
/// 50ms after it).
pub(crate) const PLAIN_AUTO_SUBMIT_DELAY: Duration = Duration::from_millis(110);

/// How the transcript is put on the clipboard, and so what the restore's quiet
/// period counts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) enum Publish {
    /// An ordinary write. Nothing reports a read: count from the chord.
    Plain,
    /// A lazy promise: count from the last receipt.
    Promise,
}

impl Publish {
    /// What the log lines of a transaction start with.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub fn tag(self) -> &'static str {
        match self {
            Publish::Plain => "[paste]",
            Publish::Promise => "[reliable-paste]",
        }
    }
}

/// Upper bound on how long the transcript may occupy the clipboard before we
/// restore regardless of receipts. Long enough that a realistically loaded
/// target always gets to read first; short enough that a lost keystroke does
/// not strand the transcript on the clipboard for long.
pub(crate) const RESTORE_TIMEOUT: Duration = Duration::from_secs(8);

/// When the chord could not be injected at all, no legitimate receipt can
/// arrive, so restore quickly instead of waiting out the full timeout.
pub(crate) const FAILED_INJECTION_TIMEOUT: Duration = Duration::from_millis(500);

/// Shared, cross-thread record of one paste transaction.
#[derive(Debug)]
pub(crate) struct TxState {
    pub publish: Publish,
    /// How long the transcript stays on the clipboard before the previous one
    /// is restored (`clipboard_restore_delay_ms`), counted from `quiet_since`.
    pub quiet_period: Duration,
    /// When the transcript was published to the clipboard.
    pub published_at: Instant,
    /// When the paste chord was injected. Only receipts *after* this count as
    /// evidence the target read the transcript — earlier reads are eager third
    /// parties reacting to the clipboard change itself.
    pub injected_at: Option<Instant>,
    /// The chord could not be sent; short-circuit the wait.
    pub injection_failed: bool,
    /// When the chord had been sent in full, modifier released.
    pub chord_sent_at: Option<Instant>,
    /// Times at which a consumer requested the clipboard data.
    pub receipts: Vec<Instant>,
    /// Someone else took clipboard ownership (user copied elsewhere, ...).
    pub ownership_lost: bool,
    /// A newer paste transaction settled this one early (see flush logic in
    /// the platform modules).
    pub cancelled: bool,
    /// The post-paste Enter (auto-submit) has been sent for this transaction.
    pub auto_submit_sent: bool,
    /// First post-injection receipt has been logged.
    pub logged_receipt: bool,
}

impl TxState {
    pub fn new(publish: Publish, quiet_period: Duration) -> Self {
        Self {
            publish,
            quiet_period,
            published_at: Instant::now(),
            injected_at: None,
            injection_failed: false,
            chord_sent_at: None,
            receipts: Vec::new(),
            ownership_lost: false,
            cancelled: false,
            auto_submit_sent: false,
            logged_receipt: false,
        }
    }

    /// Records a read receipt, logging the first one that counts as evidence.
    pub fn record_receipt(&mut self, at: Instant) {
        self.receipts.push(at);
        if !self.logged_receipt {
            if let Some(injected) = self.injected_at {
                if at >= injected {
                    self.logged_receipt = true;
                    log::info!(
                        "[reliable-paste] clipboard read {}ms after chord",
                        at.duration_since(injected).as_millis()
                    );
                }
            }
        }
    }

    pub fn last_receipt_after_injection(&self) -> Option<Instant> {
        let injected = self.injected_at?;
        self.receipts.iter().copied().rev().find(|t| *t >= injected)
    }

    // macOS asks `quiet_since` instead, which also covers a plain write.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn any_receipt_after_injection(&self) -> bool {
        self.last_receipt_after_injection().is_some()
    }

    /// The latest sign that the paste is under way, which the quiet period and
    /// the auto-submit Enter count from: the last receipt of a promise, the
    /// chord of a plain write. `None` until there is one.
    pub fn quiet_since(&self) -> Option<Instant> {
        match self.publish {
            Publish::Promise => self.last_receipt_after_injection(),
            Publish::Plain => self.chord_sent_at,
        }
    }

    /// The auto-submit Enter can go out. The clipboard is restored later, once
    /// the quiet period has passed too.
    pub fn auto_submit_due(&self, now: Instant) -> bool {
        let delay = match self.publish {
            Publish::Promise => AUTO_SUBMIT_DELAY,
            Publish::Plain => PLAIN_AUTO_SUBMIT_DELAY,
        };
        self.quiet_since()
            .is_some_and(|since| now.duration_since(since) >= delay)
    }
}

pub(crate) enum WaitDecision {
    KeepWaiting,
    /// Stop waiting; settle the transaction (auto-submit + guarded restore).
    Finish,
}

/// Pure decision: given the current transaction state, keep the transcript on
/// the clipboard for the target, or finish now. Both platform event loops call
/// this.
pub(crate) fn evaluate(state: &TxState, now: Instant) -> WaitDecision {
    if state.ownership_lost || state.cancelled {
        return WaitDecision::Finish;
    }
    if let Some(since) = state.quiet_since() {
        if now.duration_since(since) >= state.quiet_period {
            return WaitDecision::Finish;
        }
    }
    let deadline = if state.injection_failed {
        FAILED_INJECTION_TIMEOUT
    } else {
        RESTORE_TIMEOUT
    };
    if now.duration_since(state.published_at) >= deadline {
        return WaitDecision::Finish;
    }
    WaitDecision::KeepWaiting
}

/// Modifier hold for the receipt-sequenced chord, kept at parity with the
/// legacy path (100ms) for the beta: that hold was added in #165 because real
/// users' systems dropped chords released too quickly, and the beta should
/// validate the receipt mechanism without changing a second variable.
///
/// Once receipts are proven in the field this becomes a safe tuning knob — a
/// chord the target never recognizes produces no receipt and is logged ("no
/// read within timeout") rather than failing silently, so a shorter hold
/// (measured working at 10ms on a fast machine, cutting visible latency from
/// ~110ms to ~20ms) can be tried as its own experiment later.
const CHORD_HOLD_MS: u64 = 100;

/// Sends the platform paste chord for the configured method.
pub(crate) fn send_chord(
    enigo: &mut enigo::Enigo,
    paste_method: &crate::settings::PasteMethod,
) -> Result<(), String> {
    use crate::settings::PasteMethod;
    match paste_method {
        PasteMethod::CtrlV => crate::input::send_paste_ctrl_v(enigo, CHORD_HOLD_MS),
        PasteMethod::CtrlShiftV => crate::input::send_paste_ctrl_shift_v(enigo, CHORD_HOLD_MS),
        PasteMethod::ShiftInsert => crate::input::send_paste_shift_insert(enigo, CHORD_HOLD_MS),
        other => Err(format!(
            "Invalid paste method for clipboard paste: {:?}",
            other
        )),
    }
}

/// The settings a paste takes besides the paste method.
#[derive(Clone, Copy)]
pub(crate) struct PasteOptions {
    /// Windows only publishes promises.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub publish: Publish,
    pub auto_submit: bool,
    pub auto_submit_key: crate::settings::AutoSubmitKey,
    pub clipboard_handling: crate::settings::ClipboardHandling,
    /// See `TxState::quiet_period`.
    pub quiet_period: Duration,
}

/// Attempts the receipt-sequenced paste. Returns `Err` before anything has
/// been published when the platform transaction cannot start, in which case
/// the caller should fall back to the legacy paste path. On `Ok`, publishing
/// and chord injection have completed and the guarded restore (plus
/// auto-submit) finishes asynchronously.
#[cfg(target_os = "windows")]
pub(crate) fn try_reliable_paste(
    text: &str,
    app_handle: &tauri::AppHandle,
    paste_method: &crate::settings::PasteMethod,
    enigo: &mut enigo::Enigo,
    options: PasteOptions,
) -> Result<(), String> {
    platform::run(text, app_handle, paste_method, enigo, options)
}

/// Pastes `text` through the clipboard. Call from a worker thread, as
/// `clipboard::paste` does: it hops to the main thread itself. On `Ok` the
/// chord has been sent and the guarded restore (plus auto-submit) finishes
/// asynchronously. On `Err` nothing was pasted; a transcript that was already
/// published is taken off the clipboard again shortly.
#[cfg(target_os = "macos")]
pub(crate) fn paste(
    text: String,
    app_handle: &tauri::AppHandle,
    paste_method: crate::settings::PasteMethod,
    options: PasteOptions,
    plain_pre_delay: Duration,
) -> Result<(), String> {
    platform::paste(text, app_handle, paste_method, options, plain_pre_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET_PERIOD: Duration = Duration::from_millis(1000);

    fn state_after_publish(published_ago: Duration) -> TxState {
        let mut s = TxState::new(Publish::Promise, QUIET_PERIOD);
        s.published_at = Instant::now() - published_ago;
        s
    }

    #[test]
    fn keeps_waiting_without_receipt_within_timeout() {
        let s = state_after_publish(Duration::from_millis(100));
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn finishes_after_quiet_period_once_read() {
        let mut s = state_after_publish(Duration::from_millis(1300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(1250));
        s.receipts.push(Instant::now() - QUIET_PERIOD);
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn auto_submit_is_due_before_the_quiet_period_ends() {
        let mut s = state_after_publish(Duration::from_millis(400));
        s.injected_at = Some(Instant::now() - Duration::from_millis(350));
        assert!(!s.auto_submit_due(Instant::now()));
        s.receipts.push(Instant::now() - Duration::from_millis(50));
        assert!(!s.auto_submit_due(Instant::now()));
        s.receipts[0] = Instant::now() - AUTO_SUBMIT_DELAY;
        assert!(s.auto_submit_due(Instant::now()));
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn waits_through_quiet_period_after_recent_read() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        s.receipts.push(Instant::now() - Duration::from_millis(50));
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn pre_injection_receipt_does_not_count() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.receipts.push(Instant::now() - Duration::from_millis(200));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        assert!(!s.any_receipt_after_injection());
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn finishes_on_timeout_without_receipt() {
        let s = state_after_publish(RESTORE_TIMEOUT);
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn failed_injection_uses_short_timeout() {
        let mut s = state_after_publish(FAILED_INJECTION_TIMEOUT);
        s.injection_failed = true;
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn plain_paste_counts_the_quiet_period_from_the_chord() {
        let mut s = TxState::new(Publish::Plain, QUIET_PERIOD);
        let now = Instant::now();
        // Nothing reports a read of a plain write; a stray receipt is ignored.
        s.injected_at = Some(now);
        s.receipts.push(now);
        assert!(s.quiet_since().is_none());

        s.chord_sent_at = Some(now);
        let just_before = now + QUIET_PERIOD - Duration::from_millis(1);
        assert!(matches!(
            evaluate(&s, just_before),
            WaitDecision::KeepWaiting
        ));
        assert!(matches!(
            evaluate(&s, now + QUIET_PERIOD),
            WaitDecision::Finish
        ));
    }

    #[test]
    fn plain_paste_sends_auto_submit_shortly_after_the_chord() {
        let mut s = TxState::new(Publish::Plain, QUIET_PERIOD);
        let now = Instant::now();
        assert!(!s.auto_submit_due(now));
        s.chord_sent_at = Some(now);
        assert!(!s.auto_submit_due(now + Duration::from_millis(50)));
        assert!(s.auto_submit_due(now + PLAIN_AUTO_SUBMIT_DELAY));
    }

    #[test]
    fn plain_paste_with_a_failed_chord_restores_quickly() {
        let mut s = TxState::new(Publish::Plain, QUIET_PERIOD);
        s.published_at = Instant::now() - FAILED_INJECTION_TIMEOUT;
        s.injection_failed = true;
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn ownership_loss_finishes_immediately() {
        let mut s = state_after_publish(Duration::from_millis(10));
        s.ownership_lost = true;
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }
}
