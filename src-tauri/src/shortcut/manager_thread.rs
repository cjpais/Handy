use std::sync::mpsc;
use std::thread::{self, JoinHandle};

/// Create the manager on its owning thread before reporting startup success.
pub(super) fn spawn_manager_thread<T>(
    initialize: impl FnOnce() -> Result<T, String> + Send + 'static,
    run: impl FnOnce(T) + Send + 'static,
) -> Result<JoinHandle<()>, String> {
    let (startup_tx, startup_rx) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("handy-keys-manager".to_string())
        .spawn(move || match initialize() {
            Ok(manager) => {
                if startup_tx.send(Ok(())).is_ok() {
                    run(manager);
                }
            }
            Err(error) => {
                let _ = startup_tx.send(Err(error));
            }
        })
        .map_err(|error| format!("Failed to spawn HotkeyManager thread: {error}"))?;

    let startup_result = startup_rx.recv().unwrap_or_else(|_| {
        Err("HotkeyManager thread exited before reporting startup".to_string())
    });
    if let Err(error) = startup_result {
        let _ = handle.join();
        return Err(error);
    }
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::spawn_manager_thread;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::mpsc;

    #[test]
    fn startup_failure_reaches_caller_without_running_manager_loop() {
        let (tx, rx) = mpsc::channel();
        let result = spawn_manager_thread(
            || Err::<(), _>("Accessibility permission denied".to_string()),
            move |_| tx.send(()).unwrap(),
        );

        assert_eq!(result.unwrap_err(), "Accessibility permission denied");
        assert!(rx.recv().is_err());
    }

    #[test]
    fn successful_startup_keeps_manager_on_owning_thread() {
        let (command_tx, command_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let handle = spawn_manager_thread(
            || Ok(Rc::new(Cell::new(41))),
            move |manager| {
                command_rx.recv().unwrap();
                manager.set(manager.get() + 1);
                result_tx.send(manager.get()).unwrap();
            },
        )
        .unwrap();

        command_tx.send(()).unwrap();
        assert_eq!(result_rx.recv().unwrap(), 42);
        handle.join().unwrap();
    }

    #[test]
    fn startup_panic_returns_error_instead_of_dead_manager() {
        let result =
            spawn_manager_thread(|| -> Result<(), String> { panic!("startup panic") }, |_| {});
        assert_eq!(
            result.unwrap_err(),
            "HotkeyManager thread exited before reporting startup"
        );
    }
}
