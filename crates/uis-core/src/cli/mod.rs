//! The two LLM command lines, run as throwaway sessions.
//!
//! - Codex (`codex exec --ephemeral`) is the only one that makes pictures (its built-in image tool);
//!   it can also rewrite text.
//! - Claude (`claude -p --no-session-persistence --safe-mode`) rewrites notes and can review a
//!   result; it makes no pictures.
//!
//! Every call gets a new empty folder under `<home>\runtime\tmp` as its working folder (removed
//! after), its prompt as UTF-8 bytes on stdin (no shell), a time limit, and an environment without
//! the variables that would tie it to another session or switch it to API billing. Each call is
//! written to `<home>\logs\llm.log` (time, CLI, purpose, time taken, result code).

pub mod claude;
pub mod codex;
pub mod errors;

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::home::Home;
use crate::packs::Pack;
use crate::prompt::{self, Assembled, Rewritten};
use crate::settings::Settings;
use errors::{FailKind, Failure};

pub const CLIS: &[&str] = &["claude", "codex"];

/// A review answer has a yes/no `pass` and (when present) a list of `issues`. Anything else (an
/// edit list, a sentence) is not a review, whatever the CLI's exit code said.
pub fn is_review_answer(v: &serde_json::Value) -> bool {
    v.get("pass").is_some_and(|p| p.is_boolean()) && v.get("issues").is_none_or(|i| i.is_array())
}

/// Where a CLI was found.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub path: PathBuf,
    /// `settings`, `copy` (the tool's copy of the desktop app's), `path`, `npm`, `profile`
    pub source: String,
    /// A `.cmd` that has to run through `cmd /d /c`
    pub via_cmd: bool,
}

