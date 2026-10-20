#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
#![allow(unused)]

/* taken from https://github.com/wusyong/carbon-bindgen/blob/467fca5d71047050b632fbdfb41b1f14575a8499/bindings.rs */

use std::ffi::{c_long, c_void};

use objc2::encode::{Encode, Encoding, RefEncode};

pub type UInt32 = ::std::os::raw::c_uint;
pub type SInt32 = ::std::os::raw::c_int;
pub type OSStatus = SInt32;
pub type FourCharCode = UInt32;
pub type OSType = FourCharCode;
pub type ByteCount = ::std::os::raw::c_ulong;
pub type ItemCount = ::std::os::raw::c_ulong;
pub type OptionBits = UInt32;
pub type EventKind = UInt32;
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct OpaqueEventRef {
    _unused: [u8; 0],
}
pub type EventRef = *mut OpaqueEventRef;
pub type EventParamName = OSType;
pub type EventParamType = OSType;
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct OpaqueEventHandlerRef {
    _unused: [u8; 0],
}
pub type EventHandlerRef = *mut OpaqueEventHandlerRef;
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct OpaqueEventHandlerCallRef {
    _unused: [u8; 0],
}
pub type EventHandlerCallRef = *mut OpaqueEventHandlerCallRef;
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct OpaqueEventTargetRef {
    _unused: [u8; 0],
}
pub type EventTargetRef = *mut OpaqueEventTargetRef;
pub type EventHandlerProcPtr = ::std::option::Option<
    unsafe extern "C" fn(
        inHandlerCallRef: EventHandlerCallRef,
        inEvent: EventRef,
        inUserData: *mut ::std::os::raw::c_void,
    ) -> OSStatus,
>;
pub type EventHandlerUPP = EventHandlerProcPtr;
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct OpaqueEventHotKeyRef {
    _unused: [u8; 0],
}
pub type EventHotKeyRef = *mut OpaqueEventHotKeyRef;

pub type _bindgen_ty_1637 = ::std::os::raw::c_uint;
pub const kEventParamDirectObject: _bindgen_ty_1637 = 757935405;
pub const kEventParamDragRef: _bindgen_ty_1637 = 1685217639;
pub type _bindgen_ty_1921 = ::std::os::raw::c_uint;
pub const typeEventHotKeyID: _bindgen_ty_1921 = 1751869796;
pub type _bindgen_ty_1939 = ::std::os::raw::c_uint;
pub const kEventClassKeyboard: _bindgen_ty_1939 = 1801812322;
pub type _bindgen_ty_1980 = ::std::os::raw::c_uint;
pub const kEventHotKeyPressed: _bindgen_ty_1980 = 5;
pub type _bindgen_ty_1981 = ::std::os::raw::c_uint;
pub const kEventHotKeyReleased: _bindgen_ty_1981 = 6;
pub type _bindgen_ty_1 = ::std::os::raw::c_uint;
pub const noErr: _bindgen_ty_1 = 0;

#[repr(C, packed(2))]
#[derive(Debug, Copy, Clone)]
pub struct EventHotKeyID {
    pub signature: OSType,
    pub id: UInt32,
}

#[repr(C, packed(2))]
#[derive(Debug, Copy, Clone)]
pub struct EventTypeSpec {
    pub eventClass: OSType,
    pub eventKind: EventKind,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    pub fn GetEventParameter(
        inEvent: EventRef,
        inName: EventParamName,
        inDesiredType: EventParamType,
        outActualType: *mut EventParamType,
        inBufferSize: ByteCount,
        outActualSize: *mut ByteCount,
        outData: *mut ::std::os::raw::c_void,
    ) -> OSStatus;
    pub fn GetEventKind(inEvent: EventRef) -> EventKind;
    pub fn GetApplicationEventTarget() -> EventTargetRef;
    pub fn InstallEventHandler(
        inTarget: EventTargetRef,
        inHandler: EventHandlerUPP,
        inNumTypes: ItemCount,
        inList: *const EventTypeSpec,
        inUserData: *mut ::std::os::raw::c_void,
        outRef: *mut EventHandlerRef,
    ) -> OSStatus;
    pub fn RemoveEventHandler(inHandlerRef: EventHandlerRef) -> OSStatus;
    pub fn RegisterEventHotKey(
        inHotKeyCode: UInt32,
        inHotKeyModifiers: UInt32,
        inHotKeyID: EventHotKeyID,
        inTarget: EventTargetRef,
        inOptions: OptionBits,
        outRef: *mut EventHotKeyRef,
    ) -> OSStatus;
    pub fn UnregisterEventHotKey(inHotKey: EventHotKeyRef) -> OSStatus;
}

