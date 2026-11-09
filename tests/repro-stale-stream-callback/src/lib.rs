#![allow(dead_code)]

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};

type AudioFrameCallback = Arc<dyn Fn(&[f32]) + Send + Sync + 'static>;
type StreamFinalizeReply = Result<Option<()>, ()>;
struct CaptureSink;
struct TranscriptionManager {
    router: Arc<StreamRouter>,
    stream_active: AtomicBool,
}

include!(concat!(env!("OUT_DIR"), "/production.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(receiver: &mpsc::Receiver<StreamCmd>) -> Vec<Vec<f32>> {
        receiver
            .try_iter()
            .map(|command| match command {
                StreamCmd::Feed(frame) => frame,
                _ => panic!("unexpected non-feed command on replacement route"),
            })
            .collect()
    }

    #[test]
    fn fresh_callback_delivers_frame() {
        let router = Arc::new(StreamRouter::new());
        let callback = manager_audio_callback(Arc::clone(&router));
        let (receiver, _) = router.open();
        let mut samples = Vec::new();
        CaptureSink::emit(&mut samples, &callback, &[2.0]);
        assert_eq!(samples, vec![2.0]);
        assert_eq!(frames(&receiver), vec![vec![2.0]]);
    }

    #[test]
    fn callback_with_no_route_is_a_no_op() {
        let router = Arc::new(StreamRouter::new());
        let callback = manager_audio_callback(Arc::clone(&router));
        CaptureSink::emit(&mut Vec::new(), &callback, &[1.0]);
        let (receiver, _) = router.open();
        assert!(frames(&receiver).is_empty());
        let route = router.take().expect("open route");
        drop(route);
        CaptureSink::emit(&mut Vec::new(), &callback, &[2.0]);
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn old_callback_must_not_feed_replacement_route() {
        let router = Arc::new(StreamRouter::new());
        let callback = manager_audio_callback(Arc::clone(&router));
        let manager = TranscriptionManager {
            router: Arc::clone(&router),
            stream_active: AtomicBool::new(true),
        };
        let (old_receiver, old_cancelled) = router.open();
        let old_callback = Arc::clone(&callback);
        let (paused_tx, paused_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let old_delivery = std::thread::spawn(move || {
            paused_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            let mut old_samples = Vec::new();
            CaptureSink::emit(&mut old_samples, &old_callback, &[1.0]);
            old_samples
        });
        paused_rx.recv().unwrap();

        manager.cancel_stream();
        assert!(old_cancelled.load(Ordering::Acquire));
        assert!(matches!(old_receiver.recv(), Ok(StreamCmd::Cancel)));
        assert!(matches!(
            old_receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        let (replacement_receiver, _) = router.open();

        resume_tx.send(()).unwrap();
        assert_eq!(old_delivery.join().unwrap(), vec![1.0]);
        CaptureSink::emit(&mut Vec::new(), &callback, &[2.0]);
        let received = frames(&replacement_receiver);
        println!("replacement frames: {received:?}");
        assert_eq!(
            received,
            vec![vec![2.0]],
            "replacement must receive its fresh frame, without the retired recording's frame"
        );
    }
}
