//! Real hf-hub -> bounded Handy progress -> verified HTTP fallback, with no
//! Tauri application or external server needed.

use super::*;
use crate::managers::model::hf_download::HfDownloadProgress;
use hf_hub::api::tokio::{ApiBuilder, ApiError};
use hf_hub::Cache;
use std::sync::{Arc, Mutex};
use tokio::task::JoinSet;

const TOTAL: usize = 64 * 1024;

struct HubServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for HubServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn payload() -> Vec<u8> {
    (0..TOTAL)
        .map(|i| ((i * 7 + (i >> 8)) % 251) as u8)
        .collect()
}

async fn serve_hub(ignore_ranges: bool, corrupt_mirror: bool) -> HubServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let bytes = Arc::new(payload());
    let task = tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((mut socket, _)) = accepted else { break };
                    let bytes = bytes.clone();
                    connections.spawn(async move {
                        loop {
                            let request = read_request_head(&mut socket).await.to_lowercase();
                            if request.is_empty() { return; }
                            let mirror = request.starts_with("get /mirror ");
                            let range = request.lines().find_map(|line| line.strip_prefix("range: bytes="));
                            let (start, end) = range.and_then(|r| r.trim().split_once('-'))
                                .map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap_or(TOTAL - 1)))
                                .unwrap_or((0, TOTAL - 1));
                            let metadata = !mirror && start == 0 && end == 0;
                            let partial = !mirror && (!ignore_ranges || metadata);
                            let (begin, last) = if partial { (start, end.min(TOTAL - 1)) } else { (0, TOTAL - 1) };
                            let status = if partial { "206 Partial Content" } else { "200 OK" };
                            // Keep the fixture connection open after the complete
                            // body, as both Hub/CDN and HTTP mirror servers may do.
                            let mut headers = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n", last - begin + 1);
                            if partial {
                                headers.push_str(&format!("Content-Range: bytes {begin}-{last}/{TOTAL}\r\n"));
                            }
                            if metadata {
                                headers.push_str(&format!("ETag: \"test-model\"\r\nX-Repo-Commit: {}\r\nX-Linked-Size: {TOTAL}\r\n", "a".repeat(40)));
                            }
                            headers.push_str("\r\n");
                            let mut body = bytes[begin..=last].to_vec();
                            if mirror && corrupt_mirror { body[0] ^= 1; }
                            if socket.write_all(headers.as_bytes()).await.is_err()
                                || socket.write_all(&body).await.is_err() { return; }
                        }
                    });
                }
                _ = connections.join_next(), if !connections.is_empty() => {}
            }
        }
    });
    HubServer { url, task }
}

async fn download_from_hub(
    server: &HubServer,
    dir: &Path,
    streams: usize,
    progress: HfDownloadProgress,
    attempt: CancellationToken,
) -> std::result::Result<std::path::PathBuf, ApiError> {
    let api = ApiBuilder::from_cache(Cache::new(dir.join("hub-cache")))
        .with_token(None)
        .with_endpoint(server.url.clone())
        .with_progress(false)
        .with_chunk_size(Some(8 * 1024))
        .with_max_files(streams)
        .build()
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        api.model("synthetic/model".into())
            .download_with_progress_cancellable("model.gguf", progress, attempt),
    )
    .await
    .expect("the byte budget must cancel a live oversized transfer promptly")
}

async fn oversized_hub_fallback(streams: usize, resume_mirror: bool, corrupt_mirror: bool) {
    let server = serve_hub(true, corrupt_mirror).await;
    let dir = TempDir::new().unwrap();
    let parent = CancellationToken::new();
    let attempt = parent.child_token();
    let progress_events = Arc::new(Mutex::new(Vec::new()));
    let captured = progress_events.clone();
    let progress = HfDownloadProgress::new(attempt.clone(), move |received, total| {
        captured.lock().unwrap().push((received, total));
    });
    let result = download_from_hub(&server, dir.path(), streams, progress.clone(), attempt).await;
    assert!(matches!(result, Err(ApiError::Cancelled)), "{result:?}");
    assert!(progress.exceeded_total().is_some());
    assert!(!parent.is_cancelled(), "fallback is not user cancellation");
    assert!(progress_events
        .lock()
        .unwrap()
        .iter()
        .all(|(received, total)| received <= total));
    assert!(
        Cache::new(dir.path().join("hub-cache"))
            .model("synthetic/model".into())
            .get("model.gguf")
            .is_none(),
        "oversized HF bytes must never become a completed model"
    );

    let partial = dir.path().join("model.partial");
    if resume_mirror {
        fs::write(&partial, b"old partial").unwrap();
    }
    let verification = Mutex::new(Vec::new());
    let outcome = ModelManager::download_http_resumable_with_events(
        "synthetic/model",
        &format!("{}/mirror", server.url),
        &partial,
        Some(TOTAL as u64),
        Some(&sha_hex(&payload())),
        &parent,
        &|event| match event {
            HttpDownloadEvent::Progress(progress) => assert!(progress.percentage <= 100.0),
            HttpDownloadEvent::VerificationStarted => verification.lock().unwrap().push("started"),
            HttpDownloadEvent::VerificationCompleted => {
                verification.lock().unwrap().push("verified")
            }
        },
    )
    .await;
    if corrupt_mirror {
        assert!(outcome.unwrap_err().to_string().contains("corrupt"));
        assert!(!partial.exists(), "failed hash must remove tainted bytes");
        assert_eq!(*verification.lock().unwrap(), ["started"]);
    } else {
        assert!(matches!(outcome.unwrap(), HttpDownloadOutcome::Completed));
        assert_eq!(fs::read(&partial).unwrap(), payload());
        assert_eq!(*verification.lock().unwrap(), ["started", "verified"]);
    }
}

#[tokio::test]
async fn hf_oversized_sequential_transfer_can_use_verified_mirror() {
    oversized_hub_fallback(1, false, false).await;
}

#[tokio::test]
async fn hf_oversized_parallel_transfer_can_restart_mirror_partial() {
    oversized_hub_fallback(4, true, false).await;
}

#[tokio::test]
async fn hf_oversized_transfer_cannot_accept_corrupt_mirror() {
    oversized_hub_fallback(4, false, true).await;
}

#[tokio::test]
async fn hf_valid_ranges_still_complete_byte_exactly() {
    for streams in [1, 4] {
        let server = serve_hub(false, false).await;
        let dir = TempDir::new().unwrap();
        let parent = CancellationToken::new();
        let attempt = parent.child_token();
        let progress = HfDownloadProgress::new(attempt.clone(), |received, total| {
            assert!(received <= total);
        });
        let path = download_from_hub(&server, dir.path(), streams, progress.clone(), attempt)
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), payload());
        assert!(progress.exceeded_total().is_none());
        assert!(!parent.is_cancelled());
    }
}
