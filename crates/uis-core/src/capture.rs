//! A capture request from start to end: probe → job JSON → before-capture hooks → open editor
//! (path A) or batch run (path D) → clean-up → after-capture hooks → report (and thumbnails).
//!
//! The job (`unity-icon-studio.capture/1`) is the same for both paths. Each item's values come
//! from, in order (later wins): the pack's preset for its kind (`kinds.<kind>.capture`), the kind
//! rule that matched its path (the rule's `capture` object and its other keys such as
//! `hideObjectsNamed`), then what a person set for that prefab (`front`, `rotation`, `yawSign`).
//!
//! Every report is also written to `<out>\capture-run.json`.

use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::home::Home;
use crate::hooks::{self, HookContext, HookRun};
use crate::journal::{self, Journal};
use crate::packs::{self, Pack};
use crate::prefabs::{self, PrefabItem, ScanOptions, MODEL_EXTS};
use crate::project::{self, Lock};
use crate::projects::{self, Override};
use crate::settings::{self, Settings};
use crate::unity::{attach, batch, find, gitguard, probe};

pub const JOB_SCHEMA: &str = "unity-icon-studio.capture/1";
pub const REPORT_SCHEMA: &str = "unity-icon-studio.capture-run/1";

#[derive(Debug, Clone, Default)]
pub struct CaptureRequest {
    pub project: PathBuf,
    /// Asset paths from the project folder (`Assets/...prefab`, or a model file)
    pub items: Vec<String>,
    /// None: the project's pack on the list, else the default pack
    pub pack_id: Option<String>,
    /// None: `capture.mode` of the settings (`auto`, `attach`, `batch`)
    pub mode: Option<String>,
    /// None: `<work>\<project>\captures\<run id>`
    pub out_dir: Option<PathBuf>,
    /// `capture` (1024, the pack's supersample) or `thumb` (256, supersample 1)
    pub purpose: String,
    /// Replace the settings' limits (tests)
    pub batch_timeout: Option<Duration>,
    pub attach_timeout: Option<Duration>,
    pub retry_wait: Option<Duration>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemReport {
    pub id: String,
    pub prefab: String,
    pub kind: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    pub coverage: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureReport {
    pub schema: String,
    pub run_id: String,
    /// `attach`, `batch`, or `blocked`
    pub mode: String,
    pub reason: String,
    pub project: String,
    pub out_dir: String,
    pub job_file: String,
    pub result_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_file: Option<String>,
    pub unity_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_version: Option<String>,
    /// Every item came out
    pub ok: bool,
    /// Why the run did not go through (`blocked:…`, `hookRefused:…`, `editorMissing:…`, …)
    pub error: Option<String>,
    pub items: Vec<ItemReport>,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub hooks: Vec<HookRun>,
    pub git: Option<gitguard::GuardReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_removed: Option<bool>,
    /// Open scenes were as dirty after the capture as before (path A)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_dirty_same: Option<bool>,
    /// Unity still runs (the wait ran out); the journal keeps the run
    pub pending: bool,
    pub log_tail: Vec<String>,
    pub unity_ms: u64,
    pub elapsed_ms: u64,
}

/// A step of the run, for the screen and the command line.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureEvent {
    /// `probe`, `hooks`, `unity`, `cleanup`, `done`
    pub stage: String,
    pub message: String,
    pub done: u32,
    pub total: u32,
    pub elapsed_ms: u64,
}

// ------------------------------------------------------------------ job

/// Keys of a kind rule that are not capture values.
const RULE_KEYS: &[&str] = &["match", "kind", "capture"];

fn merge_into(base: &mut Map<String, Value>, over: &Map<String, Value>) {
    for (k, v) in over {
        match (base.get_mut(k), v) {
            (Some(Value::Object(b)), Value::Object(o)) => merge_into(b, o),
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// A file-name-safe id from the asset name: letters, digits, `-` and `_` (others become `_`).
pub fn item_id(asset: &str) -> String {
    let stem = Path::new(asset).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let id: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if id.trim_matches('_').is_empty() {
        "item".into()
    } else {
        id
    }
}

/// The capture values of one item: pack preset of its kind, then its rule, then a person's choice.
pub fn item_values(pack: &Pack, kind: &str, rule: Option<&Value>, person: Option<&Override>) -> Map<String, Value> {
    let mut v = pack.kind_preset(kind).get("capture").and_then(|c| c.as_object()).cloned().unwrap_or_default();
    if let Some(rule) = rule.and_then(|r| r.as_object()) {
        if let Some(c) = rule.get("capture").and_then(|c| c.as_object()) {
            merge_into(&mut v, c);
        }
        let rest: Map<String, Value> = rule.iter().filter(|(k, _)| !RULE_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        merge_into(&mut v, &rest);
    }
    if let Some(o) = person {
        if let Some(f) = &o.front {
            v.insert("front".into(), json!(f));
        }
        if let Some(r) = o.rotation {
            v.insert("rotation".into(), json!(r));
        }
        if let Some(sign) = o.yaw_sign {
            let yaw = v.get("yaw").and_then(|y| y.as_f64()).unwrap_or(28.0).abs();
            v.insert("yaw".into(), json!(if sign < 0 { -yaw } else { yaw }));
        }
    }
    // The pack keeps the ambient level with the light; the script takes an RGB colour at the top
    if let Some(Value::Object(light)) = v.get_mut("light") {
        if let Some(a) = light.remove("ambient") {
            let rgb = match a {
                Value::Number(n) => {
                    let x = n.as_f64().unwrap_or(0.6);
                    json!([x, x, x])
                }
                other => other,
            };
            v.insert("ambient".into(), rgb);
        }
    }
    v
}

/// What the job needs to know about one asset.
#[derive(Debug, Clone, Default)]
pub struct JobItem {
    pub asset: String,
    pub guid: Option<String>,
    pub kind: String,
    pub rule: Option<String>,
}

/// The capture job: output next to the job file, flush delayed calls in batch mode.
pub fn build_job(pack: &Pack, items: &[JobItem], overrides: &HashMap<String, Override>, purpose: &str) -> Value {
    let rules = pack.kind_rules();
    let mut used: HashSet<String> = HashSet::new();
    let mut list = Vec::new();
    for it in items {
        let rule = it.rule.as_ref().and_then(|p| rules.iter().find(|r| &r.pattern == p)).map(|r| &r.extra);
        let person = it.guid.as_ref().and_then(|g| overrides.get(g));
        let mut v = item_values(pack, &it.kind, rule, person);
        if purpose == "thumb" {
            v.insert("size".into(), json!(256));
            v.insert("supersample".into(), json!(1));
        }
        let base = item_id(&it.asset);
        let mut id = base.clone();
        let mut n = 2;
        while !used.insert(id.to_lowercase()) {
            id = format!("{base}-{n}");
            n += 1;
        }
        let mut item = Map::new();
        item.insert("id".into(), json!(id));
        item.insert("prefab".into(), json!(it.asset));
        item.insert("output".into(), json!(format!("{id}.png")));
        merge_into(&mut item, &v);
        list.push(Value::Object(item));
    }
    json!({
        "schema": JOB_SCHEMA,
        "outputDir": ".",
        "resultPath": "result.json",
        "flushDelayCalls": true,
        "defaults": { "size": 1024, "background": "transparent" },
        "items": list,
    })
}

// ------------------------------------------------------------------ helpers

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `YYYYMMDD-HHMMSS-xxxx` in local time.
pub fn new_run_id() -> String {
    let t = local_time();
    let salt = (now_ms() as u64 ^ (std::process::id() as u64) << 16).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 48;
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}-{:04x}", t.0, t.1, t.2, t.3, t.4, t.5, salt & 0xffff)
}

#[cfg(windows)]
fn local_time() -> (u16, u16, u16, u16, u16, u16) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

#[cfg(not(windows))]
fn local_time() -> (u16, u16, u16, u16, u16, u16) {
    let s = (now_ms() / 1000) as u64;
    let days = s / 86400;
    // Civil date from days since 1970 (UTC)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let r = s % 86400;
    (y as u16, m as u16, d as u16, (r / 3600) as u16, (r % 3600 / 60) as u16, (r % 60) as u16)
}

/// Absolute, without the `\\?\` prefix, with the folder's own case where it exists.
pub fn absolute(path: &Path) -> PathBuf {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
    let text = p.display().to_string();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() {
        "project".into()
    } else {
        s
    }
}

fn work_dir(home: &Home, s: &Settings) -> PathBuf {
    s.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir())
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("write:{e}"))? + "\n";
    settings::write_replace(path, &text)
}

fn read_json(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_str(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}')).ok()
}

/// The project on the settings list for this folder (its pack and the person's choices).
fn listed(s: &Settings, project: &Path) -> Option<settings::ProjectEntry> {
    let here = project.display().to_string();
    s.projects.iter().find(|p| probe::same_path(&absolute(Path::new(&p.path)).display().to_string(), &here)).cloned()
}

struct Run<'a> {
    home: &'a Home,
    settings: Settings,
    report: CaptureReport,
    started: Instant,
    events: &'a mut dyn FnMut(&CaptureEvent),
}

impl Run<'_> {
    fn event(&mut self, stage: &str, message: impl Into<String>, done: u32, total: u32) {
        let e = CaptureEvent { stage: stage.into(), message: message.into(), done, total, elapsed_ms: self.started.elapsed().as_millis() as u64 };
        (self.events)(&e);
    }

