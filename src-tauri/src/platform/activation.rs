//! Notifications when a window stops being active.
//!
//! Focus events are no good for flyouts here: clicking inside a WebView2 window moves focus
//! from the top-level window to its browser child, which reads as a blur. Activation only
//! changes when the user switches to another window or app.

use parking_lot::Mutex;
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WA_INACTIVE, WM_ACTIVATE, WM_ACTIVATEAPP};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    /// Another window, even one of ours, became active.
    WindowDeactivated,
    /// A different program became active.
    AppDeactivated,
}

type Handler = Box<dyn Fn() + Send + Sync>;

static HANDLERS: Mutex<Option<HashMap<isize, (When, Handler)>>> = parking_lot::const_mutex(None);

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    let event = match msg {
        WM_ACTIVATEAPP if wparam.0 == 0 => Some(When::AppDeactivated),
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => Some(When::WindowDeactivated),
        _ => None,
    };
    if let Some(event) = event {
        if let Some((when, handler)) = HANDLERS.lock().as_ref().and_then(|m| m.get(&(hwnd.0 as isize))) {
            if *when == event {
                handler();
            }
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Calls `handler` on the window's thread when `hwnd` is deactivated. The handler must not
/// block; hand real work to another thread.
pub fn on_deactivate(hwnd: HWND, when: When, handler: impl Fn() + Send + Sync + 'static) {
    HANDLERS
        .lock()
        .get_or_insert_with(HashMap::new)
        .insert(hwnd.0 as isize, (when, Box::new(handler)));
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(subclass), 0x4B49_5749, 0);
    }
}
