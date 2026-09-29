//! Detects "Shift while dragging a file" anywhere on screen.
//!
//! A low-level mouse hook reports button and move events. The hook itself only forwards
//! events to a channel, so it never slows the system mouse. A controller thread turns them
//! into drag state and, while a drag is in progress, polls the modifier keys.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GA_ROOT, GetAncestor, GetClassNameW, GetMessageW, GetWindowThreadProcessId,
    MSG, MSLLHOOKSTRUCT, SetWindowsHookExW, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WindowFromPoint,
};

#[derive(Debug, Clone, Copy)]
enum MouseMsg {
    Down(i32, i32),
    Move(i32, i32),
    Up,
}

static TX: OnceLock<Sender<MouseMsg>> = OnceLock::new();
static BUTTON_DOWN: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let (x, y) = (info.pt.x, info.pt.y);
        let msg = match wparam.0 as u32 {
            WM_LBUTTONDOWN => {
                BUTTON_DOWN.store(true, Ordering::Relaxed);
                Some(MouseMsg::Down(x, y))
            }
            WM_LBUTTONUP => {
                BUTTON_DOWN.store(false, Ordering::Relaxed);
                Some(MouseMsg::Up)
            }
            // Moves only matter while the button is held.
            WM_MOUSEMOVE if BUTTON_DOWN.load(Ordering::Relaxed) => Some(MouseMsg::Move(x, y)),
            _ => None,
        };
        if let (Some(m), Some(tx)) = (msg, TX.get()) {
            let _ = tx.send(m);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// What the gesture asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Open the wheel at the cursor. `tools` is true when Ctrl was also held.
    Open { x: i32, y: i32, tools: bool },
    /// Ctrl was tapped while the wheel was open.
    ToggleMode,
    /// The mouse button was released or Esc was pressed.
    DragEnded,
}

/// Decides, per drag, whether the gesture may trigger. Called with the screen point where
/// the drag started.
pub trait Policy: Send + 'static {
    fn enabled(&self) -> bool;
    fn any_app(&self) -> bool;
    fn wheel_open(&self) -> bool;
    fn signal(&self, s: Signal);
}

fn key_down(vk: i32) -> bool {
    let state = unsafe { GetAsyncKeyState(vk) };
    state < 0
}

fn class_name(hwnd: windows::Win32::Foundation::HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// True when the drag started in File Explorer, on the desktop, or in a file dialog.
fn started_in_explorer(x: i32, y: i32) -> bool {
    let hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
    if hwnd.is_invalid() {
        return false;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    matches!(
        class_name(root).as_str(),
        "CabinetWClass" | "ExploreWClass" | "Progman" | "WorkerW" | "#32770"
    )
}

fn started_in_this_app(x: i32, y: i32) -> bool {
    let hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == unsafe { GetCurrentProcessId() }
}

/// Starts the hook thread and the controller thread.
pub fn start<P: Policy>(policy: P) {
    let (tx, rx) = channel();
    if TX.set(tx).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("gesture-hook".into())
        .spawn(|| unsafe {
            match SetWindowsHookExW(WH_MOUSE_LL, Some(hook), None, 0) {
                Ok(_hook) => {
                    let mut msg = MSG::default();
                    // The hook is serviced by this thread's message loop.
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                }
                Err(e) => log::error!("could not install the mouse hook: {e}"),
            }
        })
        .expect("failed to start the gesture hook thread");

    std::thread::Builder::new()
        .name("gesture".into())
        .spawn(move || run(rx, policy))
        .expect("failed to start the gesture thread");
}

#[derive(Debug)]
enum State {
    Idle,
    Pressed { x: i32, y: i32, eligible: bool },
    Dragging {
        eligible: bool,
        /// Shift has triggered the wheel during this press of Shift already.
        consumed: bool,
        ctrl_was_down: bool,
        opened_at: Option<Instant>,
    },
}

const DRAG_THRESHOLD: i32 = 6;

fn run<P: Policy>(rx: Receiver<MouseMsg>, policy: P) {
    let mut state = State::Idle;
    loop {
        let msg = match state {
            State::Dragging { .. } => match rx.recv_timeout(Duration::from_millis(15)) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            _ => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            },
        };

        match (msg, &mut state) {
            (Some(MouseMsg::Down(x, y)), _) => {
                let eligible = policy.enabled()
                    && !started_in_this_app(x, y)
                    && (policy.any_app() || started_in_explorer(x, y));
                state = State::Pressed { x, y, eligible };
            }
            (Some(MouseMsg::Move(mx, my)), State::Pressed { x, y, eligible }) => {
                if (mx - *x).abs() > DRAG_THRESHOLD || (my - *y).abs() > DRAG_THRESHOLD {
                    state = State::Dragging {
                        eligible: *eligible,
                        consumed: false,
                        ctrl_was_down: key_down(VK_CONTROL.0 as i32),
                        opened_at: None,
                    };
                }
            }
            (Some(MouseMsg::Up), State::Dragging { opened_at, .. }) => {
                if opened_at.is_some() || policy.wheel_open() {
                    policy.signal(Signal::DragEnded);
                }
                state = State::Idle;
            }
            (Some(MouseMsg::Up), _) => state = State::Idle,
            _ => {}
        }

        if let State::Dragging {
            eligible: true,
            consumed,
            ctrl_was_down,
            opened_at,
        } = &mut state
        {
            let shift = key_down(VK_SHIFT.0 as i32);
            let ctrl = key_down(VK_CONTROL.0 as i32);
            if !shift {
                *consumed = false;
            }
            if shift && !*consumed && !policy.wheel_open() {
                let (x, y) = super::overlay::cursor_pos();
                *consumed = true;
                *opened_at = Some(Instant::now());
                policy.signal(Signal::Open { x, y, tools: ctrl });
            } else if opened_at.is_some() && policy.wheel_open() {
                if ctrl && !*ctrl_was_down {
                    policy.signal(Signal::ToggleMode);
                }
                if key_down(VK_ESCAPE.0 as i32) {
                    policy.signal(Signal::DragEnded);
                    *opened_at = None;
                }
            }
            *ctrl_was_down = ctrl;
        }
    }
}
