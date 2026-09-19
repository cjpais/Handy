use clipboard_win::{formats::CF_DIB, raw, register_format, Clipboard};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use std::{thread, time::Duration};

const EXCLUDE_FROM_MONITORING: &str = "ExcludeClipboardContentFromMonitorProcessing";
const CAN_INCLUDE_IN_HISTORY: &str = "CanIncludeInClipboardHistory";
const CAN_UPLOAD_TO_CLOUD: &str = "CanUploadToCloudClipboard";
const EXCLUSION_VALUE: [u8; 4] = 0u32.to_ne_bytes();
const OPEN_MAX_ATTEMPTS: usize = 20;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(25);

struct ExclusionFormats {
    monitoring: u32,
    history: u32,
    cloud: u32,
}

impl ExclusionFormats {
    fn register() -> Result<Self, String> {
        Ok(Self {
            monitoring: register_format(EXCLUDE_FROM_MONITORING)
                .ok_or_else(|| format!("Failed to register {EXCLUDE_FROM_MONITORING}"))?
                .get(),
            history: register_format(CAN_INCLUDE_IN_HISTORY)
                .ok_or_else(|| format!("Failed to register {CAN_INCLUDE_IN_HISTORY}"))?
                .get(),
            cloud: register_format(CAN_UPLOAD_TO_CLOUD)
                .ok_or_else(|| format!("Failed to register {CAN_UPLOAD_TO_CLOUD}"))?
                .get(),
        })
    }

    fn apply(&self) -> Result<(), String> {
        raw::set_without_clear(self.monitoring, &EXCLUSION_VALUE)
            .map_err(|e| format!("Failed to exclude clipboard data from monitoring: {e}"))?;
        raw::set_without_clear(self.history, &EXCLUSION_VALUE)
            .map_err(|e| format!("Failed to exclude clipboard data from history: {e}"))?;
        raw::set_without_clear(self.cloud, &EXCLUSION_VALUE)
            .map_err(|e| format!("Failed to exclude clipboard data from cloud sync: {e}"))?;
        Ok(())
    }
}

fn open_clipboard() -> Result<Clipboard, String> {
    let mut last_error = None;

    for attempt in 0..OPEN_MAX_ATTEMPTS {
        match Clipboard::new() {
            Ok(clipboard) => return Ok(clipboard),
            Err(error) => last_error = Some(error),
        }

        if attempt + 1 < OPEN_MAX_ATTEMPTS {
            thread::sleep(OPEN_RETRY_DELAY);
        }
    }

    Err(format!(
        "Failed to open Windows clipboard after {} attempts: {}",
        OPEN_MAX_ATTEMPTS,
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    ))
}

pub(super) fn write_excluded_text(text: &str) -> Result<(), String> {
    let exclusions = ExclusionFormats::register()?;
    let _clipboard = open_clipboard()?;

    raw::set_string(text).map_err(|e| format!("Failed to write text to clipboard: {e}"))?;
    exclusions.apply()
}

pub(super) fn clear_excluded() -> Result<(), String> {
    let exclusions = ExclusionFormats::register()?;
    let _clipboard = open_clipboard()?;

    raw::empty().map_err(|e| format!("Failed to clear clipboard: {e}"))?;
    exclusions.apply()
}

