use log::{debug, info, warn};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, PartialEq)]
pub enum ActivityStatus {
    Idle,
    Recording,
    Transcribing,
    Processing,
}

impl ActivityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActivityStatus::Idle => "idle",
            ActivityStatus::Recording => "recording",
            ActivityStatus::Transcribing => "transcribing",
            ActivityStatus::Processing => "processing",
        }
    }
}

pub struct StatusManager {
    current: Arc<Mutex<ActivityStatus>>,
    #[cfg(target_os = "linux")]
    tx: Option<tokio::sync::mpsc::UnboundedSender<ActivityStatus>>,
}

impl StatusManager {
    pub fn new() -> Arc<Self> {
        let current = Arc::new(Mutex::new(ActivityStatus::Idle));

        #[cfg(target_os = "linux")]
        let tx = {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let current_for_dbus = current.clone();
            tauri::async_runtime::spawn(async move {
                run_dbus_service(current_for_dbus, rx).await;
            });
            Some(tx)
        };
        #[cfg(not(target_os = "linux"))]
        let tx = None;

        Arc::new(Self { current, tx })
    }

    pub fn set_status(&self, status: ActivityStatus) {
        let mut current = self.current.lock().unwrap();
        if *current != status {
            *current = status.clone();
            drop(current);
            debug!("Activity status changed to: {}", status.as_str());
            #[cfg(target_os = "linux")]
            if let Some(ref tx) = self.tx {
                let _ = tx.send(status);
            }
        }
    }

    pub fn current_status(&self) -> ActivityStatus {
        self.current.lock().unwrap().clone()
    }
}

/// Convenience helper to set status without panicking if the manager is missing.
pub fn set_status_safe(app: &AppHandle, status: ActivityStatus) {
    if let Some(sm) = app.try_state::<Arc<StatusManager>>() {
        sm.set_status(status);
    }
}

#[cfg(target_os = "linux")]
struct StatusInterface {
    current: Arc<Mutex<ActivityStatus>>,
}

#[cfg(target_os = "linux")]
#[zbus::interface(name = "com.pais.Handy.Status")]
impl StatusInterface {
    #[zbus(property, name = "Status")]
    fn status(&self) -> String {
        self.current.lock().unwrap().as_str().to_string()
    }

    #[zbus(name = "GetStatus")]
    fn get_status(&self) -> String {
        self.status()
    }
}

#[cfg(target_os = "linux")]
async fn run_dbus_service(
    current: Arc<Mutex<ActivityStatus>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<ActivityStatus>,
) {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to connect to D-Bus session bus: {}", e);
            return;
        }
    };

    if let Err(e) = conn.request_name("com.pais.Handy").await {
        warn!("Failed to request D-Bus name com.pais.Handy: {}", e);
        return;
    }

    let iface = StatusInterface { current };
    if let Err(e) = conn.object_server().at("/com/pais/Handy", iface).await {
        warn!("Failed to serve D-Bus interface at /com/pais/Handy: {}", e);
        return;
    }

    info!("D-Bus status service registered on com.pais.Handy");

    while let Some(status) = rx.recv().await {
        let status_str = status.as_str();
        debug!("Emitting D-Bus StatusChanged signal: {}", status_str);
        if let Err(e) = conn
            .emit_signal(
                None::<&str>,
                "/com/pais/Handy",
                "com.pais.Handy.Status",
                "StatusChanged",
                &status_str,
            )
            .await
        {
            warn!("Failed to emit D-Bus StatusChanged signal: {}", e);
        }
    }
}
