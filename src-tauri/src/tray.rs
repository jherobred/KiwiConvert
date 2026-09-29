//! The notification area icon. Left click opens the hub; right click shows the menu.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open KiwiConvert", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause the Shift gesture", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit KiwiConvert", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &settings, &PredefinedMenuItem::separator(app)?, &quit])?;

    let pause_item = pause.clone();
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("the app has an icon"))
        .tooltip("KiwiConvert: hold Shift while dragging a file")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => crate::ui::show_hub(app, None),
            "pause" => {
                let paused = pause_item.is_checked().unwrap_or(false);
                crate::PAUSED.store(paused, std::sync::atomic::Ordering::Relaxed);
                let _ = app.emit("app://paused", paused);
            }
            "settings" => {
                crate::ui::show_hub(app, None);
                let _ = app.emit_to("hub", "hub://settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                let pos = rect.position.to_physical::<i32>(1.0);
                let size = rect.size.to_physical::<i32>(1.0);
                crate::ui::toggle_hub(tray.app_handle(), Some((pos.x + size.width / 2, pos.y)));
            }
        })
        .build(app)?;
    Ok(())
}
