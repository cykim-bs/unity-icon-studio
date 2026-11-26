//! Tauri commands. Each one calls uis-core; long work runs on a blocking thread so the window
//! stays responsive. Errors are short codes (`read:…`, `write:…`, `outside`) the page turns into text.

use serde::Serialize;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use tauri_plugin_opener::OpenerExt;
use uis_core::home::Home;
use uis_core::settings::{self, Settings};

fn home() -> Home {
    Home::resolve()
}

/// What the settings About tab and the status bar show.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    core_version: String,
    home: String,
    home_override: bool,
    settings_file: String,
    settings_problem: Option<String>,
    dev: bool,
}

#[tauri::command]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    let home = home();
    AppInfo {
        version: app.package_info().version.to_string(),
        core_version: uis_core::VERSION.into(),
        home: home.root().display().to_string(),
        home_override: Home::is_override(),
        settings_file: home.settings_file().display().to_string(),
        settings_problem: settings::file_problem(&home),
        dev: cfg!(debug_assertions),
    }
}

#[tauri::command]
pub fn settings_get() -> Settings {
    settings::load(&home())
}

/// Stores only the keys in `patch` (top-level names or dotted paths) and returns the settings now
/// in effect.
#[tauri::command]
pub fn settings_set(patch: Map<String, Value>) -> Result<Settings, String> {
    settings::update(&home(), &patch)
}

/// Back to the defaults for `keys` (dotted paths), or for everything but the project list.
#[tauri::command]
pub fn settings_reset(keys: Vec<String>) -> Result<Settings, String> {
    settings::reset(&home(), &keys)
}

/// True when `path` is inside `root` (both made absolute; case-insensitive on Windows).
fn is_inside(path: &Path, root: &Path) -> bool {
    let norm = |p: &Path| -> String {
        let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let text = abs.to_string_lossy().replace('/', "\\");
        let text = text.trim_start_matches(r"\\?\").trim_end_matches('\\').to_string();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    let (p, r) = (norm(path), norm(root));
    p == r || p.starts_with(&(r + "\\"))
}

/// Opens a folder or file in Explorer, only inside the tool's home or the export folder.
#[tauri::command]
pub fn open_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let home = home();
    let target = PathBuf::from(&path);
    let s = settings::load(&home);
    let mut roots = vec![home.root().to_path_buf()];
    if let Some(dir) = &s.export_dir {
        roots.push(PathBuf::from(dir));
    }
    if let Some(dir) = &s.work_dir {
        roots.push(PathBuf::from(dir));
    }
    if !roots.iter().any(|r| is_inside(&target, r)) {
        return Err("outside".into());
    }
    if !target.exists() {
        return Err("missing".into());
    }
    app.opener().open_path(path, None::<&str>).map_err(|e| format!("open:{e}"))
}

/// The page has painted once: show the window. Later calls (a page reload) do nothing.
#[tauri::command]
pub fn window_ready(app: tauri::AppHandle) {
    crate::show_first(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_checks_whole_folder_names() {
        assert!(is_inside(Path::new("C:/fake/a/b/c.png"), Path::new("C:/fake/a/b")));
        assert!(is_inside(Path::new("c:/FAKE/A/B"), Path::new("C:/fake/a/b/")));
        assert!(!is_inside(Path::new("C:/fake/a/bc/d"), Path::new("C:/fake/a/b")));
        assert!(!is_inside(Path::new("C:/fake/other"), Path::new("C:/fake/a")));
    }
}
