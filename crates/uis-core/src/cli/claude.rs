//! Claude Code CLI as a throwaway session: `-p --no-session-persistence --safe-mode`.
//!
//! - `--safe-mode` keeps the person's CLAUDE.md, rules, skills, plugins, hooks and MCP servers out
//!   of the call (without it one short call carried about 180 thousand input tokens). It works with
//!   a subscription login (`--bare` does not: it reads only API keys).
//! - The child gets no `CLAUDE*` or `ANTHROPIC_*` variables (except `CLAUDE_CONFIG_DIR`, where a
//!   person keeps their login): a session variable would tie it to the session that started the
//!   tool, and an API key would quietly switch it to API billing.
//! - Answers: one JSON object; `structured_output` holds the answer that matches `--json-schema`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::errors::{self, FailKind, Failure};
use super::{Call, CliState, Found};
use crate::home::Home;
use crate::proc::{self, Cmd, CLAUDE_ENV_PREFIXES};
use crate::settings::Settings;

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const KEEP_VAR: &str = "CLAUDE_CONFIG_DIR";

pub fn find(settings: &Settings) -> Option<Found> {
    // A path in the settings is the one to use: when it is wrong, say so instead of quietly
    // running another copy
    if let Some(p) = settings.claude.path.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()).map(PathBuf::from) {
        if !p.is_file() {
            return None;
        }
        let via_cmd = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd"));
        return Some(Found { path: p, source: "settings".into(), via_cmd });
    }
    if let Some(p) = super::on_path(&["claude.exe"]) {
        return Some(Found { path: p, source: "path".into(), via_cmd: false });
    }
    let profile = std::env::var_os("USERPROFILE").map(PathBuf::from)?;
    let p = profile.join(".local").join("bin").join("claude.exe");
    if p.is_file() {
        return Some(Found { path: p, source: "profile".into(), via_cmd: false });
    }
    super::on_path(&["claude.cmd"]).map(|p| Found { path: p, source: "path".into(), via_cmd: true })
}

/// The command with a clean environment.
pub fn command(found: &Found) -> Cmd {
    let base = if found.via_cmd { Cmd::new(proc::system_exe("cmd.exe")).args(["/d", "/c"]).arg(found.path.clone()) } else { Cmd::new(&found.path) };
    let mut cmd = base.remove_env_prefixes(CLAUDE_ENV_PREFIXES);
    if let Some(v) = std::env::var_os(KEEP_VAR) {
        cmd = cmd.env(KEEP_VAR, v.to_string_lossy().into_owned());
    }
    cmd
}

pub fn state(settings: &Settings) -> CliState {
    let enabled = settings.claude.enabled;
    let Some(found) = find(settings) else {
        return CliState { found: false, enabled, problem: Some("notFound".into()), ..CliState::default() };
    };
    let mut st = CliState {
        found: true,
        path: Some(found.path.display().to_string()),
        source: Some(found.source.clone()),
        enabled,
        ..CliState::default()
    };
    match proc::run(&command(&found).arg("--version").timeout(PROBE_TIMEOUT)) {
        Ok(out) if out.success() => st.version = super::version_line(&out.stdout_text()),
        Ok(out) => st.problem = Some(format!("version:{}", out.code())),
        Err(e) => {
            st.problem = Some(e);
            return st;
        }
    }
    if let Ok(out) = proc::run(&command(&found).args(["auth", "status"]).timeout(PROBE_TIMEOUT)) {
        let v: Option<Value> = serde_json::from_str(out.stdout_text().trim()).ok();
        st.logged_in = v.and_then(|v| v.get("loggedIn").and_then(Value::as_bool)).or(Some(false));
    }
    st
}

fn call(purpose: &str, out: Option<&proc::Output>, failure: Option<&Failure>, cost: Option<f64>) -> Call {
    Call {
        cli: "claude".into(),
        purpose: purpose.into(),
        attempt: 1,
        elapsed_ms: out.map_or(0, |o| o.elapsed_ms),
        code: failure.map(Failure::code).unwrap_or_else(|| "ok".into()),
        cost_usd: cost,
    }
}

fn compact(schema: &str) -> String {
    serde_json::from_str::<Value>(schema).map(|v| v.to_string()).unwrap_or_else(|_| schema.to_string())
}