    fn finish(mut self, error: Option<String>) -> CaptureReport {
        if error.is_some() {
            self.report.error = error;
        }
        self.report.elapsed_ms = self.started.elapsed().as_millis() as u64;
        self.report.ok = self.report.error.is_none() && self.report.failed == 0 && !self.report.items.is_empty();
        if !self.report.out_dir.is_empty() {
            if let Ok(v) = serde_json::to_value(&self.report) {
                let _ = write_json(&Path::new(&self.report.out_dir).join("capture-run.json"), &v);
            }
        }
        let message = match &self.report.error {
            Some(e) => e.clone(),
            None => format!("{} ok, {} failed", self.report.items.len() - self.report.failed, self.report.failed),
        };
        let n = self.report.items.len() as u32;
        self.event("done", message, n, n);
        self.report
    }
}

// ------------------------------------------------------------------ capture

/// Runs a capture request. Never ends Unity, never opens or closes a normal editor.
pub fn capture(home: &Home, req: &CaptureRequest, events: &mut dyn FnMut(&CaptureEvent)) -> CaptureReport {
    let settings = settings::load(home);
    let project = absolute(&req.project);
    let run_id = new_run_id();
    let mut run = Run {
        home,
        settings,
        report: CaptureReport {
            schema: REPORT_SCHEMA.into(),
            run_id: run_id.clone(),
            project: project.display().to_string(),
            ..CaptureReport::default()
        },
        started: Instant::now(),
        events,
    };
    if req.items.is_empty() {
        return run.finish(Some("noItems".into()));
    }
    if let Err(project::NotProject(why)) = project::check_project(&project) {
        run.report.mode = "blocked".into();
        return run.finish(Some(format!("notProject:{why}")));
    }

    // Probe
    let mut capture_settings = run.settings.capture.clone();
    if let Some(m) = &req.mode {
        capture_settings.mode = m.clone();
    }
    run.event("probe", "probe", 0, req.items.len() as u32);
    let pr = probe::capture_probe(&project, &capture_settings, req.retry_wait.unwrap_or(Duration::from_secs(5)));
    run.report.mode = pr.probe.mode.clone();
    run.report.reason = pr.probe.reason.clone();
    run.report.unity_version = pr.probe.facts.unity_version.clone();
    if pr.probe.mode == "blocked" {
        return run.finish(Some(format!("blocked:{}", pr.probe.reason)));
    }

    // Job
    let entry = listed(&run.settings, &project);
    let pack_id = req.pack_id.clone().or_else(|| entry.as_ref().map(|e| e.pack_id.clone())).unwrap_or_else(|| packs::DEFAULT_ID.into());
    let pack = match packs::load(home, &pack_id) {
        Ok(p) => p,
        Err(e) => return run.finish(Some(format!("pack:{e}"))),
    };
    let state = entry.as_ref().map(|e| projects::load_state(home, &e.id)).unwrap_or_default();
    let overrides: HashMap<String, Override> = state.overrides.clone().into_iter().collect();
    let wants_models = req.items.iter().any(|a| Path::new(a).extension().is_some_and(|x| MODEL_EXTS.iter().any(|m| x.eq_ignore_ascii_case(m))));
    let scan = prefabs::scan(
        home,
        &project,
        &pack,
        &ScanOptions {
            force: false,
            include_models: wants_models,
            kind_overrides: overrides.iter().filter_map(|(g, o)| o.kind.clone().map(|k| (g.clone(), k))).collect(),
        },
    );
    let by_path: HashMap<String, PrefabItem> = scan
        .map(|s| s.prefabs.into_iter().map(|p| (p.path.to_lowercase(), p)).collect())
        .unwrap_or_default();
    let items: Vec<JobItem> = req
        .items
        .iter()
        .map(|a| {
            let asset = a.replace('\\', "/");
            match by_path.get(&asset.to_lowercase()) {
                Some(p) => JobItem { asset: p.path.clone(), guid: Some(p.guid.clone()).filter(|g| !g.is_empty()), kind: p.kind.clone(), rule: p.rule.clone() },
                None => JobItem { asset, guid: None, kind: "generic".into(), rule: None },
            }
        })
        .collect();
    let purpose = if req.purpose.is_empty() { "capture" } else { req.purpose.as_str() };
    let job = build_job(&pack, &items, &overrides, purpose);
    let out_dir = req
        .out_dir
        .clone()
        .map(|d| absolute(&d))
        .unwrap_or_else(|| work_dir(home, &run.settings).join(slug(&pr.probe.facts.name)).join("captures").join(&run_id));
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        return run.finish(Some(format!("out:{e}")));
    }
    let out_dir = absolute(&out_dir);
    run.report.out_dir = out_dir.display().to_string();
    let job_file = out_dir.join("job-in.json");
    run.report.job_file = job_file.display().to_string();
    run.report.result_file = out_dir.join("result.json").display().to_string();
    // A result left from an earlier run in the same folder must not be read as this one's
    let _ = std::fs::remove_file(out_dir.join("result.json"));
    if let Err(e) = write_json(&job_file, &job) {
        return run.finish(Some(e));
    }
    run.report.items = items
        .iter()
        .zip(job["items"].as_array().cloned().unwrap_or_default())
        .map(|(it, j)| ItemReport { id: j["id"].as_str().unwrap_or_default().into(), prefab: it.asset.clone(), kind: it.kind.clone(), ..ItemReport::default() })
        .collect();

    let runtime = match batch::runtime_files(&home.runtime_dir()) {
        Ok(d) => d,
        Err(e) => return run.finish(Some(e)),
    };
    let ctx = HookContext {
        mode: pr.probe.mode.clone(),
        project_path: project.display().to_string(),
        run_id: run_id.clone(),
        state_dir: home.hook_state_dir().display().to_string(),
    };
    let report = if pr.probe.mode == "attach" {
        attach_path(run, &project, &runtime, &job_file, &ctx, req)
    } else {
        batch_path(run, &project, &runtime, &job_file, &ctx, req)
    };
    thumbnails(home, &project, &items, &job, report)
}