pub(super) fn write_excluded_image(rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    let png = encode_png(rgba, width, height)?;
    let dib = encode_cf_dib(rgba, width, height)?;
    let exclusions = ExclusionFormats::register()?;
    let png_format = register_format("PNG")
        .ok_or_else(|| "Failed to register PNG clipboard format".to_string())?
        .get();
    let _clipboard = open_clipboard()?;

    raw::empty().map_err(|e| format!("Failed to clear clipboard before image restore: {e}"))?;

    let image_result = (|| {
        // PNG is set first because Chromium and other modern applications prefer it.
        raw::set_without_clear(png_format, &png)
            .map_err(|e| format!("Failed to restore PNG clipboard format: {e}"))?;
        // CF_DIB keeps the restored screenshot pasteable in native Windows applications.
        raw::set_without_clear(CF_DIB, &dib)
            .map_err(|e| format!("Failed to restore CF_DIB clipboard format: {e}"))?;
        Ok(())
    })();

    // Apply exclusions even if one image format failed, so a partial restore still
    // cannot leak into clipboard history while the clipboard remains locked.
    let exclusion_result = exclusions.apply();
    image_result.and(exclusion_result)
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    validate_rgba(rgba, width, height)?;

    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|e| format!("Failed to encode clipboard image as PNG: {e}"))?;
    Ok(png)
}

fn encode_cf_dib(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    validate_rgba(rgba, width, height)?;

    const BITMAP_INFO_HEADER_SIZE: u32 = 40;
    const PLANES: u16 = 1;
    const BITS_PER_PIXEL: u16 = 32;
    const BI_RGB: u32 = 0;

    let pixel_bytes: u32 = rgba
        .len()
        .try_into()
        .map_err(|_| "Clipboard image is too large for CF_DIB".to_string())?;
    let capacity = usize::try_from(BITMAP_INFO_HEADER_SIZE)
        .ok()
        .and_then(|header| header.checked_add(rgba.len()))
        .ok_or_else(|| "Clipboard image size overflow".to_string())?;
    let width =
        i32::try_from(width).map_err(|_| "Clipboard image width is too large".to_string())?;
    let height_i32 =
        i32::try_from(height).map_err(|_| "Clipboard image height is too large".to_string())?;

    let mut dib = Vec::with_capacity(capacity);
    dib.extend_from_slice(&BITMAP_INFO_HEADER_SIZE.to_le_bytes());
    dib.extend_from_slice(&width.to_le_bytes());
    dib.extend_from_slice(&height_i32.to_le_bytes());
    dib.extend_from_slice(&PLANES.to_le_bytes());
    dib.extend_from_slice(&BITS_PER_PIXEL.to_le_bytes());
    dib.extend_from_slice(&BI_RGB.to_le_bytes());
    dib.extend_from_slice(&pixel_bytes.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "Clipboard image row size overflow".to_string())?;

    // Positive DIB height means bottom-up rows. Convert RGBA to native BGRA while flipping.
    for row in (0..height as usize).rev() {
        let start = row * row_bytes;
        for pixel in rgba[start..start + row_bytes].chunks_exact(4) {
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }

    Ok(dib)
}

fn validate_rgba(rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    let expected_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Clipboard image dimensions overflow".to_string())?;

    if width == 0 || height == 0 {
        return Err("Clipboard image dimensions must be non-zero".to_string());
    }

    if rgba.len() != expected_len {
        return Err(format!(
            "Clipboard image has {} RGBA bytes, expected {expected_len}",
            rgba.len()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cf_dib_is_bgra_and_bottom_up() {
        // Top row: red, green. Bottom row: blue, white.
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 255, 255,
        ];

        let dib = encode_cf_dib(&rgba, 2, 2).unwrap();

        assert_eq!(&dib[0..4], &40u32.to_le_bytes());
        assert_eq!(&dib[4..8], &2i32.to_le_bytes());
        assert_eq!(&dib[8..12], &2i32.to_le_bytes());
        assert_eq!(
            &dib[40..],
            &[
                255, 0, 0, 64, 255, 255, 255, 255, // bottom row
                0, 0, 255, 255, 0, 255, 0, 128, // top row
            ]
        );
    }

    #[test]
    fn rejects_invalid_rgba_length() {
        let error = encode_cf_dib(&[0; 3], 1, 1).unwrap_err();
        assert!(error.contains("expected 4"));
    }

    #[test]
    fn rejects_empty_dimensions() {
        assert!(encode_cf_dib(&[], 0, 0).is_err());
    }
}
