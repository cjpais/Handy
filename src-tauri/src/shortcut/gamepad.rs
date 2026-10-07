//! Gamepad shortcut management and background polling
//!
//! Handles detecting gamepad input combination (defaulting to `select + start`)
//! to trigger voice transcription globally.

use crate::settings;
use crate::shortcut::handler::handle_shortcut_event;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static SUSPENDED: AtomicBool = AtomicBool::new(false);
static CURRENT_BINDING: Lazy<RwLock<String>> =
    Lazy::new(|| RwLock::new("select + start".to_string()));

/// Check if a given shortcut ID is a gamepad binding
pub fn is_gamepad_binding(id: &str) -> bool {
    id == "transcribe_gamepad" || id.ends_with("_gamepad") || id.starts_with("gamepad_")
}

/// Normalize gamepad button names from a string like "select + start" or "select+start"
pub fn parse_gamepad_buttons(raw: &str) -> Vec<String> {
    raw.split(&['+', ',', ' '][..])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Update the active gamepad binding in memory
pub fn update_binding(_app: &AppHandle, id: &str, binding_str: &str) {
    if is_gamepad_binding(id) {
        if let Ok(mut guard) = CURRENT_BINDING.write() {
            *guard = binding_str.trim().to_lowercase();
            info!("Updated gamepad binding to: {}", *guard);
        }
    }
}

/// Suspend gamepad listening (e.g. while recording shortcuts in settings UI)
pub fn suspend_gamepad() {
    SUSPENDED.store(true, Ordering::Relaxed);
}

/// Resume gamepad listening
pub fn resume_gamepad() {
    SUSPENDED.store(false, Ordering::Relaxed);
}

/// Get the current gamepad binding string
pub fn get_gamepad_binding(app: &AppHandle) -> String {
    let settings = settings::get_settings(app);
    settings
        .bindings
        .get("transcribe_gamepad")
        .map(|b| b.current_binding.clone())
        .unwrap_or_else(|| {
            CURRENT_BINDING
                .read()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "select + start".to_string())
        })
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
mod windows_xinput {
    use windows::core::{s, w};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct XINPUT_GAMEPAD {
        pub wButtons: u16,
        pub bLeftTrigger: u8,
        pub bRightTrigger: u8,
        pub sThumbLX: i16,
        pub sThumbLY: i16,
        pub sThumbRX: i16,
        pub sThumbRY: i16,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct XINPUT_STATE {
        pub dwPacketNumber: u32,
        pub Gamepad: XINPUT_GAMEPAD,
    }

    pub const XINPUT_GAMEPAD_DPAD_UP: u16 = 0x0001;
    pub const XINPUT_GAMEPAD_DPAD_DOWN: u16 = 0x0002;
    pub const XINPUT_GAMEPAD_DPAD_LEFT: u16 = 0x0004;
    pub const XINPUT_GAMEPAD_DPAD_RIGHT: u16 = 0x0008;
    pub const XINPUT_GAMEPAD_START: u16 = 0x0010;
    pub const XINPUT_GAMEPAD_BACK: u16 = 0x0020;
    pub const XINPUT_GAMEPAD_LEFT_THUMB: u16 = 0x0040;
    pub const XINPUT_GAMEPAD_RIGHT_THUMB: u16 = 0x0080;
    pub const XINPUT_GAMEPAD_LEFT_SHOULDER: u16 = 0x0100;
    pub const XINPUT_GAMEPAD_RIGHT_SHOULDER: u16 = 0x0200;
    pub const XINPUT_GAMEPAD_A: u16 = 0x1000;
    pub const XINPUT_GAMEPAD_B: u16 = 0x2000;
    pub const XINPUT_GAMEPAD_X: u16 = 0x4000;
    pub const XINPUT_GAMEPAD_Y: u16 = 0x8000;

    type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

    pub struct XInputDriver {
        get_state: XInputGetStateFn,
    }

    impl XInputDriver {
        pub fn load() -> Option<Self> {
            unsafe {
                let module = LoadLibraryW(w!("xinput1_4.dll"))
                    .or_else(|_| LoadLibraryW(w!("xinput1_3.dll")))
                    .or_else(|_| LoadLibraryW(w!("xinput9_1_0.dll")))
                    .ok()?;
                let proc = GetProcAddress(module, s!("XInputGetState"))?;
                let get_state: XInputGetStateFn = std::mem::transmute(proc);
                Some(Self { get_state })
            }
        }

        pub fn get_state(&self, user_index: u32) -> Option<XINPUT_STATE> {
            let mut state = XINPUT_STATE::default();
            let res = unsafe { (self.get_state)(user_index, &mut state) };
            if res == 0 {
                Some(state)
            } else {
                None
            }
        }
    }

    pub fn button_matches(btn_str: &str, state: &XINPUT_STATE) -> bool {
        match btn_str {
            "select" | "back" | "view" | "share" => {
                (state.Gamepad.wButtons & XINPUT_GAMEPAD_BACK) != 0
            }
            "start" | "menu" | "options" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_START) != 0,
            "a" | "south" | "cross" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_A) != 0,
            "b" | "east" | "circle" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_B) != 0,
            "x" | "west" | "square" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_X) != 0,
            "y" | "north" | "triangle" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_Y) != 0,
            "lb" | "l1" | "left_shoulder" | "left_bumper" => {
                (state.Gamepad.wButtons & XINPUT_GAMEPAD_LEFT_SHOULDER) != 0
            }
            "rb" | "r1" | "right_shoulder" | "right_bumper" => {
                (state.Gamepad.wButtons & XINPUT_GAMEPAD_RIGHT_SHOULDER) != 0
            }
            "lt" | "l2" | "left_trigger" => state.Gamepad.bLeftTrigger > 30,
            "rt" | "r2" | "right_trigger" => state.Gamepad.bRightTrigger > 30,
            "ls" | "l3" | "left_thumb" | "left_stick" => {
                (state.Gamepad.wButtons & XINPUT_GAMEPAD_LEFT_THUMB) != 0
            }
            "rs" | "r3" | "right_thumb" | "right_stick" => {
                (state.Gamepad.wButtons & XINPUT_GAMEPAD_RIGHT_THUMB) != 0
            }
            "dpad_up" | "up" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_DPAD_UP) != 0,
            "dpad_down" | "down" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_DPAD_DOWN) != 0,
            "dpad_left" | "left" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_DPAD_LEFT) != 0,
            "dpad_right" | "right" => (state.Gamepad.wButtons & XINPUT_GAMEPAD_DPAD_RIGHT) != 0,
            _ => false,
        }
    }
}