fn before_hooks(run: &mut Run, ctx: &HookContext) -> (Vec<HookRun>, Option<String>) {
    let hooks = run.settings.capture.hooks.before_capture.clone();
    if !hooks.is_empty() {
        run.event("hooks", "beforeCapture", 0, run.report.items.len() as u32);
    }
    let runs = hooks::run_phase("before", &hooks, ctx);
    let err = hooks::phase_error(&runs);
    (runs, err)
}

fn after_hooks(settings: &Settings, ctx: &HookContext) -> Vec<HookRun> {
    hooks::run_phase("after", &settings.capture.hooks.after_capture, ctx)
}

/// After-hooks are due when no before-hook ran or one of them went on (it may hold something).
fn after_due(before: &[HookRun]) -> bool {
    before.is_empty() || before.iter().any(|h| h.ok())
}

fn attach_path(mut run: Run, project: &Path, runtime: &Path, job_file: &Path, ctx: &HookContext, req: &CaptureRequest) -> CaptureReport {
    let cli = match find::find_unity_cli(&run.settings.capture) {
        Ok(c) => c,
        Err(e) => return run.finish(Some(e)),
    };
    let (before, err) = before_hooks(&mut run, ctx);
    let due = after_due(&before);
    run.report.hooks.extend(before);
    if let Some(e) = err {
        if due {
            let after = after_hooks(&run.settings, ctx);
            run.report.hooks.extend(after);
        }
        return run.finish(Some(e));
    }
    let git_before = gitguard::snapshot(project);
    let total = run.report.items.len() as u32;
    run.event("unity", "attach", 0, total);
    let limit = req.attach_timeout.unwrap_or(Duration::from_secs(run.settings.capture.attach_timeout_sec as u64));
    let t = Instant::now();
    let a = attach::run_attach(&cli, project, &runtime.join(batch::SCRIPT_NAME), job_file, limit);
    run.report.unity_ms = t.elapsed().as_millis() as u64;
    run.report.cli_version = a.cli_version.clone();
    run.report.editor = Some(cli.display().to_string());
    // Path A never writes into the project; files the open editor rewrites are its own business
    run.report.git = Some(gitguard::restore_and_compare(project, &git_before, &[], false, Path::new("")));
    let result = a.result.clone().or_else(|| read_json(Path::new(&run.report.result_file)));
    let error = match (&a.error, &result) {
        (Some(e), _) => Some(format!("attach:{e}")),
        (None, None) => Some("attach:noResult".into()),
        _ => None,
    };
    if let Some(r) = &result {
        apply_result(&mut run.report, r);
        let before = r.get("sceneDirtyBefore");
        run.report.scene_dirty_same = Some(before.is_some() && before == r.get("sceneDirtyAfter"));
    }
    if due {
        let after = after_hooks(&run.settings, ctx);
        run.report.hooks.extend(after);
    }
    run.finish(error)
}

