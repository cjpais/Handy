#[cfg(any(all(unix, not(target_os = "macos")), target_os = "windows"))]
use log::trace;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(test)]
use strum_macros::EnumIter;

// A key on the keyboard.
/// Use [`Key::Unicode`] to enter arbitrary Unicode chars.
/// If a key is missing, please open an issue in our repo and we will quickly
/// add it. In the mean time, you can simulate that key by using [`Key::Other`]
/// or the [`crate::Keyboard::raw`] function. Some of the keys are only
/// available on a specific platform. Use conditional compilation to use them.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(test, derive(EnumIter))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    #[cfg(target_os = "windows")]
    Num0,
    #[cfg(target_os = "windows")]
    Num1,
    #[cfg(target_os = "windows")]
    Num2,
    #[cfg(target_os = "windows")]
    Num3,
    #[cfg(target_os = "windows")]
    Num4,
    #[cfg(target_os = "windows")]
    Num5,
    #[cfg(target_os = "windows")]
    Num6,
    #[cfg(target_os = "windows")]
    Num7,
    #[cfg(target_os = "windows")]
    Num8,
    #[cfg(target_os = "windows")]
    Num9,
    #[cfg(target_os = "windows")]
    A,
    #[cfg(target_os = "windows")]
    B,
    #[cfg(target_os = "windows")]
    C,
    #[cfg(target_os = "windows")]
    D,
    #[cfg(target_os = "windows")]
    E,
    #[cfg(target_os = "windows")]
    F,
    #[cfg(target_os = "windows")]
    G,
    #[cfg(target_os = "windows")]
    H,
    #[cfg(target_os = "windows")]
    I,
    #[cfg(target_os = "windows")]
    J,
    #[cfg(target_os = "windows")]
    K,
    #[cfg(target_os = "windows")]
    L,
    #[cfg(target_os = "windows")]
    M,
    #[cfg(target_os = "windows")]
    N,
    #[cfg(target_os = "windows")]
    O,
    #[cfg(target_os = "windows")]
    P,
    #[cfg(target_os = "windows")]
    Q,
    #[cfg(target_os = "windows")]
    R,
    #[cfg(target_os = "windows")]
    S,
    #[cfg(target_os = "windows")]
    T,
    #[cfg(target_os = "windows")]
    U,
    #[cfg(target_os = "windows")]
    V,
    #[cfg(target_os = "windows")]
    W,
    #[cfg(target_os = "windows")]
    X,
    #[cfg(target_os = "windows")]
    Y,
    #[cfg(target_os = "windows")]
    Z,
    #[cfg(target_os = "windows")]
    AbntC1,
    #[cfg(target_os = "windows")]
    AbntC2,
    #[cfg(target_os = "windows")]
    Accept,
    Add,
    /// alt key on Linux and Windows (option key on macOS)
    Alt,
    #[cfg(target_os = "windows")]
    Apps,
    #[cfg(target_os = "windows")]
    Attn,
    /// backspace key
    Backspace,
    #[cfg(all(unix, not(target_os = "macos")))]
    Break,
    #[cfg(all(unix, not(target_os = "macos")))]
    Begin,
    #[cfg(target_os = "macos")]
    BrightnessDown,
    #[cfg(target_os = "macos")]
    BrightnessUp,
    #[cfg(target_os = "windows")]
    BrowserBack,
    #[cfg(target_os = "windows")]
    BrowserFavorites,
    #[cfg(target_os = "windows")]
    BrowserForward,
    #[cfg(target_os = "windows")]
    BrowserHome,
    #[cfg(target_os = "windows")]
    BrowserRefresh,
    #[cfg(target_os = "windows")]
    BrowserSearch,
    #[cfg(target_os = "windows")]
    BrowserStop,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Cancel,
    /// caps lock key
    CapsLock,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Clear,
    #[deprecated(since = "0.0.12", note = "now renamed to Meta")]
    /// command key on macOS (super key on Linux, windows key on Windows)
    #[cfg_attr(feature = "serde", serde(alias = "cmd"))]
    Command,
    #[cfg(target_os = "macos")]
    ContrastUp,
    #[cfg(target_os = "macos")]
    ContrastDown,
    /// control key
    #[cfg_attr(feature = "serde", serde(alias = "ctrl"))]
    Control,
    #[cfg(target_os = "windows")]
    Convert,
    #[cfg(target_os = "windows")]
    Crsel,
    #[cfg(target_os = "windows")]
    DBEAlphanumeric,
    #[cfg(target_os = "windows")]
    DBECodeinput,
    #[cfg(target_os = "windows")]
    DBEDetermineString,
    #[cfg(target_os = "windows")]
    DBEEnterDLGConversionMode,
    #[cfg(target_os = "windows")]
    DBEEnterIMEConfigMode,
    #[cfg(target_os = "windows")]
    DBEEnterWordRegisterMode,
    #[cfg(target_os = "windows")]
    DBEFlushString,
    #[cfg(target_os = "windows")]
    DBEHiragana,
    #[cfg(target_os = "windows")]
    DBEKatakana,
    #[cfg(target_os = "windows")]
    DBENoCodepoint,
    #[cfg(target_os = "windows")]
    DBENoRoman,
    #[cfg(target_os = "windows")]
    DBERoman,
    #[cfg(target_os = "windows")]
    DBESBCSChar,
    #[cfg(target_os = "windows")]
    DBESChar,
    Decimal,
    /// delete key
    Delete,
    Divide,
    /// down arrow key
    DownArrow,
    #[cfg(target_os = "macos")]
    Eject,
    /// end key
    End,
    #[cfg(target_os = "windows")]
    Ereof,
    /// escape key (esc)
    Escape,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Execute,
    #[cfg(target_os = "windows")]
    Exsel,
    /// F1 key
    F1,
    /// F2 key
    F2,
    /// F3 key
    F3,
    /// F4 key
    F4,
    /// F5 key
    F5,
    /// F6 key
    F6,
    /// F7 key
    F7,
    /// F8 key
    F8,
    /// F9 key
    F9,
    /// F10 key
    F10,
    /// F11 key
    F11,
    /// F12 key
    F12,
    /// F13 key
    F13,
    /// F14 key
    F14,
    /// F15 key
    F15,
    /// F16 key
    F16,
    /// F17 key
    F17,
    /// F18 key
    F18,
    /// F19 key
    F19,
    /// F20 key
    F20,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    /// F21 key
    F21,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    /// F22 key
    F22,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    /// F23 key
    F23,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    /// F24 key
    F24,
    #[cfg(all(unix, not(target_os = "macos")))]
    F25,
    #[cfg(all(unix, not(target_os = "macos")))]
    F26,
    #[cfg(all(unix, not(target_os = "macos")))]
    F27,
    #[cfg(all(unix, not(target_os = "macos")))]
    F28,
    #[cfg(all(unix, not(target_os = "macos")))]
    F29,
    #[cfg(all(unix, not(target_os = "macos")))]
    F30,
    #[cfg(all(unix, not(target_os = "macos")))]
    F31,
    #[cfg(all(unix, not(target_os = "macos")))]
    F32,
    #[cfg(all(unix, not(target_os = "macos")))]
    F33,
    #[cfg(all(unix, not(target_os = "macos")))]
    F34,
    #[cfg(all(unix, not(target_os = "macos")))]
    F35,
    #[cfg(target_os = "macos")]
    Function,
    #[cfg(target_os = "windows")]
    Final,
    #[cfg(all(unix, not(target_os = "macos")))]
    Find,
    #[cfg(target_os = "windows")]
    GamepadA,
    #[cfg(target_os = "windows")]
    GamepadB,
    #[cfg(target_os = "windows")]
    GamepadDPadDown,
    #[cfg(target_os = "windows")]
    GamepadDPadLeft,
    #[cfg(target_os = "windows")]
    GamepadDPadRight,
    #[cfg(target_os = "windows")]
    GamepadDPadUp,
    #[cfg(target_os = "windows")]
    GamepadLeftShoulder,
    #[cfg(target_os = "windows")]
    GamepadLeftThumbstickButton,
    #[cfg(target_os = "windows")]
    GamepadLeftThumbstickDown,
    #[cfg(target_os = "windows")]
    GamepadLeftThumbstickLeft,
    #[cfg(target_os = "windows")]
    GamepadLeftThumbstickRight,
    #[cfg(target_os = "windows")]
    GamepadLeftThumbstickUp,
    #[cfg(target_os = "windows")]
    GamepadLeftTrigger,
    #[cfg(target_os = "windows")]
    GamepadMenu,
    #[cfg(target_os = "windows")]
    GamepadRightShoulder,
    #[cfg(target_os = "windows")]
    GamepadRightThumbstickButton,
    #[cfg(target_os = "windows")]
    GamepadRightThumbstickDown,
    #[cfg(target_os = "windows")]
    GamepadRightThumbstickLeft,
    #[cfg(target_os = "windows")]
    GamepadRightThumbstickRight,
    #[cfg(target_os = "windows")]
    GamepadRightThumbstickUp,
    #[cfg(target_os = "windows")]
    GamepadRightTrigger,
    #[cfg(target_os = "windows")]
    GamepadView,
    #[cfg(target_os = "windows")]
    GamepadX,
    #[cfg(target_os = "windows")]
    GamepadY,
    #[cfg(target_os = "windows")]
    Hangeul,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Hangul,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Hanja,
    Help,
    /// home key
    Home,
    #[cfg(target_os = "windows")]
    Ico00,
    #[cfg(target_os = "windows")]
    IcoClear,
    #[cfg(target_os = "windows")]
    IcoHelp,
    #[cfg(target_os = "macos")]
    IlluminationDown,
    #[cfg(target_os = "macos")]
    IlluminationUp,
    #[cfg(target_os = "macos")]
    IlluminationToggle,
    #[cfg(target_os = "windows")]
    IMEOff,
    #[cfg(target_os = "windows")]
    IMEOn,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Insert,
    #[cfg(target_os = "windows")]
    Junja,
    #[cfg(target_os = "windows")]
    Kana,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Kanji,
    #[cfg(target_os = "windows")]
    LaunchApp1,
    #[cfg(target_os = "windows")]
    LaunchApp2,
    #[cfg(target_os = "windows")]
    LaunchMail,
    #[cfg(target_os = "windows")]
    LaunchMediaSelect,
    #[cfg(target_os = "macos")]
    /// Opens launchpad
    Launchpad,
    #[cfg(target_os = "macos")]
    LaunchPanel,
    #[cfg(target_os = "windows")]
    LButton,
    LControl,
    /// left arrow key
    LeftArrow,
    #[cfg(all(unix, not(target_os = "macos")))]
    Linefeed,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    LMenu,
    LShift,
    #[cfg(target_os = "windows")]
    LWin,
    #[cfg(target_os = "windows")]
    MButton,
    #[cfg(target_os = "macos")]
    MediaFast,
    MediaNextTrack,
    MediaPlayPause,
    MediaPrevTrack,
    #[cfg(target_os = "macos")]
    MediaRewind,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    MediaStop,
    /// meta key (also known as "windows", "super", and "command")
    Meta,
    #[cfg(target_os = "macos")]
    /// Opens mission control
    MissionControl,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    ModeChange,
    Multiply,
    #[cfg(target_os = "windows")]
    NavigationAccept,
    #[cfg(target_os = "windows")]
    NavigationCancel,
    #[cfg(target_os = "windows")]
    NavigationDown,
    #[cfg(target_os = "windows")]
    NavigationLeft,
    #[cfg(target_os = "windows")]
    NavigationMenu,
    #[cfg(target_os = "windows")]
    NavigationRight,
    #[cfg(target_os = "windows")]
    NavigationUp,
    #[cfg(target_os = "windows")]
    NavigationView,
    #[cfg(target_os = "windows")]
    NoName,
    #[cfg(target_os = "windows")]
    NonConvert,
    #[cfg(target_os = "windows")]
    None,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Numlock,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    #[cfg(target_os = "windows")]
    OEM1,
    #[cfg(target_os = "windows")]
    OEM102,
    #[cfg(target_os = "windows")]
    OEM2,
    #[cfg(target_os = "windows")]
    OEM3,
    #[cfg(target_os = "windows")]
    OEM4,
    #[cfg(target_os = "windows")]
    OEM5,
    #[cfg(target_os = "windows")]
    OEM6,
    #[cfg(target_os = "windows")]
    OEM7,
    #[cfg(target_os = "windows")]
    OEM8,
    #[cfg(target_os = "windows")]
    OEMAttn,
    #[cfg(target_os = "windows")]
    OEMAuto,
    #[cfg(target_os = "windows")]
    OEMAx,
    #[cfg(target_os = "windows")]
    OEMBacktab,
    #[cfg(target_os = "windows")]
    OEMClear,
    #[cfg(target_os = "windows")]
    OEMComma,
    #[cfg(target_os = "windows")]
    OEMCopy,
    #[cfg(target_os = "windows")]
    OEMCusel,
    #[cfg(target_os = "windows")]
    OEMEnlw,
    #[cfg(target_os = "windows")]
    OEMFinish,
    #[cfg(target_os = "windows")]
    OEMFJJisho,
    #[cfg(target_os = "windows")]
    OEMFJLoya,
    #[cfg(target_os = "windows")]
    OEMFJMasshou,
    #[cfg(target_os = "windows")]
    OEMFJRoya,
    #[cfg(target_os = "windows")]
    OEMFJTouroku,
    #[cfg(target_os = "windows")]
    OEMJump,
    #[cfg(target_os = "windows")]
    OEMMinus,
    #[cfg(target_os = "windows")]
    OEMNECEqual,
    #[cfg(target_os = "windows")]
    OEMPA1,
    #[cfg(target_os = "windows")]
    OEMPA2,
    #[cfg(target_os = "windows")]
    OEMPA3,
    #[cfg(target_os = "windows")]
    OEMPeriod,
    #[cfg(target_os = "windows")]
    OEMPlus,
    #[cfg(target_os = "windows")]
    OEMReset,
    #[cfg(target_os = "windows")]
    OEMWsctrl,
    /// option key on macOS (alt key on Linux and Windows)
    Option,
    #[cfg(target_os = "windows")]
    PA1,
    #[cfg(target_os = "windows")]
    Packet,
    /// page down key
    PageDown,
    /// page up key
    PageUp,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Pause,
    #[cfg(target_os = "windows")]
    Play,
    #[cfg(target_os = "macos")]
    Power,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    #[deprecated(since = "0.2.2", note = "now renamed to PrintScr")]
    Print,
    /// Take a screenshot
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    #[doc(alias = "Print")]
    #[doc(alias = "Snapshot")]
    PrintScr,
    #[cfg(target_os = "windows")]
    Processkey,
    #[cfg(target_os = "windows")]
    RButton,
    #[cfg(target_os = "macos")]
    RCommand,
    RControl,
    #[cfg(all(unix, not(target_os = "macos")))]
    Redo,
    /// return key
    Return,
    /// right arrow key
    RightArrow,
    #[cfg(target_os = "windows")]
    RMenu,
    #[cfg(target_os = "macos")]
    ROption,
    RShift,
    #[cfg(target_os = "windows")]
    RWin,
    #[cfg(target_os = "windows")]
    Scroll,
    #[cfg(all(unix, not(target_os = "macos")))]
    ScrollLock,
    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    Select,
    #[cfg(all(unix, not(target_os = "macos")))]
    ScriptSwitch,
    #[cfg(target_os = "windows")]
    Separator,
    /// shift key
    Shift,
    #[cfg(all(unix, not(target_os = "macos")))]
    /// Lock shift key
    ShiftLock,
    #[cfg(target_os = "windows")]
    Sleep,
    #[cfg(target_os = "windows")]
    #[deprecated(since = "0.2.2", note = "now renamed to PrintScr")]
    Snapshot,
    /// space key
    Space,
    Subtract,
    #[deprecated(since = "0.0.12", note = "now renamed to Meta")]
    /// super key on linux (command key on macOS, windows key on Windows)
    Super,
    #[cfg(all(unix, not(target_os = "macos")))]
    SysReq,
    /// tab key (tabulator)
    Tab,
    #[cfg(all(unix, not(target_os = "macos")))]
    Undo,
    /// up arrow key
    UpArrow,
    #[cfg(target_os = "macos")]
    VidMirror,
    VolumeDown,
    VolumeMute,
    VolumeUp,
    #[cfg(all(unix, not(target_os = "macos")))]
    /// microphone mute toggle on linux
    MicMute,
    #[deprecated(since = "0.0.12", note = "now renamed to Meta")]
    /// windows key on Windows (super key on Linux, command key on macOS)
    Windows,
    #[cfg(target_os = "windows")]
    XButton1,
    #[cfg(target_os = "windows")]
    XButton2,
    #[cfg(target_os = "windows")]
    Zoom,
    /// Unicode character
    #[doc(alias = "Layout")]
    #[cfg_attr(feature = "serde", serde(alias = "uni"))]
    #[cfg_attr(feature = "serde", serde(alias = "Uni"))]
    #[cfg_attr(feature = "serde", serde(alias = "Char"))]
    #[cfg_attr(feature = "serde", serde(alias = "char"))]
    Unicode(char),
    /// Use this for keys that are not listed here that you know the
    /// value of. Let us know if you think the key should be listed so
    /// we can add it
    /// On Linux, this will result in a keysym,
    /// On Windows, this will result in a `Virtual_Key` and
    /// On macOS, this will yield a `KeyCode`
    Other(u32),
}

