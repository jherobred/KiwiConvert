//! Routes a chosen wheel action to a background job or a tool window.

use crate::registry::{Action, Fmt, Kind, Tool, kind_of};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub fn start(app: &AppHandle, paths: Vec<PathBuf>, action: Action) {
    if paths.is_empty() {
        return;
    }
    match action {
        Action::Convert { to } => {
            start_convert(app, paths, to);
        }
        Action::Tool { tool } if tool.is_instant() => {
            start_tool(app, tool, paths, serde_json::Value::Null);
        }
        Action::Tool { tool } => crate::ui::open_tool(app, tool, paths),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_heavy(paths: &[PathBuf], to: Option<Fmt>) -> bool {
    paths
        .iter()
        .any(|p| matches!(kind_of(p), Kind::Video | Kind::Audio))
        || matches!(to, Some(Fmt::Heic | Fmt::Avif | Fmt::Mp4 | Fmt::Webm))
}

pub fn start_convert(app: &AppHandle, paths: Vec<PathBuf>, to: Fmt) -> String {
    let title = if paths.len() == 1 {
        file_name(&paths[0])
    } else {
        format!("{} files", paths.len())
    };
    let detail = match to {
        Fmt::Extract => "Extracting".to_string(),
        _ => format!("Converting to {}", to.label()),
    };
    let first = paths.first().cloned();
    let heavy = is_heavy(&paths, Some(to));
    let manager = app.state::<crate::jobs::JobManager>();
    manager.spawn(app, title, detail, first, heavy, move |ctx| {
        crate::convert::run(ctx, &paths, to)
    })
}

pub fn start_tool(
    app: &AppHandle,
    tool: Tool,
    paths: Vec<PathBuf>,
    options: serde_json::Value,
) -> String {
    let title = if paths.len() == 1 {
        file_name(&paths[0])
    } else {
        format!("{} files", paths.len())
    };
    let detail = crate::tools::describe(tool, &options);
    let first = paths.first().cloned();
    let heavy = is_heavy(&paths, None);
    let manager = app.state::<crate::jobs::JobManager>();
    manager.spawn(app, title, detail, first, heavy, move |ctx| {
        crate::tools::run(ctx, tool, &paths, &options)
    })
}
