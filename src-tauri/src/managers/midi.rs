use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use midir::{Ignore, MidiInput, MidiInputConnection};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::TranscriptionCoordinator;

#[derive(Clone, Debug)]
pub struct MidiRuntimeConfig {
    pub enabled: bool,
    pub trigger: Option<Vec<u8>>,
    pub push_to_talk: bool,
}

impl Default for MidiRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: None,
            push_to_talk: true,
        }
    }
}

pub struct MidiManager {
    midi_input: Arc<Mutex<Option<MidiInput>>>,
    connection: Arc<Mutex<Option<MidiInputConnection<()>>>>,
    cached_ports: Arc<Mutex<Vec<String>>>,
    app_handle: AppHandle,
    binding_mode: Arc<Mutex<bool>>,
    runtime_config: Arc<Mutex<MidiRuntimeConfig>>,
}

impl MidiManager {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            midi_input: Arc::new(Mutex::new(None)),
            connection: Arc::new(Mutex::new(None)),
            cached_ports: Arc::new(Mutex::new(Vec::new())),
            app_handle,
            binding_mode: Arc::new(Mutex::new(false)),
            runtime_config: Arc::new(Mutex::new(MidiRuntimeConfig::default())),
        }
    }

    pub fn update_runtime_config(&self, config: MidiRuntimeConfig) {
        if let Ok(mut lock) = self.runtime_config.lock() {
            *lock = config;
        }
    }

    pub fn update_push_to_talk(&self, push_to_talk: bool) {
        if let Ok(mut lock) = self.runtime_config.lock() {
            lock.push_to_talk = push_to_talk;
        }
    }

    pub fn get_ports(&self) -> Result<Vec<String>> {
        let is_connected = self
            .connection
            .lock()
            .map_err(|e| anyhow!("Lock error: {}", e))?
            .is_some();

        if is_connected {
            let cached = self
                .cached_ports
                .lock()
                .map_err(|e| anyhow!("Lock error: {}", e))?
                .clone();
            return Ok(cached);
        }

        self.ensure_midi_input()?;

        let ports = {
            let midi_lock = self
                .midi_input
                .lock()
                .map_err(|e| anyhow!("Lock error: {}", e))?;

            let midi_in = midi_lock
                .as_ref()
                .ok_or_else(|| anyhow!("MIDI support could not be initialized"))?;

            list_port_names(midi_in)
        };

        self.update_cached_ports(&ports);
        debug!("Detected {} MIDI input port(s)", ports.len());
        Ok(ports)
    }

    pub fn set_binding_mode(&self, binding: bool) -> Result<()> {
        let mut lock = self
            .binding_mode
            .lock()
            .map_err(|e| anyhow!("Lock error: {}", e))?;
        *lock = binding;
        Ok(())
    }

    pub fn disconnect(&self) -> Result<()> {
        let maybe_conn = {
            let mut conn_lock = self
                .connection
                .lock()
                .map_err(|e| anyhow!("Lock error: {}", e))?;
            conn_lock.take()
        };

        if let Some(conn) = maybe_conn {
            let (midi_in, _) = conn.close();
            let mut midi_lock = self
                .midi_input
                .lock()
                .map_err(|e| anyhow!("Lock error: {}", e))?;
            *midi_lock = Some(midi_in);
        }

        Ok(())
    }

    pub fn connect(&self, device_name: &str) -> Result<()> {
        self.disconnect()?;
        self.ensure_midi_input()?;

        let mut midi_in = {
            let mut midi_lock = self
                .midi_input
                .lock()
                .map_err(|e| anyhow!("Lock error: {}", e))?;

            midi_lock
                .take()
                .ok_or_else(|| anyhow!("MIDI support could not be initialized"))?
        };

        midi_in.ignore(Ignore::None);
        let port_names = list_port_names(&midi_in);
        self.update_cached_ports(&port_names);

        let port = match midi_in
            .ports()
            .into_iter()
            .find(|p| midi_in.port_name(p).is_ok_and(|name| name == device_name))
        {
            Some(port) => port,
            None => {
                let mut midi_lock = self
                    .midi_input
                    .lock()
                    .map_err(|e| anyhow!("Lock error: {}", e))?;
                *midi_lock = Some(midi_in);
                return Err(anyhow!("MIDI device '{}' not found", device_name));
            }
        };

        let app_handle = self.app_handle.clone();
        let binding_mode = self.binding_mode.clone();
        let runtime_config = self.runtime_config.clone();

        let conn = match midi_in.connect(
            &port,
            "handy-midi",
            move |_stamp, message, _| {
                let is_binding_mode = binding_mode.lock().map(|guard| *guard).unwrap_or(false);

                if is_binding_mode {
                    if !is_bindable_message(message) {
                        return;
                    }

                    let trigger = normalize_trigger_bytes(message);
                    info!("Bound MIDI trigger: {:?}", trigger);
                    let _ = app_handle.emit("midi-trigger-bound", trigger);
                    return;
                }

                let config = runtime_config
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();

                if !config.enabled {
                    return;
                }

                let Some(trigger) = config.trigger.as_ref() else {
                    return;
                };

                if !matches_trigger(message, trigger) {
                    return;
                }

                let Some(is_press) = infer_press_state(message) else {
                    return;
                };

                let supports_release_semantics = message.len() >= 3;
                let effective_push_to_talk = if supports_release_semantics {
                    config.push_to_talk
                } else {
                    false
                };

                debug!(
                    "Matched MIDI trigger {:?}; is_press={}, push_to_talk={}",
                    trigger, is_press, effective_push_to_talk
                );

                if let Some(coordinator) = app_handle.try_state::<TranscriptionCoordinator>() {
                    coordinator.send_input("transcribe", "MIDI", is_press, effective_push_to_talk);
                }
            },
            (),
        ) {
            Ok(conn) => conn,
            Err(err) => {
                let message = err.to_string();
                let midi_in = err.into_inner();
                let mut midi_lock = self
                    .midi_input
                    .lock()
                    .map_err(|e| anyhow!("Lock error: {}", e))?;
                *midi_lock = Some(midi_in);
                return Err(anyhow!(message));
            }
        };

        let mut conn_lock = self
            .connection
            .lock()
            .map_err(|e| anyhow!("Lock error: {}", e))?;
        *conn_lock = Some(conn);

        info!("Connected to MIDI device: {}", device_name);
        Ok(())
    }

    fn ensure_midi_input(&self) -> Result<()> {
        let mut midi_lock = self
            .midi_input
            .lock()
            .map_err(|e| anyhow!("Lock error: {}", e))?;

        if midi_lock.is_none() {
            *midi_lock = Some(create_midi_input("Handy MIDI Input")?);
        }

        Ok(())
    }

    fn update_cached_ports(&self, ports: &[String]) {
        if let Ok(mut cache) = self.cached_ports.lock() {
            *cache = ports.to_vec();
        }
    }
}

