//! KiwiConvert: convert files where you already are. Hold Shift while dragging a file in
//! File Explorer and a wheel of formats appears around the pointer.

mod actions;
mod commands;
mod convert;
mod engines;
mod jobs;
mod logger;
mod naming;
mod platform;
mod registry;
mod settings;
mod tools;
mod tray;
mod ui;
mod wheel;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use tauri::Manager;

/// Set from the tray menu: the gesture is ignored while paused.
pub static PAUSED: AtomicBool = AtomicBool::new(false);

/// Files passed on the command line (for example from "Open with") open the wheel.
fn file_args(args: &[String]) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

fn handle_launch(app: &tauri::AppHandle, args: &[String]) {
    let files = file_args(args);
    if files.is_empty() {
        ui::show_hub(app, None);
    } else {
        let (x, y) = platform::overlay::cursor_pos();
        for f in &files {
            let _ = app.asset_protocol_scope().allow_file(f);
        }
        wheel::open(app, x, y, registry::Mode::Convert, true, files);
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| handle_launch(app, &args)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(jobs::JobManager::new())
        .setup(|app| {
            let handle = app.handle().clone();
            logger::init(handle.path().app_log_dir().ok());
            log::info!("KiwiConvert {} starting", handle.package_info().version);
            app.manage(settings::SettingsStore::load(&handle));
            handle.state::<jobs::JobManager>().load_history(&handle);

            // The engines load in the background so the tray icon appears immediately.
            let resources = handle.path().resource_dir().ok();
            std::thread::spawn(move || {
                engines::ffmpeg::init(resources.clone());
                engines::pdf::init(resources);
            });

            ui::create_core_windows(&handle)?;
            tray::create(&handle)?;
            wheel::start_gesture(&handle);

            let args: Vec<String> = std::env::args().collect();
            let background = args.iter().any(|a| a == "--background");
            let first_run = !handle.state::<settings::SettingsStore>().get().onboarded;
            if !background || first_run {
                handle_launch(&handle, &args);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::app_info,
            commands::set_paused,
            commands::quit,
            commands::wheel_snapshot,
            commands::wheel_choose,
            commands::wheel_close,
            commands::wheel_hidden,
            commands::wheel_toggle_mode,
            commands::open_wheel,
            commands::wheel_options,
            commands::thumbnail,
            commands::jobs_active,
            commands::jobs_history,
            commands::job_cancel,
            commands::job_dismiss,
            commands::history_clear,
            commands::run_convert,
            commands::run_tool,
            commands::open_tool,
            commands::activity_regions,
            commands::activity_hide,
            commands::hub_hide,
            commands::window_ready,
            commands::tool_session,
            commands::tool_closed,
            commands::reveal,
            commands::open_file,
            commands::open_link,
            commands::allow_file,
            commands::media_info,
            commands::video_frame,
            commands::video_strip,
            commands::audio_peaks,
            commands::preview_proxy,
            commands::image_preview,
            commands::pdf_pages,
            commands::pdf_thumb,
            commands::metadata_read,
            commands::compress_estimate,
            commands::file_size,
            commands::save_rendered_image,
        ])
        .build(tauri::generate_context!())
        .expect("error while building KiwiConvert");

    app.run(|_app, event| {
        // KiwiConvert lives in the tray: closing windows must not quit it.
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
