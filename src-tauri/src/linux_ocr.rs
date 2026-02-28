use std::env;
use std::fs;
use std::os::raw::{c_int, c_long, c_uchar, c_ulong, c_void};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use x11::xlib;

use log::{info, warn};

const MIN_WINDOW_DIMENSION: i32 = 32;
const PPM_RGB_CHANNELS: usize = 3;
const PPM_MAX_COLOR_VALUE: u16 = 255;
const ACTIVE_WINDOW_PROPERTY_NAME: &str = "_NET_ACTIVE_WINDOW";
const ACTIVE_WINDOW_PROPERTY_ITEMS: c_long = 1;
const TESSERACT_EXECUTABLE: &str = "tesseract";
const TESSERACT_OUTPUT_TARGET: &str = "stdout";
const TESSERACT_PAGE_SEGMENT_MODE: &str = "6";
const XDG_SESSION_TYPE: &str = "XDG_SESSION_TYPE";
const XDG_CURRENT_DESKTOP: &str = "XDG_CURRENT_DESKTOP";
const DESKTOP_SESSION: &str = "DESKTOP_SESSION";
const SESSION_TYPE_X11: &str = "x11";
const SESSION_TYPE_WAYLAND: &str = "wayland";
const GNOME_SCREENSHOT_EXECUTABLE: &str = "gnome-screenshot";
const GNOME_SCREENSHOT_OUTPUT_FLAG: &str = "-f";
const SPECTACLE_EXECUTABLE: &str = "spectacle";
const SPECTACLE_BACKGROUND_FLAG: &str = "-b";
const SPECTACLE_NO_NOTIFY_FLAG: &str = "-n";
const SPECTACLE_OUTPUT_FLAG: &str = "-o";

static CAPTURE_BACKEND: OnceLock<CaptureBackendState> = OnceLock::new();

pub fn capture_frontmost_window_ocr_text() -> Result<String, String> {
    let backend = capture_backend();
    match backend.strategy {
        CaptureStrategy::X11 => capture_x11_ocr_text(),
        CaptureStrategy::Wayland(WaylandScreenshotTool::GnomeScreenshot) => {
            capture_wayland_ocr_text(WaylandScreenshotTool::GnomeScreenshot)
        }
        CaptureStrategy::Wayland(WaylandScreenshotTool::Spectacle) => {
            capture_wayland_ocr_text(WaylandScreenshotTool::Spectacle)
        }
        CaptureStrategy::UnsupportedWayland => Err(missing_wayland_tool_message(backend.desktop)),
    }
}

