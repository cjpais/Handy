//! Global lock with declarative UI sync.

use std::sync::Mutex;
use tauri::AppHandle;

use crate::overlay::{hide_recording_overlay, show_recording_overlay, show_transcribing_overlay};
use crate::tray::{change_tray_icon, TrayIconState};

#[derive(Debug, Clone, PartialEq)]
pub enum GlobalPhase {
    Idle,
    Recording,
    Processing,
}

pub struct GlobalController {
    phase: Mutex<GlobalPhase>,
    app: AppHandle,
}

impl GlobalController {
    pub fn new(app: AppHandle) -> Self {
        Self {
            phase: Mutex::new(GlobalPhase::Idle),
            app,
        }
    }

    fn sync_ui(&self, phase: &GlobalPhase) {
        match phase {
            GlobalPhase::Idle => {
                change_tray_icon(&self.app, TrayIconState::Idle);
                hide_recording_overlay(&self.app);
            }
            GlobalPhase::Recording => {
                change_tray_icon(&self.app, TrayIconState::Recording);
                show_recording_overlay(&self.app);
            }
            GlobalPhase::Processing => {
                change_tray_icon(&self.app, TrayIconState::Transcribing);
                show_transcribing_overlay(&self.app);
            }
        }
    }

    pub fn begin(&self) -> bool {
        let mut phase = self.phase.lock().unwrap();
        if *phase == GlobalPhase::Idle {
            *phase = GlobalPhase::Recording;
            self.sync_ui(&phase);
            true
        } else {
            false
        }
    }

    pub fn advance(&self) -> bool {
        let mut phase = self.phase.lock().unwrap();
        if *phase == GlobalPhase::Recording {
            *phase = GlobalPhase::Processing;
            self.sync_ui(&phase);
            true
        } else {
            false
        }
    }

    pub fn complete(&self) {
        let mut phase = self.phase.lock().unwrap();
        *phase = GlobalPhase::Idle;
        self.sync_ui(&phase);
    }

    pub fn is_busy(&self) -> bool {
        *self.phase.lock().unwrap() != GlobalPhase::Idle
    }

    /// Re-sync UI to current phase (e.g., after theme change)
    pub fn refresh_ui(&self) {
        let phase = self.phase.lock().unwrap();
        self.sync_ui(&phase);
    }
}
