#![allow(dead_code)]

extern crate self as anyhow;

#[derive(Debug)]
pub struct ReproError(String);

impl std::fmt::Display for ReproError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[macro_export]
macro_rules! anyhow {
    ($message:expr) => {
        $crate::ReproError($message.to_string())
    };
}

macro_rules! debug {
    ($($argument:tt)*) => { let _ = format_args!($($argument)*); };
}

macro_rules! info {
    ($($argument:tt)*) => { let _ = format_args!($($argument)*); };
}

macro_rules! warn {
    ($($argument:tt)*) => { let _ = format_args!($($argument)*); };
}

/// Real threads, stopped at named points, for the schedules in the report.
mod on_std {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, Mutex, MutexGuard};
    use std::thread;
    use std::time::Duration;

    struct Pause {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    #[derive(Default)]
    struct PauseSlot(Mutex<Option<Pause>>);

    fn pause(slot: &PauseSlot) {
        let pause = slot.0.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.entered.send(()).unwrap();
            pause.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }

    fn report_exit(reply: mpsc::Sender<()>) {
        let _ = reply.send(());
    }

    include!("world.rs");

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Where the older load waits while the test requests the unload.
        #[derive(Clone, Copy, Debug)]
        enum PauseAt {
            /// Inside the engine, before the load finishes.
            EngineLoad,
            /// After `engine.load` returned, before the manager publishes.
            Publication,
        }

        fn unload_during(at: PauseAt) -> Settled {
            let world = World::new();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let slot = match at {
                PauseAt::EngineLoad => &world.manager.engine.shared.load_pause,
                PauseAt::Publication => &world.manager.model_manager.pause,
            };
            *slot.0.lock().unwrap() = Some(Pause {
                entered: entered_tx,
                release: release_rx,
            });
            let loader = Arc::clone(&world.manager);
            let load = thread::spawn(move || loader.load_model_with_device("model-a", None));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(
                world.manager.is_model_loaded(),
                matches!(at, PauseAt::Publication),
                "engine residency at the pause"
            );
            world.on_main(|manager| manager.request_unload());
            assert!(
                !world.manager.is_model_loaded(),
                "unload must clear engine residency"
            );
            assert_eq!(world.manager.get_current_model(), None);
            assert_eq!(world.events(), ["loading_started", "unloaded"]);
            release_tx.send(()).unwrap();
            load.join().unwrap().unwrap();
            let settled = world.settle();
            println!(
                "{at:?}: after unload and old load return: held={:?}, current_model={:?}, events={:?}",
                settled.held,
                settled.current_model,
                settled.event_types()
            );
            settled
        }

        #[test]
        fn control_serial_load_then_unload() {
            let world = World::new();
            world
                .manager
                .load_model_with_device("model-a", None)
                .unwrap();
            assert!(world.manager.is_model_loaded());
            assert_eq!(
                world.manager.get_current_model().as_deref(),
                Some("model-a")
            );
            world.manager.request_unload();
            let settled = world.settle();
            settled.check(Some(1));
            assert_eq!(
                settled.event_types(),
                ["loading_started", "loading_completed", "unloaded"]
            );
        }

        #[test]
        fn control_load_after_superseded_load_succeeds() {
            let world = World::new();
            world
                .manager
                .load_model_with_device("model-a", None)
                .unwrap();
            world.manager.request_unload();
            world
                .manager
                .load_model_with_device("model-b", None)
                .unwrap();
            let settled = world.settle();
            settled.check(Some(1));
            assert_eq!(settled.current_model.as_deref(), Some("model-b"));
        }

        #[test]
        fn unload_keeps_current_model_absent_after_old_load_returns() {
            for at in [PauseAt::EngineLoad, PauseAt::Publication] {
                let settled = unload_during(at);
                assert_eq!(
                    settled.current_model, None,
                    "{at:?}: the unload must not be overwritten by the older load"
                );
            }
        }

        #[test]
        fn superseded_load_reports_cancelled_not_completed() {
            for at in [PauseAt::EngineLoad, PauseAt::Publication] {
                let settled = unload_during(at);
                assert_eq!(
                    settled.event_types(),
                    ["loading_started", "unloaded", "loading_cancelled"],
                    "{at:?}: the older load must end without reporting success"
                );
                settled.check(Some(1));
            }
        }
    }
}

/// Loom runs each scenario under every interleaving of its threads.
mod on_loom {
    use loom::sync::atomic::{AtomicU64, Ordering};
    use loom::sync::{mpsc, Arc, Mutex, MutexGuard};
    use loom::thread;

