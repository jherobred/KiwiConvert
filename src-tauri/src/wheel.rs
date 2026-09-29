//! The format wheel. Rust owns its state and does the hit testing, so a drop always lands on
//! the segment the user saw highlighted, even if the frontend is busy animating. The
//! frontend only draws what these events describe.

use crate::platform::droptarget::DropEvt;
use crate::platform::gesture::{self, Signal};
use crate::platform::overlay;
use crate::registry::{self, Kind, Mode, WheelOption};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

// Geometry shared with src/windows/wheel/geometry.ts. Logical pixels.
pub const WIN_W: f64 = 440.0;
pub const WIN_H: f64 = 480.0;
pub const CX: f64 = 220.0;
pub const CY: f64 = 220.0;
/// Inside this radius is the center: dropping there cancels.
pub const R_CENTER: f64 = 58.0;
/// Outside this radius a drop does nothing either.
pub const R_HIT: f64 = 200.0;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub count: usize,
    pub total_bytes: u64,
    pub name: String,
    pub first: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelPayload {
    pub generation: u64,
    pub open: bool,
    pub mode: Mode,
    pub click_mode: bool,
    pub files: Option<FileSummary>,
    pub options: Vec<WheelOption>,
    pub hover: i32,
}

#[derive(Default)]
struct State {
    open: bool,
    click_mode: bool,
    mode: Option<Mode>,
    paths: Vec<PathBuf>,
    options: Vec<WheelOption>,
    hover: i32,
    rect: (i32, i32, i32, i32),
    scale: f64,
    finishing: bool,
    generation: u64,
}

impl State {
    fn mode(&self) -> Mode {
        self.mode.unwrap_or(Mode::Convert)
    }

    fn payload(&self) -> WheelPayload {
        WheelPayload {
            generation: self.generation,
            open: self.open,
            mode: self.mode(),
            click_mode: self.click_mode,
            files: summarize(&self.paths),
            options: self.options.clone(),
            hover: self.hover,
        }
    }

    /// Segment index under a screen point, or -1 for the center and outside the ring.
    fn hit(&self, x: i32, y: i32) -> i32 {
        let n = self.options.len();
        if n == 0 || self.scale <= 0.0 {
            return -1;
        }
        let lx = (x - self.rect.0) as f64 / self.scale - CX;
        let ly = (y - self.rect.1) as f64 / self.scale - CY;
        segment_at(lx, ly, n)
    }
}

/// Maps a point relative to the wheel center to a segment. Segment 0 is centered at the
/// top and indices increase clockwise.
pub fn segment_at(dx: f64, dy: f64, n: usize) -> i32 {
    let d = (dx * dx + dy * dy).sqrt();
    if n == 0 || d < R_CENTER || d > R_HIT {
        return -1;
    }
    let deg = (dy.atan2(dx).to_degrees() + 90.0).rem_euclid(360.0);
    let seg = 360.0 / n as f64;
    (((deg + seg / 2.0) % 360.0) / seg).floor() as i32 % n as i32
}

static STATE: Mutex<Option<State>> = parking_lot::const_mutex(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock();
    f(guard.get_or_insert_with(State::default))
}

fn summarize(paths: &[PathBuf]) -> Option<FileSummary> {
    let first = paths.first()?;
    let total_bytes = paths
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();
    Some(FileSummary {
        count: paths.len(),
        total_bytes,
        name: first
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        first: first.to_string_lossy().into_owned(),
        kind: registry::kind_of(first),
    })
}

fn emit_state(app: &AppHandle) {
    let payload = with(|s| s.payload());
    let _ = app.emit_to("wheel", "wheel://state", payload);
}

pub fn snapshot() -> WheelPayload {
    with(|s| s.payload())
}