fn apply_result(report: &mut CaptureReport, result: &Value) {
    let items = result.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default();
    for it in report.items.iter_mut() {
        match items.iter().find(|r| r.get("id").and_then(|x| x.as_str()) == Some(it.id.as_str())) {
            Some(r) => {
                it.ok = r.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                it.error = r.get("error").and_then(|x| x.as_str()).map(str::to_string);
                it.output = r.get("output").and_then(|x| x.as_str()).map(|s| s.replace('/', "\\"));
                it.coverage = r.get("coverage").and_then(|x| x.as_f64());
            }
            None => {
                it.ok = false;
                it.error = Some("notInResult".into());
            }
        }
    }
    report.failed = report.items.iter().filter(|i| !i.ok).count();
}

fn batch_path(mut run: Run, project: &Path, runtime: &Path, job_file: &Path, ctx: &HookContext, req: &CaptureRequest) -> CaptureReport {
    let version = run.report.unity_version.clone().unwrap_or_default();
    let editor = match find::find_editor(&run.settings.capture, &version) {
        Ok(e) => e,
        Err(e) => return run.finish(Some(e)),
    };
    run.report.editor = Some(editor.path.clone());
    let project_text = project.display().to_string();
    if batch::entry_root(project).exists() {
        let why = if journal::for_project(run.home, &project_text).is_some() { "batchPending" } else { "entryExists" };
        return run.finish(Some(why.into()));
    }
    let out_dir = PathBuf::from(&run.report.out_dir);
    let log_file = out_dir.join("unity.log");
    run.report.log_file = Some(log_file.display().to_string());
    let mut j = Journal {
        run_id: run.report.run_id.clone(),
        mode: "batch".into(),
        project: project_text.clone(),
        entry_dir: batch::entry_root(project).display().to_string(),
        unity_exe: editor.path.clone(),
        state: "prepared".into(),
        out_dir: run.report.out_dir.clone(),
        job_file: job_file.display().to_string(),
        log_file: log_file.display().to_string(),
        started_ms: now_ms(),
        restore_tracked_files: run.settings.capture.restore_tracked_files,
        restore_patterns: run.settings.capture.restore_patterns.clone(),
        ..Journal::default()
    };
    if let Err(e) = journal::write(run.home, &j) {
        return run.finish(Some(e));
    }
    let (before, err) = before_hooks(&mut run, ctx);
    j.hooks_before_ok = after_due(&before);
    run.report.hooks.extend(before);
    if let Some(e) = err {
        if j.hooks_before_ok {
            let after = after_hooks(&run.settings, ctx);
            run.report.hooks.extend(after);
        }
        let _ = journal::remove(run.home, &j.run_id);
        return run.finish(Some(e));
    }
    // The hooks may have waited for a lease: someone could have opened the project meanwhile
    if project::lock_state(project) == Lock::Locked {
        let after = after_hooks(&run.settings, ctx);
        run.report.hooks.extend(after);
        let _ = journal::remove(run.home, &j.run_id);
        return run.finish(Some("blocked:open".into()));
    }
    j.git_before = gitguard::snapshot(project);
    let _ = journal::write(run.home, &j);
    if let Err(e) = batch::place_entry(project, runtime) {
        if e != "entryExists" {
            let _ = batch::remove_entry(project);
        }
        let after = after_hooks(&run.settings, ctx);
        run.report.hooks.extend(after);
        let _ = journal::remove(run.home, &j.run_id);
        return run.finish(Some(e));
    }
    let _ = std::fs::remove_file(&log_file);
    let total = run.report.items.len() as u32;
    run.event("unity", "starting Unity (batch)", 0, total);
    let pid = match batch::start(runtime, Path::new(&editor.path), project, &log_file, job_file) {
        Ok(pid) => pid,
        Err(e) => {
            // Unity did not start: nothing holds the entry folder
            run.report.entry_removed = batch::remove_entry(project).ok();
            run.report.git = Some(guard(run.home, project, &j));
            let after = after_hooks(&run.settings, ctx);
            run.report.hooks.extend(after);
            let _ = journal::remove(run.home, &j.run_id);
            return run.finish(Some(e));
        }
    };
    j.pid = pid;
    j.state = "running".into();
    let _ = journal::write(run.home, &j);
    let limit = req.batch_timeout.unwrap_or(Duration::from_secs(run.settings.capture.batch_timeout_min as u64 * 60));
    let started = Instant::now();
    let events = &mut *run.events;
    let t0 = run.started;
    let w = batch::wait(pid, limit, &log_file, &mut |p| {
        let msg = p.last.clone().unwrap_or_default();
        events(&CaptureEvent {
            stage: "unity".into(),
            message: msg,
            done: p.done,
            total: p.total.unwrap_or(total),
            elapsed_ms: t0.elapsed().as_millis() as u64,
        });
    });
    run.report.unity_ms = started.elapsed().as_millis() as u64;
    if !w.exited {
        j.state = "timedOut".into();
        let _ = journal::write(run.home, &j);
        run.report.pending = true;
        run.report.log_tail = batch::log_tail(&log_file, 20);
        return run.finish(Some("batchTimeout".into()));
    }
    run.event("cleanup", "cleanup", total, total);
    let fin = finish_batch(run.home, &run.settings, &j, w.exit_code);
    run.report.exit_code = fin.exit_code;
    run.report.entry_removed = Some(fin.entry_removed);
    run.report.git = Some(fin.git);
    run.report.hooks.extend(fin.after_hooks);
    run.report.log_tail = fin.log_tail;
    if let Some(r) = &fin.result {
        apply_result(&mut run.report, r);
    }
    run.finish(fin.error)
}

