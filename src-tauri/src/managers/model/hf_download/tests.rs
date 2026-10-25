use super::*;
use std::time::Duration;

type Events = Arc<Mutex<Vec<HfProgressSnapshot>>>;

fn reporter(parent: &CancellationToken) -> (HfDownloadProgress, CancellationToken, Events) {
    let attempt = parent.child_token();
    let events = Events::default();
    let capture = events.clone();
    (
        HfDownloadProgress::new(attempt.clone(), move |downloaded, total, rate| {
            capture.lock().unwrap().push((downloaded, total, rate));
        }),
        attempt,
        events,
    )
}

#[tokio::test]
async fn exact_size_finishes_without_cancelling() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, events) = reporter(&parent);
    progress.init(100, "model.gguf").await;
    progress.update(40).await;
    progress.update(60).await;
    progress.finish().await;
    assert!(!attempt.is_cancelled());
    assert!(progress.exceeded_total().is_none());
    assert_eq!(events.lock().unwrap().last(), Some(&(100, 100, None)));
}

#[tokio::test]
async fn oversized_transfer_cancels_only_its_attempt_and_never_emits_overrun() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, events) = reporter(&parent);
    progress.init(100, "model.gguf").await;
    progress.update(60).await;
    progress.update(41).await;
    assert!(attempt.is_cancelled());
    assert!(!parent.is_cancelled(), "the mirror must remain usable");
    assert_eq!(progress.exceeded_total(), Some((100, 101)));
    let events_before_finish = events.lock().unwrap().len();
    progress.finish().await;
    progress.update(10_000).await;
    let events = events.lock().unwrap();
    assert_eq!(events.len(), events_before_finish);
    assert!(events
        .iter()
        .all(|(downloaded, total, _)| downloaded <= total));
}

#[tokio::test]
async fn concurrent_reporters_share_one_budget() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, _) = reporter(&parent);
    progress.init(100, "model.gguf").await;
    let mut other_chunk = progress.clone();
    tokio::join!(progress.update(60), other_chunk.update(60));
    assert_eq!(progress.exceeded_total(), Some((100, 120)));
    assert!(attempt.is_cancelled());
    assert!(!parent.is_cancelled());
}

#[tokio::test]
async fn resumed_bytes_count_towards_the_same_total() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, _) = reporter(&parent);
    progress.init(100, "model.gguf").await;
    // hf-hub reports the committed resume offset through update first.
    progress.update(80).await;
    progress.update(20).await;
    assert!(!attempt.is_cancelled());
    progress.update(1).await;
    assert_eq!(progress.exceeded_total(), Some((100, 101)));
    assert!(attempt.is_cancelled());
}

#[tokio::test]
async fn empty_file_accepts_no_bytes_but_rejects_any_body() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, events) = reporter(&parent);
    progress.init(0, "empty").await;
    progress.update(0).await;
    progress.finish().await;
    assert!(!attempt.is_cancelled());
    assert_eq!(events.lock().unwrap().last(), Some(&(0, 0, None)));
    progress.update(1).await;
    assert_eq!(progress.exceeded_total(), Some((0, 1)));
    assert!(attempt.is_cancelled());
}

#[tokio::test]
async fn user_cancellation_is_not_reported_as_size_overrun() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, _) = reporter(&parent);
    progress.init(100, "model.gguf").await;
    parent.cancel();
    assert!(attempt.is_cancelled());
    assert!(progress.exceeded_total().is_none());
}

#[cfg(target_pointer_width = "64")]
#[tokio::test]
async fn byte_counter_overflow_cannot_bypass_the_budget() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, _) = reporter(&parent);
    progress.init(usize::MAX, "model.gguf").await;
    progress.update(usize::MAX).await;
    progress.update(1).await;
    assert!(attempt.is_cancelled());
    assert!(progress.exceeded_total().is_some());
}

#[tokio::test]
async fn hf_resume_offset_emits_without_a_false_zero_percent_event() {
    let parent = CancellationToken::new();
    let (mut progress, _, events) = reporter(&parent);
    progress.init(1_000, "model.gguf").await;
    assert!(events.lock().unwrap().is_empty());
    progress.update(750).await;
    assert_eq!(*events.lock().unwrap(), [(750, 1_000, None)]);
}

#[tokio::test]
async fn hf_oversized_resume_offset_cancels_before_any_progress() {
    let parent = CancellationToken::new();
    let (mut progress, attempt, events) = reporter(&parent);
    progress.init(100, "model.gguf").await;
    progress.update(101).await;
    progress.tick();
    progress.finish().await;
    assert!(attempt.is_cancelled());
    assert!(!parent.is_cancelled());
    assert_eq!(progress.exceeded_total(), Some((100, 101)));
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn hf_transfer_rate_excludes_resumed_bytes_and_decays_during_lull() {
    let now = Instant::now();
    let mut state = HfProgressState::new(now);
    state.on_init(1_000, now);
    assert_eq!(state.on_update(600, now), Some((600, 1_000, None)));
    assert_eq!(
        state.on_update(100, now + Duration::from_secs(1)),
        Some((700, 1_000, None))
    );
    let (_, _, rate) = state.on_update(100, now + Duration::from_secs(2)).unwrap();
    assert_eq!(rate, Some(100.0));
    let (downloaded, total, idle_rate) = state.on_tick(now + Duration::from_secs(4)).unwrap();
    assert_eq!((downloaded, total), (800, 1_000));
    assert!((idle_rate.unwrap() - 100.0 / 3.0).abs() < 1e-9);
}

#[test]
fn hf_idle_tick_never_reemits_an_exceeded_budget() {
    let now = Instant::now();
    let mut state = HfProgressState::new(now);
    state.on_init(100, now);
    state.on_update(0, now);
    state.on_update(50, now + Duration::from_secs(1));
    assert!(state.on_update(51, now + Duration::from_secs(2)).is_none());
    assert!(state.exceeded_total);
    assert!(state.on_tick(now + Duration::from_secs(4)).is_none());
}