pub fn initialize_capture_backend() {
    let backend = capture_backend();
    log_capture_backend(backend);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionType {
    X11,
    Wayland,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DesktopEnvironment {
    Gnome,
    Kde,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaylandScreenshotTool {
    GnomeScreenshot,
    Spectacle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureStrategy {
    X11,
    Wayland(WaylandScreenshotTool),
    UnsupportedWayland,
}

#[derive(Clone, Copy, Debug)]
struct CaptureBackendState {
    session: SessionType,
    desktop: DesktopEnvironment,
    strategy: CaptureStrategy,
}

fn capture_backend() -> &'static CaptureBackendState {
    CAPTURE_BACKEND.get_or_init(detect_capture_backend)
}

fn detect_capture_backend() -> CaptureBackendState {
    let session = detect_session_type();
    let desktop = detect_desktop_environment();
    let has_gnome_screenshot = command_exists(GNOME_SCREENSHOT_EXECUTABLE);
    let has_spectacle = command_exists(SPECTACLE_EXECUTABLE);

    let strategy = match session {
        SessionType::X11 | SessionType::Unknown => CaptureStrategy::X11,
        SessionType::Wayland => {
            match select_wayland_tool(desktop, has_gnome_screenshot, has_spectacle) {
                Some(tool) => CaptureStrategy::Wayland(tool),
                None => CaptureStrategy::UnsupportedWayland,
            }
        }
    };

    CaptureBackendState {
        session,
        desktop,
        strategy,
    }
}

fn log_capture_backend(backend: &CaptureBackendState) {
    match backend.strategy {
        CaptureStrategy::X11 => {
            info!(
                "Linux OCR capture backend initialized: X11 (session={:?}, desktop={:?})",
                backend.session, backend.desktop
            );
        }
        CaptureStrategy::Wayland(WaylandScreenshotTool::GnomeScreenshot) => {
            info!(
                "Linux OCR capture backend initialized: Wayland + gnome-screenshot (session={:?}, desktop={:?})",
                backend.session, backend.desktop
            );
        }
        CaptureStrategy::Wayland(WaylandScreenshotTool::Spectacle) => {
            info!(
                "Linux OCR capture backend initialized: Wayland + spectacle (session={:?}, desktop={:?})",
                backend.session, backend.desktop
            );
        }
        CaptureStrategy::UnsupportedWayland => {
            warn!("{}", missing_wayland_tool_message(backend.desktop));
        }
    }
}

fn missing_wayland_tool_message(desktop: DesktopEnvironment) -> String {
    match desktop {
        DesktopEnvironment::Gnome => "Wayland OCR capture is unavailable. Install `gnome-screenshot` (preferred on GNOME) or `spectacle`.".to_string(),
        DesktopEnvironment::Kde => "Wayland OCR capture is unavailable. Install `spectacle` (preferred on KDE) or `gnome-screenshot`.".to_string(),
        DesktopEnvironment::Other => "Wayland OCR capture is unavailable. Install `gnome-screenshot` or `spectacle`.".to_string(),
    }
}

fn detect_session_type() -> SessionType {
    let session = env::var(XDG_SESSION_TYPE)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    parse_session_type(if session.is_empty() {
        None
    } else {
        Some(session.as_str())
    })
}

fn parse_session_type(session_type: Option<&str>) -> SessionType {
    match session_type {
        Some(value) if value.eq_ignore_ascii_case(SESSION_TYPE_X11) => SessionType::X11,
        Some(value) if value.eq_ignore_ascii_case(SESSION_TYPE_WAYLAND) => SessionType::Wayland,
        _ => SessionType::Unknown,
    }
}

fn detect_desktop_environment() -> DesktopEnvironment {
    let current_desktop = env::var(XDG_CURRENT_DESKTOP).ok();
    let desktop_session = env::var(DESKTOP_SESSION).ok();
    parse_desktop_environment(current_desktop.as_deref(), desktop_session.as_deref())
}

fn parse_desktop_environment(
    current_desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> DesktopEnvironment {
    let desktop = current_desktop
        .or(desktop_session)
        .unwrap_or_default()
        .to_ascii_uppercase();

    if desktop.contains("KDE") || desktop.contains("PLASMA") {
        DesktopEnvironment::Kde
    } else if desktop.contains("GNOME") || desktop.contains("UBUNTU") {
        DesktopEnvironment::Gnome
    } else {
        DesktopEnvironment::Other
    }
}

fn select_wayland_tool(
    desktop: DesktopEnvironment,
    has_gnome_screenshot: bool,
    has_spectacle: bool,
) -> Option<WaylandScreenshotTool> {
    match desktop {
        DesktopEnvironment::Kde => {
            if has_spectacle {
                Some(WaylandScreenshotTool::Spectacle)
            } else if has_gnome_screenshot {
                Some(WaylandScreenshotTool::GnomeScreenshot)
            } else {
                None
            }
        }
        DesktopEnvironment::Gnome | DesktopEnvironment::Other => {
            if has_gnome_screenshot {
                Some(WaylandScreenshotTool::GnomeScreenshot)
            } else if has_spectacle {
                Some(WaylandScreenshotTool::Spectacle)
            } else {
                None
            }
        }
    }
}

fn command_exists(command: &str) -> bool {
    if command.contains('/') {
        return is_executable(Path::new(command));
    }

    env::var_os("PATH")
        .map(|path_var| {
            env::split_paths(&path_var).any(|directory| is_executable(&directory.join(command)))
        })
        .unwrap_or(false)
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

fn capture_x11_ocr_text() -> Result<String, String> {
    let display = XDisplayHandle::open()?;
    let root_window = display.root_window();
    let target_window = display.active_window().unwrap_or(root_window);

    let (rgb_pixels, width, height) = match display.capture_window_rgb(target_window) {
        Ok(result) => result,
        Err(active_error) if target_window != root_window => {
            display.capture_window_rgb(root_window).map_err(|root_error| {
                format!(
                    "Failed to capture active window ({active_error}); fallback root capture failed ({root_error})"
                )
            })?
        }
        Err(error) => return Err(error),
    };

    let ppm_file = TempPpmFile::create(width, height, &rgb_pixels)?;
    run_tesseract(ppm_file.path())
}

fn capture_wayland_ocr_text(tool: WaylandScreenshotTool) -> Result<String, String> {
    let screenshot_file = TempImageFile::create("png")?;
    run_wayland_screenshot(tool, screenshot_file.path())?;
    run_tesseract(screenshot_file.path())
}

fn run_wayland_screenshot(tool: WaylandScreenshotTool, output_path: &Path) -> Result<(), String> {
    let output = match tool {
        WaylandScreenshotTool::GnomeScreenshot => Command::new(GNOME_SCREENSHOT_EXECUTABLE)
            .arg(GNOME_SCREENSHOT_OUTPUT_FLAG)
            .arg(output_path)
            .output()
            .map_err(|error| {
                format!("Failed to launch {}: {error}", GNOME_SCREENSHOT_EXECUTABLE)
            })?,
        WaylandScreenshotTool::Spectacle => Command::new(SPECTACLE_EXECUTABLE)
            .arg(SPECTACLE_BACKGROUND_FLAG)
            .arg(SPECTACLE_NO_NOTIFY_FLAG)
            .arg(SPECTACLE_OUTPUT_FLAG)
            .arg(output_path)
            .output()
            .map_err(|error| format!("Failed to launch {}: {error}", SPECTACLE_EXECUTABLE))?,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tool_name = match tool {
            WaylandScreenshotTool::GnomeScreenshot => GNOME_SCREENSHOT_EXECUTABLE,
            WaylandScreenshotTool::Spectacle => SPECTACLE_EXECUTABLE,
        };

        if stderr.trim().is_empty() {
            return Err(format!(
                "{} exited with status {}.",
                tool_name, output.status
            ));
        }

        return Err(format!("{} failed: {}", tool_name, stderr.trim()));
    }

    let metadata = fs::metadata(output_path).map_err(|error| {
        format!(
            "Screenshot command did not produce output file '{}': {error}",
            output_path.display()
        )
    })?;

    if metadata.len() == 0 {
        return Err(format!(
            "Screenshot command produced an empty file: '{}'.",
            output_path.display()
        ));
    }

    Ok(())
}

struct XDisplayHandle {
    display: *mut xlib::Display,
}

impl XDisplayHandle {
    fn open() -> Result<Self, String> {
        let display = unsafe { xlib::XOpenDisplay(ptr::null()) };
        if display.is_null() {
            return Err(
                "Could not connect to an X11 display. OCR is currently available on Linux X11 sessions."
                    .to_string(),
            );
        }

        Ok(Self { display })
    }

    fn root_window(&self) -> xlib::Window {
        unsafe { xlib::XDefaultRootWindow(self.display) }
    }

    fn active_window(&self) -> Result<xlib::Window, String> {
        let property_name = std::ffi::CString::new(ACTIVE_WINDOW_PROPERTY_NAME)
            .map_err(|_| "Invalid active window property name".to_string())?;

        let active_window_atom =
            unsafe { xlib::XInternAtom(self.display, property_name.as_ptr(), xlib::False) };
        if active_window_atom == 0 {
            return Err("Failed to resolve _NET_ACTIVE_WINDOW atom.".to_string());
        }

        let mut actual_type: c_ulong = 0;
        let mut actual_format: c_int = 0;
        let mut item_count: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut property_data: *mut c_uchar = ptr::null_mut();

        let status = unsafe {
            xlib::XGetWindowProperty(
                self.display,
                self.root_window(),
                active_window_atom,
                0,
                ACTIVE_WINDOW_PROPERTY_ITEMS,
                xlib::False,
                xlib::AnyPropertyType as c_ulong,
                &mut actual_type,
                &mut actual_format,
                &mut item_count,
                &mut bytes_after,
                &mut property_data,
            )
        };

        if status != xlib::Success as c_int {
            return Err("Failed to read active window property from X11.".to_string());
        }

        if property_data.is_null() || item_count == 0 {
            return Err("X11 active window property returned no window.".to_string());
        }

        let window = unsafe { *(property_data.cast::<c_ulong>()) };
        unsafe {
            xlib::XFree(property_data.cast::<c_void>());
        }

        if actual_type != xlib::XA_WINDOW || actual_format != 32 || window == 0 {
            return Err("X11 active window property has an unexpected format.".to_string());
        }

        Ok(window)
    }

    fn capture_window_rgb(&self, window: xlib::Window) -> Result<(Vec<u8>, i32, i32), String> {
        let mut attributes: xlib::XWindowAttributes = unsafe { std::mem::zeroed() };
        let attributes_status =
            unsafe { xlib::XGetWindowAttributes(self.display, window, &mut attributes) };
        if attributes_status == 0 {
            return Err("Failed to read X11 window attributes.".to_string());
        }

        if attributes.map_state != xlib::IsViewable {
            return Err("Target X11 window is not viewable.".to_string());
        }

        if attributes.width < MIN_WINDOW_DIMENSION || attributes.height < MIN_WINDOW_DIMENSION {
            return Err("Foreground window is too small for OCR.".to_string());
        }

        let image = unsafe {
            xlib::XGetImage(
                self.display,
                window,
                0,
                0,
                attributes.width as u32,
                attributes.height as u32,
                xlib::XAllPlanes(),
                xlib::ZPixmap,
            )
        };

        if image.is_null() {
            return Err("Failed to capture X11 window pixels.".to_string());
        }

        let pixels_result = extract_rgb_pixels(image, attributes.width, attributes.height);
        unsafe {
            xlib::XDestroyImage(image);
        }
        let pixels = pixels_result?;

        Ok((pixels, attributes.width, attributes.height))
    }
}

impl Drop for XDisplayHandle {
    fn drop(&mut self) {
        unsafe {
            xlib::XCloseDisplay(self.display);
        }
    }
}

fn extract_rgb_pixels(
    image: *mut xlib::XImage,
    width: i32,
    height: i32,
) -> Result<Vec<u8>, String> {
    if width <= 0 || height <= 0 {
        return Err("Invalid image dimensions for OCR.".to_string());
    }

    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "Image dimensions are too large for OCR processing.".to_string())?;

    let mut rgb = Vec::with_capacity(pixel_count * PPM_RGB_CHANNELS);

    let red_mask = unsafe { (*image).red_mask as u64 };
    let green_mask = unsafe { (*image).green_mask as u64 };
    let blue_mask = unsafe { (*image).blue_mask as u64 };

    for y in 0..height {
        for x in 0..width {
            let pixel = unsafe { xlib::XGetPixel(image, x, y) } as u64;
            rgb.push(extract_channel_value(pixel, red_mask));
            rgb.push(extract_channel_value(pixel, green_mask));
            rgb.push(extract_channel_value(pixel, blue_mask));
        }
    }

    Ok(rgb)
}

fn extract_channel_value(pixel: u64, mask: u64) -> u8 {
    if mask == 0 {
        return 0;
    }

    let shift = mask.trailing_zeros();
    let channel_max = mask >> shift;
    if channel_max == 0 {
        return 0;
    }

    let raw_value = (pixel & mask) >> shift;
    ((raw_value * u64::from(PPM_MAX_COLOR_VALUE)) / channel_max) as u8
}

fn encode_ppm_bytes(width: i32, height: i32, rgb_pixels: &[u8]) -> Result<Vec<u8>, String> {
    if width <= 0 || height <= 0 {
        return Err("PPM encoding requires positive image dimensions.".to_string());
    }

    let expected_size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixel_count| pixel_count.checked_mul(PPM_RGB_CHANNELS))
        .ok_or_else(|| "PPM image dimensions are too large.".to_string())?;

    if rgb_pixels.len() != expected_size {
        return Err(format!(
            "PPM pixel payload size mismatch (expected {}, got {}).",
            expected_size,
            rgb_pixels.len()
        ));
    }

    let mut data = format!("P6\n{} {}\n{}\n", width, height, PPM_MAX_COLOR_VALUE).into_bytes();
    data.extend_from_slice(rgb_pixels);
    Ok(data)
}

struct TempPpmFile {
    path: PathBuf,
}

impl TempPpmFile {
    fn create(width: i32, height: i32, rgb_pixels: &[u8]) -> Result<Self, String> {
        let ppm_data = encode_ppm_bytes(width, height, rgb_pixels)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();

        let path = env::temp_dir().join(format!(
            "handy-ocr-{}-{}.ppm",
            std::process::id(),
            timestamp
        ));

        fs::write(&path, ppm_data)
            .map_err(|error| format!("Failed to write temporary OCR image: {error}"))?;

        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempPpmFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct TempImageFile {
    path: PathBuf,
}

impl TempImageFile {
    fn create(extension: &str) -> Result<Self, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();

        let path = env::temp_dir().join(format!(
            "handy-ocr-{}-{}.{}",
            std::process::id(),
            timestamp,
            extension
        ));

        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempImageFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn run_tesseract(image_path: &Path) -> Result<String, String> {
    let output = Command::new(TESSERACT_EXECUTABLE)
        .arg(image_path)
        .arg(TESSERACT_OUTPUT_TARGET)
        .arg("--psm")
        .arg(TESSERACT_PAGE_SEGMENT_MODE)
        .output()
        .map_err(|error| format!("Failed to launch tesseract: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.trim().is_empty() {
            return Err(format!("Tesseract exited with status {}.", output.status));
        }

        return Err(format!("Tesseract OCR failed: {}", stderr.trim()));
    }

    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_string())
        .map_err(|error| format!("Failed to decode Tesseract output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_channel_value_scales_full_range() {
        assert_eq!(extract_channel_value(0x00ff_0000, 0x00ff_0000), 255);
        assert_eq!(extract_channel_value(0x0000_0000, 0x00ff_0000), 0);
    }

    #[test]
    fn extract_channel_value_scales_sub_byte_masks() {
        assert_eq!(extract_channel_value(0x00f0_0000, 0x00f0_0000), 255);
        assert_eq!(extract_channel_value(0x0080_0000, 0x00f0_0000), 136);
    }

    #[test]
    fn encode_ppm_bytes_writes_header_and_pixels() {
        let rgb = vec![255, 0, 0, 0, 255, 0];
        let ppm = encode_ppm_bytes(2, 1, &rgb).expect("PPM encoding should succeed");

        assert!(ppm.starts_with(b"P6\n2 1\n255\n"));
        assert!(ppm.ends_with(&rgb));
    }

    #[test]
    fn encode_ppm_bytes_rejects_invalid_pixel_size() {
        let result = encode_ppm_bytes(2, 2, &[255, 0, 0]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_session_type_handles_known_values() {
        assert_eq!(parse_session_type(Some("x11")), SessionType::X11);
        assert_eq!(parse_session_type(Some("wayland")), SessionType::Wayland);
        assert_eq!(parse_session_type(Some("tty")), SessionType::Unknown);
    }

    #[test]
    fn parse_desktop_environment_handles_known_values() {
        assert_eq!(
            parse_desktop_environment(Some("ubuntu:GNOME"), None),
            DesktopEnvironment::Gnome
        );
        assert_eq!(
            parse_desktop_environment(Some("KDE"), None),
            DesktopEnvironment::Kde
        );
        assert_eq!(
            parse_desktop_environment(Some("XFCE"), None),
            DesktopEnvironment::Other
        );
    }

    #[test]
    fn select_wayland_tool_prefers_desktop_default() {
        assert_eq!(
            select_wayland_tool(DesktopEnvironment::Gnome, true, true),
            Some(WaylandScreenshotTool::GnomeScreenshot)
        );
        assert_eq!(
            select_wayland_tool(DesktopEnvironment::Kde, true, true),
            Some(WaylandScreenshotTool::Spectacle)
        );
    }

    #[test]
    fn select_wayland_tool_uses_secondary_when_primary_missing() {
        assert_eq!(
            select_wayland_tool(DesktopEnvironment::Gnome, false, true),
            Some(WaylandScreenshotTool::Spectacle)
        );
        assert_eq!(
            select_wayland_tool(DesktopEnvironment::Kde, true, false),
            Some(WaylandScreenshotTool::GnomeScreenshot)
        );
        assert_eq!(
            select_wayland_tool(DesktopEnvironment::Other, false, false),
            None
        );
    }
}