/// `name.exe` (or `.cmd`) on PATH.
pub fn on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for n in names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// A fresh empty folder for one call.
pub fn temp_dir(home: &Home, tag: &str) -> Result<PathBuf, String> {
    let dir = home.runtime_dir().join("tmp").join(format!("{}-{tag}", crate::capture::new_run_id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("tmp:{e}"))?;
    Ok(dir)
}

pub fn remove_temp(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// One CLI call, as kept in reports and `llm.log`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub cli: String,
    /// `rewrite`, `image`, `review`, `probe`
    pub purpose: String,
    pub attempt: u32,
    pub elapsed_ms: u64,
    /// `ok` or a failure code
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

pub fn log_call(home: &Home, call: &Call) {
    let _ = std::fs::create_dir_all(home.logs_dir());
    let line = format!(
        "{}\t{}\t{}\t{}\t{}ms\t{}\n",
        crate::capture::new_run_id(),
        call.cli,
        call.purpose,
        call.attempt,
        call.elapsed_ms,
        call.code.replace(['\n', '\r', '\t'], " ")
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(home.logs_dir().join("llm.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Asks a CLI for a JSON answer that matches `schema`. Implemented by [`Clis`] (the real CLIs)
/// and by fakes in tests.
pub trait TextLlm {
    /// Ok: the parsed JSON object. The call is pushed to `calls` either way.
    fn ask(&self, cli: &str, purpose: &str, prompt: &str, schema: &str, calls: &mut Vec<Call>) -> Result<Value, Failure>;
    /// True when the CLI can be tried (found, not turned off).
    fn available(&self, cli: &str) -> bool;
}

/// The real CLIs, as the settings say.
pub struct Clis<'a> {
    pub home: &'a Home,
    pub settings: &'a Settings,
}

impl TextLlm for Clis<'_> {
    fn ask(&self, cli: &str, purpose: &str, prompt: &str, schema: &str, calls: &mut Vec<Call>) -> Result<Value, Failure> {
        let attempt = calls.iter().filter(|c| c.purpose == purpose).count() as u32 + 1;
        let (result, mut call) = match cli {
            "claude" => claude::ask_json(self.home, self.settings, prompt, schema),
            _ => codex::ask_json(self.home, self.settings, prompt, schema),
        };
        call.purpose = purpose.into();
        call.attempt = attempt;
        log_call(self.home, &call);
        calls.push(call);
        result
    }
    fn available(&self, cli: &str) -> bool {
        match cli {
            "claude" => self.settings.claude.enabled && claude::find(self.settings).is_some(),
            _ => codex::find(self.home, self.settings).is_some(),
        }
    }
}

/// What a rewrite gave.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewriteReport {
    pub ok: bool,
    pub note: String,
    /// The CLI that answered (None when the note was empty)
    pub cli: Option<String>,
    /// Set when the asked CLI could not be used and the other one answered
    pub fallback_from: Option<String>,
    pub fixed: Assembled,
    /// The prompt to send: the fixed text when the note was empty
    pub final_text: String,
    pub rewritten: Option<Rewritten>,
    pub diff: Value,
    pub calls: Vec<Call>,
    /// The instruction sent (last attempt)
    pub request: Option<String>,
    /// The LLM's answer (last attempt)
    pub response: Option<Value>,
    pub failure: Option<Failure>,
    /// What the person can do after a failure: `otherCli`, `withoutNote`, `cancel`
    pub choices: Vec<String>,
}

fn other(cli: &str) -> &'static str {
    if cli == "claude" {
        "codex"
    } else {
        "claude"
    }
}

/// Works `note` into `fixed`. An empty note calls nothing. `cli` is the one asked (None: the
/// settings' `llm.rewriter`). When it is not available, or Claude says it is not logged in, the
/// other CLI is used (`fallbackFrom`). A failure never falls back to the fixed prompt quietly.
pub fn rewrite(pack: &Pack, fixed: &Assembled, note: &str, cli: &str, llm: &dyn TextLlm) -> RewriteReport {
    let mut report = RewriteReport {
        ok: true,
        note: note.to_string(),
        cli: None,
        fallback_from: None,
        fixed: fixed.clone(),
        final_text: fixed.text.clone(),
        rewritten: None,
        diff: prompt::diff(fixed, None),
        calls: Vec::new(),
        request: None,
        response: None,
        failure: None,
        choices: Vec::new(),
    };
    if prompt::note_is_empty(note) {
        return report;
    }
    let mut use_cli = if CLIS.contains(&cli) { cli } else { "claude" };
    if !llm.available(use_cli) {
        if llm.available(other(use_cli)) {
            report.fallback_from = Some(use_cli.into());
            use_cli = other(use_cli);
        } else {
            report.ok = false;
            report.failure = Some(Failure::new(FailKind::NotFound, use_cli, "no CLI found for rewriting"));
            report.choices = vec!["withoutNote".into(), "cancel".into()];
            return report;
        }
    }
    let mut retry: Option<String> = None;
    let mut switched = false;
    loop {
        let request = prompt::rewrite_prompt(fixed, note, retry.as_deref());
        report.request = Some(request.clone());
        report.cli = Some(use_cli.into());
        match llm.ask(use_cli, "rewrite", &request, prompt::REWRITE_SCHEMA, &mut report.calls) {
            Ok(value) => {
                report.response = Some(value.clone());
                match prompt::parse_edits(&value) {
                    Ok(edits) => match prompt::apply_edits(pack, fixed, &edits) {
                        Ok(r) => {
                            report.final_text = r.text.clone();
                            report.diff = prompt::diff(fixed, Some(&r));
                            report.rewritten = Some(r);
                            return report;
                        }
                        Err(e) => {
                            report.ok = false;
                            report.failure = Some(Failure::new(FailKind::BadJson, use_cli, e));
                            break;
                        }
                    },
                    Err(why) if retry.is_none() => retry = Some(why),
                    Err(why) => {
                        report.ok = false;
                        report.failure = Some(Failure::new(FailKind::BadJson, use_cli, why));
                        break;
                    }
                }
            }
            Err(f) if f.kind == FailKind::BadJson && retry.is_none() => retry = Some(f.message.clone()),
            Err(f) if f.kind == FailKind::Auth && use_cli == "claude" && !switched && report.fallback_from.is_none() && llm.available("codex") => {
                report.fallback_from = Some("claude".into());
                use_cli = "codex";
                switched = true;
                retry = None;
            }
            Err(f) => {
                report.ok = false;
                report.failure = Some(f);
                break;
            }
        }
    }
    report.final_text = fixed.text.clone();
    report.choices = vec!["otherCli".into(), "withoutNote".into(), "cancel".into()];
    report
}

/// The state of one CLI for the readiness list.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliState {
    pub found: bool,
    pub path: Option<String>,
    pub source: Option<String>,
    pub version: Option<String>,
    pub logged_in: Option<bool>,
    /// Codex: the image runner (`codex-code-mode-host.exe`) sits next to it
    pub image_tool: Option<bool>,
    pub enabled: bool,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub codex: CliState,
    pub claude: CliState,
    pub unity_cli: CliState,
    /// The Codex desktop app, when installed (its files can be copied to the tool's folder)
    pub codex_app: Option<codex::AppInfo>,
    /// The app's Codex is newer than the tool's copy, or there is no copy yet
    pub codex_copy_needed: bool,
    pub elapsed_ms: u64,
}

/// Checks the CLIs side by side (each call at most 20 seconds).
pub fn readiness(home: &Home, settings: &Settings) -> Readiness {
    let started = std::time::Instant::now();
    let (codex, claude, unity, app) = std::thread::scope(|s| {
        let a = s.spawn(|| codex::state(home, settings));
        let b = s.spawn(|| claude::state(settings));
        let c = s.spawn(|| unity_state(settings));
        let d = s.spawn(|| codex::app_info(home).ok());
        (a.join().unwrap_or_default(), b.join().unwrap_or_default(), c.join().unwrap_or_default(), d.join().ok().flatten())
    });
    let copy = codex::current_copy(home);
    let codex_copy_needed = match (&app, &copy) {
        (Some(a), Some(c)) => a.version != c.app_version,
        (Some(_), None) => codex.source.as_deref() != Some("settings") && codex.source.as_deref() != Some("path"),
        _ => false,
    };
    Readiness { codex, claude, unity_cli: unity, codex_app: app, codex_copy_needed, elapsed_ms: started.elapsed().as_millis() as u64 }
}

fn unity_state(settings: &Settings) -> CliState {
    match crate::unity::find::find_unity_cli(&settings.capture) {
        Ok(path) => CliState {
            found: true,
            version: crate::unity::attach::cli_version(&path),
            path: Some(path.display().to_string()),
            enabled: true,
            ..CliState::default()
        },
        Err(e) => CliState { found: false, enabled: true, problem: Some(e), ..CliState::default() },
    }
}

/// First line of a version print (`codex-cli 0.171.0` / `2.1.322 (Claude Code)`).
pub fn version_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(80).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::PromptContext;
    use serde_json::json;
    use std::cell::RefCell;

    /// Answers from a list, one per call; records what it was asked.
    struct Fake {
        answers: RefCell<Vec<Result<Value, Failure>>>,
        asked: RefCell<Vec<(String, String)>>,
        missing: Vec<&'static str>,
    }

    impl Fake {
        fn new(answers: Vec<Result<Value, Failure>>) -> Fake {
            Fake { answers: RefCell::new(answers), asked: RefCell::new(Vec::new()), missing: Vec::new() }
        }
    }

    impl TextLlm for Fake {
        fn ask(&self, cli: &str, purpose: &str, prompt: &str, _schema: &str, calls: &mut Vec<Call>) -> Result<Value, Failure> {
            self.asked.borrow_mut().push((cli.into(), prompt.into()));
            let r = self.answers.borrow_mut().remove(0);
            calls.push(Call {
                cli: cli.into(),
                purpose: purpose.into(),
                attempt: calls.len() as u32 + 1,
                elapsed_ms: 1,
                code: r.as_ref().map(|_| "ok".to_string()).unwrap_or_else(|f| f.code()),
                cost_usd: None,
            });
            r
        }
        fn available(&self, cli: &str) -> bool {
            !self.missing.contains(&cli)
        }
    }

    fn fixed() -> (Pack, Assembled) {
        let pack = Pack::builtin();
        let a = prompt::assemble(&pack, &PromptContext::for_kind(&pack, "building")).unwrap();
        (pack, a)
    }

    fn good() -> Value {
        json!({ "replace": [{ "id": "STYLE.color", "text": "Color: cool winter palette with a blue roof; no haze, no fog, no bloom." }],
                "add": [], "avoidAdd": [], "outline": "keep", "camera": "keep", "rejected": [], "summaryKo": "파란 지붕" })
    }

    #[test]
    fn empty_note_calls_nothing_and_sends_the_fixed_bytes() {
        let (pack, a) = fixed();
        let fake = Fake::new(Vec::new());
        for note in ["", "   ", "\r\n\t"] {
            let r = rewrite(&pack, &a, note, "claude", &fake);
            assert!(r.ok && r.calls.is_empty() && r.cli.is_none());
            assert_eq!(r.final_text.as_bytes(), a.text.as_bytes());
        }
        assert!(fake.asked.borrow().is_empty());
    }

    #[test]
    fn a_note_is_worked_in() {
        let (pack, a) = fixed();
        let fake = Fake::new(vec![Ok(good())]);
        let r = rewrite(&pack, &a, "지붕을 파란색으로", "claude", &fake);
        assert!(r.ok, "{:?}", r.failure);
        assert_eq!(r.cli.as_deref(), Some("claude"));
        assert!(r.final_text.contains("blue roof"));
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.diff["changed"], json!(true));
    }

    #[test]
    fn a_bad_answer_is_asked_once_more_then_fails_without_falling_back() {
        let (pack, a) = fixed();
        let bad = json!({ "replace": [{ "id": "STYLE.color", "text": "파란 지붕" }], "add": [], "avoidAdd": [], "outline": "keep", "camera": "keep", "rejected": [], "summaryKo": "" });
        let fake = Fake::new(vec![Ok(bad.clone()), Ok(good())]);
        let r = rewrite(&pack, &a, "지붕을 파란색으로", "codex", &fake);
        assert!(r.ok);
        assert_eq!(r.calls.len(), 2);
        assert!(fake.asked.borrow()[1].1.contains("could not be used"));

        let fake = Fake::new(vec![Ok(bad.clone()), Ok(bad)]);
        let r = rewrite(&pack, &a, "지붕을 파란색으로", "codex", &fake);
        assert!(!r.ok);
        assert_eq!(r.failure.as_ref().unwrap().kind, FailKind::BadJson);
        assert_eq!(r.final_text, a.text, "nothing applied");
        assert_eq!(r.choices, ["otherCli", "withoutNote", "cancel"]);
    }

    #[test]
    fn missing_claude_falls_back_to_codex() {
        let (pack, a) = fixed();
        let mut fake = Fake::new(vec![Ok(good())]);
        fake.missing = vec!["claude"];
        let r = rewrite(&pack, &a, "겨울", "claude", &fake);
        assert!(r.ok);
        assert_eq!((r.cli.as_deref(), r.fallback_from.as_deref()), (Some("codex"), Some("claude")));
    }

    #[test]
    fn claude_not_logged_in_falls_back_once() {
        let (pack, a) = fixed();
        let fake = Fake::new(vec![Err(Failure::new(FailKind::Auth, "claude", "Failed to authenticate")), Ok(good())]);
        let r = rewrite(&pack, &a, "겨울", "claude", &fake);
        assert!(r.ok);
        assert_eq!(r.fallback_from.as_deref(), Some("claude"));
        assert_eq!(r.calls.iter().map(|c| c.cli.as_str()).collect::<Vec<_>>(), ["claude", "codex"]);
    }

    #[test]
    fn limit_stops_with_choices() {
        let (pack, a) = fixed();
        let fake = Fake::new(vec![Err(Failure::new(FailKind::Limit, "codex", "usage limit"))]);
        let r = rewrite(&pack, &a, "겨울", "codex", &fake);
        assert!(!r.ok);
        assert_eq!(r.failure.unwrap().kind, FailKind::Limit);
        assert_eq!(r.calls.len(), 1);
    }

    #[test]
    fn nothing_found() {
        let (pack, a) = fixed();
        let mut fake = Fake::new(Vec::new());
        fake.missing = vec!["claude", "codex"];
        let r = rewrite(&pack, &a, "겨울", "claude", &fake);
        assert_eq!(r.failure.unwrap().kind, FailKind::NotFound);
        assert!(r.calls.is_empty());
    }
}