/// Initialize the gamepad background polling listener
pub fn init_gamepad(app: &AppHandle) {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    // Initialize binding from settings
    let initial_binding = get_gamepad_binding(app);
    if let Ok(mut guard) = CURRENT_BINDING.write() {
        *guard = initial_binding;
    }

    let app_handle = app.clone();

    #[cfg(target_os = "windows")]
    {
        if let Err(e) = thread::Builder::new()
            .name("handy-gamepad-listener".to_string())
            .spawn(move || {
                let driver = match windows_xinput::XInputDriver::load() {
                    Some(d) => d,
                    None => {
                        warn!("XInput driver could not be loaded; gamepad shortcuts will not be available globally");
                        return;
                    }
                };

                info!("Gamepad background listener started successfully with XInput");
                let mut was_pressed = false;

                loop {
                    thread::sleep(Duration::from_millis(20));

                    if SUSPENDED.load(Ordering::Relaxed) {
                        if was_pressed {
                            was_pressed = false;
                            let binding_str = CURRENT_BINDING
                                .read()
                                .map(|g| g.clone())
                                .unwrap_or_else(|_| "select + start".to_string());
                            handle_shortcut_event(
                                &app_handle,
                                "transcribe_gamepad",
                                &binding_str,
                                false,
                            );
                        }
                        continue;
                    }

                    let current_binding = CURRENT_BINDING
                        .read()
                        .map(|g| g.clone())
                        .unwrap_or_else(|_| "select + start".to_string());
                    let target_buttons = parse_gamepad_buttons(&current_binding);
                    if target_buttons.is_empty() {
                        continue;
                    }

                    let mut is_pressed = false;
                    for user_index in 0..4 {
                        if let Some(state) = driver.get_state(user_index) {
                            let all_match = target_buttons
                                .iter()
                                .all(|btn| windows_xinput::button_matches(btn, &state));
                            if all_match {
                                is_pressed = true;
                                break;
                            }
                        }
                    }

                    if is_pressed && !was_pressed {
                        was_pressed = true;
                        debug!("Gamepad combination '{}' pressed", current_binding);
                        handle_shortcut_event(
                            &app_handle,
                            "transcribe_gamepad",
                            &current_binding,
                            true,
                        );
                    } else if !is_pressed && was_pressed {
                        was_pressed = false;
                        debug!("Gamepad combination '{}' released", current_binding);
                        handle_shortcut_event(
                            &app_handle,
                            "transcribe_gamepad",
                            &current_binding,
                            false,
                        );
                    }
                }
            })
        {
            error!("Failed to spawn gamepad polling thread: {}", e);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        info!("Gamepad background polling via XInput is only supported on Windows; on this platform shortcuts are triggered via Gamepad API");
    }
}
