//! Captures end to end against the stub Unity (CLI and batch editor) and a git copy of
//! tests/fixtures/SampleProject. The batch editor is really started through WMI, like Unity.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uis_core::capture::{capture, recover, recovery_status, CaptureReport, CaptureRequest};
use uis_core::home::Home;
use uis_core::journal;
use uis_core::unity::batch::entry_root;

const VERSION: &str = "6000.0.1f1";
const TOUCHED: &str = "Assets/Props/Crate.prefab.meta";
const ITEMS: &[&str] = &["Assets/Props/Crate.prefab", "Assets/Buildings/House.prefab", "Assets/Characters/Robot.prefab"];

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

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=test", "-c", "user.name=test", "-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .expect("git");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Env {
    base: PathBuf,
    home: Home,
    project: PathBuf,
    cli_dir: PathBuf,
    editor_dir: PathBuf,
    hook_log: PathBuf,
}

impl Env {
    fn new(tag: &str) -> Env {
        let base = std::env::temp_dir().join(format!("uis-cap-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("Game");
        copy_dir(&fixture(), &project);
        git(&project, &["init", "-q"]);
        // Every git run (the tool's too) must see the same line-ending setting
        git(&project, &["config", "core.autocrlf", "false"]);
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-q", "-m", "fixture"]);
        let stub = PathBuf::from(env!("CARGO_BIN_EXE_uis-stub"));
        let cli_dir = base.join("cli");
        let editor_dir = base.join("editor");
        for d in [&cli_dir, &editor_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::copy(&stub, cli_dir.join("unity.exe")).unwrap();
        std::fs::copy(&stub, editor_dir.join("Unity.exe")).unwrap();
        std::fs::copy(&stub, base.join("args.exe")).unwrap();
        std::fs::copy(&stub, base.join("exit.exe")).unwrap();
        let home = Home::at(base.join("home"));
        home.ensure().unwrap();
        let env = Env { hook_log: base.join("hooks.log"), base, home, project, cli_dir, editor_dir };
        env.settings(json!({}));
        env
    }

    /// settings.json: the stub CLI and editor, a recording hook before and after, plus `extra`.
    fn settings(&self, extra: Value) {
        let args = self.base.join("args.exe").display().to_string();
        let log = self.hook_log.display().to_string();
        let hook = |phase: &str| json!({ "name": format!("rec-{phase}"), "run": [args, log, "0", phase, "{mode}", "{projectPath}", "{projectName}", "{runId}", "{stateDir}", "{owner}"], "timeoutSec": 30 });
        let mut s = json!({
            "schema": "unity-icon-studio.settings/1",
            "capture": {
                "unityCli": self.cli_dir.join("unity.exe").display().to_string(),
                "editors": { VERSION: self.editor_dir.join("Unity.exe").display().to_string() },
                "hooks": { "beforeCapture": [hook("before")], "afterCapture": [hook("after")] }
            }
        });
        merge(&mut s, &extra);
        std::fs::write(self.home.settings_file(), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    fn scenario(&self, dir: &Path, lines: &str) {
        std::fs::write(dir.join("stub-scenario.txt"), lines).unwrap();
    }

    fn request(&self, items: &[&str], mode: &str) -> CaptureRequest {
        CaptureRequest {
            project: self.project.clone(),
            items: items.iter().map(|s| s.to_string()).collect(),
            mode: Some(mode.into()),
            out_dir: Some(self.base.join("out")),
            purpose: "capture".into(),
            retry_wait: Some(Duration::from_millis(50)),
            ..CaptureRequest::default()
        }
    }

    fn run(&self, req: &CaptureRequest) -> CaptureReport {
        capture(&self.home, req, &mut |_| {})
    }

    fn hook_lines(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(&self.hook_log)
            .unwrap_or_default()
            .lines()
            .map(|l| l.split('\t').map(str::to_string).collect())
            .collect()
    }

    fn status(&self) -> String {
        git(&self.project, &["status", "--porcelain=v1"])
    }

    fn add_pipeline_package(&self) {
        let manifest = self.project.join("Packages/manifest.json");
        std::fs::write(&manifest, r#"{ "dependencies": { "com.unity.render-pipelines.universal": "17.0.0", "com.unity.pipeline": "0.10.0" } }"#).unwrap();
        git(&self.project, &["commit", "-q", "-am", "pipeline"]);
    }

    /// Holds `Temp/UnityLockfile` with no sharing, like an open editor.
    fn hold_lock(&self) -> std::fs::File {
        let temp = self.project.join("Temp");
        std::fs::create_dir_all(&temp).unwrap();
        let mut o = std::fs::OpenOptions::new();
        o.read(true).write(true).create(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            o.share_mode(0);
        }
        o.open(temp.join("UnityLockfile")).unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, o) => *b = o.clone(),
    }
}

fn assert_png(path: &str) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{path}");
}

#[test]
fn attach_captures_in_the_open_editor_and_leaves_the_project_alone() {
    let env = Env::new("attach");
    env.add_pipeline_package();
    let _lock = env.hold_lock();
    env.scenario(&env.cli_dir, &format!("instances={}|ready\n", env.project.display()));
    let before = env.status();
    let r = env.run(&env.request(ITEMS, "auto"));
    assert_eq!(r.mode, "attach", "{r:#?}");
    assert!(r.ok, "{r:#?}");
    assert_eq!(r.items.len(), 3);
    for it in &r.items {
        assert_png(it.output.as_deref().unwrap());
        assert!(it.thumb.is_some(), "thumbnail for {}", it.id);
    }
    assert_eq!(r.cli_version.as_deref(), Some("1.0.0-stub"));
    assert_eq!(r.scene_dirty_same, Some(true));
    assert!(r.git.as_ref().unwrap().same);
    assert_eq!(env.status(), before);
    assert!(!entry_root(&env.project).exists());
    let hooks = env.hook_lines();
    assert_eq!(hooks.len(), 2, "{hooks:?}");
    assert_eq!(hooks[0][0], "before");
    assert_eq!(hooks[1][0], "after");
    assert_eq!(hooks[0][1], "attach");
    assert_eq!(hooks[0][3], "Game");
    assert_eq!(hooks[0][4], r.run_id);
    assert_eq!(hooks[0][6], "unity-icon-studio");
    // The job carries the pack's values for each kind
    let job: Value = serde_json::from_str(&std::fs::read_to_string(&r.job_file).unwrap()).unwrap();
    assert_eq!(job["items"][2]["pose"], json!("defaultState"), "humanoid robot is a character");
    assert_eq!(job["items"][0]["ambient"], json!([0.6, 0.6, 0.6]));
    assert!(Path::new(&r.out_dir).join("capture-run.json").is_file());
}

#[test]
fn attach_reports_compile_errors() {
    let env = Env::new("attach-compile");
    env.add_pipeline_package();
    let _lock = env.hold_lock();
    env.scenario(&env.cli_dir, &format!("instances={}|ready\nscenario=compile_error\n", env.project.display()));
    let r = env.run(&env.request(ITEMS, "attach"));
    assert_eq!(r.error.as_deref(), Some("attach:compile:CS0103: stub compile error"), "{r:#?}");
    assert_eq!(env.hook_lines().len(), 2, "after-hook still runs");
}

#[test]
fn open_editor_that_does_not_answer_is_blocked() {
    let env = Env::new("blocked");
    let _lock = env.hold_lock();
    let r = env.run(&env.request(ITEMS, "auto"));
    assert_eq!(r.error.as_deref(), Some("blocked:noPipelinePackage"));
    env.add_pipeline_package();
    env.scenario(&env.cli_dir, "instances=C:\\fake\\Other|ready\n");
    let r = env.run(&env.request(ITEMS, "auto"));
    assert_eq!(r.error.as_deref(), Some("blocked:notResponding"));
    env.scenario(&env.cli_dir, &format!("instances={}|compiling\n", env.project.display()));
    let r = env.run(&env.request(ITEMS, "auto"));
    assert_eq!(r.error.as_deref(), Some("blocked:editorBusy:compiling"));
    let r = env.run(&env.request(ITEMS, "batch"));
    assert_eq!(r.error.as_deref(), Some("blocked:openBatchOnly"));
    assert!(env.hook_lines().is_empty(), "no hook runs for a blocked capture");
}

#[test]
fn closed_project_with_attach_only_is_blocked() {
    let env = Env::new("attach-only");
    let r = env.run(&env.request(ITEMS, "attach"));
    assert_eq!(r.error.as_deref(), Some("blocked:closedAttachOnly"));
}

#[test]
fn batch_captures_puts_rewritten_meta_back_with_a_backup() {
    let env = Env::new("batch");
    env.scenario(&env.editor_dir, &format!("delay_ms=1500\ntouch={TOUCHED}\n"));
    let before = env.status();
    let mut stages = Vec::new();
    let r = capture(&env.home, &env.request(ITEMS, "auto"), &mut |e| stages.push(e.stage.clone()));
    assert_eq!(r.mode, "batch", "{r:#?}");
    assert!(r.ok, "{r:#?}");
    assert_eq!(r.exit_code, Some(0));
    for it in &r.items {
        assert_png(it.output.as_deref().unwrap());
    }
    let g = r.git.as_ref().unwrap();
    assert_eq!(g.restored, vec![TOUCHED.to_string()], "{g:#?}");
    assert!(g.same, "{g:#?}");
    let patch = std::fs::read_to_string(g.backup.as_ref().unwrap()).unwrap();
    assert!(patch.contains("stubImporter"), "{patch}");
    assert_eq!(env.status(), before);
    assert!(!entry_root(&env.project).exists());
    assert!(!env.project.join("Assets/__UnityIconStudioCapture.meta").exists());
    assert_eq!(r.entry_removed, Some(true));
    assert!(journal::list(&env.home).is_empty());
    let hooks = env.hook_lines();
    assert_eq!(hooks.iter().map(|h| h[0].as_str()).collect::<Vec<_>>(), ["before", "after"]);
    assert_eq!(hooks[0][1], "batch");
    assert!(stages.contains(&"unity".to_string()) && stages.last().map(String::as_str) == Some("done"), "{stages:?}");
    assert!(Path::new(r.log_file.as_ref().unwrap()).is_file());
}

#[test]
fn batch_item_failure_is_exit_2_with_the_rest_captured() {
    let env = Env::new("batch-fail");
    env.scenario(&env.editor_dir, "delay_ms=1000\n");
    let r = env.run(&env.request(&["Assets/Props/Crate.prefab", "Assets/Nope/Missing.prefab"], "batch"));
    assert_eq!(r.exit_code, Some(2), "{r:#?}");
    assert_eq!(r.error, None);
    assert!(!r.ok);
    assert_eq!(r.failed, 1);
    assert!(r.items[0].ok);
    assert_eq!(r.items[1].error.as_deref(), Some("prefab_not_found"));
    assert!(journal::list(&env.home).is_empty());
}

#[test]
fn batch_crash_reports_the_exit_code_and_log_tail() {
    let env = Env::new("batch-crash");
    env.scenario(&env.editor_dir, "delay_ms=800\nscenario=crash\n");
    let r = env.run(&env.request(ITEMS, "batch"));
    assert_eq!(r.error.as_deref(), Some("batchExit:4"), "{r:#?}");
    assert!(r.log_tail.iter().any(|l| l.contains("stub crash")), "{:?}", r.log_tail);
    assert!(!entry_root(&env.project).exists());
    assert_eq!(env.hook_lines().len(), 2);
}

#[test]
fn batch_timeout_keeps_the_journal_and_recover_finishes_it() {
    let env = Env::new("batch-timeout");
    env.scenario(&env.editor_dir, &format!("delay_ms=500\nscenario=hang\nhang_ms=6000\ntouch={TOUCHED}\n"));
    let before = env.status();
    let mut req = env.request(ITEMS, "batch");
    req.batch_timeout = Some(Duration::from_secs(1));
    let r = env.run(&req);
    assert_eq!(r.error.as_deref(), Some("batchTimeout"), "{r:#?}");
    assert!(r.pending);
    assert!(entry_root(&env.project).exists(), "the folder stays while Unity runs");
    let status = recovery_status(&env.home);
    assert_eq!(status.len(), 1);
    assert!(status[0].running, "{status:?}");
    assert!(recover(&env.home).is_empty(), "nothing is finished while Unity runs");
    assert_eq!(env.hook_lines().len(), 1, "after-hook waits for the end of Unity");
    // Wait for the stub to end on its own (never ended by the tool)
    let t = Instant::now();
    while uis_core::proc::pid_alive(status[0].pid) && t.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let done = recover(&env.home);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].exit_code, Some(0), "{:?}", done[0]);
    assert!(done[0].entry_removed);
    assert!(done[0].git.same, "{:?}", done[0].git);
    assert_eq!(done[0].git.restored, vec![TOUCHED.to_string()]);
    assert_eq!(done[0].error, None);
    assert!(journal::list(&env.home).is_empty());
    assert!(!entry_root(&env.project).exists());
    assert_eq!(env.status(), before);
    let hooks = env.hook_lines();
    assert_eq!(hooks.iter().map(|h| h[0].as_str()).collect::<Vec<_>>(), ["before", "after"]);
}

#[test]
fn refusing_hook_stops_the_capture() {
    let env = Env::new("hook-refuse");
    let exit = env.base.join("exit.exe").display().to_string();
    env.settings(json!({ "capture": { "hooks": { "beforeCapture": [{ "name": "lease", "run": [exit, "4", "leased by someone else"] }] } } }));
    let r = env.run(&env.request(ITEMS, "batch"));
    assert_eq!(r.error.as_deref(), Some("hookRefused:leased by someone else"), "{r:#?}");
    assert_eq!(r.hooks.len(), 1, "no after-hook: nothing was taken");
    assert!(!entry_root(&env.project).exists());
    assert!(journal::list(&env.home).is_empty());
    assert!(env.hook_lines().is_empty());
}

#[test]
fn another_unity_version_is_refused() {
    let env = Env::new("version");
    // Only another version is set up
    let mut s: Value = serde_json::from_str(&std::fs::read_to_string(env.home.settings_file()).unwrap()).unwrap();
    s["capture"]["editors"] = json!({ "6000.0.2f1": env.editor_dir.join("Unity.exe").display().to_string() });
    std::fs::write(env.home.settings_file(), s.to_string()).unwrap();
    let r = env.run(&env.request(ITEMS, "batch"));
    let err = r.error.clone().unwrap_or_default();
    assert!(err.starts_with("editorMissing:6000.0.1f1:"), "{err}");
    assert!(err.contains("6000.0.2f1"), "{err}");
    assert!(env.hook_lines().is_empty());
    assert!(!entry_root(&env.project).exists());
}

#[test]
fn entry_folder_of_the_same_name_is_refused() {
    let env = Env::new("entry-exists");
    let mine = entry_root(&env.project).join("Editor");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("Keep.cs"), "// someone's file").unwrap();
    let r = env.run(&env.request(ITEMS, "batch"));
    assert_eq!(r.error.as_deref(), Some("entryExists"), "{r:#?}");
    assert!(mine.join("Keep.cs").is_file(), "left untouched");
    assert!(env.hook_lines().is_empty());
}