fn create_midi_input(client_name: &str) -> Result<MidiInput> {
    let mut last_error: Option<String> = None;

    for _ in 0..6 {
        match MidiInput::new(client_name) {
            Ok(midi_in) => return Ok(midi_in),
            Err(err) => {
                last_error = Some(err.to_string());
                thread::sleep(Duration::from_millis(120));
            }
        }
    }

    Err(anyhow!(
        "MIDI support could not be initialized{}",
        last_error
            .map(|err| format!(": {}", err))
            .unwrap_or_default()
    ))
}

fn list_port_names(midi_in: &MidiInput) -> Vec<String> {
    let mut port_names = Vec::new();

    for port in midi_in.ports().iter() {
        match midi_in.port_name(port) {
            Ok(name) => port_names.push(name),
            Err(err) => warn!("Skipping MIDI port with unreadable name: {}", err),
        }
    }

    port_names
}

fn is_bindable_message(message: &[u8]) -> bool {
    if message.len() < 2 {
        return false;
    }

    let status = message[0] & 0xF0;
    matches!(status, 0x80 | 0x90 | 0xA0 | 0xB0 | 0xC0 | 0xD0 | 0xE0)
}

fn normalize_trigger_bytes(message: &[u8]) -> Vec<u8> {
    vec![normalize_status_for_trigger(message[0]), message[1]]
}

fn matches_trigger(message: &[u8], trigger: &[u8]) -> bool {
    if message.len() < 2 || trigger.len() < 2 {
        return false;
    }

    let message_status = normalize_status_for_trigger(message[0]);
    let trigger_status = normalize_status_for_trigger(trigger[0]);

    message_status == trigger_status && message[1] == trigger[1]
}

fn normalize_status_for_trigger(status: u8) -> u8 {
    let channel = status & 0x0F;
    let status_type = status & 0xF0;

    match status_type {
        0x80 | 0x90 => 0x90 | channel,
        _ => status,
    }
}

fn infer_press_state(message: &[u8]) -> Option<bool> {
    if message.is_empty() {
        return None;
    }

    let status = message[0] & 0xF0;

    if message.len() < 3 {
        return Some(true);
    }

    match status {
        0x80 => Some(false),
        0x90 => Some(message[2] > 0),
        0xA0 => Some(message[2] > 0),
        0xB0 => Some(message[2] > 63),
        0xC0 => Some(true),
        0xD0 => Some(true),
        0xE0 => Some(true),
        _ => None,
    }
}