/// Opens the wheel centered on a screen point. `paths` is known up front when the wheel is
/// opened by click (from the hub); during a drag it arrives with the first drag event.
pub fn open(app: &AppHandle, x: i32, y: i32, mode: Mode, click_mode: bool, paths: Vec<PathBuf>) {
    let Some(win) = app.get_webview_window("wheel") else { return };
    let Ok(hwnd) = win.hwnd() else { return };
    let rect = overlay::anchored_rect(x, y, WIN_W, WIN_H, CX, CY);
    let scale = overlay::monitor_at(x, y).scale;
    with(|s| {
        s.generation += 1;
        s.open = true;
        s.finishing = false;
        s.click_mode = click_mode;
        s.mode = Some(mode);
        s.options = registry::wheel_options(&paths, mode);
        s.paths = paths;
        s.hover = -1;
        s.rect = rect;
        s.scale = scale;
    });
    overlay::make_floating(hwnd, !click_mode);
    log::debug!("wheel opening at {x},{y} (click mode: {click_mode})");
    emit_state(app);
    overlay::show_at(hwnd, rect.0, rect.1, rect.2, rect.3);
    if click_mode {
        overlay::activate(hwnd);
    }
    // Moving between monitors with different scaling can resize the window after it is
    // placed. Put it back once the DPI change has been handled.
    let generation = with(|s| s.generation);
    // HWND holds a raw pointer, which can't cross threads; pass the handle value instead.
    let raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        let hwnd = windows::Win32::Foundation::HWND(raw as *mut _);
        if with(|s| s.open && s.generation == generation) {
            let r = overlay::window_rect(hwnd);
            if (r.left, r.top, r.right - r.left, r.bottom - r.top) != rect {
                overlay::show_at(hwnd, rect.0, rect.1, rect.2, rect.3);
            }
        }
    });
}

pub fn toggle_mode(app: &AppHandle) {
    with(|s| {
        if !s.open || s.finishing {
            return;
        }
        let next = match s.mode() {
            Mode::Convert => Mode::Tools,
            Mode::Tools => Mode::Convert,
        };
        s.mode = Some(next);
        s.options = registry::wheel_options(&s.paths, next);
        let (x, y) = overlay::cursor_pos();
        s.hover = s.hit(x, y);
    });
    emit_state(app);
}

/// Plays the exit animation, then hides the window.
pub fn close(app: &AppHandle, reason: &str) {
    let generation = with(|s| {
        if !s.open || s.finishing {
            return None;
        }
        s.finishing = true;
        Some(s.generation)
    });
    let Some(generation) = generation else { return };
    log::debug!("wheel {generation} closing: {reason}");
    let _ = app.emit_to(
        "wheel",
        "wheel://close",
        serde_json::json!({ "reason": reason, "generation": generation }),
    );
    hide_later(app, generation, 320);
}

/// Closes a wheel that was opened by click (from the hub or "Open with") when the user
/// clicks elsewhere. Drag wheels never take activation, so they are unaffected.
pub fn close_if_click_mode(app: &AppHandle) {
    if with(|s| s.open && s.click_mode && !s.finishing) {
        close(app, "deactivated");
    }
}

fn hide_later(app: &AppHandle, generation: u64, ms: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        hide_now(&app, generation);
    });
}

/// Hides the window if it still belongs to `generation`.
pub fn hide_now(app: &AppHandle, generation: u64) {
    let should = with(|s| {
        if s.open && s.generation == generation && s.finishing {
            s.open = false;
            s.paths.clear();
            s.options.clear();
            true
        } else {
            false
        }
    });
    if should {
        if let Some(win) = app.get_webview_window("wheel") {
            if let Ok(hwnd) = win.hwnd() {
                overlay::hide(hwnd);
            }
        }
    }
}

/// Runs the action of segment `index` on the current files.
pub fn choose(app: &AppHandle, index: i32) {
    let chosen = with(|s| {
        if !s.open || s.finishing || index < 0 {
            return None;
        }
        let option = s.options.get(index as usize)?.clone();
        s.finishing = true;
        s.hover = index;
        Some((option.action, s.paths.clone(), s.generation))
    });
    let Some((action, paths, generation)) = chosen else { return };
    let _ = app.emit_to(
        "wheel",
        "wheel://chosen",
        serde_json::json!({ "index": index, "generation": generation }),
    );
    hide_later(app, generation, 560);
    crate::actions::start(app, paths, action);
}