/// Runs one `-p` call in `dir` with `tools` (`""` for none) and returns the `structured_output`.
fn run_json(found: &Found, settings: &Settings, dir: &Path, prompt: &str, schema: &str, tools: &str, purpose: &str) -> (Result<Value, Failure>, Call) {
    let mut cmd = command(found)
        .args(["-p", "--no-session-persistence", "--safe-mode", "--tools", tools])
        .arg("--model")
        .arg(&settings.claude.model)
        .arg("--effort")
        .arg(&settings.claude.effort)
        .args(["--output-format", "json", "--json-schema"])
        .arg(compact(schema));
    if !tools.is_empty() {
        cmd = cmd.arg("--allowedTools").arg(tools);
    }
    let cmd = cmd.cwd(dir).stdin(prompt.as_bytes().to_vec()).timeout(Duration::from_secs(settings.claude.timeout_sec as u64));
    let out = match proc::run(&cmd) {
        Ok(o) => o,
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "claude", e);
            return (Err(f.clone()), call(purpose, None, Some(&f), None));
        }
    };
    let json: Option<Value> = serde_json::from_str(out.stdout_text().trim()).ok();
    let cost = json.as_ref().and_then(|j| j.get("total_cost_usd")).and_then(Value::as_f64);
    if let Some(f) = errors::classify_claude(&out, json.as_ref()) {
        let c = call(purpose, Some(&out), Some(&f), cost);
        return (Err(f), c);
    }
    let value = json.and_then(|j| j.get("structured_output").cloned()).unwrap_or(Value::Null);
    (Ok(value), call(purpose, Some(&out), None, cost))
}

/// A JSON answer that matches `schema`, no tools.
pub fn ask_json(home: &Home, settings: &Settings, prompt: &str, schema: &str) -> (Result<Value, Failure>, Call) {
    let Some(found) = find(settings).filter(|_| settings.claude.enabled) else {
        let f = Failure::new(FailKind::NotFound, "claude", if settings.claude.enabled { "claude not found" } else { "claude is turned off" });
        return (Err(f.clone()), call("rewrite", None, Some(&f), None));
    };
    let dir = match super::temp_dir(home, "claude") {
        Ok(d) => d,
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "claude", e);
            return (Err(f.clone()), call("rewrite", None, Some(&f), None));
        }
    };
    let r = run_json(&found, settings, &dir, prompt, schema, "", "rewrite");
    super::remove_temp(&dir);
    r
}

/// Review of a candidate against the capture: both copied into the call's folder as `a.png` and
/// `b.png`, read with the Read tool only.
pub fn review(home: &Home, settings: &Settings, capture: &Path, candidate: &Path, prompt: &str) -> (Result<Value, Failure>, Call) {
    let Some(found) = find(settings).filter(|_| settings.claude.enabled) else {
        let f = Failure::new(FailKind::NotFound, "claude", "claude not found");
        return (Err(f.clone()), call("review", None, Some(&f), None));
    };
    let dir = match super::temp_dir(home, "claude-review") {
        Ok(d) => d,
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "claude", e);
            return (Err(f.clone()), call("review", None, Some(&f), None));
        }
    };
    let copied = std::fs::copy(capture, dir.join("a.png")).and_then(|_| std::fs::copy(candidate, dir.join("b.png")));
    let r = match copied {
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "claude", format!("copy:{e}"));
            (Err(f.clone()), call("review", None, Some(&f), None))
        }
        Ok(_) => run_json(&found, settings, &dir, prompt, crate::prompt::REVIEW_SCHEMA, "Read", "review"),
    };
    super::remove_temp(&dir);
    super::log_call(home, &r.1);
    r
}

/// The review instruction (the same for both CLIs; Claude reads `./a.png` and `./b.png`).
pub fn review_prompt(kind: &str, with_files: bool) -> String {
    let pics = if with_files {
        "Open ./a.png (the raw 3D capture) and ./b.png (the generated icon) with the Read tool."
    } else {
        "Input image 1 is the raw 3D capture and input image 2 is the generated icon."
    };
    format!(
        "{pics} Judge whether b is a usable casual mobile-game icon of the same {kind}: same subject, shape, parts and \
color layout as a, same viewpoint, a single object, no background, no text, letters, logos or frame, nothing cut off, \
bright saturated colors. Ignore the outline: the tool draws it later. Answer JSON only: pass true or false, and issues \
as short English sentences (empty when it passes)."
    )
}
