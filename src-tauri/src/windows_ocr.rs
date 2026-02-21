use std::ffi::c_void;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::Graphics::Gdi::{GetWindowDC, ReleaseDC};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

const MIN_WINDOW_DIMENSION: i32 = 32;

pub fn capture_frontmost_window_ocr_text() -> Result<String, String> {
    let (pixels, width, height) = capture_frontmost_window_pixels()?;
    recognize_text_from_bgra(&pixels, width, height)
}

fn capture_frontmost_window_pixels() -> Result<(Vec<u8>, i32, i32), String> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return Err("Unable to find foreground window.".to_string());
    }

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return Err("Failed to read foreground window bounds.".to_string());
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width < MIN_WINDOW_DIMENSION || height < MIN_WINDOW_DIMENSION {
        return Err("Foreground window is too small for OCR.".to_string());
    }

    unsafe {
        let window_dc = GetWindowDC(Some(hwnd));
        if window_dc.is_invalid() {
            return Err("Failed to acquire foreground window DC.".to_string());
        }

        let memory_dc = CreateCompatibleDC(Some(window_dc));
        if memory_dc.is_invalid() {
            let _ = ReleaseDC(Some(hwnd), window_dc);
            return Err("Failed to create compatible memory DC.".to_string());
        }

        let bitmap = CreateCompatibleBitmap(window_dc, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(Some(hwnd), window_dc);
            return Err("Failed to create compatible bitmap.".to_string());
        }

        let old_object = SelectObject(memory_dc, bitmap.into());
        if old_object.is_invalid() {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(Some(hwnd), window_dc);
            return Err("Failed to select bitmap into memory DC.".to_string());
        }

        // Prefer full-window capture, including non-client area.
        let mut captured = PrintWindow(hwnd, memory_dc, PRINT_WINDOW_FLAGS(0x0000_0002)).as_bool();
        if !captured {
            captured = BitBlt(
                memory_dc,
                0,
                0,
                width,
                height,
                Some(window_dc),
                0,
                0,
                SRCCOPY | CAPTUREBLT,
            )
            .is_ok();
        }

        if !captured {
            let _ = SelectObject(memory_dc, old_object);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(Some(hwnd), window_dc);
            return Err("Failed to capture foreground window pixels.".to_string());
        }

        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bytes = vec![0_u8; (width as usize) * (height as usize) * 4];
        let copied_scanlines = GetDIBits(
            memory_dc,
            bitmap,
            0,
            height as u32,
            Some(bytes.as_mut_ptr().cast::<c_void>()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        );

        let _ = SelectObject(memory_dc, old_object);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory_dc);
        let _ = ReleaseDC(Some(hwnd), window_dc);

        if copied_scanlines == 0 {
            return Err("Failed to read captured bitmap pixels.".to_string());
        }

        Ok((bytes, width, height))
    }
}

fn recognize_text_from_bgra(bytes: &[u8], width: i32, height: i32) -> Result<String, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let ocr_engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("Failed to create OCR engine: {e}"))?;

    let buffer = CryptographicBuffer::CreateFromByteArray(bytes)
        .map_err(|e| format!("Failed to create OCR input buffer: {e}"))?;

    let software_bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width,
        height,
        BitmapAlphaMode::Ignore,
    )
    .map_err(|e| format!("Failed to create SoftwareBitmap for OCR: {e}"))?;

    let ocr_result = ocr_engine
        .RecognizeAsync(&software_bitmap)
        .map_err(|e| format!("Failed to start OCR recognition: {e}"))?
        .get()
        .map_err(|e| format!("OCR recognition failed: {e}"))?;

    ocr_result
        .Text()
        .map_err(|e| format!("Failed to read OCR result text: {e}"))
        .map(|text| text.to_string())
}
