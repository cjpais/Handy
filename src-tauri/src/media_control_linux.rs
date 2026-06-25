use log::debug;
use zbus::blocking::{Connection, Proxy};

const PLAYERCTLD_SERVICE: &str = "org.mpris.MediaPlayer2.playerctld";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

pub fn pause_active_session() -> Result<Option<String>, String> {
    let connection = Connection::session()
        .map_err(|err| format!("Failed to connect to the Linux session bus: {err}"))?;
    let Some(service_name) = find_target_service(&connection)? else {
        return Ok(None);
    };

    {
        let proxy = match player_proxy(&connection, &service_name) {
            Ok(proxy) => proxy,
            Err(err) if service_missing_error(&err) => return Ok(None),
            Err(err) => return Err(err),
        };

        let playback_status = playback_status(&proxy)?;
        if playback_status != "Playing" {
            return Ok(None);
        }

        proxy
            .call_method("Pause", &())
            .map_err(|err| format!("Failed to pause MPRIS player '{service_name}': {err}"))?;
    }

    Ok(Some(service_name))
}

pub fn resume_session(service_name: &str) -> Result<(), String> {
    let connection = Connection::session()
        .map_err(|err| format!("Failed to connect to the Linux session bus: {err}"))?;
    let proxy = match player_proxy(&connection, service_name) {
        Ok(proxy) => proxy,
        Err(err) if service_missing_error(&err) => {
            debug!(
                "Skipping Linux media resume because MPRIS service '{}' vanished",
                service_name
            );
            return Ok(());
        }
        Err(err) => return Err(err),
    };

    let playback_status = playback_status(&proxy)?;
    if playback_status != "Paused" {
        debug!(
            "Skipping Linux media resume because MPRIS service '{}' is '{}'",
            service_name, playback_status
        );
        return Ok(());
    }

    proxy
        .call_method("Play", &())
        .map_err(|err| format!("Failed to resume MPRIS player '{service_name}': {err}"))?;

    Ok(())
}

fn find_target_service(connection: &Connection) -> Result<Option<String>, String> {
    let dbus_proxy = zbus::blocking::fdo::DBusProxy::new(connection)
        .map_err(|err| format!("Failed to create Linux D-Bus proxy: {err}"))?;
    let names = dbus_proxy
        .list_names()
        .map_err(|err| format!("Failed to list Linux D-Bus names: {err}"))?;

    let mut service_names = names
        .iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(MPRIS_PREFIX) && name != PLAYERCTLD_SERVICE)
        .collect::<Vec<_>>();
    service_names.sort();

    // Prefer a concrete player service when one is already playing so we can resume the same
    // target later. Fall back to playerctld only when it is available and no direct MPRIS
    // service can be confidently selected.
    for service_name in &service_names {
        if service_is_playing(connection, service_name)? {
            return Ok(Some(service_name.clone()));
        }
    }

    if names.iter().any(|name| name.as_str() == PLAYERCTLD_SERVICE)
        && service_is_playing(connection, PLAYERCTLD_SERVICE)?
    {
        return Ok(Some(PLAYERCTLD_SERVICE.to_string()));
    }

    Ok(None)
}

fn service_is_playing(connection: &Connection, service_name: &str) -> Result<bool, String> {
    let proxy = match player_proxy(connection, service_name) {
        Ok(proxy) => proxy,
        Err(err) if service_missing_error(&err) => return Ok(false),
        Err(err) => return Err(err),
    };

    Ok(playback_status(&proxy)? == "Playing")
}

fn player_proxy<'a>(
    connection: &'a Connection,
    service_name: &'a str,
) -> Result<Proxy<'a>, String> {
    Proxy::new(
        connection,
        service_name,
        MPRIS_OBJECT_PATH,
        MPRIS_PLAYER_INTERFACE,
    )
    .map_err(|err| format!("Failed to create MPRIS proxy for '{service_name}': {err}"))
}

fn playback_status(proxy: &Proxy<'_>) -> Result<String, String> {
    proxy.get_property("PlaybackStatus").map_err(|err| {
        if service_missing_error(&err.to_string()) {
            format!("Failed to read MPRIS PlaybackStatus because the player vanished: {err}")
        } else {
            format!("Failed to read MPRIS PlaybackStatus: {err}")
        }
    })
}

fn service_missing_error(err: &str) -> bool {
    err.contains("org.freedesktop.DBus.Error.ServiceUnknown")
        || err.contains("org.freedesktop.DBus.Error.NameHasNoOwner")
        || err.contains("UnknownMethod")
        || err.contains("UnknownObject")
}
