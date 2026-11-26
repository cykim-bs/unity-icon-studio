//! A whole run for one or more prefabs: capture (one request for all that need it) → for each
//! item in turn: prompt → rewrite (when there is a note) → pictures → post-processing → checks →
//! export (when a folder is given). Steps are reported through `events`; a stop flag ends the run
//! between steps and ends a running CLI child (never Unity: a capture always finishes its batch).
//!
//! A limit, sign-in or missing-program failure stops the items still waiting (they would fail the
//! same way and could spend more of the limit).

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::capture::{self, CaptureReport, CaptureRequest};
use crate::cli::errors::FailKind;
use crate::cli::{Clis, RewriteReport, TextLlm};
use crate::export::{self, ExportReport};
use crate::generate::{self, GenerateRequest};
use crate::home::Home;
use crate::jobs::{self, PostJobReport};
use crate::settings::Settings;

pub const RUN_SCHEMA: &str = "unity-icon-studio.run/1";

/// One prefab of a run. `capture` reuses a picture taken before (then `kind` should be given too).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunItemRequest {
    pub asset: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub capture: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct RunRequest {
    pub project: PathBuf,
    pub items: Vec<RunItemRequest>,
    pub pack_id: Option<String>,
    /// Capture mode (None: the settings)
    pub mode: Option<String>,
    pub note: String,
    /// `claude` or `codex` (None: settings `llm.rewriter`)
    pub rewriter: Option<String>,
    pub candidates: Option<u32>,
    /// Leave the pack's style references out (Codex can stall with reference pictures)
    pub no_refs: bool,
    /// Post-processing keys laid over the pack's and the settings' values (`outline`, `background`)
    pub post: Value,
    /// The rewrite may turn the outline off (a person said yes); otherwise the outline stays
    pub outline_off_ok: bool,
    /// Export the recommended candidate of each passing item here
    pub export_dir: Option<PathBuf>,
    /// None: settings `exportSizes`
    pub export_sizes: Option<Vec<u32>>,
    /// Export the recommended candidate even when it failed a check (a person looked at it)
    pub export_failing: bool,
    /// Tests: a made-up CODEX_HOME, shorter capture limits
    pub codex_home: Option<PathBuf>,
    pub batch_timeout: Option<Duration>,
}

