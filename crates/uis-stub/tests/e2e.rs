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
        // Pictures are the capture keyed out of its white (a faithful "repaint"), unless a test says
        let scenario = if scenario.contains("codex_image=") { scenario.to_string() } else { format!("codex_image=copy
{scenario}") };
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
        assert_eq!(x.files.len(), 3, "256 and 512, and the 1024 original");
        for f in &x.files {
            assert!(Path::new(&f.path).is_file());
        }
        let job: Value = serde_json::from_slice(&std::fs::read(Path::new(it.job_dir.as_ref().unwrap()).join("job.json")).unwrap()).unwrap();
        assert_eq!(job["asset"], json!(items[i]));
        assert_eq!(job["runId"], json!(r.run_id));
        assert_eq!(
            stages(&events, items[i]),
            [
                "prompt:start", "prompt:ok", "rewrite:start", "rewrite:ok", "generate:start", "generate:ok", "post:start", "post:ok",
                "review:start", "review:ok", if it.pass { "check:ok" } else { "check:fail" }, "export:start", "export:ok"
            ]
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

/// Strict: a picture with an added sparkle fails its fidelity review and one more try with the
/// correction fixes it; the retry is recorded in job.json.
#[test]
fn strict_sparkle_is_fixed_by_one_retry() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("retry", "codex_image=copy_sparkle_once
fidelity=added_once
");
    let mut req = bed.request(&["Assets/Buildings/House.prefab"], "");
    req.export_dir = None;
    let (r, events) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    let it = &r.items[0];
    assert_eq!(it.fidelity, "strict");
    assert_eq!(it.candidates.len(), 2, "{}", serde_json::to_string_pretty(&r).unwrap());
    let first = it.candidates[0].post.as_ref().unwrap();
    assert!(!first.pass);
    let f = first.fidelity.as_ref().unwrap();
    assert!(f.suspect || !f.report.pass, "the sparkle shows in the pixels too: {:?}", f.metrics);
    assert_eq!(first.reviews.len(), 1);
    assert_eq!(first.reviews[0].cli, "codex");
    assert!(!first.reviews[0].pass && first.reviews[0].failing[0].kind == "added");
    let second = &it.candidates[1];
    assert_eq!(second.attempt, 2);
    let corr = second.correction.as_deref().unwrap();
    assert!(corr.starts_with("Corrections from the previous try: do not add Yellow four-point sparkle (above the roof)"), "{corr}");
    let p2 = second.post.as_ref().unwrap();
    assert!(p2.reviews[0].pass, "{:?}", p2.reviews[0].failing);
    assert!(p2.fidelity.as_ref().unwrap().report.pass, "{:?}", p2.fidelity);
    assert_eq!(it.best, Some(2));
    let st = stages(&events, "Assets/Buildings/House.prefab");
    assert!(st.contains(&"retry:start".to_string()) && st.contains(&"retry:ok".to_string()), "{st:?}");
    // The retry's prompt is the sent one plus the correction at the end of SUBJECT
    let job = Path::new(it.job_dir.as_ref().unwrap());
    let retry_prompt = std::fs::read_to_string(job.join("prompt/final-cand-02.txt")).unwrap();
    let fixed = std::fs::read_to_string(job.join("prompt/final.txt")).unwrap();
    assert!(retry_prompt.contains(corr) && !fixed.contains("Corrections"));
    let doc: Value = serde_json::from_slice(&std::fs::read(job.join("job.json")).unwrap()).unwrap();
    assert_eq!(doc["fidelity"], json!("strict"));
    assert_eq!(doc["fidelityChecks"].as_array().unwrap().len(), 2);
    assert_eq!(doc["fidelityChecks"][1]["attempt"], json!(2));
    assert_eq!(doc["candidates"][1]["retryOf"], json!(1));
    assert!(job.join("post/cand-01/fidelity-map.png").is_file());
}

/// Strict with both reviewers: a sparkle that stays fails after the one retry; both CLIs reviewed.
#[test]
fn strict_failing_after_the_retry_is_not_exported() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("stillbad", "codex_image=copy_sparkle
fidelity=added
");
    let mut req = bed.request(&["Assets/Props/Crate.prefab"], "");
    req.fidelity_review = Some("both".into());
    let (r, _) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    let it = &r.items[0];
    assert_eq!(it.candidates.len(), 2);
    assert!(!it.pass);
    assert!(it.export.is_none(), "a failing icon is not exported");
    for c in &it.candidates {
        let p = c.post.as_ref().unwrap();
        assert_eq!(p.reviews.iter().map(|r| r.cli.as_str()).collect::<Vec<_>>(), ["claude", "codex"]);
        assert!(p.reviews.iter().all(|r| r.ok && !r.pass));
    }
    assert!(bed.calls("claude") >= 2 && bed.calls("codex") >= 4, "claude {} codex {}", bed.calls("claude"), bed.calls("codex"));
}

/// Stylized: no fidelity review and no retry; no retries setting: no retry either.
#[test]
fn stylized_and_zero_retries_skip_the_loop() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("stylized", "codex_image=copy_sparkle
fidelity=added
");
    let mut req = bed.request(&["Assets/Props/Crate.prefab"], "");
    req.export_dir = None;
    req.fidelity = Some("stylized".into());
    let (r, events) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    assert_eq!(r.items[0].fidelity, "stylized");
    assert_eq!(r.items[0].candidates.len(), 1);
    assert!(r.items[0].candidates[0].post.as_ref().unwrap().reviews.is_empty());
    assert!(!stages(&events, "Assets/Props/Crate.prefab").iter().any(|s| s.starts_with("review") || s.starts_with("retry")));
    let mut req = bed.request(&["Assets/Props/Crate.prefab"], "");
    req.export_dir = None;
    req.fidelity_retries = Some(0);
    let (r, _) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    assert_eq!(r.items[0].candidates.len(), 1);
    assert!(!r.items[0].pass);
}

#[test]
fn a_persons_camera_reaches_the_capture_and_the_prompt() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("camera", "delay_ms=200\n");
    // The project on the list, with a person's camera for Crate: flipped, looking down 45, turned 40
    let id = uis_core::projects::add(&bed.home, &bed.project).unwrap().id;
    let meta = std::fs::read_to_string(bed.project.join("Assets/Props/Crate.prefab.meta")).unwrap();
    let guid = meta.lines().find_map(|l| l.strip_prefix("guid: ")).unwrap().trim().to_string();
    let patch = json!({ "yawSign": -1, "pitch": 45, "yaw": 40 });
    uis_core::projects::set_override(&bed.home, &id, &guid, patch.as_object().unwrap()).unwrap();
    let mut req = bed.request(&["Assets/Props/Crate.prefab"], "");
    req.export_dir = None;
    let (r, _) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    let it = &r.items[0];
    assert_eq!(it.state, "ok", "{:?}", it.error);
    // The capture job had the person's values
    let values = uis_core::capture::values_of_picture(Path::new(it.capture.as_ref().unwrap())).unwrap();
    assert_eq!((values["yaw"].clone(), values["pitch"].clone()), (json!(-40.0), json!(45.0)));
    // and the CAMERA sentence says the same
    let fixed = std::fs::read_to_string(Path::new(it.job_dir.as_ref().unwrap()).join("prompt/fixed.txt")).unwrap();
    assert!(fixed.contains("looking down at about 45 degrees"), "{fixed}");
    assert!(fixed.contains("turned about 40 degrees") && fixed.contains("the left side face"), "{fixed}");
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=test", "-c", "user.name=test", "-c", "core.autocrlf=false"])
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?}");
}

#[test]
fn a_batch_that_rewrites_line_endings_leaves_the_project_clean_and_reports_real_changes() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    // The batch editor writes a text file again with CRLF (same content) and really changes a prefab
    let bed = Bed::new("eol", "delay_ms=200\ntouch_crlf=README.md\ntouch_edit=Assets/Props/CrateRed.prefab\n");
    std::fs::write(bed.project.join(".gitattributes"), "* text=auto eol=lf\n").unwrap();
    git(&bed.project, &["init", "-q"]);
    git(&bed.project, &["config", "core.autocrlf", "false"]);
    git(&bed.project, &["add", "-A"]);
    git(&bed.project, &["commit", "-q", "-m", "fixture"]);
    let mut req = bed.request(&["Assets/Props/Crate.prefab"], "");
    req.export_dir = None;
    let (r, _) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    let cap = r.capture.as_ref().unwrap();
    assert_eq!(cap.line_endings, ["README.md"], "put back: only its line endings changed");
    assert_eq!(cap.left, [" M Assets/Props/CrateRed.prefab"], "the real change is reported, not put back");
    assert!(std::fs::read(bed.project.join("README.md")).unwrap().windows(2).all(|w| w != b"\r\n"));
    assert!(std::fs::read_to_string(bed.project.join("Assets/Props/CrateRed.prefab")).unwrap().ends_with("stub edit\n"));
}

