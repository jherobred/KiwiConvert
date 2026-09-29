//! Window management. Every window loads the same frontend bundle, which routes on the
//! window label: `wheel`, `activity`, `hub`, or `tool-<n>`.

use crate::platform::overlay;
use crate::registry::Tool;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const ACTIVITY_W: f64 = 392.0;
pub const ACTIVITY_H: f64 = 620.0;
pub const HUB_W: f64 = 392.0;
pub const HUB_H: f64 = 620.0;

fn builder<'a>(app: &'a AppHandle, label: &str) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("KiwiConvert")
        .visible(false)
}

pub fn create_core_windows(app: &AppHandle) -> tauri::Result<()> {
    let wheel = builder(app, "wheel")
        .inner_size(crate::wheel::WIN_W, crate::wheel::WIN_H)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .disable_drag_drop_handler()
        .build()?;
    let hwnd = wheel.hwnd()?;
    overlay::make_floating(hwnd, true);
    let handle = app.clone();
    crate::platform::droptarget::attach(
        hwnd,
        std::sync::Arc::new(move |evt| crate::wheel::on_drop_event(&handle, evt)),
    );

    let activity = builder(app, "activity")
        .inner_size(ACTIVITY_W, ACTIVITY_H)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .disable_drag_drop_handler()
        .build()?;
    overlay::make_floating(activity.hwnd()?, true);

    let hub = builder(app, "hub")
        .inner_size(HUB_W, HUB_H)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .disable_drag_drop_handler()
        .build()?;
    let hub_hwnd = hub.hwnd()?;
    overlay::make_floating(hub_hwnd, false);
    let handle = app.clone();
    crate::platform::droptarget::attach(
        hub_hwnd,
        std::sync::Arc::new(move |evt| crate::wheel::on_hub_drop(&handle, evt)),
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Activity window (job cards, bottom-right)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

static ACTIVITY_VISIBLE: AtomicBool = AtomicBool::new(false);
static REGIONS: Mutex<Vec<Region>> = parking_lot::const_mutex(Vec::new());

pub fn show_activity(app: &AppHandle) {
    let Some(win) = app.get_webview_window("activity") else { return };
    if ACTIVITY_VISIBLE.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(hwnd) = win.hwnd() else { return };
    let (cx, cy) = overlay::cursor_pos();
    let m = overlay::monitor_at(cx, cy);
    let w = (ACTIVITY_W * m.scale).round() as i32;
    let h = (ACTIVITY_H * m.scale).round() as i32;
    overlay::show_at(hwnd, m.work.right - w, m.work.bottom - h, w, h);
    let _ = win.set_ignore_cursor_events(true);
    std::thread::spawn({
        let win = win.clone();
        move || click_through_loop(win)
    });
}

pub fn hide_activity(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("activity") {
        ACTIVITY_VISIBLE.store(false, Ordering::SeqCst);
        if let Ok(hwnd) = win.hwnd() {
            overlay::hide(hwnd);
        }
    }
}

pub fn set_activity_regions(regions: Vec<Region>) {
    *REGIONS.lock() = regions;
}

/// The activity window is mostly transparent. It only takes the mouse while the cursor is
/// over a card, so the rest of the screen stays clickable.
fn click_through_loop(win: WebviewWindow) {
    let mut ignoring = true;
    while ACTIVITY_VISIBLE.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(30));
        let Ok(hwnd) = win.hwnd() else { break };
        let r = overlay::window_rect(hwnd);
        let (cx, cy) = overlay::cursor_pos();
        let scale = win.scale_factor().unwrap_or(1.0);
        let lx = (cx - r.left) as f64 / scale;
        let ly = (cy - r.top) as f64 / scale;
        let inside = REGIONS
            .lock()
            .iter()
            .any(|g| lx >= g.x && lx <= g.x + g.w && ly >= g.y && ly <= g.y + g.h);
        if inside == ignoring {
            ignoring = !inside;
            let _ = win.set_ignore_cursor_events(ignoring);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Hub (tray panel)
// ---------------------------------------------------------------------------------------

pub fn toggle_hub(app: &AppHandle, anchor: Option<(i32, i32)>) {
    let Some(hub) = app.get_webview_window("hub") else { return };
    if hub.is_visible().unwrap_or(false) {
        let _ = hub.hide();
    } else {
        show_hub(app, anchor);
    }
}

/// Shows the hub above the tray icon (`anchor`), or centered when there is no anchor.
pub fn show_hub(app: &AppHandle, anchor: Option<(i32, i32)>) {
    let Some(hub) = app.get_webview_window("hub") else { return };
    let Ok(hwnd) = hub.hwnd() else { return };
    let (ax, ay) = anchor.unwrap_or_else(overlay::cursor_pos);
    let m = overlay::monitor_at(ax, ay);
    let w = (HUB_W * m.scale).round() as i32;
    let h = (HUB_H * m.scale).round() as i32;
    let (x, y) = if anchor.is_some() {
        let x = (ax - w / 2).clamp(m.work.left, m.work.right - w);
        // Taskbar at the bottom (the usual case) or at the top.
        let y = if ay > (m.work.top + m.work.bottom) / 2 {
            m.work.bottom - h
        } else {
            m.work.top
        };
        (x, y)
    } else {
        (
            (m.work.left + m.work.right - w) / 2,
            (m.work.top + m.work.bottom - h) / 2,
        )
    };
    overlay::show_at(hwnd, x, y, w, h);
    let _ = hub.set_always_on_top(false);
    let _ = hub.set_focus();
    let _ = tauri::Emitter::emit(&hub, "hub://shown", ());
}

// ---------------------------------------------------------------------------------------
// Tool windows
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSession {
    pub tool: Tool,
    pub paths: Vec<PathBuf>,
}

static TOOL_SESSIONS: Mutex<Option<HashMap<String, ToolSession>>> = parking_lot::const_mutex(None);
static TOOL_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn tool_session(label: &str) -> Option<ToolSession> {
    TOOL_SESSIONS.lock().as_ref()?.get(label).cloned()
}

fn tool_size(tool: Tool) -> (f64, f64) {
    match tool {
        Tool::Compress => (460.0, 600.0),
        Tool::Resize | Tool::Speed | Tool::Channels => (440.0, 520.0),
        Tool::Metadata => (680.0, 640.0),
        Tool::Bleep | Tool::Trim | Tool::Normalize => (1040.0, 640.0),
        _ => (1120.0, 760.0),
    }
}

pub fn open_tool(app: &AppHandle, tool: Tool, paths: Vec<PathBuf>) {
    // Some tools only make sense in a specific window: audio trim uses the audio editor.
    let n = TOOL_COUNTER.fetch_add(1, Ordering::SeqCst);
    let label = format!("tool-{n}");
    TOOL_SESSIONS
        .lock()
        .get_or_insert_with(HashMap::new)
        .insert(label.clone(), ToolSession { tool, paths: paths.clone() });
    for p in &paths {
        let _ = app.asset_protocol_scope().allow_file(p);
    }
    let (w, h) = tool_size(tool);
    let app = app.clone();
    // Window creation must not block the thread that received the drop.
    std::thread::spawn(move || {
        let result = builder(&app, &label)
            .inner_size(w, h)
            .min_inner_size(w.min(420.0), h.min(420.0))
            .decorations(false)
            .shadow(true)
            .resizable(true)
            .center()
            .background_color(tauri::window::Color(20, 24, 18, 255))
            .disable_drag_drop_handler()
            .build();
        if let Err(e) = result {
            log::error!("could not open the {tool:?} window: {e}");
        }
    });
}

pub fn close_tool_session(label: &str) {
    if let Some(map) = TOOL_SESSIONS.lock().as_mut() {
        map.remove(label);
    }
}
