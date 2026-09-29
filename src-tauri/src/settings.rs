//! User preferences, stored as JSON in the app config folder on this PC.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Show the wheel when Shift is pressed during a file drag.
    pub gesture_enabled: bool,
    /// Allow the gesture when dragging from any app, not just File Explorer and the desktop.
    pub any_app: bool,
    /// Open File Explorer on the finished files.
    pub reveal_outputs: bool,
    /// Save outputs here instead of beside the original.
    pub output_folder: Option<String>,
    pub jpeg_quality: u8,
    pub webp_quality: u8,
    pub avif_quality: u8,
    pub heic_quality: u8,
    /// Copy EXIF and other metadata into converted images.
    pub keep_metadata: bool,
    /// Resolution used when turning PDF pages into images.
    pub pdf_dpi: u32,
    /// "a4", "letter", or "fit" (page matches the image).
    pub page_size: String,
    /// "fast", "balanced", or "quality".
    pub video_preset: String,
    /// Use the graphics card's video encoder when one is available.
    pub gpu_encoding: bool,
    /// "system", "light", or "dark".
    pub theme: String,
    pub reduce_motion: bool,
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gesture_enabled: true,
            any_app: false,
            reveal_outputs: true,
            output_folder: None,
            jpeg_quality: 90,
            webp_quality: 85,
            avif_quality: 70,
            heic_quality: 80,
            keep_metadata: true,
            pdf_dpi: 150,
            page_size: "a4".into(),
            video_preset: "balanced".into(),
            gpu_encoding: false,
            theme: "system".into(),
            reduce_motion: false,
            onboarded: false,
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    inner: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(app: &AppHandle) -> Self {
        let dir = app
            .path()
            .app_config_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("KiwiConvert"));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        let inner = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            inner: RwLock::new(inner),
        }
    }

    pub fn get(&self) -> Settings {
        self.inner.read().clone()
    }

    pub fn set(&self, app: &AppHandle, next: Settings) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&next)?;
        std::fs::write(&self.path, json)?;
        *self.inner.write() = next.clone();
        let _ = app.emit("settings://changed", next);
        Ok(())
    }
}
