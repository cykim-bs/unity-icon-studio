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

/// Opens a folder or file in Explorer, only inside the tool's home, the work and export folders
/// and the projects on the list.
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
    // the projects on the list (Explorer only shows them; nothing is written)
    roots.extend(s.projects.iter().map(|p| PathBuf::from(&p.path)));
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

// ---------------------------------------------------------------- projects and prefabs

use std::sync::atomic::AtomicBool;
use uis_core::prefabs::{ScanOptions, ScanResult};
use uis_core::projects::{self, Override, ProjectSummary};

/// One scan at a time (a second press while one runs gets `busy`).
static SCANNING: AtomicBool = AtomicBool::new(false);

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn project_list() -> Result<Vec<ProjectSummary>, String> {
    blocking(|| Ok(projects::list(&home()))).await
}

/// Adds the Unity project at `path` (or the one it is inside). Same folder again: the same entry.
#[tauri::command]
pub async fn project_add(path: String) -> Result<ProjectSummary, String> {
    blocking(move || projects::add(&home(), Path::new(&path))).await
}

/// The folder picker, then `project_add`. None when the person cancelled.
#[tauri::command]
pub async fn project_pick(app: tauri::AppHandle) -> Result<Option<ProjectSummary>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    let Some(folder) = picked else { return Ok(None) };
    let path = folder.into_path().map_err(|e| format!("path:{e}"))?;
    blocking(move || projects::add(&home(), &path).map(Some)).await
}

#[tauri::command]
pub fn project_remove(id: String) -> Result<(), String> {
    projects::remove(&home(), &id)
}

/// How a capture would run now (lock file, version, packages, branch).
#[tauri::command]
pub async fn project_probe(id: String) -> Result<ProjectSummary, String> {
    blocking(move || {
        let home = home();
        let entry = projects::entry(&home, &id)?;
        Ok(projects::summary(&home, &entry))
    })
    .await
}

/// Unity Hub's project list (read only).
#[tauri::command]
pub async fn hub_projects() -> Result<Vec<uis_core::project::HubProject>, String> {
    blocking(|| Ok(uis_core::project::hub_projects())).await
}

#[tauri::command]
pub async fn prefab_scan(id: String, force: bool, include_models: bool) -> Result<ScanResult, String> {
    let Some(_guard) = uis_core::proc::Busy::try_take(&SCANNING) else {
        return Err("busy".into());
    };
    blocking(move || {
        let home = home();
        let entry = projects::entry(&home, &id)?;
        let pack = uis_core::packs::load(&home, &entry.pack_id).map_err(|e| format!("pack:{e}"))?;
        let options = ScanOptions { force, include_models, kind_overrides: projects::kind_overrides(&home, &id) };
        uis_core::prefabs::scan(&home, Path::new(&entry.path), &pack, &options)
    })
    .await
}

/// A person's choice for one prefab (kind, side, rotation, front axis); `null` clears a key.
#[tauri::command]
pub fn prefab_override(id: String, guid: String, patch: Map<String, Value>) -> Result<Override, String> {
    projects::set_override(&home(), &id, &guid, &patch)
}

#[tauri::command]
pub fn pack_list() -> Vec<uis_core::packs::PackSummary> {
    uis_core::packs::list(&home())
}

/// A pack after `extends` (read only).
#[tauri::command]
pub fn pack_get(id: String) -> Result<Value, String> {
    uis_core::packs::load(&home(), &id).map(|p| p.doc)
}

// ---------------------------------------------------------------- capture and recovery

use uis_core::capture::{BatchFinish, CaptureEvent, CaptureReport, CaptureRequest, RecoveryEntry};

/// One capture at a time (Unity takes one request per project anyway).
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Captures `items` (asset paths) of a listed project: in its open editor or in batch mode.
/// `purpose` is `capture` or `thumb`. Steps come through `on_event`.
#[tauri::command]
pub async fn capture_only(
    id: String,
    items: Vec<String>,
    purpose: Option<String>,
    on_event: tauri::ipc::Channel<CaptureEvent>,
) -> Result<CaptureReport, String> {
    let Some(_guard) = uis_core::proc::Busy::try_take(&CAPTURING) else {
        return Err("busy".into());
    };
    blocking(move || {
        let home = home();
        let entry = projects::entry(&home, &id)?;
        let req = CaptureRequest {
            project: PathBuf::from(&entry.path),
            items,
            pack_id: Some(entry.pack_id.clone()),
            purpose: purpose.unwrap_or_else(|| "capture".into()),
            ..CaptureRequest::default()
        };
        Ok(uis_core::capture::capture(&home, &req, &mut |e| {
            let _ = on_event.send(e.clone());
        }))
    })
    .await
}

/// A PNG the tool wrote (a capture or a thumbnail) as a `data:` URL, only from the tool's home
/// and work folder.
#[tauri::command]
pub async fn capture_image(path: String) -> Result<String, String> {
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let p = Path::new(&path);
        let work = s.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
        if !(is_inside(p, home.root()) || is_inside(p, &work)) {
            return Err("outside".into());
        }
        uis_core::image::png_data_url(p)
    })
    .await
}

/// Batch captures left by an earlier run (Unity still running or not cleaned up yet).
#[tauri::command]
pub async fn recovery_status() -> Result<Vec<RecoveryEntry>, String> {
    blocking(|| Ok(uis_core::capture::recovery_status(&home()))).await
}

/// Finishes the left batch captures whose Unity has ended.
#[tauri::command]
pub async fn recovery_run() -> Result<Vec<BatchFinish>, String> {
    let Some(_guard) = uis_core::proc::Busy::try_take(&CAPTURING) else {
        return Err("busy".into());
    };
    blocking(|| Ok(uis_core::capture::recover(&home()))).await
}
