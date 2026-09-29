//! Helpers for floating windows that appear over other apps without taking focus.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetCursorPos, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST, SW_HIDE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// Keeps a window out of Alt+Tab and stops clicks on it from activating it.
pub fn make_floating(hwnd: HWND, no_activate: bool) {
    unsafe {
        let before = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut ex = before | WS_EX_TOOLWINDOW.0 as isize;
        if no_activate {
            ex |= WS_EX_NOACTIVATE.0 as isize;
        } else {
            ex &= !(WS_EX_NOACTIVATE.0 as isize);
        }
        if ex != before {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
            // Style changes are cached until the frame is recalculated. Without this the
            // window briefly grows a tool-window caption.
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// Shows a window topmost at a physical-pixel rectangle without activating it.
pub fn show_at(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

pub fn hide(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

pub fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    r
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    /// Work area (screen minus taskbar) in physical pixels.
    pub work: RECT,
    /// Display scale, 1.0 at 96 DPI.
    pub scale: f64,
}

pub fn monitor_at(x: i32, y: i32) -> Monitor {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(hmon, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        Monitor {
            work: info.rcWork,
            scale: dx.max(96) as f64 / 96.0,
        }
    }
}

/// Places a `w`x`h` logical-pixel window so that logical point (`ax`, `ay`) inside it sits
/// under screen point (`x`, `y`), shifted as needed to stay inside the monitor's work area.
/// Returns the physical rectangle (x, y, w, h).
pub fn anchored_rect(x: i32, y: i32, w: f64, h: f64, ax: f64, ay: f64) -> (i32, i32, i32, i32) {
    let m = monitor_at(x, y);
    let pw = (w * m.scale).round() as i32;
    let ph = (h * m.scale).round() as i32;
    let mut left = x - (ax * m.scale).round() as i32;
    let mut top = y - (ay * m.scale).round() as i32;
    left = left.clamp(m.work.left, (m.work.right - pw).max(m.work.left));
    top = top.clamp(m.work.top, (m.work.bottom - ph).max(m.work.top));
    (left, top, pw, ph)
}
