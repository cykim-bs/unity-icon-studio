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

/// The camera of one prefab: the kind's preset, its rule's values, what a person changed and the
/// values a capture uses now (the same merge as the capture job).
#[tauri::command]
pub fn prefab_camera(id: String, kind: String, rule: Option<String>, guid: Option<String>) -> Result<Value, String> {
    let home = home();
    let entry = projects::entry(&home, &id)?;
    let pack = uis_core::packs::load(&home, &entry.pack_id).map_err(|e| format!("pack:{e}"))?;
    let rules = pack.kind_rules();
    let rule_extra = rule.as_ref().and_then(|p| rules.iter().find(|r| &r.pattern == p)).map(|r| r.extra.clone());
    let person = guid
        .as_ref()
        .and_then(|g| projects::load_state(&home, &id).overrides.get(&g.to_ascii_lowercase()).cloned());
    let preset = uis_core::capture::item_values(&pack, &kind, None, None);
    let with_rule = uis_core::capture::item_values(&pack, &kind, rule_extra.as_ref(), None);
    let effective = uis_core::capture::item_values(&pack, &kind, rule_extra.as_ref(), person.as_ref());
    Ok(serde_json::json!({ "preset": preset, "rule": with_rule, "person": person, "effective": effective }))
}

/// The style pack a listed project uses.
#[tauri::command]
pub fn project_set_pack(id: String, pack_id: String) -> Result<ProjectSummary, String> {
    projects::set_pack(&home(), &id, &pack_id)
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

/// The camera values a capture picture was taken with (`job-in.json` next to it), for the
/// prompt shown on screen: the sentence must say what the picture shows. Only in the tool's
/// home and work folder; None for a picture without a capture job.
#[tauri::command]
pub async fn capture_values(path: String) -> Result<Option<Map<String, Value>>, String> {
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let p = Path::new(&path);
        let work = s.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
        if !(is_inside(p, home.root()) || is_inside(p, &work)) {
            return Err("outside".into());
        }
        Ok(uis_core::capture::values_of_picture(p))
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

// ---------------------------------------------------------------- prompt, CLIs, raw pictures

use std::sync::Mutex;
use uis_core::cli::codex::CopyInfo;
use uis_core::cli::{Readiness, RewriteReport};
use uis_core::generate::{GenerateReport, GenerateRequest};
use uis_core::prompt::Assembled;

/// One LLM job at a time (a rewrite, a picture or a review).
static LLM_BUSY: AtomicBool = AtomicBool::new(false);
static COPYING: AtomicBool = AtomicBool::new(false);
/// The last readiness check (the screen asks often; the CLIs are asked again on `refresh`).
static READY: Mutex<Option<Readiness>> = Mutex::new(None);

/// Codex, Claude and the unity CLI: found, version, signed in, image runner, the desktop app.
#[tauri::command]
pub async fn readiness(refresh: bool) -> Result<Readiness, String> {
    if !refresh {
        if let Some(r) = READY.lock().map_err(|e| e.to_string())?.clone() {
            return Ok(r);
        }
    }
    let r = blocking(|| {
        let home = home();
        Ok(uis_core::cli::readiness(&home, &settings::load(&home)))
    })
    .await?;
    *READY.lock().map_err(|e| e.to_string())? = Some(r.clone());
    Ok(r)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyProgress {
    done: u64,
    total: u64,
}

/// Copies the Codex desktop app's `codex*.exe` into the tool's folder (about 0.5 GB, no download).
#[tauri::command]
pub async fn codex_copy_from_app(on_event: tauri::ipc::Channel<CopyProgress>) -> Result<CopyInfo, String> {
    let Some(_guard) = uis_core::proc::Busy::try_take(&COPYING) else {
        return Err("busy".into());
    };
    let info = blocking(move || {
        let home = home();
        let mut last = 0u64;
        uis_core::cli::codex::copy_from_app(&home, &mut |done, total| {
            if done == total || done >= last + (16 << 20) {
                last = done;
                let _ = on_event.send(CopyProgress { done, total });
            }
        })
    })
    .await?;
    if let Ok(mut r) = READY.lock() {
        *r = None;
    }
    Ok(info)
}

/// `camera`: the item's capture values (`yaw`, `pitch`, `projection`, `fov`), so the CAMERA
/// sentence shown is the one a capture with those values gets.
fn assemble_for(
    home: &Home,
    pack_id: &str,
    kind: &str,
    yaw: Option<f64>,
    pitch: Option<f64>,
    camera: Option<&Map<String, Value>>,
) -> Result<(uis_core::packs::Pack, Assembled), String> {
    let s = settings::load(home);
    let pack = uis_core::packs::load(home, pack_id).map_err(|e| format!("pack:{e}"))?;
    let refs = if s.generate.style_refs { uis_core::generate::pack_refs(&pack, None).len() } else { 0 };
    let mut ctx = uis_core::generate::context(&pack, &s, kind, refs, yaw, pitch);
    if let Some(c) = camera {
        ctx = ctx.with_capture(c);
    }
    let fixed = uis_core::prompt::assemble(&pack, &ctx)?;
    Ok((pack, fixed))
}

/// The fixed prompt for a kind (no LLM).
#[tauri::command]
pub async fn prompt_preview(pack_id: String, kind: String, yaw: Option<f64>, pitch: Option<f64>, camera: Option<Map<String, Value>>) -> Result<Assembled, String> {
    blocking(move || assemble_for(&home(), &pack_id, &kind, yaw, pitch, camera.as_ref()).map(|(_, a)| a)).await
}

/// The note worked into the fixed prompt (an empty note calls nothing). No picture is made.
#[tauri::command]
pub async fn prompt_rewrite(
    pack_id: String,
    kind: String,
    note: String,
    cli: Option<String>,
    yaw: Option<f64>,
    pitch: Option<f64>,
    camera: Option<Map<String, Value>>,
) -> Result<RewriteReport, String> {
    let Some(_guard) = uis_core::proc::Busy::try_take(&LLM_BUSY) else {
        return Err("busy".into());
    };
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let (pack, fixed) = assemble_for(&home, &pack_id, &kind, yaw, pitch, camera.as_ref())?;
        let cli = cli.unwrap_or_else(|| s.llm.rewriter.clone());
        let clis = uis_core::cli::Clis { home: &home, settings: &s };
        Ok(uis_core::cli::rewrite(&pack, &fixed, &note, &cli, &clis))
    })
    .await
}

/// Raw pictures from a capture: prompt (+ rewrite) and Codex. The capture must be in the tool's
/// home or work folder.
#[tauri::command]
pub async fn generate_raw(
    pack_id: String,
    kind: String,
    capture: String,
    name: String,
    note: String,
    cli: Option<String>,
) -> Result<GenerateReport, String> {
    let Some(_guard) = uis_core::proc::Busy::try_take(&LLM_BUSY) else {
        return Err("busy".into());
    };
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let work = s.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
        let input = PathBuf::from(&capture);
        if !(is_inside(&input, home.root()) || is_inside(&input, &work)) {
            return Err("outside".into());
        }
        let req = GenerateRequest {
            input,
            kind,
            pack_id: Some(pack_id),
            name: Some(name),
            note,
            rewriter: cli,
            ..GenerateRequest::default()
        };
        Ok(uis_core::generate::generate(&home, &s, &req))
    })
    .await
}

