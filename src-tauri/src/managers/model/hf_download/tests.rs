use super::*;

type Events = Arc<Mutex<Vec<(u64, u64)>>>;

fn reporter(parent: &CancellationToken) -> (HfDownloadProgress, CancellationToken, Events) {
    let attempt = parent.child_token();
    let events = Events::default();
    let capture = events.clone();
    (
        HfDownloadProgress::new(attempt.clone(), move |downloaded, total| {
            capture.lock().unwrap().push((downloaded, total));
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
    assert_eq!(events.lock().unwrap().last(), Some(&(100, 100)));
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
    assert!(events.iter().all(|(downloaded, total)| downloaded <= total));
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
    assert_eq!(events.lock().unwrap().last(), Some(&(0, 0)));
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
