//! Placing and showing the popup window.
//!
//! The popup has two modes:
//! - Menu: a focused, keyboard-driven window for picking an action.
//! - Toast: a small, non-activating status pill shown while a direct
//!   shortcut (e.g. Fix) works, so the user's app keeps the keyboard.

use serde::Serialize;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

use crate::winsys;

pub const LABEL: &str = "popup";
/// Transparent margin around the card in popup.html, in CSS pixels.
pub const CARD_MARGIN: f64 = 12.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Hidden,
    Menu,
    Toast,
}

/// Where the popup should appear: under the caret, or under the mouse.
#[derive(Clone, Copy, Debug, Default)]
pub struct Anchor {
    pub x: i32,
    pub top: i32,
    pub bottom: i32,
}

#[derive(Default)]
pub struct PopupState {
    pub mode: Mode,
    pub anchor: Anchor,
    /// Id of an open/toast request that the page hasn't presented yet.
    pub pending: u64,
    pub toast_id: u64,
    /// The user dragged the popup; keep it where they put it until it closes.
    pub user_moved: bool,
    /// The popup opened above the caret, so it grows upward.
    pub flipped: bool,
    /// The current request is on screen (as a loading card or for real).
    pub presented: bool,
}

/// Resize after the content changed without the window jumping around:
/// keep the top edge (or the bottom edge, when the popup sits above the
/// caret), stay on screen, and move + resize in a single step.
pub fn resize_anchored(win: &WebviewWindow, width: f64, height: f64, keep_bottom: bool) {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else { return };
    let scale = win.scale_factor().unwrap_or(1.0);
    let w = (width * scale).round() as i32;
    let h = (height * scale).round() as i32;
    if w == size.width as i32 && h == size.height as i32 {
        return;
    }
    let (left, top, right, bottom) = winsys::work_area_at(pos.x + w / 2, pos.y + 10);
    let y = if keep_bottom { pos.y + size.height as i32 - h } else { pos.y };
    let x = pos.x.clamp(left, (right - w).max(left));
    let y = y.clamp(top, (bottom - h).max(top));
    winsys::set_bounds(raw_hwnd(win), x, y, w, h);
}

/// Make the window taller without moving it, up to the bottom of the screen.
/// Returns the height it got, in CSS pixels.
pub fn grow_down(win: &WebviewWindow, width: f64, height: f64) -> f64 {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else { return height };
    let scale = win.scale_factor().unwrap_or(1.0);
    let w = (width * scale).round() as i32;
    let (_, _, _, bottom) = winsys::work_area_at(pos.x + w / 2, pos.y + 10);
    let room = (bottom - pos.y).max(size.height as i32);
    let h = ((height * scale).round() as i32).clamp(size.height as i32, room);
    winsys::set_bounds(raw_hwnd(win), pos.x, pos.y, w, h);
    h as f64 / scale
}

pub fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

pub fn raw_hwnd(win: &WebviewWindow) -> isize {
    #[cfg(windows)]
    {
        win.hwnd().map(|h| h.0 as isize).unwrap_or(0)
    }
    #[cfg(not(windows))]
    {
        let _ = win;
        0
    }
}

pub fn anchor_for(target: isize) -> Anchor {
    if target != 0 {
        if let Some((left, top, _right, bottom)) = winsys::caret_rect(target) {
            return Anchor { x: left, top, bottom };
        }
    }
    let (x, y) = winsys::cursor_pos();
    Anchor { x, top: y - 4, bottom: y + 16 }
}

/// Size the window (CSS pixels) and position it next to the anchor,
/// flipping above it and clamping so it stays on screen.
/// Returns true when it was placed above the anchor.
pub fn place(win: &WebviewWindow, anchor: Anchor, width: f64, height: f64) -> bool {
    let (left, top, right, bottom) = winsys::work_area_at(anchor.x, anchor.bottom);
    let raw = raw_hwnd(win);
    let scale = winsys::monitor_scale_at(anchor.x, anchor.bottom)
        .unwrap_or_else(|| win.scale_factor().unwrap_or(1.0));
    let w = (width * scale).round() as i32;
    let h = (height * scale).round() as i32;
    let margin = (CARD_MARGIN * scale).round() as i32;
    let gap = (6.0 * scale).round() as i32;

    let mut x = anchor.x - margin - (12.0 * scale) as i32;
    let mut y = anchor.bottom + gap - margin;
    let flipped = y + h > bottom && anchor.top - gap + margin - h >= top;
    if flipped {
        y = anchor.top - gap + margin - h;
    }
    x = x.clamp(left, (right - w).max(left));
    y = y.clamp(top, (bottom - h).max(top));
    if raw != 0 {
        winsys::set_bounds(raw, x, y, w, h);
    } else {
        let _ = win.set_size(LogicalSize::new(width, height));
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
    flipped
}

/// Place the status toast at the bottom center of the screen the user is
/// working on, just above the taskbar.
pub fn place_toast(win: &WebviewWindow, anchor: Anchor, width: f64, height: f64) {
    let (left, top, right, bottom) = winsys::work_area_at(anchor.x, anchor.bottom);
    let raw = raw_hwnd(win);
    let scale = winsys::monitor_scale_at(anchor.x, anchor.bottom)
        .unwrap_or_else(|| win.scale_factor().unwrap_or(1.0));
    let w = (width * scale).round() as i32;
    let h = (height * scale).round() as i32;
    let x = (left + right - w) / 2;
    let y = (bottom - h - (28.0 * scale) as i32).max(top);
    if raw != 0 {
        winsys::set_bounds(raw, x, y, w, h);
    } else {
        let _ = win.set_size(LogicalSize::new(width, height));
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

pub fn show(win: &WebviewWindow, focus: bool) {
    let raw = raw_hwnd(win);
    if raw != 0 {
        // Re-verify WS_POPUP / no-caption style before showing (Tao's queued
        // post-create window_flags sync can re-apply WS_CAPTION after setup).
        winsys::configure_popup_window(raw);
    }
    if focus {
        winsys::set_no_activate(raw, false);
        if raw != 0 {
            // Show via Win32 directly so Tao doesn't re-apply WS_CAPTION and
            // flash a classic window frame.
            winsys::show_window(raw);
            if winsys::foreground_window() != raw {
                winsys::focus_window(raw);
            }
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    } else {
        winsys::set_no_activate(raw, true);
        if raw != 0 {
            winsys::show_no_activate(raw);
        } else {
            let _ = win.show();
        }
    }
}
