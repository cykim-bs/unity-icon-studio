//! Whole runs (uis-core pipeline.rs) against the stub Unity, Codex and Claude: capture in batch
//! mode → prompt → rewrite → picture → post-processing → checks → export, plus failures.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use uis_core::home::Home;
use uis_core::pipeline::{self, RunEvent, RunItemRequest, RunReport, RunRequest};

const VERSION: &str = "6000.0.1f1";

/// One run at a time: the stub editor is started through WMI and the runs share nothing else,
/// but they are slow enough that running them together only adds noise.
static ONE: Mutex<()> = Mutex::new(());

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/SampleProject")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

struct Bed {
    base: PathBuf,
    home: Home,
    project: PathBuf,
    bin: PathBuf,
}

impl Bed {
    fn new(tag: &str, scenario: &str) -> Bed {
        let base = std::env::temp_dir().join(format!("uis-e2e-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("Game");
        copy_dir(&fixture(), &project);
        let bin = base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let stub = PathBuf::from(env!("CARGO_BIN_EXE_uis-stub"));
        for name in ["codex", "claude", "codex-code-mode-host", "unity", "Unity"] {
            std::fs::copy(&stub, bin.join(format!("{name}.exe"))).unwrap();
        }
        std::fs::write(bin.join("stub-scenario.txt"), scenario).unwrap();
        let home = Home::at(base.join("home"));
        home.ensure().unwrap();
        let s = json!({
            "schema": "unity-icon-studio.settings/1",
            "capture": {
                "unityCli": bin.join("unity.exe").display().to_string(),
                "editors": { VERSION: bin.join("Unity.exe").display().to_string() }
            },
            "codex": { "path": bin.join("codex.exe").display().to_string(), "textTimeoutSec": 10, "imageTimeoutSec": 20 },
            "claude": { "path": bin.join("claude.exe").display().to_string(), "timeoutSec": 10 },
            "exportSizes": [256, 512]
        });
        std::fs::write(home.settings_file(), serde_json::to_string_pretty(&s).unwrap()).unwrap();
        Bed { base, home, project, bin }
    }

    fn request(&self, items: &[&str], note: &str) -> RunRequest {
        RunRequest {
            project: self.project.clone(),
            items: items.iter().map(|a| RunItemRequest { asset: a.to_string(), ..Default::default() }).collect(),
            mode: Some("batch".into()),
            note: note.into(),
            export_dir: Some(self.base.join("out")),
            codex_home: Some(self.base.join("codex-home")),
            ..RunRequest::default()
        }
    }

    fn run(&self, req: &RunRequest, cancel: Arc<AtomicBool>) -> (RunReport, Vec<RunEvent>) {
        let s = uis_core::settings::load(&self.home);
        let mut events = Vec::new();
        let r = pipeline::run(&self.home, &s, req, cancel, &mut |e| events.push(e.clone()));
        (r, events)
    }

    fn calls(&self, role: &str) -> usize {
        std::fs::read_to_string(self.bin.join("calls.log")).unwrap_or_default().lines().filter(|l| l.starts_with(&format!("{role}\t"))).count()
    }
}

impl Drop for Bed {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }
}

fn stages(events: &[RunEvent], item: &str) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.item.as_deref() == Some(item) && e.state != "progress")
        .map(|e| format!("{}:{}", e.stage, e.state))
        .collect()
}

#[test]
fn two_items_from_capture_to_export() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("ok", "delay_ms=200\n");
    let items = ["Assets/Props/Crate.prefab", "Assets/Buildings/House.prefab"];
    let mut req = bed.request(&items, "make the roof blue");
    req.export_failing = true;
    let (r, events) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    assert!(r.ok, "{}", serde_json::to_string_pretty(&r).unwrap());
    assert_eq!(r.capture.as_ref().unwrap().mode, "batch");
    for (i, it) in r.items.iter().enumerate() {
        assert_eq!(it.state, "ok");
        assert_eq!(it.best, Some(1));
        assert!(it.rewrite.as_ref().unwrap().cli.is_some(), "the note was rewritten");
        let x = it.export.as_ref().expect("exported");
        assert!(x.ok, "{:?}", x.error);
        assert_eq!(x.files.len(), 2, "256 and 512");
        for f in &x.files {
            assert!(Path::new(&f.path).is_file());
        }
        let job: Value = serde_json::from_slice(&std::fs::read(Path::new(it.job_dir.as_ref().unwrap()).join("job.json")).unwrap()).unwrap();
        assert_eq!(job["asset"], json!(items[i]));
        assert_eq!(job["runId"], json!(r.run_id));
        assert_eq!(
            stages(&events, items[i]),
            ["prompt:start", "prompt:ok", "rewrite:start", "rewrite:ok", "generate:start", "generate:ok", "post:start", "post:ok", if it.pass { "check:ok" } else { "check:fail" }, "export:start", "export:ok"]
        );
    }
    assert!(bed.base.join("out/icon_Crate.png").is_file());
    assert!(bed.base.join("out/icon_House_512.png").is_file());
    assert_eq!(events.first().unwrap().stage, "capture");
    assert_eq!(events.last().unwrap().stage, "done");
    // The run file and the job list
    let s = uis_core::settings::load(&bed.home);
    assert!(bed.home.default_work_dir().join("runs").join(format!("{}.json", r.run_id)).is_file());
    let jobs = uis_core::jobs::list(&bed.home, &s, 10);
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0]["exported"], json!(true));
    // The project was not left with the capture entry
    assert!(!bed.project.join("Assets/__UnityIconStudioCapture").exists());
}