/// An LLM's pass or fail for a candidate against its capture (`{pass, issues}`).
#[tauri::command]
pub async fn review(capture: String, candidate: String, kind: String, cli: Option<String>) -> Result<Value, String> {
    let Some(_guard) = uis_core::proc::Busy::try_take(&LLM_BUSY) else {
        return Err("busy".into());
    };
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let work = s.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
        let (a, b) = (PathBuf::from(&capture), PathBuf::from(&candidate));
        for p in [&a, &b] {
            if !(is_inside(p, home.root()) || is_inside(p, &work)) {
                return Err("outside".into());
            }
        }
        let cli = cli.unwrap_or_else(|| if s.llm.review == "codex" { "codex".into() } else { "claude".into() });
        let (result, call) = if cli == "codex" {
            uis_core::cli::codex::review(&home, &s, &a, &b, &uis_core::cli::claude::review_prompt(&kind, false))
        } else {
            uis_core::cli::claude::review(&home, &s, &a, &b, &uis_core::cli::claude::review_prompt(&kind, true))
        };
        Ok(match result {
            Ok(v) => serde_json::json!({ "ok": true, "review": v, "call": call }),
            Err(f) => serde_json::json!({ "ok": false, "failure": f, "call": call }),
        })
    })
    .await
}

/// Pictures the tool asked Codex for today (status bar).
#[tauri::command]
pub fn images_today() -> usize {
    uis_core::generate::images_today(&home())
}