/// Drag events from the wheel window's drop target.
pub fn on_drop_event(app: &AppHandle, evt: DropEvt) -> bool {
    match evt {
        DropEvt::Enter { paths, x, y, .. } => {
            let changed = with(|s| {
                if !s.open || s.finishing {
                    return false;
                }
                if s.paths != paths {
                    s.options = registry::wheel_options(&paths, s.mode());
                    s.paths = paths;
                }
                s.hover = s.hit(x, y);
                true
            });
            if changed {
                for p in with(|s| s.paths.clone()) {
                    let _ = app.asset_protocol_scope().allow_file(&p);
                }
                emit_state(app);
            }
            with(|s| s.hover >= 0)
        }
        DropEvt::Over { x, y, .. } => {
            let (changed, hover) = with(|s| {
                let h = if s.open && !s.finishing { s.hit(x, y) } else { -1 };
                let changed = h != s.hover;
                s.hover = h;
                (changed, h)
            });
            if changed {
                let _ = app.emit_to("wheel", "wheel://hover", hover);
            }
            hover >= 0
        }
        DropEvt::Leave => {
            // OLE also reports a leave when the pointer crosses between the window and its
            // WebView2 child. Only close once the pointer is really outside the wheel.
            let app = app.clone();
            let generation = with(|s| s.generation);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(90));
                let (x, y) = overlay::cursor_pos();
                let outside = with(|s| {
                    let (l, t, w, h) = s.rect;
                    s.generation == generation && (x < l || y < t || x >= l + w || y >= t + h)
                });
                if outside {
                    close(&app, "leave");
                }
            });
            false
        }
        DropEvt::Drop { x, y, .. } => {
            let index = with(|s| if s.open && !s.finishing { s.hit(x, y) } else { -1 });
            if index >= 0 {
                choose(app, index);
                true
            } else {
                close(app, "cancel");
                false
            }
        }
    }
}

/// Files dropped on the hub open the wheel at the drop point in click mode.
pub fn on_hub_drop(app: &AppHandle, evt: DropEvt) -> bool {
    match evt {
        DropEvt::Enter { .. } | DropEvt::Over { .. } => {
            let _ = app.emit_to("hub", "hub://drag", true);
            true
        }
        DropEvt::Leave => {
            let _ = app.emit_to("hub", "hub://drag", false);
            false
        }
        DropEvt::Drop { paths, x, y, .. } => {
            let _ = app.emit_to("hub", "hub://drag", false);
            let app = app.clone();
            // Let the OLE drop finish before the wheel takes over the screen.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                open(&app, x, y, Mode::Convert, true, paths);
            });
            true
        }
    }
}

/// Connects the global gesture to the wheel.
struct GesturePolicy {
    app: AppHandle,
}

impl gesture::Policy for GesturePolicy {
    fn enabled(&self) -> bool {
        !crate::PAUSED.load(std::sync::atomic::Ordering::Relaxed)
            && self.app.state::<crate::settings::SettingsStore>().get().gesture_enabled
    }

    fn any_app(&self) -> bool {
        self.app.state::<crate::settings::SettingsStore>().get().any_app
    }

    fn wheel_open(&self) -> bool {
        with(|s| s.open && !s.finishing)
    }

    fn signal(&self, s: Signal) {
        match s {
            Signal::Open { x, y, tools } => {
                let mode = if tools { Mode::Tools } else { Mode::Convert };
                open(&self.app, x, y, mode, false, Vec::new());
            }
            Signal::ToggleMode => toggle_mode(&self.app),
            Signal::DragEnded => {
                // The drop (if any) is delivered right after the button is released. Give it
                // a moment before treating the release as a cancel.
                let app = self.app.clone();
                let generation = with(|s| s.generation);
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(180));
                    if with(|s| s.open && !s.finishing && !s.click_mode && s.generation == generation) {
                        close(&app, "release");
                    }
                });
            }
        }
    }
}

pub fn start_gesture(app: &AppHandle) {
    gesture::start(GesturePolicy { app: app.clone() });
}

#[cfg(test)]
mod tests {
    use super::segment_at;

    #[test]
    fn top_is_segment_zero_and_indices_go_clockwise() {
        assert_eq!(segment_at(0.0, -120.0, 8), 0);
        assert_eq!(segment_at(120.0, 0.0, 8), 2);
        assert_eq!(segment_at(0.0, 120.0, 8), 4);
        assert_eq!(segment_at(-120.0, 0.0, 8), 6);
        // Just left of top belongs to the top segment for 8 segments (22.5 degrees each side).
        assert_eq!(segment_at(-30.0, -120.0, 8), 0);
    }

    #[test]
    fn center_and_outside_are_not_segments() {
        assert_eq!(segment_at(10.0, 10.0, 8), -1);
        assert_eq!(segment_at(0.0, -260.0, 8), -1);
        assert_eq!(segment_at(0.0, -120.0, 0), -1);
    }
}
