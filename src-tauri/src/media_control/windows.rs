//! Windows media control through the system media transport controls.
//!
//! `GlobalSystemMediaTransportControlsSessionManager` is the same surface the
//! hardware media keys and the volume-flyout controls drive, so anything that
//! publishes a media session — browsers, Spotify, the Media Player, most
//! players — is reachable, and each session can be paused individually instead
//! of firing a blind play/pause toggle at whoever happens to have focus.
//!
//! Sessions are remembered by their source app user model id, which is stable
//! across the pause, so the resume goes back to the same applications.

use log::{debug, warn};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

pub fn pause_playing_media() -> Vec<String> {
    let mut paused = Vec::new();

    for session in sessions() {
        let Ok(app_id) = session.SourceAppUserModelId() else {
            continue;
        };
        let app_id = app_id.to_string();

        let Ok(info) = session.GetPlaybackInfo() else {
            continue;
        };
        if info.PlaybackStatus().ok()
            != Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
        {
            continue;
        }
        // A session that advertises no pause control (a live stream, say) is
        // one we could not put back afterwards either.
        let can_pause = info
            .Controls()
            .and_then(|controls| controls.IsPauseEnabled())
            .unwrap_or(false);
        if !can_pause {
            debug!("Media session '{app_id}' is playing but cannot be paused");
            continue;
        }

        match session.TryPauseAsync().and_then(|request| request.get()) {
            Ok(true) => paused.push(app_id),
            Ok(false) => debug!("Media session '{app_id}' refused to pause"),
            Err(e) => warn!("Failed to pause media session '{app_id}': {e}"),
        }
    }

    paused
}

pub fn resume_media(players: &[String]) {
    for session in sessions() {
        let Ok(app_id) = session.SourceAppUserModelId() else {
            continue;
        };
        let app_id = app_id.to_string();
        if !players.contains(&app_id) {
            continue;
        }

        if let Err(e) = session.TryPlayAsync().and_then(|request| request.get()) {
            warn!("Failed to resume media session '{app_id}': {e}");
        }
    }
}

fn sessions() -> Vec<GlobalSystemMediaTransportControlsSession> {
    let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .and_then(|request| request.get())
    {
        Ok(manager) => manager,
        Err(e) => {
            warn!("Failed to open the system media transport controls: {e}");
            return Vec::new();
        }
    };

    match manager.GetSessions() {
        Ok(sessions) => sessions.into_iter().collect(),
        Err(e) => {
            warn!("Failed to list media sessions: {e}");
            Vec::new()
        }
    }
}