/* Core Graphics */

/// Possible tapping points for events.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum CGEventTapLocation {
    Hid,
    Session,
    AnnotatedSession,
}

// The next three enums are taken from:
// [Ref](https://github.com/phracker/MacOSX-SDKs/blob/ef9fe35d5691b6dd383c8c46d867a499817a01b6/MacOSX10.15.sdk/System/Library/Frameworks/CoreGraphics.framework/Versions/A/Headers/CGEventTypes.h)
/* Constants that specify where a new event tap is inserted into the list of active event taps. */
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum CGEventTapPlacement {
    HeadInsertEventTap = 0,
    TailAppendEventTap,
}

/* Constants that specify whether a new event tap is an active filter or a passive listener. */
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum CGEventTapOptions {
    Default = 0x00000000,
    ListenOnly = 0x00000001,
}

/// Constants that specify the different types of input events.
///
/// [Ref](http://opensource.apple.com/source/IOHIDFamily/IOHIDFamily-700/IOHIDSystem/IOKit/hidsystem/IOLLEvent.h)
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CGEventType {
    Null = 0,

    // Mouse events.
    LeftMouseDown = 1,
    LeftMouseUp = 2,
    RightMouseDown = 3,
    RightMouseUp = 4,
    MouseMoved = 5,
    LeftMouseDragged = 6,
    RightMouseDragged = 7,

    // Keyboard events.
    KeyDown = 10,
    KeyUp = 11,
    FlagsChanged = 12,

    // Composite events.
    AppKitDefined = 13,
    SystemDefined = 14,
    ApplicationDefined = 15,

    // Specialized control devices.
    ScrollWheel = 22,
    TabletPointer = 23,
    TabletProximity = 24,
    OtherMouseDown = 25,
    OtherMouseUp = 26,
    OtherMouseDragged = 27,

    // Out of band event types. These are delivered to the event tap callback
    // to notify it of unusual conditions that disable the event tap.
    TapDisabledByTimeout = 0xFFFFFFFE,
    TapDisabledByUserInput = 0xFFFFFFFF,
}

pub type CGEventMask = u64;
#[macro_export]
macro_rules! CGEventMaskBit {
    ($eventType:expr) => {
        1 << $eventType as CGEventMask
    };
}

pub enum CGEvent {}
pub type CGEventRef = *const CGEvent;

unsafe impl RefEncode for CGEvent {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
}

pub type CGEventTapProxy = *const c_void;
type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    etype: CGEventType,
    event: CGEventRef,
    user_info: *const c_void,
) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGEventTapCreate(
        tap: CGEventTapLocation,
        place: CGEventTapPlacement,
        options: CGEventTapOptions,
        events_of_interest: CGEventMask,
        callback: CGEventTapCallBack,
        user_info: *const c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

/* Core Foundation */

pub enum CFAllocator {}
pub type CFAllocatorRef = *mut CFAllocator;
pub enum CFRunLoop {}
pub type CFRunLoopRef = *mut CFRunLoop;
pub type CFRunLoopMode = CFStringRef;
pub enum CFRunLoopObserver {}
pub type CFRunLoopObserverRef = *mut CFRunLoopObserver;
pub enum CFRunLoopTimer {}
pub type CFRunLoopTimerRef = *mut CFRunLoopTimer;
pub enum CFRunLoopSource {}
pub type CFRunLoopSourceRef = *mut CFRunLoopSource;
pub enum CFString {}
pub type CFStringRef = *const CFString;

pub enum CFMachPort {}
pub type CFMachPortRef = *mut CFMachPort;

pub type CFIndex = c_long;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFRunLoopCommonModes: CFRunLoopMode;
    pub static kCFAllocatorDefault: CFAllocatorRef;

    pub fn CFRunLoopGetMain() -> CFRunLoopRef;

    pub fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
    pub fn CFMachPortInvalidate(port: CFMachPortRef);
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFRunLoopMode);
    pub fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFRunLoopMode);
    pub fn CFRelease(cftype: *const c_void);
}