/// A step of a run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    pub run_id: String,
    /// `capture`, `prompt`, `rewrite`, `generate`, `post`, `check`, `export`, `done`
    pub stage: String,
    /// `start`, `ok`, `fail`, `skip`, `cancel`, `progress`
    pub state: String,
    /// The item's asset path (None for the capture of all items and for `done`)
    pub item: Option<String>,
    /// 1-based position of the item, and how many there are
    pub index: u32,
    pub total: u32,
    pub message: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCandidate {
    pub cand: u32,
    pub ok: bool,
    pub raw: Option<String>,
    pub failure: Option<crate::cli::errors::Failure>,
    pub post: Option<PostJobReport>,
    pub post_error: Option<String>,
    /// Checks passed (of all, every size)
    pub passed: usize,
    pub checked: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunItem {
    pub asset: String,
    pub name: String,
    pub kind: String,
    pub capture: Option<String>,
    pub job_dir: Option<String>,
    /// `ok` (made and checked), `failed`, `cancelled`, `stopped` (an earlier item hit a limit)
    pub state: String,
    /// The step that failed
    pub stage: Option<String>,
    pub error: Option<String>,
    pub rewrite: Option<RewriteReport>,
    pub candidates: Vec<RunCandidate>,
    /// The recommended candidate: passing with the most passed checks, else the most passed
    pub best: Option<u32>,
    pub pass: bool,
    /// The rewrite asked for no outline and nobody said yes: the outline stayed
    pub outline_kept: bool,
    pub export: Option<ExportReport>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub schema: String,
    pub run_id: String,
    pub project: String,
    pub ok: bool,
    pub cancelled: bool,
    /// Why the items still waiting were not run (`limit`, `auth`, `notFound`)
    pub stopped: Option<String>,
    pub error: Option<String>,
    pub capture: Option<CaptureSummary>,
    pub items: Vec<RunItem>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSummary {
    pub run_id: String,
    pub mode: String,
    pub reason: String,
    pub ok: bool,
    pub error: Option<String>,
    pub failed: usize,
    pub pending: bool,
    pub elapsed_ms: u64,
}

impl From<&CaptureReport> for CaptureSummary {
    fn from(c: &CaptureReport) -> Self {
        CaptureSummary {
            run_id: c.run_id.clone(),
            mode: c.mode.clone(),
            reason: c.reason.clone(),
            ok: c.ok,
            error: c.error.clone(),
            failed: c.failed,
            pending: c.pending,
            elapsed_ms: c.elapsed_ms,
        }
    }
}

fn stem(asset: &str) -> String {
    Path::new(asset).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| asset.to_string())
}

/// Checks passed and checked over every size.
fn score(p: &PostJobReport) -> (usize, usize) {
    p.sizes.iter().fold((0, 0), |(a, b), s| (a + s.checks.items.iter().filter(|i| i.pass).count(), b + s.checks.items.len()))
}

/// The recommended candidate of an item (see [`RunItem::best`]).
pub fn best_candidate(cands: &[RunCandidate]) -> Option<u32> {
    let rows: Vec<(u32, bool, usize)> = cands.iter().filter_map(|c| c.post.as_ref().map(|p| (c.cand, p.pass, c.passed))).collect();
    pick_best(&rows)
}

/// (candidate, passed every check, checks passed) of the post-processed candidates.
fn pick_best(rows: &[(u32, bool, usize)]) -> Option<u32> {
    let pick = |only_pass: bool| {
        rows.iter().filter(|r| !only_pass || r.1).max_by_key(|r| (r.2, std::cmp::Reverse(r.0))).map(|r| r.0)
    };
    pick(true).or_else(|| pick(false))
}

/// Adds the run's facts to `job.json` (project, asset, run, recommended candidate).
fn note_job(job_dir: &Path, project: &Path, run_id: &str, item: &RunItem) {
    let file = job_dir.join("job.json");
    let Some(mut doc) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return };
    if let Some(o) = doc.as_object_mut() {
        o.insert("project".into(), json!(project.display().to_string()));
        o.insert("projectName".into(), json!(project.file_name().map(|n| n.to_string_lossy().into_owned())));
        o.insert("asset".into(), json!(item.asset));
        o.insert("runId".into(), json!(run_id));
        o.insert("best".into(), json!(item.best));
        o.insert("pass".into(), json!(item.pass));
        o.insert("state".into(), json!(item.state));
        o.insert("failedStage".into(), json!(item.stage));
        o.insert("runError".into(), json!(item.error));
    }
    let _ = std::fs::write(&file, serde_json::to_string_pretty(&doc).unwrap_or_default());
}

pub fn run(home: &Home, settings: &Settings, req: &RunRequest, cancel: Arc<AtomicBool>, events: &mut dyn FnMut(&RunEvent)) -> RunReport {
    let clis = Clis { home, settings };
    run_with(home, settings, req, &clis, cancel, events)
}