#[test]
fn an_empty_note_calls_no_rewriter_and_reuses_a_capture() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("reuse", "");
    let pic = bed.base.join("cap.png");
    image::RgbaImage::from_fn(128, 128, |x, y| image::Rgba([200, 120, 60, if (32..96).contains(&x) && (24..104).contains(&y) { 255 } else { 0 }]))
        .save(&pic)
        .unwrap();
    let mut req = bed.request(&[], "  \n ");
    req.items = vec![RunItemRequest { asset: "Assets/Props/Crate.prefab".into(), kind: Some("prop".into()), capture: Some(pic) }];
    req.export_dir = None;
    let (r, events) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    assert!(r.ok, "{:?}", r.items[0].error);
    assert!(r.capture.is_none(), "no capture was needed");
    assert_eq!(events[0].stage, "capture");
    assert_eq!(events[0].state, "skip");
    assert!(stages(&events, "Assets/Props/Crate.prefab").contains(&"rewrite:skip".to_string()));
    assert_eq!(bed.calls("claude"), 0);
    assert_eq!(r.items[0].kind, "prop");
    assert!(r.items[0].export.is_none());
}

#[test]
fn a_limit_stops_the_items_still_waiting() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("limit", "delay_ms=200\ncodex=limit\n");
    let (r, _) = bed.run(&bed.request(&["Assets/Props/Crate.prefab", "Assets/Buildings/House.prefab"], ""), Arc::new(AtomicBool::new(false)));
    assert!(!r.ok);
    assert_eq!(r.stopped.as_deref(), Some("limit"));
    assert_eq!(r.items[0].state, "failed");
    assert_eq!(r.items[0].stage.as_deref(), Some("generate"));
    assert_eq!(r.items[0].error.as_deref(), Some("image:limit"));
    assert_eq!(r.items[1].state, "stopped");
    assert_eq!(bed.calls("codex"), 1, "the second item was not sent");
}

#[test]
fn a_missing_prefab_fails_at_capture_and_the_rest_goes_on() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("missing", "delay_ms=200\n");
    let (r, _) = bed.run(&bed.request(&["Assets/Props/Nope.prefab", "Assets/Props/Crate.prefab"], ""), Arc::new(AtomicBool::new(false)));
    assert!(!r.ok);
    assert_eq!(r.items[0].state, "failed");
    assert_eq!(r.items[0].stage.as_deref(), Some("capture"));
    assert_eq!(r.items[0].error.as_deref(), Some("prefab_not_found"));
    assert_eq!(r.items[1].state, "ok");
}

#[test]
fn a_rewrite_failure_sends_no_picture() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("rwfail", "delay_ms=200\nclaude=bad_json\n");
    let mut req = bed.request(&["Assets/Props/Crate.prefab"], "snow on top");
    req.rewriter = Some("claude".into());
    let (r, _) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    assert_eq!(r.items[0].state, "failed");
    assert_eq!(r.items[0].stage.as_deref(), Some("rewrite"));
    assert!(r.items[0].error.as_deref().unwrap().starts_with("rewrite:"));
    assert_eq!(bed.calls("codex"), 0, "no fixed prompt sent behind the person's back");
}

#[test]
fn a_stop_ends_the_picture_and_cancels_the_rest() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("cancel", "delay_ms=200\ncodex=timeout\n");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let req = bed.request(&["Assets/Props/Crate.prefab", "Assets/Buildings/House.prefab"], "");
    let s = uis_core::settings::load(&bed.home);
    let started = std::time::Instant::now();
    let r = pipeline::run(&bed.home, &s, &req, cancel, &mut |e| {
        if e.stage == "generate" && e.state == "start" {
            let f = flag.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(800));
                f.store(true, std::sync::atomic::Ordering::SeqCst);
            });
        }
    });
    assert!(r.cancelled);
    assert!(started.elapsed().as_secs() < 15, "the stub would sleep 30 s: the stop ended it");
    assert_eq!(r.items[0].state, "cancelled");
    assert_eq!(r.items[1].state, "cancelled");
    assert_eq!(bed.calls("codex"), 1);
}