    #[derive(Default)]
    struct PauseSlot {}

    fn pause(_slot: &PauseSlot) {}

    /// `request_unload` drops its end before the owner answers, and Loom
    /// counts a message nobody can receive as a leak. Nothing in a scenario
    /// waits for the worker's exit, so the owner doesn't report it.
    fn report_exit(reply: mpsc::Sender<()>) {
        drop(reply);
    }

    include!("world.rs");

    #[cfg(test)]
    mod tests {
        use super::*;

        fn spawn(
            world: &World,
            work: impl FnOnce(&TranscriptionManager) + Send + 'static,
        ) -> thread::JoinHandle<()> {
            let manager = Arc::clone(&world.manager);
            thread::spawn(move || work(&manager))
        }

        fn load(manager: &TranscriptionManager, model_id: &str) {
            manager.load_model_with_device(model_id, None).unwrap();
        }

        /// Run `scenario` under every interleaving, or under every one with
        /// at most `preemptions` forced thread switches, and print how many
        /// that was.
        fn explore(
            name: &str,
            preemptions: Option<usize>,
            scenario: impl Fn() + Sync + Send + 'static,
        ) {
            use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
            let runs = std::sync::Arc::new(AtomicUsize::new(0));
            let counted = std::sync::Arc::clone(&runs);
            let mut model = loom::model::Builder::new();
            if model.preemption_bound.is_none() {
                model.preemption_bound = preemptions;
            }
            let bound = model.preemption_bound;
            model.check(move || {
                counted.fetch_add(1, Relaxed);
                scenario();
            });
            let runs = runs.load(Relaxed);
            match bound {
                Some(bound) => {
                    println!("{name}: {runs} interleavings, at most {bound} preemptions")
                }
                None => println!("{name}: {runs} interleavings, exhaustive"),
            }
        }

        /// The tray's Unload, on the main thread, against a load in flight.
        #[test]
        fn unload_races_one_load() {
            explore("unload_races_one_load", None, || {
                let world = World::new();
                let loading = spawn(&world, |manager| load(manager, "model-a"));
                world.on_main(|manager| manager.request_unload());
                loading.join().unwrap();
                world.settle().check(Some(1));
            });
        }

        /// The same unload anywhere across two loads that follow each other.
        #[test]
        fn unload_races_two_loads_in_a_row() {
            explore("unload_races_two_loads_in_a_row", None, || {
                let world = World::new();
                let loading = spawn(&world, |manager| {
                    load(manager, "model-a");
                    load(manager, "model-b");
                });
                world.on_main(|manager| manager.request_unload());
                loading.join().unwrap();
                world.settle().check(Some(1));
            });
        }

        /// The tray's Unload and the idle watcher's, against one load. Four
        /// threads are too many to exhaust, so this one is bounded.
        #[test]
        fn two_unloads_race_one_load() {
            explore("two_unloads_race_one_load", Some(4), || {
                let world = World::new();
                let loading = spawn(&world, |manager| load(manager, "model-a"));
                let watcher = spawn(&world, |manager| manager.request_unload());
                world.on_main(|manager| manager.request_unload());
                loading.join().unwrap();
                watcher.join().unwrap();
                world.settle().check(Some(2));
            });
        }

        /// Two loads at once are outside the repair: the engine's snapshot
        /// doesn't say which load produced it. Production never runs them,
        /// because both callers claim `is_loading` first. This fails if that
        /// limit is ever lifted, so the claim can be dropped from the README.
        #[test]
        #[should_panic(expected = "must name what the engine holds")]
        fn overlapping_loads_are_outside_the_repair() {
            explore("overlapping_loads_are_outside_the_repair", None, || {
                let world = World::new();
                let first = spawn(&world, |manager| load(manager, "model-a"));
                let second = spawn(&world, |manager| load(manager, "model-b"));
                first.join().unwrap();
                second.join().unwrap();
                world.settle().check(Some(0));
            });
        }

        /// A transcription finds the worker gone and reports it, while the
        /// same model is loaded again.
        #[test]
        fn lost_worker_report_races_reload() {
            explore("lost_worker_report_races_reload", None, || {
                let world = World::new();
                load(&world.manager, "model-a");
                let transcription = spawn(&world, |manager| {
                    manager.engine.lose();
                    manager.forget_model_if_engine_dropped("model-a", "worker lost");
                });
                let loading = spawn(&world, |manager| load(manager, "model-a"));
                transcription.join().unwrap();
                loading.join().unwrap();
                world.settle().check(None);
            });
        }
    }
}
