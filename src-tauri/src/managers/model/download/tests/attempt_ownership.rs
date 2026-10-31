use super::*;
use crate::managers::model::{DownloadCleanup, DownloadTokens};
use std::collections::HashMap;
use std::sync::Mutex;

#[tokio::test]
async fn retry_can_cancel_http_after_retired_attempt_cleanup() {
    let models = Mutex::new(HashMap::new());
    let flags = DownloadTokens::default();
    let old = DownloadCleanup::new(&models, &flags, "model");
    old.cancel_token.cancel();
    let retry = DownloadCleanup::new(&models, &flags, "model");
    let retired = Mutex::new(Some(old));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let (finished, keep_open) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert!(read_request_head(&mut socket).await.ends_with("\r\n\r\n"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nabc")
            .await
            .unwrap();
        let _ = keep_open.await;
    });
    let dir = TempDir::new().unwrap();
    let partial = dir.path().join("model.partial");
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        ModelManager::download_http_resumable_with_events(
            "model",
            &url,
            &partial,
            Some(6),
            None,
            &retry.cancel_token,
            &|event| {
                if let HttpDownloadEvent::Progress(progress) = event {
                    if progress.downloaded == 3 {
                        // A finishes only after B has received/written bytes.
                        drop(retired.lock().unwrap().take());
                        let current = flags.lock().unwrap().get("model").cloned();
                        current.expect("retry must remain registered").cancel();
                    }
                }
            },
        ),
    )
    .await
    .expect("the retry's registered token must stop its stalled body")
    .unwrap();
    let _ = finished.send(());
    server.await.unwrap();
    assert!(matches!(outcome, HttpDownloadOutcome::Cancelled));
    assert_eq!(fs::read(partial).unwrap(), b"abc");
    assert!(retry.cancel_token.is_cancelled());
}
