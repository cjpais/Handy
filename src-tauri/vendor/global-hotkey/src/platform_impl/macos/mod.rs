use keyboard_types::{Code, Modifiers};
use objc2::{msg_send, rc::Retained, ClassType};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventSubtype, NSEventType};
use std::{
    collections::{BTreeMap, HashSet},
    ffi::c_void,
    ptr,
    sync::{Arc, Mutex},
};

use crate::{
    hotkey::HotKey,
    platform_impl::platform::ffi::{
        kCFAllocatorDefault, kCFRunLoopCommonModes, CFMachPortCreateRunLoopSource,
        CFRunLoopAddSource, CFRunLoopGetMain, CGEventMask, CGEventRef, CGEventTapCreate,
        CGEventTapEnable, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventTapProxy, CGEventType,
    },
    CGEventMaskBit, GlobalHotKeyEvent,
};

use self::ffi::{
    kEventClassKeyboard, kEventHotKeyPressed, kEventHotKeyReleased, kEventParamDirectObject, noErr,
    typeEventHotKeyID, CFMachPortInvalidate, CFMachPortRef, CFRelease, CFRunLoopRemoveSource,
    CFRunLoopSourceRef, EventHandlerCallRef, EventHandlerRef, EventHotKeyID, EventHotKeyRef,
    EventRef, EventTypeSpec, GetApplicationEventTarget, GetEventKind, GetEventParameter,
    InstallEventHandler, OSStatus, RegisterEventHotKey, RemoveEventHandler, UnregisterEventHotKey,
};

mod ffi;

pub struct GlobalHotKeyManager {
    event_handler_ptr: EventHandlerRef,
    hotkeys: Mutex<BTreeMap<u32, HotKeyWrapper>>,
    event_tap: Mutex<Option<CFMachPortRef>>,
    event_tap_source: Mutex<Option<CFRunLoopSourceRef>>,
    media_hotkeys: Arc<Mutex<HashSet<HotKey>>>,
}

unsafe impl Send for GlobalHotKeyManager {}
unsafe impl Sync for GlobalHotKeyManager {}