pub fn run_with(home: &Home, settings: &Settings, req: &RunRequest, llm: &dyn TextLlm, cancel: Arc<AtomicBool>, events: &mut dyn FnMut(&RunEvent)) -> RunReport {
    let started = Instant::now();
    let run_id = capture::new_run_id();
    let project = capture::absolute(&req.project);
    let total = req.items.len() as u32;
    let mut report = RunReport {
        schema: RUN_SCHEMA.into(),
        run_id: run_id.clone(),
        project: project.display().to_string(),
        ok: false,
        cancelled: false,
        stopped: None,
        error: None,
        capture: None,
        items: Vec::new(),
        elapsed_ms: 0,
    };
    let mut emit = |stage: &str, state: &str, item: Option<&str>, index: u32, message: String| {
        events(&RunEvent {
            run_id: run_id.clone(),
            stage: stage.into(),
            state: state.into(),
            item: item.map(str::to_string),
            index,
            total,
            message,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    };
    let stopped = || cancel.load(Ordering::SeqCst);

    let mut items: Vec<RunItem> = req
        .items
        .iter()
        .map(|i| RunItem {
            asset: i.asset.clone(),
            name: stem(&i.asset),
            kind: i.kind.clone().unwrap_or_else(|| "generic".into()),
            capture: i.capture.as_ref().filter(|p| p.is_file()).map(|p| p.display().to_string()),
            job_dir: None,
            state: "waiting".into(),
            stage: None,
            error: None,
            rewrite: None,
            candidates: Vec::new(),
            best: None,
            pass: false,
            outline_kept: false,
            export: None,
            elapsed_ms: 0,
        })
        .collect();

    // ---- capture: one request for every item without a picture
    let need: Vec<String> = items.iter().filter(|i| i.capture.is_none()).map(|i| i.asset.clone()).collect();
    if need.is_empty() {
        emit("capture", "skip", None, 0, String::new());
    } else if stopped() {
        report.cancelled = true;
    } else {
        emit("capture", "start", None, 0, format!("{}", need.len()));
        let creq = CaptureRequest {
            project: project.clone(),
            items: need.clone(),
            pack_id: req.pack_id.clone(),
            mode: req.mode.clone(),
            purpose: "capture".into(),
            batch_timeout: req.batch_timeout,
            ..CaptureRequest::default()
        };
        let cap = capture::capture(home, &creq, &mut |e| {
            emit("capture", "progress", None, e.done, format!("{}:{}/{}", e.stage, e.done, e.total));
        });
        for it in items.iter_mut().filter(|i| i.capture.is_none()) {
            match cap.items.iter().find(|c| c.prefab == it.asset) {
                Some(c) if c.ok && c.output.is_some() => {
                    it.capture = c.output.clone();
                    it.kind = c.kind.clone();
                }
                Some(c) => {
                    it.state = "failed".into();
                    it.stage = Some("capture".into());
                    it.error = Some(c.error.clone().unwrap_or_else(|| "capture".into()));
                    if !c.kind.is_empty() {
                        it.kind = c.kind.clone();
                    }
                }
                None => {
                    it.state = "failed".into();
                    it.stage = Some("capture".into());
                    it.error = Some(cap.error.clone().unwrap_or_else(|| "capture".into()));
                }
            }
        }
        let ok = items.iter().any(|i| i.capture.is_some());
        emit("capture", if cap.error.is_none() && ok { "ok" } else { "fail" }, None, 0, cap.error.clone().unwrap_or_default());
        report.capture = Some(CaptureSummary::from(&cap));
    }

    // ---- each item in turn
    let mut halt: Option<String> = None;
    for (n, it) in items.iter_mut().enumerate() {
        let index = n as u32 + 1;
        if it.state == "failed" {
            continue;
        }
        if let Some(why) = &halt {
            it.state = "stopped".into();
            it.error = Some(why.clone());
            continue;
        }
        if stopped() {
            it.state = "cancelled".into();
            continue;
        }
        let item_started = Instant::now();
        let asset = it.asset.clone();
        let greq = GenerateRequest {
            input: PathBuf::from(it.capture.clone().unwrap_or_default()),
            kind: it.kind.clone(),
            pack_id: req.pack_id.clone(),
            name: Some(it.name.clone()),
            note: req.note.clone(),
            rewriter: req.rewriter.clone(),
            candidates: req.candidates,
            no_pack_refs: req.no_refs,
            codex_home: req.codex_home.clone(),
            ..GenerateRequest::default()
        };
        let mut last_stage = String::new();
        let g = crate::proc::with_cancel(cancel.clone(), || {
            generate::generate_staged(home, settings, &greq, llm, &mut |stage, state| {
                last_stage = stage.to_string();
                emit(stage, state, Some(&asset), index, String::new());
            })
        });
        it.job_dir = Some(g.job_dir.clone());
        it.rewrite = g.rewrite.clone();
        let cancelled_now = stopped();
        if let Some(e) = &g.error {
            it.state = if cancelled_now { "cancelled".into() } else { "failed".into() };
            it.stage = Some(if e.starts_with("rewrite:") { "rewrite".into() } else if last_stage.is_empty() { "prompt".into() } else { last_stage.clone() });
            it.error = Some(e.clone());
        }
        // Pictures, then post-processing and checks of each
        let outline_off = g.rewrite.as_ref().is_some_and(|r| r.diff.get("outline").and_then(Value::as_str) == Some("off"));
        let mut post = req.post.clone();
        if !post.is_object() {
            post = json!({});
        }
        if outline_off {
            if req.outline_off_ok {
                post["outline"] = json!("none");
            } else {
                it.outline_kept = true;
            }
        }
        for (ci, c) in g.candidates.iter().enumerate() {
            let cand = ci as u32 + 1;
            let mut rc = RunCandidate { cand, ok: c.ok, raw: c.raw.clone(), failure: c.failure.clone(), post: None, post_error: None, passed: 0, checked: 0 };
            if c.ok {
                emit("generate", "ok", Some(&asset), index, format!("{cand}"));
                if stopped() {
                    it.candidates.push(rc);
                    continue;
                }
                emit("post", "start", Some(&asset), index, format!("{cand}"));
                match jobs::post_candidate(home, settings, Path::new(&g.job_dir), cand, &post) {
                    Ok(p) => {
                        emit("post", "ok", Some(&asset), index, format!("{cand}"));
                        let (a, b) = score(&p);
                        rc.passed = a;
                        rc.checked = b;
                        emit("check", if p.pass { "ok" } else { "fail" }, Some(&asset), index, format!("{a}/{b}"));
                        rc.post = Some(p);
                    }
                    Err(e) => {
                        emit("post", "fail", Some(&asset), index, e.clone());
                        rc.post_error = Some(e);
                    }
                }
            } else if let Some(f) = &c.failure {
                emit("generate", if cancelled_now { "cancel" } else { "fail" }, Some(&asset), index, f.kind.code().to_string());
                if matches!(f.kind, FailKind::Limit | FailKind::Auth | FailKind::NotFound) {
                    halt = Some(f.kind.code().to_string());
                }
            }
            it.candidates.push(rc);
        }
        it.best = best_candidate(&it.candidates);
        let best = it.best.and_then(|b| it.candidates.iter().find(|c| c.cand == b));
        it.pass = best.and_then(|c| c.post.as_ref()).is_some_and(|p| p.pass);
        if it.error.is_none() {
            if cancelled_now && best.is_none() {
                it.state = "cancelled".into();
            } else if let Some(b) = best {
                it.state = "ok".into();
                let _ = b;
            } else {
                it.state = "failed".into();
                let failed = it.candidates.iter().find_map(|c| c.failure.as_ref().map(|f| format!("image:{}", f.kind.code())));
                let post_failed = it.candidates.iter().find_map(|c| c.post_error.clone());
                it.stage = Some(if failed.is_some() || post_failed.is_none() { "generate".into() } else { "post".into() });
                it.error = failed.or(post_failed).or_else(|| Some("noImage".into()));
            }
        }
        // Export the recommended candidate when it passed
        if let (Some(folder), Some(b), true) = (&req.export_dir, it.best, it.pass || req.export_failing) {
            if !stopped() {
                emit("export", "start", Some(&asset), index, String::new());
                let dir = PathBuf::from(&g.job_dir);
                let post_dir = dir.join("post").join(format!("cand-{b:02}"));
                let sizes = req.export_sizes.clone().unwrap_or_else(|| settings.export_sizes.clone());
                let icons: Vec<(u32, PathBuf)> = sizes
                    .iter()
                    .map(|s| (*s, if *s == 1024 { post_dir.join("master-1024.png") } else { post_dir.join(format!("icon-{s}.png")) }))
                    .filter(|(_, f)| f.is_file())
                    .collect();
                let x = export::export(&icons, folder, &settings.export_name, &it.name, settings, Some(&dir));
                emit("export", if x.ok { "ok" } else { "fail" }, Some(&asset), index, x.error.clone().unwrap_or_default());
                it.export = Some(x);
            }
        }
        it.elapsed_ms = item_started.elapsed().as_millis() as u64;
        note_job(Path::new(&g.job_dir), &project, &run_id, it);
    }

    report.cancelled = report.cancelled || stopped();
    report.stopped = halt;
    if report.cancelled {
        for it in items.iter_mut().filter(|i| i.state == "waiting") {
            it.state = "cancelled".into();
        }
    }
    report.ok = !items.is_empty() && items.iter().all(|i| i.state == "ok");
    if items.is_empty() {
        report.error = Some("noItems".into());
    } else if let Some(c) = &report.capture {
        if c.error.is_some() && items.iter().all(|i| i.stage.as_deref() == Some("capture")) {
            report.error = c.error.clone();
        }
    }
    report.items = items;
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    let done_state = if report.cancelled { "cancel" } else if report.ok { "ok" } else { "fail" };
    emit("done", done_state, None, 0, String::new());
    write_run(home, settings, &report);
    report
}

/// `<work>\runs\<run id>.json`: what a run did (the screen's 「Runs」 list reads the job folders).
fn write_run(home: &Home, settings: &Settings, report: &RunReport) {
    let work = settings.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
    let dir = work.join("runs");
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(format!("{}.json", report.run_id)), serde_json::to_string_pretty(report).unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_prefers_passing_then_most_checks_then_first() {
        assert_eq!(pick_best(&[(1, false, 12), (2, true, 10), (3, true, 11)]), Some(3));
        assert_eq!(pick_best(&[(1, false, 9), (2, false, 11)]), Some(2));
        assert_eq!(pick_best(&[(1, true, 11), (2, true, 11)]), Some(1));
        assert_eq!(pick_best(&[]), None);
    }
}