#[cfg(all(unix, not(target_os = "macos")))]
/// Converts a Key to a Keysym
impl From<Key> for xkeysym::Keysym {
    #[allow(clippy::too_many_lines)]
    fn from(key: Key) -> Self {
        use xkeysym::Keysym;

        trace!("Key::from(key: {key:?})");

        #[allow(clippy::match_same_arms)]
        match key {
            Key::Unicode(c) => xkeysym::Keysym::from_char(c),
            Key::Add => Keysym::KP_Add,
            Key::Alt | Key::Option => Keysym::Alt_L,
            Key::Backspace => Keysym::BackSpace,
            Key::Begin => Keysym::Begin,
            Key::Break => Keysym::Break,
            Key::Cancel => Keysym::Cancel,
            Key::CapsLock => Keysym::Caps_Lock,
            Key::Clear => Keysym::Clear,
            Key::Control | Key::LControl => Keysym::Control_L,
            Key::Decimal => Keysym::KP_Decimal,
            Key::Delete => Keysym::Delete,
            Key::Divide => Keysym::KP_Divide,
            Key::DownArrow => Keysym::Down,
            Key::End => Keysym::End,
            Key::Escape => Keysym::Escape,
            Key::Execute => Keysym::Execute,
            Key::F1 => Keysym::F1,
            Key::F2 => Keysym::F2,
            Key::F3 => Keysym::F3,
            Key::F4 => Keysym::F4,
            Key::F5 => Keysym::F5,
            Key::F6 => Keysym::F6,
            Key::F7 => Keysym::F7,
            Key::F8 => Keysym::F8,
            Key::F9 => Keysym::F9,
            Key::F10 => Keysym::F10,
            Key::F11 => Keysym::F11,
            Key::F12 => Keysym::F12,
            Key::F13 => Keysym::F13,
            Key::F14 => Keysym::F14,
            Key::F15 => Keysym::F15,
            Key::F16 => Keysym::F16,
            Key::F17 => Keysym::F17,
            Key::F18 => Keysym::F18,
            Key::F19 => Keysym::F19,
            Key::F20 => Keysym::F20,
            Key::F21 => Keysym::F21,
            Key::F22 => Keysym::F22,
            Key::F23 => Keysym::F23,
            Key::F24 => Keysym::F24,
            Key::F25 => Keysym::F25,
            Key::F26 => Keysym::F26,
            Key::F27 => Keysym::F27,
            Key::F28 => Keysym::F28,
            Key::F29 => Keysym::F29,
            Key::F30 => Keysym::F30,
            Key::F31 => Keysym::F31,
            Key::F32 => Keysym::F32,
            Key::F33 => Keysym::F33,
            Key::F34 => Keysym::F34,
            Key::F35 => Keysym::F35,
            Key::Find => Keysym::Find,
            Key::Hangul => Keysym::Hangul,
            Key::Hanja => Keysym::Hangul_Hanja,
            Key::Help => Keysym::Help,
            Key::Home => Keysym::Home,
            Key::Insert => Keysym::Insert,
            Key::Kanji => Keysym::Kanji,
            Key::LeftArrow => Keysym::Left,
            Key::Linefeed => Keysym::Linefeed,
            Key::LMenu => Keysym::Menu,
            Key::ModeChange => Keysym::Mode_switch,
            Key::Multiply => Keysym::KP_Multiply,
            Key::MediaNextTrack => Keysym::XF86_AudioNext,
            Key::MediaPlayPause => Keysym::XF86_AudioPlay,
            Key::MediaPrevTrack => Keysym::XF86_AudioPrev,
            Key::MediaStop => Keysym::XF86_AudioStop,
            Key::Numlock => Keysym::Num_Lock,
            Key::Numpad0 => Keysym::KP_0,
            Key::Numpad1 => Keysym::KP_1,
            Key::Numpad2 => Keysym::KP_2,
            Key::Numpad3 => Keysym::KP_3,
            Key::Numpad4 => Keysym::KP_4,
            Key::Numpad5 => Keysym::KP_5,
            Key::Numpad6 => Keysym::KP_6,
            Key::Numpad7 => Keysym::KP_7,
            Key::Numpad8 => Keysym::KP_8,
            Key::Numpad9 => Keysym::KP_9,
            Key::PageDown => Keysym::Page_Down,
            Key::PageUp => Keysym::Page_Up,
            Key::Pause => Keysym::Pause,
            Key::Print => Keysym::Print,
            Key::PrintScr => Keysym::Print,
            Key::RControl => Keysym::Control_R,
            Key::Redo => Keysym::Redo,
            Key::Return => Keysym::Return,
            Key::RightArrow => Keysym::Right,
            Key::RShift => Keysym::Shift_R,
            Key::ScrollLock => Keysym::Scroll_Lock,
            Key::Select => Keysym::Select,
            Key::ScriptSwitch => Keysym::script_switch,
            Key::Shift | Key::LShift => Keysym::Shift_L,
            Key::ShiftLock => Keysym::Shift_Lock,
            Key::Space => Keysym::space,
            Key::Subtract => Keysym::KP_Subtract,
            Key::SysReq => Keysym::Sys_Req,
            Key::Tab => Keysym::Tab,
            Key::Undo => Keysym::Undo,
            Key::UpArrow => Keysym::Up,
            Key::VolumeDown => Keysym::XF86_AudioLowerVolume,
            Key::VolumeUp => Keysym::XF86_AudioRaiseVolume,
            Key::VolumeMute => Keysym::XF86_AudioMute,
            Key::MicMute => Keysym::XF86_AudioMicMute,
            Key::Command | Key::Super | Key::Windows | Key::Meta => Keysym::Super_L,
            Key::Other(v) => Keysym::from(v),
        }
    }
}

