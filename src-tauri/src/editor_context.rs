//! Read-only cursor context. Never logs or persists editor contents.

fn capitalize_at_cursor(text: &str, offset: usize) -> Option<bool> {
    let utf16: Vec<_> = text.encode_utf16().collect();
    let prefix = String::from_utf16(utf16.get(..offset)?).ok()?;
    let line = prefix.rsplit(['\n', '\r']).next()?.trim_end();
    Some(line.trim().is_empty() || line.ends_with('.'))
}

#[cfg(not(target_os = "macos"))]
pub fn capitalization() -> Option<bool> {
    None
}

#[cfg(target_os = "macos")]
pub fn capitalization() -> Option<bool> {
    use std::ffi::{c_char, c_void};
    type Ref = *const c_void;
    #[repr(C)]
    struct Range {
        location: isize,
        length: isize,
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> Ref;
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
        fn AXValueGetValue(value: Ref, kind: u32, output: *mut c_void) -> bool;
        fn AXValueGetType(value: Ref) -> u32;
        fn AXValueGetTypeID() -> usize;
        fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: Ref);
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringCreateWithCString(allocator: Ref, bytes: *const c_char, encoding: u32) -> Ref;
        fn CFStringGetLength(value: Ref) -> isize;
        fn CFStringGetCharacters(value: Ref, range: Range, buffer: *mut u16);
    }
    struct Owned(Ref);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }
    unsafe fn attribute(element: Ref, name: &'static [u8]) -> Option<Owned> {
        let key = CFStringCreateWithCString(std::ptr::null(), name.as_ptr().cast(), 0x08000100);
        if key.is_null() {
            return None;
        }
        let key = Owned(key);
        let mut value = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(element, key.0, &mut value);
        if status != 0 || value.is_null() {
            return None;
        }
        Some(Owned(value))
    }
    // All copied CF objects are released, including on unsupported-editor paths.
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let system = Owned(system);
        AXUIElementSetMessagingTimeout(system.0, 0.2);
        let focused = attribute(system.0, b"AXFocusedUIElement\0")?;
        AXUIElementSetMessagingTimeout(focused.0, 0.2);
        let selection = attribute(focused.0, b"AXSelectedTextRange\0")?;
        if CFGetTypeID(selection.0) != AXValueGetTypeID() || AXValueGetType(selection.0) != 3 {
            return None;
        }
        let mut range = Range {
            location: 0,
            length: 0,
        };
        if !AXValueGetValue(selection.0, 3, (&mut range as *mut Range).cast()) || range.location < 0
        {
            return None;
        }
        if range.location == 0 {
            return Some(true);
        }
        let value = attribute(focused.0, b"AXValue\0")?;
        if CFGetTypeID(value.0) != CFStringGetTypeID() {
            return None;
        }
        let length = CFStringGetLength(value.0);
        if length < range.location || range.location > 1_000_000 {
            return None;
        }
        let mut units = vec![0; range.location as usize];
        CFStringGetCharacters(
            value.0,
            Range {
                location: 0,
                length: range.location,
            },
            units.as_mut_ptr(),
        );
        let prefix = String::from_utf16(&units).ok()?;
        capitalize_at_cursor(&prefix, units.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_lines_and_sentences_use_real_cursor_position() {
        assert_eq!(capitalize_at_cursor("", 0), Some(true));
        assert_eq!(capitalize_at_cursor("hello", 5), Some(false));
        assert_eq!(capitalize_at_cursor("hello.  ", 8), Some(true));
        assert_eq!(capitalize_at_cursor("hello\n  ", 8), Some(true));
        assert_eq!(capitalize_at_cursor("hello\nworld", 6), Some(true));
        assert_eq!(capitalize_at_cursor("hello\nworld", 9), Some(false));
        assert_eq!(capitalize_at_cursor("😀hello", 7), Some(false));
        assert_eq!(capitalize_at_cursor("hello", 20), None);
    }
}
