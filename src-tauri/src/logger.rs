//! A small file logger. Logs stay on this PC and help when reporting a bug.

use parking_lot::Mutex;
use std::io::Write;
use std::path::PathBuf;

struct FileLogger {
    file: Mutex<Option<std::fs::File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info || cfg!(debug_assertions)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) || !record.target().starts_with("kiwiconvert") {
            return;
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("{secs} {:<5} {}\n", record.level(), record.args());
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.flush();
        }
    }
}

/// Logs to `dir/kiwiconvert.log`, starting fresh once the file passes 1 MB.
pub fn init(dir: Option<PathBuf>) {
    let file = dir.and_then(|d| {
        std::fs::create_dir_all(&d).ok()?;
        let path = d.join("kiwiconvert.log");
        if std::fs::metadata(&path).map(|m| m.len() > 1_048_576).unwrap_or(false) {
            let _ = std::fs::rename(&path, d.join("kiwiconvert.old.log"));
        }
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
    });
    let logger = Box::leak(Box::new(FileLogger { file: Mutex::new(file) }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info });
    }
}