/// Converts a Key to a Virtual Key
#[cfg(target_os = "windows")]
impl TryFrom<Key> for windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY {
    type Error = &'static str;

    #[allow(clippy::too_many_lines)]
    fn try_from(key: Key) -> Result<Self, Self::Error> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VIRTUAL_KEY, VK__none_, VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7, VK_8, VK_9,
            VK_A, VK_ABNT_C1, VK_ABNT_C2, VK_ACCEPT, VK_ADD, VK_APPS, VK_ATTN, VK_B, VK_BACK,
            VK_BROWSER_BACK, VK_BROWSER_FAVORITES, VK_BROWSER_FORWARD, VK_BROWSER_HOME,
            VK_BROWSER_REFRESH, VK_BROWSER_SEARCH, VK_BROWSER_STOP, VK_C, VK_CANCEL, VK_CAPITAL,
            VK_CLEAR, VK_CONTROL, VK_CONVERT, VK_CRSEL, VK_D, VK_DBE_ALPHANUMERIC,
            VK_DBE_CODEINPUT, VK_DBE_DBCSCHAR, VK_DBE_DETERMINESTRING,
            VK_DBE_ENTERDLGCONVERSIONMODE, VK_DBE_ENTERIMECONFIGMODE, VK_DBE_ENTERWORDREGISTERMODE,
            VK_DBE_FLUSHSTRING, VK_DBE_HIRAGANA, VK_DBE_KATAKANA, VK_DBE_NOCODEINPUT,
            VK_DBE_NOROMAN, VK_DBE_ROMAN, VK_DBE_SBCSCHAR, VK_DECIMAL, VK_DELETE, VK_DIVIDE,
            VK_DOWN, VK_E, VK_END, VK_EREOF, VK_ESCAPE, VK_EXECUTE, VK_EXSEL, VK_F, VK_F1, VK_F2,
            VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_F11, VK_F12, VK_F13,
            VK_F14, VK_F15, VK_F16, VK_F17, VK_F18, VK_F19, VK_F20, VK_F21, VK_F22, VK_F23, VK_F24,
            VK_FINAL, VK_G, VK_GAMEPAD_A, VK_GAMEPAD_B, VK_GAMEPAD_DPAD_DOWN, VK_GAMEPAD_DPAD_LEFT,
            VK_GAMEPAD_DPAD_RIGHT, VK_GAMEPAD_DPAD_UP, VK_GAMEPAD_LEFT_SHOULDER,
            VK_GAMEPAD_LEFT_THUMBSTICK_BUTTON, VK_GAMEPAD_LEFT_THUMBSTICK_DOWN,
            VK_GAMEPAD_LEFT_THUMBSTICK_LEFT, VK_GAMEPAD_LEFT_THUMBSTICK_RIGHT,
            VK_GAMEPAD_LEFT_THUMBSTICK_UP, VK_GAMEPAD_LEFT_TRIGGER, VK_GAMEPAD_MENU,
            VK_GAMEPAD_RIGHT_SHOULDER, VK_GAMEPAD_RIGHT_THUMBSTICK_BUTTON,
            VK_GAMEPAD_RIGHT_THUMBSTICK_DOWN, VK_GAMEPAD_RIGHT_THUMBSTICK_LEFT,
            VK_GAMEPAD_RIGHT_THUMBSTICK_RIGHT, VK_GAMEPAD_RIGHT_THUMBSTICK_UP,
            VK_GAMEPAD_RIGHT_TRIGGER, VK_GAMEPAD_VIEW, VK_GAMEPAD_X, VK_GAMEPAD_Y, VK_H,
            VK_HANGEUL, VK_HANGUL, VK_HANJA, VK_HELP, VK_HOME, VK_I, VK_ICO_00, VK_ICO_CLEAR,
            VK_ICO_HELP, VK_IME_OFF, VK_IME_ON, VK_INSERT, VK_J, VK_JUNJA, VK_K, VK_KANA, VK_KANJI,
            VK_L, VK_LAUNCH_APP1, VK_LAUNCH_APP2, VK_LAUNCH_MAIL, VK_LAUNCH_MEDIA_SELECT,
            VK_LBUTTON, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_M, VK_MBUTTON,
            VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_MENU,
            VK_MODECHANGE, VK_MULTIPLY, VK_N, VK_NAVIGATION_ACCEPT, VK_NAVIGATION_CANCEL,
            VK_NAVIGATION_DOWN, VK_NAVIGATION_LEFT, VK_NAVIGATION_MENU, VK_NAVIGATION_RIGHT,
            VK_NAVIGATION_UP, VK_NAVIGATION_VIEW, VK_NEXT, VK_NONAME, VK_NONCONVERT, VK_NUMLOCK,
            VK_NUMPAD0, VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD4, VK_NUMPAD5, VK_NUMPAD6,
            VK_NUMPAD7, VK_NUMPAD8, VK_NUMPAD9, VK_O, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4,
            VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_8, VK_OEM_102, VK_OEM_ATTN, VK_OEM_AUTO,
            VK_OEM_AX, VK_OEM_BACKTAB, VK_OEM_CLEAR, VK_OEM_COMMA, VK_OEM_COPY, VK_OEM_CUSEL,
            VK_OEM_ENLW, VK_OEM_FINISH, VK_OEM_FJ_JISHO, VK_OEM_FJ_LOYA, VK_OEM_FJ_MASSHOU,
            VK_OEM_FJ_ROYA, VK_OEM_FJ_TOUROKU, VK_OEM_JUMP, VK_OEM_MINUS, VK_OEM_NEC_EQUAL,
            VK_OEM_PA1, VK_OEM_PA2, VK_OEM_PA3, VK_OEM_PERIOD, VK_OEM_PLUS, VK_OEM_RESET,
            VK_OEM_WSCTRL, VK_P, VK_PA1, VK_PACKET, VK_PAUSE, VK_PLAY, VK_PRINT, VK_PRIOR,
            VK_PROCESSKEY, VK_Q, VK_R, VK_RBUTTON, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU,
            VK_RSHIFT, VK_RWIN, VK_S, VK_SCROLL, VK_SELECT, VK_SEPARATOR, VK_SHIFT, VK_SLEEP,
            VK_SNAPSHOT, VK_SPACE, VK_SUBTRACT, VK_T, VK_TAB, VK_U, VK_UP, VK_V, VK_VOLUME_DOWN,
            VK_VOLUME_MUTE, VK_VOLUME_UP, VK_W, VK_X, VK_XBUTTON1, VK_XBUTTON2, VK_Y, VK_Z,
            VK_ZOOM,
        };

        trace!("Key::try_from(key: {key:?})");
        let vk = match key {
            Key::Num0 => VK_0,
            Key::Num1 => VK_1,
            Key::Num2 => VK_2,
            Key::Num3 => VK_3,
            Key::Num4 => VK_4,
            Key::Num5 => VK_5,
            Key::Num6 => VK_6,
            Key::Num7 => VK_7,
            Key::Num8 => VK_8,
            Key::Num9 => VK_9,
            Key::A => VK_A,
            Key::B => VK_B,
            Key::C => VK_C,
            Key::D => VK_D,
            Key::E => VK_E,
            Key::F => VK_F,
            Key::G => VK_G,
            Key::H => VK_H,
            Key::I => VK_I,
            Key::J => VK_J,
            Key::K => VK_K,
            Key::L => VK_L,
            Key::M => VK_M,
            Key::N => VK_N,
            Key::O => VK_O,
            Key::P => VK_P,
            Key::Q => VK_Q,
            Key::R => VK_R,
            Key::S => VK_S,
            Key::T => VK_T,
            Key::U => VK_U,
            Key::V => VK_V,
            Key::W => VK_W,
            Key::X => VK_X,
            Key::Y => VK_Y,
            Key::Z => VK_Z,
            Key::AbntC1 => VK_ABNT_C1,
            Key::AbntC2 => VK_ABNT_C2,
            Key::Accept => VK_ACCEPT,
            Key::Add => VK_ADD,
            Key::Alt | Key::Option => VK_MENU,
            Key::Apps => VK_APPS,
            Key::Attn => VK_ATTN,
            Key::Backspace => VK_BACK,
            Key::BrowserBack => VK_BROWSER_BACK,
            Key::BrowserFavorites => VK_BROWSER_FAVORITES,
            Key::BrowserForward => VK_BROWSER_FORWARD,
            Key::BrowserHome => VK_BROWSER_HOME,
            Key::BrowserRefresh => VK_BROWSER_REFRESH,
            Key::BrowserSearch => VK_BROWSER_SEARCH,
            Key::BrowserStop => VK_BROWSER_STOP,
            Key::Cancel => VK_CANCEL,
            Key::CapsLock => VK_CAPITAL,
            Key::Clear => VK_CLEAR,
            Key::Control => VK_CONTROL,
            Key::Convert => VK_CONVERT,
            Key::Crsel => VK_CRSEL,
            Key::DBEAlphanumeric => VK_DBE_ALPHANUMERIC,
            Key::DBECodeinput => VK_DBE_CODEINPUT,
            Key::DBEDetermineString => VK_DBE_DETERMINESTRING,
            Key::DBEEnterDLGConversionMode => VK_DBE_ENTERDLGCONVERSIONMODE,
            Key::DBEEnterIMEConfigMode => VK_DBE_ENTERIMECONFIGMODE,
            Key::DBEEnterWordRegisterMode => VK_DBE_ENTERWORDREGISTERMODE,
            Key::DBEFlushString => VK_DBE_FLUSHSTRING,
            Key::DBEHiragana => VK_DBE_HIRAGANA,
            Key::DBEKatakana => VK_DBE_KATAKANA,
            Key::DBENoCodepoint => VK_DBE_NOCODEINPUT,
            Key::DBENoRoman => VK_DBE_NOROMAN,
            Key::DBERoman => VK_DBE_ROMAN,
            Key::DBESBCSChar => VK_DBE_SBCSCHAR,
            Key::DBESChar => VK_DBE_DBCSCHAR,
            Key::Decimal => VK_DECIMAL,
            Key::Delete => VK_DELETE,
            Key::Divide => VK_DIVIDE,
            Key::DownArrow => VK_DOWN,
            Key::End => VK_END,
            Key::Ereof => VK_EREOF,
            Key::Escape => VK_ESCAPE,
            Key::Execute => VK_EXECUTE,
            Key::Exsel => VK_EXSEL,
            Key::F1 => VK_F1,
            Key::F2 => VK_F2,
            Key::F3 => VK_F3,
            Key::F4 => VK_F4,
            Key::F5 => VK_F5,
            Key::F6 => VK_F6,
            Key::F7 => VK_F7,
            Key::F8 => VK_F8,
            Key::F9 => VK_F9,
            Key::F10 => VK_F10,
            Key::F11 => VK_F11,
            Key::F12 => VK_F12,
            Key::F13 => VK_F13,
            Key::F14 => VK_F14,
            Key::F15 => VK_F15,
            Key::F16 => VK_F16,
            Key::F17 => VK_F17,
            Key::F18 => VK_F18,
            Key::F19 => VK_F19,
            Key::F20 => VK_F20,
            Key::F21 => VK_F21,
            Key::F22 => VK_F22,
            Key::F23 => VK_F23,
            Key::F24 => VK_F24,
            Key::Final => VK_FINAL,
            Key::GamepadA => VK_GAMEPAD_A,
            Key::GamepadB => VK_GAMEPAD_B,
            Key::GamepadDPadDown => VK_GAMEPAD_DPAD_DOWN,
            Key::GamepadDPadLeft => VK_GAMEPAD_DPAD_LEFT,
            Key::GamepadDPadRight => VK_GAMEPAD_DPAD_RIGHT,
            Key::GamepadDPadUp => VK_GAMEPAD_DPAD_UP,
            Key::GamepadLeftShoulder => VK_GAMEPAD_LEFT_SHOULDER,
            Key::GamepadLeftThumbstickButton => VK_GAMEPAD_LEFT_THUMBSTICK_BUTTON,
            Key::GamepadLeftThumbstickDown => VK_GAMEPAD_LEFT_THUMBSTICK_DOWN,
            Key::GamepadLeftThumbstickLeft => VK_GAMEPAD_LEFT_THUMBSTICK_LEFT,
            Key::GamepadLeftThumbstickRight => VK_GAMEPAD_LEFT_THUMBSTICK_RIGHT,
            Key::GamepadLeftThumbstickUp => VK_GAMEPAD_LEFT_THUMBSTICK_UP,
            Key::GamepadLeftTrigger => VK_GAMEPAD_LEFT_TRIGGER,
            Key::GamepadMenu => VK_GAMEPAD_MENU,
            Key::GamepadRightShoulder => VK_GAMEPAD_RIGHT_SHOULDER,
            Key::GamepadRightThumbstickButton => VK_GAMEPAD_RIGHT_THUMBSTICK_BUTTON,
            Key::GamepadRightThumbstickDown => VK_GAMEPAD_RIGHT_THUMBSTICK_DOWN,
            Key::GamepadRightThumbstickLeft => VK_GAMEPAD_RIGHT_THUMBSTICK_LEFT,
            Key::GamepadRightThumbstickRight => VK_GAMEPAD_RIGHT_THUMBSTICK_RIGHT,
            Key::GamepadRightThumbstickUp => VK_GAMEPAD_RIGHT_THUMBSTICK_UP,
            Key::GamepadRightTrigger => VK_GAMEPAD_RIGHT_TRIGGER,
            Key::GamepadView => VK_GAMEPAD_VIEW,
            Key::GamepadX => VK_GAMEPAD_X,
            Key::GamepadY => VK_GAMEPAD_Y,
            Key::Hangeul => VK_HANGEUL,
            Key::Hangul => VK_HANGUL,
            Key::Hanja => VK_HANJA,
            Key::Help => VK_HELP,
            Key::Home => VK_HOME,
            Key::Ico00 => VK_ICO_00,
            Key::IcoClear => VK_ICO_CLEAR,
            Key::IcoHelp => VK_ICO_HELP,
            Key::IMEOff => VK_IME_OFF,
            Key::IMEOn => VK_IME_ON,
            Key::Insert => VK_INSERT,
            Key::Junja => VK_JUNJA,
            Key::Kana => VK_KANA,
            Key::Kanji => VK_KANJI,
            Key::LaunchApp1 => VK_LAUNCH_APP1,
            Key::LaunchApp2 => VK_LAUNCH_APP2,
            Key::LaunchMail => VK_LAUNCH_MAIL,
            Key::LaunchMediaSelect => VK_LAUNCH_MEDIA_SELECT,
            Key::LButton => VK_LBUTTON,
            Key::LControl => VK_LCONTROL,
            Key::LeftArrow => VK_LEFT,
            Key::LMenu => VK_LMENU,
            Key::LShift => VK_LSHIFT,
            Key::MButton => VK_MBUTTON,
            Key::MediaNextTrack => VK_MEDIA_NEXT_TRACK,
            Key::MediaPlayPause => VK_MEDIA_PLAY_PAUSE,
            Key::MediaPrevTrack => VK_MEDIA_PREV_TRACK,
            Key::MediaStop => VK_MEDIA_STOP,
            Key::ModeChange => VK_MODECHANGE,
            Key::Multiply => VK_MULTIPLY,
            Key::NavigationAccept => VK_NAVIGATION_ACCEPT,
            Key::NavigationCancel => VK_NAVIGATION_CANCEL,
            Key::NavigationDown => VK_NAVIGATION_DOWN,
            Key::NavigationLeft => VK_NAVIGATION_LEFT,
            Key::NavigationMenu => VK_NAVIGATION_MENU,
            Key::NavigationRight => VK_NAVIGATION_RIGHT,
            Key::NavigationUp => VK_NAVIGATION_UP,
            Key::NavigationView => VK_NAVIGATION_VIEW,
            Key::NoName => VK_NONAME,
            Key::NonConvert => VK_NONCONVERT,
            Key::None => VK__none_,
            Key::Numlock => VK_NUMLOCK,
            Key::Numpad0 => VK_NUMPAD0,
            Key::Numpad1 => VK_NUMPAD1,
            Key::Numpad2 => VK_NUMPAD2,
            Key::Numpad3 => VK_NUMPAD3,
            Key::Numpad4 => VK_NUMPAD4,
            Key::Numpad5 => VK_NUMPAD5,
            Key::Numpad6 => VK_NUMPAD6,
            Key::Numpad7 => VK_NUMPAD7,
            Key::Numpad8 => VK_NUMPAD8,
            Key::Numpad9 => VK_NUMPAD9,
            Key::OEM1 => VK_OEM_1,
            Key::OEM102 => VK_OEM_102,
            Key::OEM2 => VK_OEM_2,
            Key::OEM3 => VK_OEM_3,
            Key::OEM4 => VK_OEM_4,
            Key::OEM5 => VK_OEM_5,
            Key::OEM6 => VK_OEM_6,
            Key::OEM7 => VK_OEM_7,
            Key::OEM8 => VK_OEM_8,
            Key::OEMAttn => VK_OEM_ATTN,
            Key::OEMAuto => VK_OEM_AUTO,
            Key::OEMAx => VK_OEM_AX,
            Key::OEMBacktab => VK_OEM_BACKTAB,
            Key::OEMClear => VK_OEM_CLEAR,
            Key::OEMComma => VK_OEM_COMMA,
            Key::OEMCopy => VK_OEM_COPY,
            Key::OEMCusel => VK_OEM_CUSEL,
            Key::OEMEnlw => VK_OEM_ENLW,
            Key::OEMFinish => VK_OEM_FINISH,
            Key::OEMFJJisho => VK_OEM_FJ_JISHO,
            Key::OEMFJLoya => VK_OEM_FJ_LOYA,
            Key::OEMFJMasshou => VK_OEM_FJ_MASSHOU,
            Key::OEMFJRoya => VK_OEM_FJ_ROYA,
            Key::OEMFJTouroku => VK_OEM_FJ_TOUROKU,
            Key::OEMJump => VK_OEM_JUMP,
            Key::OEMMinus => VK_OEM_MINUS,
            Key::OEMNECEqual => VK_OEM_NEC_EQUAL,
            Key::OEMPA1 => VK_OEM_PA1,
            Key::OEMPA2 => VK_OEM_PA2,
            Key::OEMPA3 => VK_OEM_PA3,
            Key::OEMPeriod => VK_OEM_PERIOD,
            Key::OEMPlus => VK_OEM_PLUS,
            Key::OEMReset => VK_OEM_RESET,
            Key::OEMWsctrl => VK_OEM_WSCTRL,
            Key::PA1 => VK_PA1,
            Key::Packet => VK_PACKET,
            Key::PageDown => VK_NEXT,
            Key::PageUp => VK_PRIOR,
            Key::Pause => VK_PAUSE,
            Key::Play => VK_PLAY,
            Key::Print => VK_PRINT,
            Key::PrintScr | Key::Snapshot => VK_SNAPSHOT,
            Key::Processkey => VK_PROCESSKEY,
            Key::RButton => VK_RBUTTON,
            Key::RControl => VK_RCONTROL,
            Key::Return => VK_RETURN,
            Key::RightArrow => VK_RIGHT,
            Key::RMenu => VK_RMENU,
            Key::RShift => VK_RSHIFT,
            Key::RWin => VK_RWIN,
            Key::Scroll => VK_SCROLL,
            Key::Select => VK_SELECT,
            Key::Separator => VK_SEPARATOR,
            Key::Shift => VK_SHIFT,
            Key::Sleep => VK_SLEEP,
            Key::Space => VK_SPACE,
            Key::Subtract => VK_SUBTRACT,
            Key::Tab => VK_TAB,
            Key::UpArrow => VK_UP,
            Key::VolumeDown => VK_VOLUME_DOWN,
            Key::VolumeMute => VK_VOLUME_MUTE,
            Key::VolumeUp => VK_VOLUME_UP,
            Key::XButton1 => VK_XBUTTON1,
            Key::XButton2 => VK_XBUTTON2,
            Key::Zoom => VK_ZOOM,
            Key::Unicode(c) => 'unicode_handling: {
                // Handle special characters separately
                match c {
                    '\n' => break 'unicode_handling VK_RETURN,

                    '\r' => { // TODO: What is the correct key to type here?
                        // break 'unicode_handling VK_,
                    }
                    '\t' => break 'unicode_handling VK_TAB,
                    '\0' => {
                        return Err("Invalid mapping");
                    }
                    _ => (),
                }

                let layout = crate::Enigo::get_keyboard_layout();

                let mut buffer = [0; 2];
                let utf16_surrogates = c.encode_utf16(&mut buffer);
                if utf16_surrogates.len() != 1 {
                    return Err("Character can't be mapped to only one virtual key");
                }
                // Translate a character to the corresponding virtual-key code and shift state.
                // If the function succeeds, the low-order byte of the return value contains the
                // virtual-key code and the high-order byte contains the shift state, which can
                // be a combination of the following flag bits. If the function finds no key
                // that translates to the passed character code, both the low-order and
                // high-order bytes contain -1
                let vk = unsafe {
                    windows::Win32::UI::Input::KeyboardAndMouse::VkKeyScanExW(
                        utf16_surrogates[0],
                        layout,
                    )
                };
                // TODO: Check if the condition should be <=
                if vk < 0 {
                    return Err("Character can't be mapped to virtual key");
                }
                VIRTUAL_KEY(vk as u16)
            }
            Key::Other(v) => {
                let Ok(v) = u16::try_from(v) else {
                    return Err("virtual keycodes on Windows have to fit into u16");
                };
                VIRTUAL_KEY(v)
            }
            Key::Super | Key::Command | Key::Windows | Key::Meta | Key::LWin => VK_LWIN,
        };

        trace!("virtual key: {vk:?})");
        Ok(vk)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
#[cfg(any(
    feature = "wayland",
    feature = "x11rb",
    feature = "libei_tokio",
    feature = "libei_smol"
))]
pub(crate) type ModifierBitflag = u32;
