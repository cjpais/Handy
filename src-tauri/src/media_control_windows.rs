use log::debug;
use windows::core::HRESULT;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

struct WinRtGuard {
    should_uninitialize: bool,
}

impl WinRtGuard {
    fn initialize() -> Result<Self, String> {
        match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Ok(Self {
                should_uninitialize: true,
            }),
            Err(err) if err.code() == RPC_E_CHANGED_MODE => Ok(Self {
                should_uninitialize: false,
            }),
            Err(err) => Err(format!("Failed to initialize WinRT: {err}")),
        }
    }
}

impl Drop for WinRtGuard {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe { RoUninitialize() };
        }
    }
}

pub fn pause_active_session() -> Result<Option<String>, String> {
    let _guard = WinRtGuard::initialize()?;
    let manager = request_manager()?;
    let Some(session) = find_playing_session(&manager)? else {
        return Ok(None);
    };

    let Some(playback_info) = playback_info(&session)? else {
        return Ok(None);
    };
    let controls = playback_info
        .Controls()
        .map_err(|err| format!("Failed to get Windows playback controls: {err}"))?;

    if !controls
        .IsPauseEnabled()
        .map_err(|err| format!("Failed to query Windows pause support: {err}"))?
    {
        return Ok(None);
    }

    let paused = session
        .TryPauseAsync()
        .map_err(|err| format!("Failed to request Windows pause: {err}"))?
        .get()
        .map_err(|err| format!("Failed to wait for Windows pause: {err}"))?;

    if !paused {
        return Ok(None);
    }

    let Some(source_app_user_model_id) = session_source_app_user_model_id(&session)? else {
        debug!("Skipped storing paused Windows session because its source app id disappeared");
        return Ok(None);
    };

    Ok(Some(source_app_user_model_id))
}

pub fn resume_session(source_app_user_model_id: &str) -> Result<(), String> {
    let _guard = WinRtGuard::initialize()?;
    let manager = request_manager()?;
    let Some(session) = find_session_by_source_app_id(&manager, source_app_user_model_id)? else {
        debug!(
            "Skipping Windows media resume because session '{}' no longer exists",
            source_app_user_model_id
        );
        return Ok(());
    };

    let Some(playback_info) = playback_info(&session)? else {
        debug!(
            "Skipping Windows media resume because session '{}' no longer exposes playback info",
            source_app_user_model_id
        );
        return Ok(());
    };
    let status = playback_info
        .PlaybackStatus()
        .map_err(|err| format!("Failed to query Windows playback status: {err}"))?;

    if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
        return Ok(());
    }

    let controls = playback_info
        .Controls()
        .map_err(|err| format!("Failed to get Windows playback controls: {err}"))?;

    if !controls
        .IsPlayEnabled()
        .map_err(|err| format!("Failed to query Windows play support: {err}"))?
    {
        debug!(
            "Skipping Windows media resume because session '{}' no longer supports play",
            source_app_user_model_id
        );
        return Ok(());
    }

    let resumed = session
        .TryPlayAsync()
        .map_err(|err| format!("Failed to request Windows play: {err}"))?
        .get()
        .map_err(|err| format!("Failed to wait for Windows play: {err}"))?;

    if !resumed {
        debug!(
            "Windows media session '{}' declined the play request; ignoring",
            source_app_user_model_id
        );
    }

    Ok(())
}

fn request_manager() -> Result<GlobalSystemMediaTransportControlsSessionManager, String> {
    GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|err| format!("Failed to request Windows media session manager: {err}"))?
        .get()
        .map_err(|err| format!("Failed to wait for Windows media session manager: {err}"))
}

fn find_playing_session(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
) -> Result<Option<GlobalSystemMediaTransportControlsSession>, String> {
    if let Some(current_session) = current_session(manager)? {
        if session_is_playing(&current_session)? {
            return Ok(Some(current_session));
        }
    }

    let sessions = manager
        .GetSessions()
        .map_err(|err| format!("Failed to enumerate Windows media sessions: {err}"))?;
    let count = sessions
        .Size()
        .map_err(|err| format!("Failed to query Windows media session count: {err}"))?;

    for index in 0..count {
        let session = sessions.GetAt(index).map_err(|err| {
            format!("Failed to read Windows media session at index {index}: {err}")
        })?;

        if session_is_playing(&session)? {
            return Ok(Some(session));
        }
    }

    Ok(None)
}

fn find_session_by_source_app_id(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    source_app_user_model_id: &str,
) -> Result<Option<GlobalSystemMediaTransportControlsSession>, String> {
    if let Some(current_session) = current_session(manager)? {
        if session_source_app_user_model_id(&current_session)?.as_deref()
            == Some(source_app_user_model_id)
        {
            return Ok(Some(current_session));
        }
    }

    let sessions = manager
        .GetSessions()
        .map_err(|err| format!("Failed to enumerate Windows media sessions: {err}"))?;
    let count = sessions
        .Size()
        .map_err(|err| format!("Failed to query Windows media session count: {err}"))?;

    for index in 0..count {
        let session = sessions.GetAt(index).map_err(|err| {
            format!("Failed to read Windows media session at index {index}: {err}")
        })?;

        if session_source_app_user_model_id(&session)?.as_deref() == Some(source_app_user_model_id)
        {
            return Ok(Some(session));
        }
    }

    Ok(None)
}

fn current_session(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
) -> Result<Option<GlobalSystemMediaTransportControlsSession>, String> {
    match manager.GetCurrentSession() {
        Ok(session) => Ok(Some(session)),
        Err(err) if err.code() == HRESULT(0x80070490u32 as i32) => Ok(None),
        Err(err) if err.code() == HRESULT(0x80004005u32 as i32) => Ok(None),
        Err(err) => Err(format!(
            "Failed to get current Windows media session: {err}"
        )),
    }
}

fn playback_info(
    session: &GlobalSystemMediaTransportControlsSession,
) -> Result<
    Option<windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackInfo>,
    String,
> {
    match session.GetPlaybackInfo() {
        Ok(info) => Ok(Some(info)),
        Err(err) if session_missing_error(err.code()) => Ok(None),
        Err(err) => Err(format!("Failed to get Windows playback info: {err}")),
    }
}

fn session_is_playing(session: &GlobalSystemMediaTransportControlsSession) -> Result<bool, String> {
    let Some(playback_info) = playback_info(session)? else {
        return Ok(false);
    };

    let status = playback_info
        .PlaybackStatus()
        .map_err(|err| format!("Failed to query Windows playback status: {err}"))?;

    Ok(status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
}

fn session_source_app_user_model_id(
    session: &GlobalSystemMediaTransportControlsSession,
) -> Result<Option<String>, String> {
    match session.SourceAppUserModelId() {
        Ok(value) => Ok(Some(value.to_string())),
        Err(err) if session_missing_error(err.code()) => Ok(None),
        Err(err) => Err(format!("Failed to query Windows source app id: {err}")),
    }
}

fn session_missing_error(code: HRESULT) -> bool {
    code == HRESULT(0x80070490u32 as i32) || code == HRESULT(0x80004005u32 as i32)
}