impl GlobalHotKeyManager {
    pub fn new() -> crate::Result<Self> {
        let pressed_event_type = EventTypeSpec {
            eventClass: kEventClassKeyboard,
            eventKind: kEventHotKeyPressed,
        };
        let released_event_type = EventTypeSpec {
            eventClass: kEventClassKeyboard,
            eventKind: kEventHotKeyReleased,
        };
        let event_types = [pressed_event_type, released_event_type];

        let ptr = unsafe {
            let mut handler_ref: EventHandlerRef = std::mem::zeroed();

            let result = InstallEventHandler(
                GetApplicationEventTarget(),
                Some(hotkey_handler),
                2,
                event_types.as_ptr(),
                std::ptr::null_mut(),
                &mut handler_ref,
            );

            if result != noErr as _ {
                return Err(crate::Error::OsError(std::io::Error::last_os_error()));
            }

            handler_ref
        };

        Ok(Self {
            event_handler_ptr: ptr,
            hotkeys: Mutex::new(BTreeMap::new()),
            event_tap: Mutex::new(None),
            event_tap_source: Mutex::new(None),
            media_hotkeys: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    pub fn register(&self, hotkey: HotKey) -> crate::Result<()> {
        let mut mods: u32 = 0;
        if hotkey.mods.contains(Modifiers::SHIFT) {
            mods |= 512;
        }
        if hotkey.mods.intersects(Modifiers::SUPER | Modifiers::META) {
            mods |= 256;
        }
        if hotkey.mods.contains(Modifiers::ALT) {
            mods |= 2048;
        }
        if hotkey.mods.contains(Modifiers::CONTROL) {
            mods |= 4096;
        }

        if let Some(scan_code) = key_to_scancode(hotkey.key) {
            let hotkey_id = EventHotKeyID {
                id: hotkey.id(),
                signature: {
                    let mut res: u32 = 0;
                    // can't find a resource for "htrs" so we construct it manually
                    // the construction method below is taken from https://github.com/soffes/HotKey/blob/c13662730cb5bc28de4a799854bbb018a90649bf/Sources/HotKey/HotKeysController.swift#L27
                    // and confirmed by applying the same method to `kEventParamDragRef` which is equal to `drag` in C
                    // and converted to `1685217639` by rust-bindgen.
                    for c in "htrs".chars() {
                        res = (res << 8) + c as u32;
                    }
                    res
                },
            };

            let ptr = unsafe {
                let mut hotkey_ref: EventHotKeyRef = std::mem::zeroed();
                let result = RegisterEventHotKey(
                    scan_code,
                    mods,
                    hotkey_id,
                    GetApplicationEventTarget(),
                    0,
                    &mut hotkey_ref,
                );

                if result != noErr as _ {
                    return Err(crate::Error::FailedToRegister(format!(
                        "RegisterEventHotKey failed for {}",
                        hotkey.key
                    )));
                }

                hotkey_ref
            };

            self.hotkeys
                .lock()
                .unwrap()
                .insert(hotkey.id(), HotKeyWrapper { ptr, hotkey });
            Ok(())
        } else if is_media_key(hotkey.key) {
            {
                let mut media_hotkeys = self.media_hotkeys.lock().unwrap();
                if !media_hotkeys.insert(hotkey) {
                    return Err(crate::Error::AlreadyRegistered(hotkey));
                }
            }
            self.start_watching_media_keys()
        } else {
            Err(crate::Error::FailedToRegister(format!(
                "Unknown scancode for {}",
                hotkey.key
            )))
        }
    }

    pub fn unregister(&self, hotkey: HotKey) -> crate::Result<()> {
        if is_media_key(hotkey.key) {
            let mut media_hotkey = self.media_hotkeys.lock().unwrap();
            media_hotkey.remove(&hotkey);
            if media_hotkey.is_empty() {
                self.stop_watching_media_keys();
            }
        } else if let Some(hotkeywrapper) = self.hotkeys.lock().unwrap().remove(&hotkey.id()) {
            unsafe { self.unregister_hotkey_ptr(hotkeywrapper.ptr, hotkey) }?;
        }

        Ok(())
    }

    pub fn register_all(&self, hotkeys: &[HotKey]) -> crate::Result<()> {
        for hotkey in hotkeys {
            self.register(*hotkey)?;
        }
        Ok(())
    }

    pub fn unregister_all(&self, hotkeys: &[HotKey]) -> crate::Result<()> {
        for hotkey in hotkeys {
            self.unregister(*hotkey)?;
        }
        Ok(())
    }

    unsafe fn unregister_hotkey_ptr(
        &self,
        ptr: EventHotKeyRef,
        hotkey: HotKey,
    ) -> crate::Result<()> {
        if UnregisterEventHotKey(ptr) != noErr as _ {
            return Err(crate::Error::FailedToUnRegister(hotkey));
        }

        Ok(())
    }

    fn start_watching_media_keys(&self) -> crate::Result<()> {
        let mut event_tap = self.event_tap.lock().unwrap();
        let mut event_tap_source = self.event_tap_source.lock().unwrap();

        if event_tap.is_some() || event_tap_source.is_some() {
            return Ok(());
        }

        unsafe {
            let event_mask: CGEventMask = CGEventMaskBit!(CGEventType::SystemDefined);
            let tap = CGEventTapCreate(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                event_mask,
                media_key_event_callback,
                Arc::into_raw(self.media_hotkeys.clone()) as *const c_void,
            );
            if tap.is_null() {
                return Err(crate::Error::FailedToWatchMediaKeyEvent);
            }
            *event_tap = Some(tap);

            let loop_source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0);
            if loop_source.is_null() {
                // cleanup event_tap
                CFMachPortInvalidate(tap);
                CFRelease(tap as *const c_void);
                *event_tap = None;

                return Err(crate::Error::FailedToWatchMediaKeyEvent);
            }
            *event_tap_source = Some(loop_source);

            let run_loop = CFRunLoopGetMain();
            CFRunLoopAddSource(run_loop, loop_source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);

            Ok(())
        }
    }

    fn stop_watching_media_keys(&self) {
        unsafe {
            if let Some(event_tap_source) = self.event_tap_source.lock().unwrap().take() {
                let run_loop = CFRunLoopGetMain();
                CFRunLoopRemoveSource(run_loop, event_tap_source, kCFRunLoopCommonModes);
                CFRelease(event_tap_source as *const c_void);
            }
            if let Some(event_tap) = self.event_tap.lock().unwrap().take() {
                CFMachPortInvalidate(event_tap);
                CFRelease(event_tap as *const c_void);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
enum NX_KEYTYPE {
    Play = 16, // Actually it's Play/Pause
    Next = 17,
    Previous = 18,
    Fast = 19,
    Rewind = 20,
}

impl TryFrom<isize> for NX_KEYTYPE {
    type Error = String;

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        match value {
            16 => Ok(NX_KEYTYPE::Play),
            17 => Ok(NX_KEYTYPE::Next),
            18 => Ok(NX_KEYTYPE::Previous),
            19 => Ok(NX_KEYTYPE::Fast),
            20 => Ok(NX_KEYTYPE::Rewind),
            _ => Err(String::from("Not defined media key")),
        }
    }
}

impl From<NX_KEYTYPE> for Code {
    fn from(nx_keytype: NX_KEYTYPE) -> Self {
        match nx_keytype {
            NX_KEYTYPE::Play => Code::MediaPlayPause,
            NX_KEYTYPE::Next => Code::MediaTrackNext,
            NX_KEYTYPE::Previous => Code::MediaTrackPrevious,
            NX_KEYTYPE::Fast => Code::MediaFastForward,
            NX_KEYTYPE::Rewind => Code::MediaRewind,
        }
    }
}

impl Drop for GlobalHotKeyManager {
    fn drop(&mut self) {
        let hotkeys = self.hotkeys.lock().unwrap().clone();
        for (_, hotkeywrapper) in hotkeys {
            let _ = self.unregister(hotkeywrapper.hotkey);
        }
        unsafe {
            RemoveEventHandler(self.event_handler_ptr);
        }
        self.stop_watching_media_keys()
    }
}

unsafe extern "C" fn hotkey_handler(
    _next_handler: EventHandlerCallRef,
    event: EventRef,
    _user_data: *mut c_void,
) -> OSStatus {
    let mut event_hotkey: EventHotKeyID = std::mem::zeroed();

    let result = GetEventParameter(
        event,
        kEventParamDirectObject,
        typeEventHotKeyID,
        std::ptr::null_mut(),
        std::mem::size_of::<EventHotKeyID>() as _,
        std::ptr::null_mut(),
        &mut event_hotkey as *mut _ as *mut _,
    );

    if result == noErr as _ {
        let event_kind = GetEventKind(event);
        match event_kind {
            #[allow(non_upper_case_globals)]
            kEventHotKeyPressed => GlobalHotKeyEvent::send(GlobalHotKeyEvent {
                id: event_hotkey.id,
                state: crate::HotKeyState::Pressed,
            }),
            #[allow(non_upper_case_globals)]
            kEventHotKeyReleased => GlobalHotKeyEvent::send(GlobalHotKeyEvent {
                id: event_hotkey.id,
                state: crate::HotKeyState::Released,
            }),
            _ => {}
        };
    }

    noErr as _
}

unsafe extern "C" fn media_key_event_callback(
    _proxy: CGEventTapProxy,
    ev_type: CGEventType,
    event: CGEventRef,
    user_info: *const c_void,
) -> CGEventRef {
    if ev_type != CGEventType::SystemDefined {
        return event;
    }

    let ns_event: Retained<NSEvent> = msg_send![NSEvent::class(), eventWithCGEvent: event];
    let event_type = ns_event.r#type();
    let event_subtype = ns_event.subtype();

    if event_type == NSEventType::SystemDefined && event_subtype == NSEventSubtype::ScreenChanged {
        // Key
        let data_1 = ns_event.data1();
        let nx_keytype = NX_KEYTYPE::try_from((data_1 & 0xFFFF0000) >> 16);
        if nx_keytype.is_err() {
            return event;
        }
        let nx_keytype = nx_keytype.unwrap();

        // Modifiers
        let flags = ns_event.modifierFlags();
        let mut mods = Modifiers::empty();
        if flags.contains(NSEventModifierFlags::Shift) {
            mods |= Modifiers::SHIFT;
        }
        if flags.contains(NSEventModifierFlags::Control) {
            mods |= Modifiers::CONTROL;
        }
        if flags.contains(NSEventModifierFlags::Option) {
            mods |= Modifiers::ALT;
        }
        if flags.contains(NSEventModifierFlags::Command) {
            mods |= Modifiers::META;
        }

        // Generate hotkey for matching
        let hotkey = HotKey::new(Some(mods), nx_keytype.into());

        // Prevent Arc been releaded after callback returned
        let media_hotkeys = &*(user_info as *const Mutex<HashSet<HotKey>>);

        if let Some(media_hotkey) = media_hotkeys.lock().unwrap().get(&hotkey) {
            let key_flags = data_1 & 0x0000FFFF;
            let is_pressed: bool = ((key_flags & 0xFF00) >> 8) == 0xA;
            GlobalHotKeyEvent::send(GlobalHotKeyEvent {
                id: media_hotkey.id(),
                state: match is_pressed {
                    true => crate::HotKeyState::Pressed,
                    false => crate::HotKeyState::Released,
                },
            });

            // Hotkey was found, return null to stop propagate event
            return ptr::null();
        }
    }

    event
}

#[derive(Clone, Copy, Debug)]
struct HotKeyWrapper {
    ptr: EventHotKeyRef,
    hotkey: HotKey,
}

// can be found in https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX10.6.sdk/System/Library/Frameworks/Carbon.framework/Versions/A/Frameworks/HIToolbox.framework/Versions/A/Headers/Events.h
pub fn key_to_scancode(code: Code) -> Option<u32> {
    match code {
        Code::KeyA => Some(0x00),
        Code::KeyS => Some(0x01),
        Code::KeyD => Some(0x02),
        Code::KeyF => Some(0x03),
        Code::KeyH => Some(0x04),
        Code::KeyG => Some(0x05),
        Code::KeyZ => Some(0x06),
        Code::KeyX => Some(0x07),
        Code::KeyC => Some(0x08),
        Code::KeyV => Some(0x09),
        Code::KeyB => Some(0x0b),
        Code::KeyQ => Some(0x0c),
        Code::KeyW => Some(0x0d),
        Code::KeyE => Some(0x0e),
        Code::KeyR => Some(0x0f),
        Code::KeyY => Some(0x10),
        Code::KeyT => Some(0x11),
        Code::Digit1 => Some(0x12),
        Code::Digit2 => Some(0x13),
        Code::Digit3 => Some(0x14),
        Code::Digit4 => Some(0x15),
        Code::Digit6 => Some(0x16),
        Code::Digit5 => Some(0x17),
        Code::Equal => Some(0x18),
        Code::Digit9 => Some(0x19),
        Code::Digit7 => Some(0x1a),
        Code::Minus => Some(0x1b),
        Code::Digit8 => Some(0x1c),
        Code::Digit0 => Some(0x1d),
        Code::BracketRight => Some(0x1e),
        Code::KeyO => Some(0x1f),
        Code::KeyU => Some(0x20),
        Code::BracketLeft => Some(0x21),
        Code::KeyI => Some(0x22),
        Code::KeyP => Some(0x23),
        Code::Enter => Some(0x24),
        Code::KeyL => Some(0x25),
        Code::KeyJ => Some(0x26),
        Code::Quote => Some(0x27),
        Code::KeyK => Some(0x28),
        Code::Semicolon => Some(0x29),
        Code::Backslash => Some(0x2a),
        Code::Comma => Some(0x2b),
        Code::Slash => Some(0x2c),
        Code::KeyN => Some(0x2d),
        Code::KeyM => Some(0x2e),
        Code::Period => Some(0x2f),
        Code::Tab => Some(0x30),
        Code::Space => Some(0x31),
        Code::Backquote => Some(0x32),
        Code::Backspace => Some(0x33),
        Code::Escape => Some(0x35),
        Code::F17 => Some(0x40),
        Code::NumpadDecimal => Some(0x41),
        Code::NumpadMultiply => Some(0x43),
        Code::NumpadAdd => Some(0x45),
        Code::NumLock => Some(0x47),
        Code::AudioVolumeUp => Some(0x48),
        Code::AudioVolumeDown => Some(0x49),
        Code::AudioVolumeMute => Some(0x4a),
        Code::NumpadDivide => Some(0x4b),
        Code::NumpadEnter => Some(0x4c),
        Code::NumpadSubtract => Some(0x4e),
        Code::F18 => Some(0x4f),
        Code::F19 => Some(0x50),
        Code::NumpadEqual => Some(0x51),
        Code::Numpad0 => Some(0x52),
        Code::Numpad1 => Some(0x53),
        Code::Numpad2 => Some(0x54),
        Code::Numpad3 => Some(0x55),
        Code::Numpad4 => Some(0x56),
        Code::Numpad5 => Some(0x57),
        Code::Numpad6 => Some(0x58),
        Code::Numpad7 => Some(0x59),
        Code::F20 => Some(0x5a),
        Code::Numpad8 => Some(0x5b),
        Code::Numpad9 => Some(0x5c),
        Code::F5 => Some(0x60),
        Code::F6 => Some(0x61),
        Code::F7 => Some(0x62),
        Code::F3 => Some(0x63),
        Code::F8 => Some(0x64),
        Code::F9 => Some(0x65),
        Code::F11 => Some(0x67),
        Code::F13 => Some(0x69),
        Code::F16 => Some(0x6a),
        Code::F14 => Some(0x6b),
        Code::F10 => Some(0x6d),
        Code::F12 => Some(0x6f),
        Code::F15 => Some(0x71),
        Code::Insert => Some(0x72),
        Code::Home => Some(0x73),
        Code::PageUp => Some(0x74),
        Code::Delete => Some(0x75),
        Code::F4 => Some(0x76),
        Code::End => Some(0x77),
        Code::F2 => Some(0x78),
        Code::PageDown => Some(0x79),
        Code::F1 => Some(0x7a),
        Code::ArrowLeft => Some(0x7b),
        Code::ArrowRight => Some(0x7c),
        Code::ArrowDown => Some(0x7d),
        Code::ArrowUp => Some(0x7e),
        Code::CapsLock => Some(0x39),
        Code::PrintScreen => Some(0x46),
        _ => None,
    }
}

fn is_media_key(code: Code) -> bool {
    matches!(
        code,
        Code::MediaPlayPause
            | Code::MediaTrackNext
            | Code::MediaTrackPrevious
            | Code::MediaFastForward
            | Code::MediaRewind
    )
}
