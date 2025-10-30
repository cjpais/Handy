#[cfg(target_os = "linux")]
use crate::actions::ACTION_MAP;
#[cfg(target_os = "linux")]
use log::{error, info};
#[cfg(target_os = "linux")]
use tauri::AppHandle;
#[cfg(target_os = "linux")]
use zbus::connection;

/// D-Bus interface for Handy transcription control
#[cfg(target_os = "linux")]
pub struct HandyTranscription {
    app_handle: AppHandle,
}

#[cfg(target_os = "linux")]
#[zbus::interface(name = "com.pais.Handy.Transcription")]
impl HandyTranscription {
    /// Start a new transcription session
    async fn start_transcription(&self) -> zbus::fdo::Result<()> {
        info!("D-Bus: StartTranscription called");
        
        // Get the transcribe action from the ACTION_MAP
        if let Some(action) = ACTION_MAP.get("transcribe") {
            // Call start with dummy values for binding_id and shortcut_str
            action.start(&self.app_handle, "dbus", "dbus-trigger");
            Ok(())
        } else {
            error!("D-Bus: Failed to find transcribe action in ACTION_MAP");
            Err(zbus::fdo::Error::Failed(
                "Transcribe action not found".to_string(),
            ))
        }
    }

    /// Stop the current transcription session
    async fn stop_transcription(&self) -> zbus::fdo::Result<()> {
        info!("D-Bus: StopTranscription called");
        
        // Get the transcribe action from the ACTION_MAP
        if let Some(action) = ACTION_MAP.get("transcribe") {
            // Call stop with dummy values for binding_id and shortcut_str
            action.stop(&self.app_handle, "dbus", "dbus-trigger");
            Ok(())
        } else {
            error!("D-Bus: Failed to find transcribe action in ACTION_MAP");
            Err(zbus::fdo::Error::Failed(
                "Transcribe action not found".to_string(),
            ))
        }
    }
}

/// Initialize the D-Bus service
/// This should be called on app startup to register the D-Bus interface
#[cfg(target_os = "linux")]
pub async fn init_dbus_service(app_handle: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    info!("Initializing D-Bus service...");
    
    let transcription_service = HandyTranscription {
        app_handle: app_handle.clone(),
    };

    // Build the D-Bus connection and register the interface
    let _connection = connection::Builder::session()?
        .name("com.pais.Handy")?
        .serve_at("/com/pais/Handy", transcription_service)?
        .build()
        .await?;

    info!("D-Bus service registered at com.pais.Handy on /com/pais/Handy");
    
    // Keep the connection alive by storing it
    // We need to prevent it from being dropped
    // In Tauri, we can use app.manage() or just hold the connection in a static
    std::mem::forget(_connection);
    
    Ok(())
}

/// Stub for non-Linux platforms
#[cfg(not(target_os = "linux"))]
pub async fn init_dbus_service(_app_handle: tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // No-op on non-Linux platforms
    Ok(())
}