#[test]
fn fidelity_of_a_job_is_the_apps_and_a_finished_icon_leads_back_to_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let bed = Bed::new("fidjob", "codex_image=copy\n");
    let mut req = bed.request(&["Assets/Buildings/House.prefab"], "");
    req.export_dir = None;
    let (r, _) = bed.run(&req, Arc::new(AtomicBool::new(false)));
    let it = &r.items[0];
    assert_eq!(it.fidelity, "strict");
    let job = PathBuf::from(it.job_dir.as_ref().unwrap());
    let app = it.candidates[0].post.as_ref().unwrap().fidelity.clone().unwrap();
    let s = uis_core::settings::load(&bed.home);
    let cli = uis_core::jobs::candidate_fidelity(&bed.home, &s, &job, 1, None).unwrap();
    assert_eq!(cli.report.metrics, app.metrics, "the same pair and preparation as the app");
    assert_eq!(cli.report.report.pass, app.report.pass);
    assert_eq!(cli.raw, job.join("gen").join("cand-01").join("raw.png"));
    // A finished icon given directly is taken back to its job and candidate
    let master = job.join("post").join("cand-01").join("master-1024.png");
    assert_eq!(uis_core::jobs::job_of_finished_picture(&master), Some((job.clone(), 1)));
    assert_eq!(uis_core::jobs::job_of_finished_picture(&job.join("capture").join("capture.png")), None);
}