fn guard(home: &Home, project: &Path, j: &Journal) -> gitguard::GuardReport {
    let backup = gitguard::backup_path(&home.backups_dir(), &j.run_id);
    gitguard::restore_and_compare(project, &j.git_before, &j.restore_patterns, j.restore_tracked_files, &backup)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchFinish {
    pub run_id: String,
    pub project: String,
    pub exit_code: Option<i32>,
    pub entry_removed: bool,
    pub git: gitguard::GuardReport,
    pub after_hooks: Vec<HookRun>,
    #[serde(skip)]
    pub result: Option<Value>,
    pub log_tail: Vec<String>,
    pub error: Option<String>,
}

/// After Unity ended: remove the entry folder, put re-serialized files back, run the after-capture
/// hooks, read the result, and drop the journal. Used by a normal run and by recovery.
pub fn finish_batch(home: &Home, s: &Settings, j: &Journal, exit_code: Option<i32>) -> BatchFinish {
    let project = PathBuf::from(&j.project);
    let log_file = PathBuf::from(&j.log_file);
    let exit_code = exit_code.or_else(|| batch::read_progress(&log_file).exit_code);
    let mut error = None;
    let entry_removed = match batch::remove_entry(&project) {
        Ok(r) => r,
        Err(e) => {
            error = Some(e);
            false
        }
    };
    let git = guard(home, &project, j);
    let ctx = HookContext {
        mode: j.mode.clone(),
        project_path: j.project.clone(),
        run_id: j.run_id.clone(),
        state_dir: home.hook_state_dir().display().to_string(),
    };
    let after = if j.hooks_before_ok { after_hooks(s, &ctx) } else { Vec::new() };
    let result = read_json(&Path::new(&j.out_dir).join("result.json"));
    let log_tail = match exit_code {
        Some(0) | Some(2) => Vec::new(),
        _ => batch::log_tail(&log_file, 20),
    };
    if error.is_none() {
        error = match (exit_code, &result) {
            (Some(0), Some(_)) | (Some(2), Some(_)) => None,
            (Some(code), _) => Some(format!("batchExit:{code}")),
            (None, _) => Some("batchExit:unknown".into()),
        };
    }
    if !entry_removed && batch::entry_root(&project).exists() {
        // Keep the journal: the folder is still there
    } else {
        let _ = journal::remove(home, &j.run_id);
    }
    BatchFinish { run_id: j.run_id.clone(), project: j.project.clone(), exit_code, entry_removed, git, after_hooks: after, result, log_tail, error }
}

// ------------------------------------------------------------------ recovery

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryEntry {
    pub run_id: String,
    pub project: String,
    pub state: String,
    pub pid: u32,
    /// Unity still runs for it
    pub running: bool,
    pub started_ms: i64,
}

/// Journal entries left from earlier runs.
pub fn recovery_status(home: &Home) -> Vec<RecoveryEntry> {
    journal::list(home)
        .into_iter()
        .map(|j| RecoveryEntry {
            running: batch::unity_running(j.pid, &j.unity_exe),
            run_id: j.run_id,
            project: j.project,
            state: j.state,
            pid: j.pid,
            started_ms: j.started_ms,
        })
        .collect()
}

/// Finishes every journal entry whose Unity has ended (or never started). Runs still going are
/// left for later.
pub fn recover(home: &Home) -> Vec<BatchFinish> {
    let s = settings::load(home);
    journal::list(home)
        .into_iter()
        .filter(|j| !batch::unity_running(j.pid, &j.unity_exe))
        .map(|j| finish_batch(home, &s, &j, None))
        .collect()
}

// ------------------------------------------------------------------ thumbnails

/// 128 px copies of what came out, kept by prefab guid in `<cache>\<project>\thumbs\`, keyed by the
/// prefab's modified time and the capture values, so a changed prefab or value makes a new one.
fn thumbnails(home: &Home, project: &Path, items: &[JobItem], job: &Value, mut report: CaptureReport) -> CaptureReport {
    let dir = home.cache_dir().join(prefabs::project_hash(project)).join("thumbs");
    let jobs = job["items"].as_array().cloned().unwrap_or_default();
    for (i, it) in report.items.iter_mut().enumerate() {
        let (Some(out), Some(src)) = (it.output.clone().filter(|_| it.ok), items.get(i)) else { continue };
        let Some(guid) = &src.guid else { continue };
        let mtime = std::fs::metadata(project.join(&src.asset))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let mut values = jobs.get(i).cloned().unwrap_or(Value::Null);
        if let Value::Object(m) = &mut values {
            m.remove("output");
            m.remove("id");
        }
        let key = format!("{mtime}:{values}");
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in key.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        let path = dir.join(format!("{guid}-{:08x}.png", h as u32));
        if crate::image::thumbnail(Path::new(&out), &path, 128).is_ok() {
            it.thumb = Some(path.display().to_string());
        }
    }
    if let Ok(v) = serde_json::to_value(&report) {
        if !report.out_dir.is_empty() {
            let _ = write_json(&Path::new(&report.out_dir).join("capture-run.json"), &v);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_with_rules() -> Pack {
        let mut p = Pack::builtin();
        let local: Value = serde_json::from_str(
            r#"{ "kinds": { "building": { "capture": { "front": "-z" } } },
                 "kindRules": [ { "match": "**/Hats/*.prefab", "kind": "prop", "capture": { "front": "+z", "rotation": [270, 0, 0] } },
                                { "match": "**/People/**", "kind": "character", "hideObjectsNamed": ["Shadow"] } ] }"#,
        )
        .unwrap();
        if let (Value::Object(base), Value::Object(over)) = (&mut p.doc, &local) {
            for (k, v) in over {
                match (base.get_mut(k), v) {
                    (Some(Value::Object(b)), Value::Object(o)) => merge_into(b, o),
                    _ => {
                        base.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        p
    }

    #[test]
    fn values_come_from_kind_then_rule_then_person() {
        let p = pack_with_rules();
        let b = item_values(&p, "building", None, None);
        assert_eq!((b["front"].clone(), b["yaw"].clone(), b["pitch"].clone()), (json!("-z"), json!(28), json!(30)));
        assert_eq!(b["ambient"], json!([0.6, 0.6, 0.6]));
        assert!(b["light"].get("ambient").is_none());
        assert_eq!(b["light"]["shadows"], json!(true));

        let rules = p.kind_rules();
        let hat = item_values(&p, "prop", Some(&rules[0].extra), None);
        assert_eq!((hat["front"].clone(), hat["rotation"].clone(), hat["yaw"].clone()), (json!("+z"), json!([270, 0, 0]), json!(25)));
        assert_eq!(hat["light"]["shadows"], json!(false));
        assert!(hat.get("match").is_none() && hat.get("kind").is_none());

        let person = item_values(&p, "character", Some(&rules[1].extra), None);
        assert_eq!(person["hideObjectsNamed"], json!(["Shadow"]));
        assert_eq!(person["pose"], json!("defaultState"));

        let o = Override { yaw_sign: Some(-1), front: Some("+x".into()), rotation: Some([0.0, 90.0, 0.0]), kind: None };
        let mine = item_values(&p, "building", None, Some(&o));
        assert_eq!((mine["yaw"].clone(), mine["front"].clone(), mine["rotation"].clone()), (json!(-28.0), json!("+x"), json!([0.0, 90.0, 0.0])));
    }

    #[test]
    fn job_ids_are_unique_and_thumbs_are_small() {
        let p = Pack::builtin();
        let items = [
            JobItem { asset: "Assets/A/Crate.prefab".into(), kind: "prop".into(), ..JobItem::default() },
            JobItem { asset: "Assets/B/Crate.prefab".into(), kind: "prop".into(), ..JobItem::default() },
            JobItem { asset: "Assets/C/My Crate (1).fbx".into(), kind: "generic".into(), ..JobItem::default() },
        ];
        let job = build_job(&p, &items, &HashMap::new(), "thumb");
        let ids: Vec<&str> = job["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["Crate", "Crate-2", "My_Crate__1_"]);
        assert_eq!(job["items"][0]["size"], json!(256));
        assert_eq!(job["items"][0]["supersample"], json!(1));
        assert_eq!(job["items"][2]["prefab"], json!("Assets/C/My Crate (1).fbx"));
        assert_eq!(job["schema"], json!(JOB_SCHEMA));
        assert_eq!(job["flushDelayCalls"], json!(true));
    }

    #[test]
    fn run_ids_are_dated() {
        let id = new_run_id();
        assert_eq!(id.len(), 20, "{id}");
        assert_eq!(&id[8..9], "-");
    }
}
