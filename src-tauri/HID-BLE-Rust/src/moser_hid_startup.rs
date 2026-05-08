use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Receiver,
    Bluetooth,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechRecognitionMode {
    SpeechInput,
    VoiceTranslationInput,
    VoiceSearch,
    SpeechInputAi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMKey {
    None,
    PhoneticTyping,
    TranslationTyping,
    VoiceSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonFunctionDefinition {
    VoiceTyping,
    VoiceCommand,
    Other,
}

#[derive(Debug, Clone)]
pub struct HandlerConfig {
    pub manufacturer: i32,
    pub mouse_connection_mode: ConnectionMode,
    pub mouser_ble_online: bool,
    pub mouser_ble_manufacturer_id: i32,
    pub stop_voice_typing: bool,
    pub button_function_definition: ButtonFunctionDefinition,
    pub mouse_m_key: MouseMKey,
    pub speech_mode: SpeechRecognitionMode,
}

impl Default for HandlerConfig {
    fn default() -> Self {
        Self {
            manufacturer: 0,
            mouse_connection_mode: ConnectionMode::None,
            mouser_ble_online: false,
            mouser_ble_manufacturer_id: 0,
            stop_voice_typing: false,
            button_function_definition: ButtonFunctionDefinition::Other,
            mouse_m_key: MouseMKey::None,
            speech_mode: SpeechRecognitionMode::SpeechInput,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct HandlerState {
    pub is_ble_mode: bool,
    pub is_recording: bool,
    pub is_main_window_open: bool,
    pub mouse_m_key_released: bool,
}

#[derive(Debug)]
pub struct DebounceState {
    last_tick: Option<Instant>,
    pub time_consuming_count: i32,
    pub time_consuming_count1: i32,
    threshold: Duration,
}

impl Default for DebounceState {
    fn default() -> Self {
        Self {
            last_tick: None,
            time_consuming_count: 0,
            time_consuming_count1: 0,
            threshold: Duration::from_millis(400),
        }
    }
}

impl DebounceState {
    pub fn should_return(&mut self) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_tick {
            if now.duration_since(last) < self.threshold && self.time_consuming_count1 != 0 {
                self.time_consuming_count1 = 1;
                return true;
            }
        }

        self.time_consuming_count = 0;
        self.time_consuming_count1 = 1;
        self.time_consuming_count += 1;
        self.last_tick = Some(now);
        false
    }
}

pub trait MoserHost {
    type Error;

    fn log_debug(&mut self, msg: &str);
    fn log_error(&mut self, msg: &str);

    fn send_bytes_mouse_recording_start(&mut self) -> Result<(), Self::Error>;
    fn send_bytes_mouse_recording_stop(&mut self) -> Result<(), Self::Error>;

    fn mouse_recording_start(&mut self) -> Result<(), Self::Error>;
    fn mouse_recording_stop(&mut self) -> Result<(), Self::Error>;

    fn m_key_execute(&mut self) -> Result<(), Self::Error>;
    fn m_key_execute_on_click(&mut self) -> Result<(), Self::Error>;

    fn open_main_window(&mut self) -> Result<(), Self::Error>;
    fn close_main_window(&mut self) -> Result<(), Self::Error>;

    fn decode_adpcm_to_pcm(&mut self, adpcm_60: &[u8]) -> Result<Vec<u8>, Self::Error>;
    fn append_pcm(&mut self, pcm: &[u8]) -> Result<(), Self::Error>;
}

pub struct MoserHidStartupHandler {
    pub config: HandlerConfig,
    pub state: HandlerState,
    pub debounce: DebounceState,
}

impl Default for MoserHidStartupHandler {
    fn default() -> Self {
        Self {
            config: HandlerConfig::default(),
            state: HandlerState::default(),
            debounce: DebounceState::default(),
        }
    }
}

impl MoserHidStartupHandler {
    pub fn data_received<H: MoserHost>(
        &mut self,
        mut data: Vec<u8>,
        host: &mut H,
    ) -> Result<(), H::Error> {
        if data.is_empty() {
            return Ok(());
        }

        self.state.is_ble_mode =
            matches!(self.config.mouse_connection_mode, ConnectionMode::Bluetooth)
                || self.config.mouser_ble_online
                || self.config.mouser_ble_manufacturer_id == 7;

        if self.state.is_ble_mode && data.len() >= 2 && data[0] == 0xCC && data[1] == 0x3C {
            data.remove(0);
        }

        let (idx1, idx2) = if self.state.is_ble_mode {
            if data.first() == Some(&0x3C) {
                (2usize, 3usize)
            } else {
                (3usize, 4usize)
            }
        } else {
            (2usize, 3usize)
        };

        // Skip Manufacturer == 0 by request.
        if self.config.manufacturer == 0 {
            return Ok(());
        }

        if self.config.manufacturer != 1 {
            return Ok(());
        }

        self.try_handle_audio_frame(&data, host)?;

        if data.first() == Some(&0x3C) {
            return Ok(());
        }

        if data.len() <= idx2 {
            return Ok(());
        }

        let opcode = data[idx1];
        let subcode = data[idx2];

        match (opcode, subcode) {
            (32, 1) => {
                if !self.state.is_recording {
                    if self.debounce.should_return() {
                        return Ok(());
                    }
                    host.send_bytes_mouse_recording_start()?;
                    self.config.speech_mode = SpeechRecognitionMode::SpeechInput;
                    host.mouse_recording_start()?;
                    self.state.is_recording = true;
                } else {
                    host.send_bytes_mouse_recording_stop()?;
                    host.mouse_recording_stop()?;
                    self.state.is_recording = false;
                }
            }
            (32, 3) => {
                if self.debounce.should_return() {
                    return Ok(());
                }
                host.send_bytes_mouse_recording_start()?;
                self.config.speech_mode = SpeechRecognitionMode::SpeechInput;
                host.mouse_recording_start()?;
            }
            (32, 4) => {
                host.send_bytes_mouse_recording_stop()?;
                host.mouse_recording_stop()?;
            }
            (34, 1) => {
                if self.debounce.should_return() {
                    return Ok(());
                }
                host.m_key_execute_on_click()?;
            }
            (34, 3) => {
                if self.debounce.should_return() {
                    return Ok(());
                }
                if matches!(
                    self.config.mouse_m_key,
                    MouseMKey::PhoneticTyping
                        | MouseMKey::TranslationTyping
                        | MouseMKey::VoiceSearch
                ) {
                    host.send_bytes_mouse_recording_start()?;
                }
                host.m_key_execute()?;
            }
            (34, 4) => {
                self.state.mouse_m_key_released = true;
                if matches!(
                    self.config.mouse_m_key,
                    MouseMKey::PhoneticTyping
                        | MouseMKey::TranslationTyping
                        | MouseMKey::VoiceSearch
                ) {
                    host.send_bytes_mouse_recording_stop()?;
                    host.mouse_recording_stop()?;
                }
            }
            (35, 3) => {
                if self.debounce.should_return() {
                    return Ok(());
                }
                host.send_bytes_mouse_recording_start()?;
                match self.config.button_function_definition {
                    ButtonFunctionDefinition::VoiceTyping => {
                        self.config.speech_mode = SpeechRecognitionMode::SpeechInput;
                        host.mouse_recording_start()?;
                    }
                    ButtonFunctionDefinition::VoiceCommand => {
                        self.config.mouse_m_key = MouseMKey::VoiceSearch;
                        host.m_key_execute()?;
                    }
                    ButtonFunctionDefinition::Other => {
                        self.config.speech_mode = SpeechRecognitionMode::SpeechInput;
                        host.mouse_recording_start()?;
                    }
                }
            }
            (35, 4) => {
                host.send_bytes_mouse_recording_stop()?;
                host.mouse_recording_stop()?;
            }
            (48, 1) => {
                if self.debounce.should_return() {
                    return Ok(());
                }
                if self.state.is_main_window_open {
                    host.close_main_window()?;
                    self.state.is_main_window_open = false;
                } else {
                    host.open_main_window()?;
                    self.state.is_main_window_open = true;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn try_handle_audio_frame<H: MoserHost>(
        &mut self,
        data: &[u8],
        host: &mut H,
    ) -> Result<(), H::Error> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() < 63 || data[0] != 0x3C {
            return Ok(());
        }
        let adpcm = &data[1..61];
        let pcm = host.decode_adpcm_to_pcm(adpcm)?;
        host.append_pcm(&pcm)?;
        Ok(())
    }
}