/// The job folder must be inside the tool's home or work folder.
fn job_folder(home: &Home, s: &Settings, job: &str) -> Result<PathBuf, String> {
    let work = s.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
    let dir = PathBuf::from(job);
    if !(is_inside(&dir, home.root()) || is_inside(&dir, &work)) {
        return Err("outside".into());
    }
    if !dir.join("job.json").is_file() {
        return Err("noJob".into());
    }
    Ok(dir)
}

/// Post-processing of a candidate again with other options (no new picture).
#[tauri::command]
pub async fn post_rerun(job: String, cand: u32, options: Map<String, Value>) -> Result<uis_core::jobs::PostJobReport, String> {
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let dir = job_folder(&home, &s, &job)?;
        uis_core::jobs::post_candidate(&home, &s, &dir, cand.max(1), &Value::Object(options))
    })
    .await
}

/// Copies the finished icons of a candidate to the export folder (never into a Unity project).
#[tauri::command]
pub async fn export(
    job: String,
    cand: u32,
    sizes: Vec<u32>,
    name: String,
    folder: Option<String>,
    rule: Option<String>,
) -> Result<uis_core::export::ExportReport, String> {
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let dir = job_folder(&home, &s, &job)?;
        let folder = folder.filter(|f| !f.trim().is_empty()).or_else(|| s.export_dir.clone()).ok_or("noExportDir")?;
        let post = dir.join("post").join(format!("cand-{:02}", cand.max(1)));
        // The 1024 picture goes along when the settings say so (like the command line)
        let sizes = uis_core::export::sizes_with_original(&sizes, &s);
        let mut icons = Vec::new();
        for size in sizes {
            let f = if size == 1024 { post.join("master-1024.png") } else { post.join(format!("icon-{size}.png")) };
            if !f.is_file() {
                return Err(format!("noIcon:{size}"));
            }
            icons.push((size, f));
        }
        // A file name typed on the screen replaces the settings' rule for this export
        let rule = rule.filter(|r| !r.trim().is_empty()).unwrap_or_else(|| s.export_name.clone());
        Ok(uis_core::export::export(&icons, Path::new(&folder), &rule, &name, &s, Some(&dir)))
    })
    .await
}

/// Recent job folders (newest first).
#[tauri::command]
pub async fn jobs_list(limit: Option<usize>) -> Result<Vec<Value>, String> {
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        Ok(uis_core::jobs::list(&home, &s, limit.unwrap_or(50)))
    })
    .await
}

// ---------------------------------------------------------------- whole runs, jobs, thumbnails

use std::sync::Arc;
use uis_core::pipeline::{RunEvent, RunItemRequest, RunReport, RunRequest};

/// The stop flag of the run in progress (one at a time).
static RUN_STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    id: String,
    items: Vec<RunItemRequest>,
    #[serde(default)]
    note: String,
    cli: Option<String>,
    candidates: Option<u32>,
    outline: Option<String>,
    background: Option<String>,
    #[serde(default)]
    outline_off_ok: bool,
    #[serde(default)]
    no_refs: bool,
    /// Export passing items here as they finish
    export_dir: Option<String>,
    /// `strict` or `stylized` (None: the pack's, else the settings')
    #[serde(default)]
    fidelity: Option<String>,
}

/// Clears the stop flag when the run ends, however it ends.
struct RunSlot;
impl Drop for RunSlot {
    fn drop(&mut self) {
        if let Ok(mut s) = RUN_STOP.lock() {
            *s = None;
        }
    }
}

