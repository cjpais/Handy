//! Notch-integrated recording indicator (macOS only).
//!
//! When `overlay_notch` is enabled and the screen under the cursor has a camera
//! housing, the recording overlay panel is moved flush to the top edge of that
//! screen and widened around the notch, so the indicator reads as part of it.
//! Screens without a notch keep the regular pill overlay.
//!
//! All geometry is in Cocoa global coordinates (points, bottom-left origin), as
//! reported by `NSScreen`, so mixed-scale and external displays need no
//! conversion. Nothing here is hard-coded to a particular MacBook: the notch
//! size comes from `safeAreaInsets` and the `auxiliaryTop{Left,Right}Area`
//! rectangles of the target screen.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde::Serialize;

/// How far the indicator extends past each side of the camera housing.
pub const SIDE_WIDTH: f64 = 107.5;
/// Concave "ear" that blends each top corner into the menu bar.
pub const EAR_WIDTH: f64 = 16.0;
/// Clickable square around the cancel button (22pt button plus padding).
const CANCEL_HIT_SIZE: f64 = 28.0;
/// Center of the cancel button, measured from the right edge of the body
/// (12pt margin + half of the 22pt button). Keep in sync with the CSS.
const CANCEL_CENTER_INSET: f64 = 23.0;

/// Rectangle in Cocoa global coordinates (bottom-left origin, points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn from_ns(rect: NSRect) -> Self {
        Self {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        }
    }

    fn to_ns(self) -> NSRect {
        NSRect::new(
            NSPoint::new(self.x, self.y),
            NSSize::new(self.width, self.height),
        )
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// Geometry sent to the overlay webview so it can draw around the notch.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct NotchGeometry {
    /// Width of the camera housing, kept empty by the indicator.
    pub notch_width: f64,
    /// Height of the notch (equals the menu bar height on notched screens).
    pub height: f64,
    pub side_width: f64,
    pub ear_width: f64,
}

/// Where the overlay panel goes on a notched screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotchLayout {
    /// Overlay panel frame.
    pub window: Rect,
    /// Area where the panel accepts clicks (the cancel button).
    pub cancel_hit: Rect,
    pub geometry: NotchGeometry,
}

/// Computes the overlay layout for a screen, or `None` when it has no notch.
///
/// `left_area` / `right_area` are the screen's `auxiliaryTopLeftArea` and
/// `auxiliaryTopRightArea`. Only their widths are used, so the result does not
/// depend on which coordinate space AppKit reports them in.
pub fn layout_for_screen(
    screen_frame: Rect,
    top_inset: f64,
    left_area: Rect,
    right_area: Rect,
) -> Option<NotchLayout> {
    let notch_width = screen_frame.width - left_area.width - right_area.width;
    if top_inset <= 0.0 || left_area.width <= 0.0 || right_area.width <= 0.0 || notch_width <= 0.0 {
        return None;
    }

    let height = top_inset;
    let notch_left = screen_frame.x + left_area.width;
    let width = notch_width + 2.0 * (SIDE_WIDTH + EAR_WIDTH);
    let window = Rect {
        x: notch_left - SIDE_WIDTH - EAR_WIDTH,
        y: screen_frame.y + screen_frame.height - height,
        width,
        height,
    };

    let body_right = window.x + window.width - EAR_WIDTH;
    let cancel_hit = Rect {
        x: body_right - CANCEL_CENTER_INSET - CANCEL_HIT_SIZE / 2.0,
        y: window.y + (height - CANCEL_HIT_SIZE) / 2.0,
        width: CANCEL_HIT_SIZE,
        height: CANCEL_HIT_SIZE,
    };

    Some(NotchLayout {
        window,
        cancel_hit,
        geometry: NotchGeometry {
            notch_width,
            height,
            side_width: SIDE_WIDTH,
            ear_width: EAR_WIDTH,
        },
    })
}

fn screen_layout(screen: &NSScreen) -> Option<NotchLayout> {
    layout_for_screen(
        Rect::from_ns(screen.frame()),
        screen.safeAreaInsets().top,
        Rect::from_ns(screen.auxiliaryTopLeftArea()),
        Rect::from_ns(screen.auxiliaryTopRightArea()),
    )
}

