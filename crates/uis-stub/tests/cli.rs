//! Prompt, rewrite and picture runs against the stub `codex` and `claude` (no limits spent).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uis_core::cli::errors::FailKind;
use uis_core::generate::{self, GenerateRequest};
use uis_core::home::Home;
use uis_core::settings::Settings;

/// Tests that change this process's environment run one at a time.
static ENV: Mutex<()> = Mutex::new(());

struct Bed {
    root: PathBuf,
    home: Home,
    bin: PathBuf,
    codex_home: PathBuf,
}

impl Bed {
    fn new(tag: &str, scenario: &str) -> Bed {
        let root = std::env::temp_dir().join(format!("uis-cli-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let stub = PathBuf::from(env!("CARGO_BIN_EXE_uis-stub"));
        for name in ["codex", "claude", "codex-code-mode-host"] {
            std::fs::copy(&stub, bin.join(format!("{name}.exe"))).unwrap();
        }
        std::fs::write(bin.join("stub-scenario.txt"), scenario).unwrap();
        let home = Home::at(root.join("home"));
        home.ensure().unwrap();
        let mut s = Settings::default();
        s.codex.path = Some(bin.join("codex.exe").display().to_string());
        s.claude.path = Some(bin.join("claude.exe").display().to_string());
        s.claude.timeout_sec = 10;
        s.codex.text_timeout_sec = 10;
        std::fs::write(home.settings_file(), serde_json::to_string_pretty(&s).unwrap()).unwrap();
        let codex_home = root.join("codex-home");
        Bed { root, home, bin, codex_home }
    }

    fn settings(&self) -> Settings {
        uis_core::settings::load(&self.home)
    }

    fn capture(&self) -> PathBuf {
        let p = self.root.join("capture.png");
        let img = image::RgbaImage::from_fn(256, 256, |x, y| {
            let inside = (64..192).contains(&x) && (48..208).contains(&y);
            image::Rgba([180, 90 + (y % 60) as u8, 60, if inside { 255 } else { 0 }])
        });
        img.save(&p).unwrap();
        p
    }

    fn request(&self, kind: &str, note: &str) -> GenerateRequest {
        GenerateRequest {
            input: self.capture(),
            kind: kind.into(),
            note: note.into(),
            out_dir: Some(self.root.join("job")),
            codex_home: Some(self.codex_home.clone()),
            ..GenerateRequest::default()
        }
    }

    /// `calls.log` lines as (role, kind, env names)
    fn calls(&self) -> Vec<(String, String, String)> {
        std::fs::read_to_string(self.bin.join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (f[0].to_string(), f[1].to_string(), f.get(3).unwrap_or(&"").to_string())
            })
            .collect()
    }

    fn stdin(&self, role: &str, n: usize) -> Vec<u8> {
        std::fs::read(self.bin.join(format!("{role}-stdin-{n}.txt"))).unwrap()
    }

    fn reply(&self, n: usize, v: Value) {
        std::fs::write(self.bin.join(format!("reply-{n}.json")), v.to_string()).unwrap();
    }
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn readiness_finds_both_and_the_image_runner() {
    let bed = Bed::new("ready", "");
    let r = uis_core::cli::readiness(&bed.home, &bed.settings());
    assert!(r.codex.found && r.claude.found);
    assert_eq!(r.codex.source.as_deref(), Some("settings"));
    assert_eq!(r.codex.version.as_deref(), Some("codex-cli 0.0.0-stub"));
    assert_eq!((r.codex.logged_in, r.codex.image_tool), (Some(true), Some(true)));
    assert_eq!(r.claude.logged_in, Some(true));
    let bed = Bed::new("ready-out", "codex_login=no\nclaude_login=no\n");
    std::fs::remove_file(bed.bin.join("codex-code-mode-host.exe")).unwrap();
    let r = uis_core::cli::readiness(&bed.home, &bed.settings());
    assert_eq!((r.codex.logged_in, r.codex.image_tool, r.claude.logged_in), (Some(false), Some(false), Some(false)));
}

#[test]
fn empty_note_sends_the_fixed_prompt_bytes_and_calls_no_llm() {
    let bed = Bed::new("empty", "");
    let r = generate::generate(&bed.home, &bed.settings(), &bed.request("building", "  \n "));
    assert!(r.ok, "{:?} {:?}", r.error, r.candidates.first().map(|c| &c.failure));
    let calls = bed.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!((calls[0].0.as_str(), calls[0].1.as_str()), ("codex", "image"));
    let job = bed.root.join("job");
    let fixed = std::fs::read(job.join("prompt/fixed.txt")).unwrap();
    assert_eq!(fixed, std::fs::read(job.join("prompt/final.txt")).unwrap());
    let sent = bed.stdin("codex", 1);
    let start = sent.windows(12).position(|w| w == b"PROMPT: <<<\n").unwrap() + 12;
    let end = sent.len() - b"\n>>>\n".len();
    assert_eq!(&sent[start..end], &fixed[..], "the fixed bytes are what was sent");
    assert!(!job.join("prompt/rewrite-request.txt").exists());
    let c = &r.candidates[0];
    assert_eq!((c.harvest_stage, c.width, c.has_alpha), (Some(2), 320, true));
    assert!(job.join("gen/cand-01/raw.png").is_file());
    assert!(c.cleaned);
    assert!(!bed.codex_home.join("generated_images").join(c.thread_id.as_deref().unwrap()).exists());
    let jobj: Value = serde_json::from_str(&read(&job.join("job.json"))).unwrap();
    assert_eq!(jobj["schema"], json!("unity-icon-studio.job/1"));
    assert_eq!(jobj["rewriteCalls"], json!([]));
    // The capture went in on white, opaque
    let onwhite = image::open(job.join("capture/capture-onwhite.png")).unwrap();
    assert!(!onwhite.color().has_alpha());
}

#[test]
fn harvest_from_events_and_from_the_thread_folder() {
    for (stage, sc) in [(1u8, "codex_harvest=1\n"), (3u8, "codex_harvest=3\n")] {
        let bed = Bed::new(&format!("harvest{stage}"), sc);
        let r = generate::generate(&bed.home, &bed.settings(), &bed.request("prop", ""));
        assert!(r.ok, "{:?}", r.candidates.first().map(|c| &c.failure));
        assert_eq!(r.candidates[0].harvest_stage, Some(stage));
    }
}

#[test]
fn a_note_is_rewritten_by_claude_with_a_clean_environment() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let vars = [("CLAUDE_EFFORT", "max"), ("ANTHROPIC_API_KEY", "sk-test"), ("OPENAI_API_KEY", "sk-test2"), ("CLAUDECODE", "1")];
    let saved: Vec<_> = vars.iter().map(|(k, _)| (*k, std::env::var_os(k))).collect();
    for (k, v) in vars {
        std::env::set_var(k, v);
    }
    let bed = Bed::new("note", "");
    let r = generate::generate(&bed.home, &bed.settings(), &bed.request("building", "지붕을 파란색으로, 겨울 느낌"));
    for (k, v) in saved {
        match v {
            Some(v) => std::env::set_var(k, v),
            None => std::env::remove_var(k),
        }
    }
    assert!(r.ok, "{:?}", r.error);
    let rw = r.rewrite.as_ref().unwrap();
    assert_eq!(rw.cli.as_deref(), Some("claude"));
    let calls = bed.calls();
    assert_eq!(calls.iter().map(|c| (c.0.as_str(), c.1.as_str())).collect::<Vec<_>>(), [("claude", "text"), ("codex", "image")]);
    // No session or key variables reached Claude (its own config folder may)
    let claude_side = |n: &&str| n.to_uppercase().starts_with("CLAUDE") || n.to_uppercase().starts_with("ANTHROPIC");
    for name in calls[0].2.split(',').filter(claude_side) {
        assert_eq!(name.to_uppercase(), "CLAUDE_CONFIG_DIR", "leaked {name}");
    }
    assert!(!calls[1].2.split(',').any(|n| n.eq_ignore_ascii_case("OPENAI_API_KEY")), "{}", calls[1].2);
    // The note went to Claude as UTF-8
    let asked = String::from_utf8(bed.stdin("claude", 1)).unwrap();
    assert!(asked.contains("\"note\": \"지붕을 파란색으로, 겨울 느낌\""));
    let job = bed.root.join("job");
    let fin = read(&job.join("prompt/final.txt"));
    assert!(fin.contains("sky-blue roof") && fin.contains("A thin layer of snow rests on the roof."));
    let fixed = uis_core::prompt::assemble(&uis_core::packs::Pack::builtin(), &generate::context(&uis_core::packs::Pack::builtin(), &bed.settings(), "building", 0, None, None)).unwrap();
    for s in fixed.sentences.iter().filter(|s| s.locked) {
        assert!(fin.contains(&s.text), "locked {} kept", s.id);
    }
    let diff: Value = serde_json::from_str(&read(&job.join("prompt/diff.json"))).unwrap();
    assert_eq!(diff["changed"], json!(true));
    // The final text is what went to Codex
    let sent = String::from_utf8(bed.stdin("codex", 1)).unwrap();
    assert!(sent.contains(&fin));
}

#[test]
fn codex_rewrites_too() {
    let bed = Bed::new("codex-text", "");
    let mut req = bed.request("building", "겨울 느낌");
    req.rewriter = Some("codex".into());
    req.prompt_only = true;
    let r = generate::generate(&bed.home, &bed.settings(), &req);
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(bed.calls().iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["text"]);
    assert!(r.final_text.contains("sky-blue roof"));
}

#[test]
fn locked_edits_and_forbidden_parts_are_rejected() {
    let bed = Bed::new("rejected", "");
    bed.reply(1, json!({
        "replace": [{ "id": "OUTPUT.spec", "text": "Output: an icon on a snowy village background." }],
        "add": [{ "block": "SUBJECT", "text": "Add the game logo above the door." }],
        "avoidAdd": [], "outline": "keep", "camera": "keep",
        "rejected": [{ "part": "배경을 넣고", "reason": "배경 금지" }], "summaryKo": "적용할 것 없음"
    }));
    let mut req = bed.request("building", "배경을 넣고 게임 로고 추가");
    req.prompt_only = true;
    let r = generate::generate(&bed.home, &bed.settings(), &req);
    assert!(r.ok);
    let rw = r.rewrite.unwrap().rewritten.unwrap();
    assert!(!rw.changed);
    assert_eq!(r.final_text, r.fixed);
    let reasons: Vec<&str> = rw.rejected.iter().map(|x| x.reason.as_str()).collect();
    assert_eq!(reasons, ["배경 금지", "locked:OUTPUT.spec", "forbidden:logo"]);
}

#[test]
fn claude_not_signed_in_falls_back_to_codex() {
    let bed = Bed::new("fallback", "claude=auth\n");
    let mut req = bed.request("prop", "빨간 리본");
    req.prompt_only = true;
    let r = generate::generate(&bed.home, &bed.settings(), &req);
    assert!(r.ok, "{:?}", r.error);
    let rw = r.rewrite.unwrap();
    assert_eq!((rw.cli.as_deref(), rw.fallback_from.as_deref()), (Some("codex"), Some("claude")));
    assert_eq!(rw.calls.iter().map(|c| c.cli.as_str()).collect::<Vec<_>>(), ["claude", "codex"]);
}

#[test]
fn rewrite_failures_stop_before_any_picture() {
    // A model error: no picture, no quiet fallback to the fixed prompt
    let bed = Bed::new("rw-model", "claude=model\n");
    let r = generate::generate(&bed.home, &bed.settings(), &bed.request("prop", "빨간 리본"));
    assert!(!r.ok);
    assert!(r.error.as_deref().unwrap().starts_with("rewrite:model:"), "{:?}", r.error);
    assert!(r.candidates.is_empty());
    assert!(!bed.calls().iter().any(|c| c.0 == "codex"));
    assert!(!bed.root.join("job/prompt/final.txt").exists());
    // Not JSON twice: one retry, then a failure with choices
    let bed = Bed::new("rw-json", "claude=bad_json\n");
    let r = generate::generate(&bed.home, &bed.settings(), &bed.request("prop", "빨간 리본"));
    let rw = r.rewrite.unwrap();
    assert_eq!(rw.calls.len(), 2);
    assert_eq!(rw.failure.unwrap().kind, FailKind::BadJson);
    assert_eq!(rw.choices, ["otherCli", "withoutNote", "cancel"]);
}

#[test]
fn claude_time_limit() {
    let bed = Bed::new("rw-timeout", "claude=timeout\n");
    let mut req = bed.request("prop", "빨간 리본");
    req.prompt_only = true;
    let started = std::time::Instant::now();
    let r = generate::generate(&bed.home, &bed.settings(), &req);
    assert!(started.elapsed().as_secs() < 25);
    assert_eq!(r.rewrite.unwrap().failure.unwrap().kind, FailKind::Timeout);
}

#[test]
fn picture_failures_are_classified() {
    let cases = [
        ("limit", FailKind::Limit),
        ("auth", FailKind::Auth),
        ("model", FailKind::Model),
        ("no_tool", FailKind::NoImageTool),
        ("refused", FailKind::Refused),
        ("no_image", FailKind::NoImage),
    ];
    for (sc, kind) in cases {
        let bed = Bed::new(&format!("img-{sc}"), &format!("codex={sc}\n"));
        let mut req = bed.request("building", "");
        req.candidates = Some(2);
        let r = generate::generate(&bed.home, &bed.settings(), &req);
        assert!(!r.ok, "{sc}");
        let c = &r.candidates[0];
        assert_eq!(c.failure.as_ref().unwrap().kind, kind, "{sc}: {:?}", c.failure);
        assert!(c.raw.is_none());
        // A limit or sign-in problem stops the queue; the others go on to the next candidate
        let want = if matches!(kind, FailKind::Limit | FailKind::Auth) { 1 } else { 2 };
        assert_eq!(r.candidates.len(), want, "{sc}");
        let meta: Value = serde_json::from_str(&read(&bed.root.join("job/gen/cand-01/meta.json"))).unwrap();
        assert!(meta["code"].as_str().unwrap().starts_with(kind.code()), "{sc}: {meta}");
    }
}

#[test]
fn every_call_is_logged() {
    let bed = Bed::new("log", "");
    let r = generate::generate(&bed.home, &bed.settings(), &bed.request("building", "겨울"));
    assert!(r.ok);
    let log = read(&bed.home.logs_dir().join("llm.log"));
    let purposes: Vec<&str> = log.lines().map(|l| l.split('\t').nth(2).unwrap()).collect();
    assert_eq!(purposes, ["rewrite", "image"]);
    assert_eq!(generate::images_today(&bed.home), 1);
}

#[test]
fn review_with_claude() {
    let bed = Bed::new("review", "");
    let cap = bed.capture();
    let (r, call) = uis_core::cli::claude::review(&bed.home, &bed.settings(), &cap, &cap, &uis_core::cli::claude::review_prompt("building", true));
    assert_eq!(r.unwrap(), json!({ "pass": true, "issues": [] }));
    assert_eq!(call.code, "ok");
    assert_eq!(bed.calls()[0].1, "review");
}

#[test]
fn picture_then_post_checks_and_export() {
    let bed = Bed::new("post", "");
    let mut req = bed.request("prop", "");
    req.name = Some("Crate".into());
    let r = generate::generate(&bed.home, &bed.settings(), &req);
    assert!(r.ok, "{:?}", r.error);
    // The session is kept lean by default
    let args = read(&bed.bin.join("codex-args-1.txt"));
    assert!(args.contains("--disable\napps") && args.contains("skills.include_instructions=false"), "{args}");
    let job = bed.root.join("job");
    let p = uis_core::jobs::post_candidate(&bed.home, &bed.settings(), &job, 1, &json!({})).expect("post");
    assert_eq!(p.post.alpha_source, "native");
    assert!(job.join("post/cand-01/icon-256.png").is_file() && job.join("post/cand-01/master-1024.png").is_file());
    assert!(job.join("post/cand-01/checks.json").is_file() && job.join("post/cand-01/options.json").is_file());
    let s = &p.sizes[0];
    // The stub picture is dark near its corner, so only the stroke color is checked here
    assert!(s.checks.metrics.outline_delta_e < 2.0, "{}", s.checks.metrics.outline_color);
    assert!(p.generated.iou.unwrap() > 0.8, "{:?}", p.generated.iou);
    // Overrides from the command line or the window
    let q = uis_core::jobs::post_candidate(&bed.home, &bed.settings(), &job, 1, &json!({ "outline": "none" })).unwrap();
    assert_eq!((q.options.outline.as_str(), q.post.layout.stroke_px), ("none", 0.0));
    assert!(!q.sizes[0].checks.items.iter().any(|i| i.group == "outline"));
    // Export: out of the job, never into a Unity project
    let out = bed.root.join("out");
    let icons = vec![(256, job.join("post/cand-01/icon-256.png"))];
    let e = uis_core::export::export(&icons, &out, "icon_{prefab}", "Crate", &bed.settings(), Some(&job));
    assert!(e.ok && out.join("icon_Crate.png").is_file(), "{:?}", e.error);
    let proj = bed.root.join("Game");
    std::fs::create_dir_all(proj.join("Assets")).unwrap();
    std::fs::create_dir_all(proj.join("ProjectSettings")).unwrap();
    let e = uis_core::export::export(&icons, &proj.join("Assets/UI"), "icon_{prefab}", "Crate", &bed.settings(), None);
    assert!(!e.ok && e.error.unwrap().starts_with("insideProject:"));
    let jobj: Value = serde_json::from_str(&read(&job.join("job.json"))).unwrap();
    assert_eq!(jobj["name"], json!("Crate"));
}

#[test]
fn lean_session_flags_can_be_turned_off() {
    let bed = Bed::new("notrim", "");
    let mut s = bed.settings();
    s.codex.trim_context = false;
    let r = generate::generate(&bed.home, &s, &bed.request("building", ""));
    assert!(r.ok);
    let args = read(&bed.bin.join("codex-args-1.txt"));
    assert!(!args.contains("--disable") && args.contains("--ignore-user-config"), "{args}");
}