/// A whole run for the picked prefabs: capture (the ones without a picture, in one request),
/// then for each in turn prompt → rewrite → picture → post-processing → checks. Steps come
/// through `on_event`. Earlier captures in `items[].capture` must be in the tool's folders.
#[tauri::command]
pub async fn run_start(args: RunArgs, on_event: tauri::ipc::Channel<RunEvent>) -> Result<RunReport, String> {
    let Some(cap_guard) = uis_core::proc::Busy::try_take(&CAPTURING) else {
        return Err("busy".into());
    };
    let Some(llm_guard) = uis_core::proc::Busy::try_take(&LLM_BUSY) else {
        return Err("busy".into());
    };
    let stop = Arc::new(AtomicBool::new(false));
    *RUN_STOP.lock().map_err(|e| e.to_string())? = Some(stop.clone());
    blocking(move || {
        let _slot = RunSlot;
        let _guards = (cap_guard, llm_guard);
        let home = home();
        let s = settings::load(&home);
        let entry = projects::entry(&home, &args.id)?;
        let work = s.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
        let mut items = args.items;
        for it in &mut items {
            if let Some(c) = &it.capture {
                if !(is_inside(c, home.root()) || is_inside(c, &work)) {
                    it.capture = None;
                }
            }
        }
        let mut post = Map::new();
        if let Some(v) = args.outline {
            post.insert("outline".into(), Value::String(v));
        }
        if let Some(v) = args.background {
            post.insert("background".into(), Value::String(v));
        }
        let req = RunRequest {
            project: PathBuf::from(&entry.path),
            items,
            pack_id: Some(entry.pack_id.clone()),
            note: args.note,
            rewriter: args.cli,
            candidates: args.candidates,
            fidelity: args.fidelity,
            post: Value::Object(post),
            outline_off_ok: args.outline_off_ok,
            no_refs: args.no_refs,
            export_dir: args.export_dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from),
            ..RunRequest::default()
        };
        Ok(uis_core::pipeline::run(&home, &s, &req, stop, &mut |e| {
            let _ = on_event.send(e.clone());
        }))
    })
    .await
}

/// Stops the run in progress: a running Codex or Claude ends at once; a capture finishes its
/// request first (Unity is never ended).
#[tauri::command]
pub fn run_cancel() -> bool {
    match RUN_STOP.lock().ok().and_then(|s| s.clone()) {
        Some(flag) => {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// A job folder for the 「Runs」 tab: `job.json`, each post-processed candidate's report, the
/// before/after of the prompt and the last export.
#[tauri::command]
pub async fn job_get(job: String) -> Result<Value, String> {
    blocking(move || {
        let home = home();
        let s = settings::load(&home);
        let dir = job_folder(&home, &s, &job)?;
        let read = |p: PathBuf| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let doc = read(dir.join("job.json")).unwrap_or(Value::Null);
        let posts: Vec<Value> = uis_core::jobs::candidates(&dir)
            .into_iter()
            .filter_map(|c| read(dir.join("post").join(format!("cand-{c:02}")).join("checks.json")))
            .collect();
        let capture = dir.join("capture").join("capture.png");
        Ok(serde_json::json!({
            "dir": dir.display().to_string(),
            "job": doc,
            "posts": posts,
            "diff": read(dir.join("prompt").join("diff.json")),
            "capture": capture.is_file().then(|| capture.display().to_string()),
            "export": read(dir.join("export.json")),
        }))
    })
    .await
}

/// Thumbnails (data: URLs, 128 px) of a project's prefabs that were captured before, by guid.
#[tauri::command]
pub async fn prefab_thumbs(id: String) -> Result<std::collections::HashMap<String, String>, String> {
    blocking(move || {
        let home = home();
        let entry = projects::entry(&home, &id)?;
        let found = uis_core::capture::thumbs(&home, Path::new(&entry.path));
        Ok(found.into_iter().filter_map(|(g, p)| uis_core::image::png_data_url(&p).ok().map(|u| (g, u))).collect())
    })
    .await
}

/// The folder picker for the export folder (None: cancelled). A folder inside a Unity project
/// comes back as an error (`insideProject:…`).
#[tauri::command]
pub async fn export_pick(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    let Some(folder) = picked else { return Ok(None) };
    let path = folder.into_path().map_err(|e| format!("path:{e}"))?;
    if let Some(why) = uis_core::export::refusal(&path, &settings::load(&home())) {
        return Err(why);
    }
    Ok(Some(path.display().to_string()))
}

/// Why `folder` may not receive icons (`insideProject:…`), or None.
#[tauri::command]
pub fn export_check(folder: String) -> Option<String> {
    uis_core::export::refusal(Path::new(&folder), &settings::load(&home()))
}