/// Layout for the screen under the mouse pointer, the same screen the regular
/// overlay follows. `None` when that screen has no notch.
pub fn layout_for_cursor_screen(mtm: MainThreadMarker) -> Option<NotchLayout> {
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|screen| Rect::from_ns(screen.frame()).contains(mouse.x, mouse.y))
        .or_else(|| NSScreen::mainScreen(mtm))?;
    screen_layout(&screen)
}

/// Re-reads the layout of the screen a notch overlay currently sits on, so a
/// resolution or arrangement change can be followed. `None` when that screen is
/// gone (e.g. clamshell mode) or no longer reports a notch.
pub fn layout_for_window_screen(mtm: MainThreadMarker, window: &Rect) -> Option<NotchLayout> {
    let top_center = (
        window.x + window.width / 2.0,
        window.y + window.height - 1.0,
    );
    NSScreen::screens(mtm)
        .iter()
        .find(|screen| Rect::from_ns(screen.frame()).contains(top_center.0, top_center.1))
        .and_then(|screen| screen_layout(&screen))
}

/// Moves the panel to `frame` without activating it.
pub fn set_window_frame(ns_window: &NSWindow, frame: Rect) {
    ns_window.setFrame_display(frame.to_ns(), true);
}

/// Lets clicks fall through to the menu bar everywhere except the cancel button.
pub fn update_mouse_passthrough(ns_window: &NSWindow, cancel_hit: &Rect) {
    let mouse = NSEvent::mouseLocation();
    let ignore = !cancel_hit.contains(mouse.x, mouse.y);
    if ns_window.ignoresMouseEvents() != ignore {
        log::debug!("notch overlay click-through: {ignore}");
        ns_window.setIgnoresMouseEvents(ignore);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn screen_without_notch_has_no_layout() {
        let frame = rect(-676.0, 1329.0, 3440.0, 1440.0);
        let empty = rect(0.0, 0.0, 0.0, 0.0);
        assert_eq!(layout_for_screen(frame, 0.0, empty, empty), None);
    }

    #[test]
    fn notch_is_measured_from_the_auxiliary_areas() {
        // 16" MacBook Pro at "More Space": 220pt notch, 38pt menu bar.
        let frame = rect(0.0, 0.0, 2056.0, 1329.0);
        let layout = layout_for_screen(
            frame,
            38.0,
            rect(0.0, 1291.0, 918.0, 38.0),
            rect(1138.0, 1291.0, 918.0, 38.0),
        )
        .unwrap();

        assert_eq!(layout.geometry.notch_width, 220.0);
        assert_eq!(layout.geometry.height, 38.0);
        // Flush with the top edge and centered on the notch.
        assert_eq!(layout.window.y + layout.window.height, 1329.0);
        assert_eq!(layout.window.x + layout.window.width / 2.0, 918.0 + 110.0);
        assert_eq!(layout.window.width, 220.0 + 2.0 * (SIDE_WIDTH + EAR_WIDTH));
    }

    #[test]
    fn layout_follows_screen_origin_not_area_origin() {
        // A notched screen placed to the right of, and below, the primary one.
        // The auxiliary areas are deliberately reported in local coordinates.
        let frame = rect(3440.0, -200.0, 1512.0, 982.0);
        let layout = layout_for_screen(
            frame,
            32.0,
            rect(0.0, 950.0, 664.0, 32.0),
            rect(849.0, 950.0, 663.0, 32.0),
        )
        .unwrap();

        assert_eq!(layout.geometry.notch_width, 185.0);
        assert_eq!(layout.window.y, -200.0 + 982.0 - 32.0);
        assert_eq!(layout.window.x, 3440.0 + 664.0 - SIDE_WIDTH - EAR_WIDTH);
    }

    #[test]
    fn cancel_hit_area_sits_inside_the_right_side() {
        let layout = layout_for_screen(
            rect(0.0, 0.0, 1512.0, 982.0),
            32.0,
            rect(0.0, 950.0, 664.0, 32.0),
            rect(849.0, 950.0, 663.0, 32.0),
        )
        .unwrap();

        let notch_right = 664.0 + 185.0;
        let body_right = layout.window.x + layout.window.width - EAR_WIDTH;
        assert!(layout.cancel_hit.x > notch_right);
        assert!(layout.cancel_hit.x + layout.cancel_hit.width <= body_right);
        assert!(layout.cancel_hit.y >= layout.window.y - 1.0);
    }
}
