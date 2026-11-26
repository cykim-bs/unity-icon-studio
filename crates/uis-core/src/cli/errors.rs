//! What went wrong in a Codex or Claude run, from its output (3-10 of the plan).
//!
//! Codex `--json` prints events one per line. `item.completed` items of type `error` are warnings
//! (ignored settings, no code mode) and do not fail a run; top-level `error` events and
//! `turn.failed` carry the real messages. Claude `--output-format json` prints one object with
//! `is_error`, `terminal_reason` and `result`; some failures (an unknown model) print plain text.
//!
//! Kinds: `timeout`, `limit`, `auth`, `model`, `no_image_tool`, `refused`, `no_image`, `bad_json`,
//! `spawn`, `not_found`, `exit`.

use serde::Serialize;
use serde_json::Value;

use crate::proc::{Outcome, Output};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailKind {
    Timeout,
    Limit,
    Auth,
    Model,
    NoImageTool,
    Refused,
    NoImage,
    BadJson,
    Spawn,
    NotFound,
    Exit,
}

impl FailKind {
    pub fn code(self) -> &'static str {
        match self {
            FailKind::Timeout => "timeout",
            FailKind::Limit => "limit",
            FailKind::Auth => "auth",
            FailKind::Model => "model",
            FailKind::NoImageTool => "no_image_tool",
            FailKind::Refused => "refused",
            FailKind::NoImage => "no_image",
            FailKind::BadJson => "bad_json",
            FailKind::Spawn => "spawn",
            FailKind::NotFound => "not_found",
            FailKind::Exit => "exit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: FailKind,
    /// `codex` or `claude`
    pub cli: String,
    /// The first meaningful line of what the CLI said (secrets masked)
    pub message: String,
    pub exit_code: Option<i32>,
}

impl Failure {
    pub fn new(kind: FailKind, cli: &str, message: impl Into<String>) -> Failure {
        Failure { kind, cli: cli.into(), message: crate::hooks::mask(&message.into()).chars().take(400).collect(), exit_code: None }
    }
    /// `kind:message`, for logs and job files
    pub fn code(&self) -> String {
        format!("{}:{}", self.kind.code(), self.message)
    }
}

const LIMIT: &[&str] = &[
    "429", "rate limit", "rate_limit", "ratelimit", "usage limit", "usage_limit", "quota", "limit reached", "too many requests",
    "hit your limit", "reached your limit", "out of credits", "insufficient credits",
];
const AUTH: &[&str] = &[
    "401", "unauthorized", "missing bearer", "not logged in", "failed to authenticate", "authentication failed", "oauth",
    "please log in", "please login", "codex login", "claude auth login", "invalid api key", "token has expired", "/login",
];
const MODEL: &[&str] = &[
    "model is not supported", "is not supported when using codex", "issue with the selected model", "model_not_found",
    "unknown model", "does not have access to model", "model not found",
];
const NO_TOOL: &[&str] = &[
    "image_gen is not a function", "이미지 생성 실행기", "image generation tool is not available", "image generation is not available",
    "no image generation tool", "code-mode host",
];
const REFUSED: &[&str] = &[
    "can't help", "cannot help", "can't assist", "cannot assist", "can't create", "cannot create", "unable to help",
    "not able to help", "i won't", "refuse", "content policy", "safety policy", "against our policy", "도와드릴 수 없",
    "만들 수 없습니다", "생성할 수 없습니다", "정책상",
];

fn has_any(text: &str, list: &[&str]) -> bool {
    let lower = text.to_lowercase();
    list.iter().any(|p| lower.contains(p))
}

/// Codex `--json` events (lines that do not parse are skipped).
pub fn parse_events(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok()).collect()
}

/// Messages of the real errors: top-level `error` events and `turn.failed`.
pub fn codex_error_messages(events: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for e in events {
        match e.get("type").and_then(Value::as_str) {
            Some("error") => {
                if let Some(m) = e.get("message").and_then(Value::as_str) {
                    out.push(m.to_string());
                }
            }
            Some("turn.failed") => {
                if let Some(m) = e.pointer("/error/message").and_then(Value::as_str) {
                    out.push(m.to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// Warnings: `item.completed` items of type `error`.
pub fn codex_warnings(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.get("type").and_then(Value::as_str) == Some("item.completed"))
        .filter(|e| e.pointer("/item/type").and_then(Value::as_str) == Some("error"))
        .filter_map(|e| e.pointer("/item/message").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// The text of the agent's messages, in order.
pub fn codex_agent_messages(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.get("type").and_then(Value::as_str) == Some("item.completed"))
        .filter(|e| e.pointer("/item/type").and_then(Value::as_str) == Some("agent_message"))
        .filter_map(|e| e.pointer("/item/text").and_then(Value::as_str).map(str::to_string))
        .collect()
}

pub fn codex_thread_id(events: &[Value]) -> Option<String> {
    events
        .iter()
        .find(|e| e.get("type").and_then(Value::as_str) == Some("thread.started"))
        .and_then(|e| e.get("thread_id").and_then(Value::as_str))
        .map(str::to_string)
}

fn first_text(out: &Output) -> String {
    let err = out.stderr_text();
    let text = if err.trim().is_empty() { out.stdout_text() } else { err };
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

fn by_text(text: &str) -> Option<FailKind> {
    if has_any(text, LIMIT) {
        Some(FailKind::Limit)
    } else if has_any(text, AUTH) {
        Some(FailKind::Auth)
    } else if has_any(text, MODEL) {
        Some(FailKind::Model)
    } else {
        None
    }
}

/// A Codex run that ended. `image`: the run was asked for a picture, and `harvested` says whether
/// one was found. None when the run went through.
pub fn classify_codex(out: &Output, events: &[Value], last_message: &str, image: bool, harvested: bool) -> Option<Failure> {
    let mut f = classify_codex_inner(out, events, last_message, image, harvested)?;
    f.exit_code = match out.outcome {
        Outcome::Exited(n) => Some(n),
        Outcome::TimedOut => None,
    };
    Some(f)
}

fn classify_codex_inner(out: &Output, events: &[Value], last_message: &str, image: bool, harvested: bool) -> Option<Failure> {
    if out.outcome == Outcome::TimedOut {
        return Some(Failure::new(FailKind::Timeout, "codex", "time limit reached"));
    }
    let errors = codex_error_messages(events);
    // The last error says the most (Reconnecting 1/5 ... then the final status)
    let error_text = errors.last().cloned().unwrap_or_default();
    let all_errors = format!("{}\n{}", errors.join("\n"), out.stderr_text());
    let failed = !out.success() || events.iter().any(|e| e.get("type").and_then(Value::as_str) == Some("turn.failed"));
    if failed {
        let message = if error_text.is_empty() { first_text(out) } else { error_text };
        let kind = by_text(&message).or_else(|| by_text(&all_errors)).unwrap_or(FailKind::Exit);
        return Some(Failure::new(kind, "codex", message));
    }
    if image && !harvested {
        let said = format!("{}\n{}", codex_agent_messages(events).join("\n"), last_message);
        if let Some(kind) = by_text(&said) {
            return Some(Failure::new(kind, "codex", last_line(&said)));
        }
        let warnings = codex_warnings(events).join("\n");
        if has_any(&said, NO_TOOL) || (has_any(&warnings, &["code-mode host"]) && !said.to_lowercase().contains(".png")) {
            return Some(Failure::new(FailKind::NoImageTool, "codex", last_line(&said)));
        }
        if has_any(&said, REFUSED) {
            return Some(Failure::new(FailKind::Refused, "codex", last_line(&said)));
        }
        return Some(Failure::new(FailKind::NoImage, "codex", last_line(&said)));
    }
    None
}

fn last_line(text: &str) -> String {
    text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// A Claude run that ended. `json` is its stdout parsed, when it parsed. None when it went through
/// (the caller still checks `structured_output`).
pub fn classify_claude(out: &Output, json: Option<&Value>) -> Option<Failure> {
    let exit_code = match out.outcome {
        Outcome::Exited(n) => Some(n),
        Outcome::TimedOut => None,
    };
    let mut f = classify_claude_inner(out, json)?;
    f.exit_code = exit_code;
    Some(f)
}

fn classify_claude_inner(out: &Output, json: Option<&Value>) -> Option<Failure> {
    if out.outcome == Outcome::TimedOut {
        return Some(Failure::new(FailKind::Timeout, "claude", "time limit reached"));
    }
    if let Some(j) = json {
        let is_error = j.get("is_error").and_then(Value::as_bool).unwrap_or(false);
        let result = j.get("result").and_then(Value::as_str).unwrap_or("").to_string();
        if is_error || !out.success() {
            let reason = j.get("terminal_reason").and_then(Value::as_str).unwrap_or("");
            let message = if result.trim().is_empty() { format!("{reason} {}", first_text(out)) } else { result.clone() };
            let kind = by_text(&message).unwrap_or(FailKind::Exit);
            return Some(Failure::new(kind, "claude", message.trim()));
        }
        if j.get("structured_output").is_none_or(|v| !v.is_object()) {
            if has_any(&result, REFUSED) {
                return Some(Failure::new(FailKind::Refused, "claude", last_line(&result)));
            }
            return Some(Failure::new(FailKind::BadJson, "claude", last_line(&result)));
        }
        return None;
    }
    let text = format!("{}\n{}", out.stdout_text(), out.stderr_text());
    let kind = by_text(&text).unwrap_or(if out.success() { FailKind::BadJson } else { FailKind::Exit });
    Some(Failure::new(kind, "claude", first_text(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            outcome: Outcome::Exited(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            elapsed_ms: 1,
            pid: 1,
        }
    }

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!("../../../../tests/fixtures/cli/", $name))
        };
    }

    fn codex(code: i32, file: &str, last: &str, image: bool, harvested: bool) -> Option<Failure> {
        let out = output(code, file, "");
        let events = parse_events(file.as_bytes());
        classify_codex(&out, &events, last, image, harvested)
    }

    #[test]
    fn codex_image_ok() {
        let f = fixture!("codex-image-ok.jsonl");
        let events = parse_events(f.as_bytes());
        assert_eq!(codex_thread_id(&events).as_deref(), Some("00000000-0000-7000-8000-000000000001"));
        assert!(codex(0, f, "", true, true).is_none());
        // the warnings alone do not fail it
        assert_eq!(codex_warnings(&events).len(), 2);
        assert!(codex_agent_messages(&events).last().unwrap().ends_with(".png"));
    }

    #[test]
    fn codex_text_ok() {
        assert!(codex(0, fixture!("codex-text-ok.jsonl"), "", false, false).is_none());
    }

    #[test]
    fn codex_bad_model() {
        let f = codex(1, fixture!("codex-model.jsonl"), "", false, false).unwrap();
        assert_eq!(f.kind, FailKind::Model);
        assert!(f.message.contains("not supported"));
        assert_eq!(f.exit_code, Some(1));
    }

    #[test]
    fn codex_not_logged_in() {
        let f = codex(1, fixture!("codex-auth.jsonl"), "", true, false).unwrap();
        assert_eq!(f.kind, FailKind::Auth);
        assert!(f.message.contains("401"));
    }

    #[test]
    fn codex_no_image_tool() {
        let f = fixture!("codex-no-tool.jsonl");
        let fail = codex(0, f, "", true, false).unwrap();
        assert_eq!(fail.kind, FailKind::NoImageTool);
        assert_eq!(fail.exit_code, Some(0));
        let app_server = "{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"tools.image_gen is not a function\"}}\n";
        assert_eq!(codex(0, app_server, "", true, false).unwrap().kind, FailKind::NoImageTool);
    }

    #[test]
    fn codex_limit() {
        let f = codex(1, fixture!("codex-limit.jsonl"), "", true, false).unwrap();
        assert_eq!(f.kind, FailKind::Limit);
    }

    #[test]
    fn codex_exit_zero_without_a_picture() {
        let events = "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"Done.\"}}\n{\"type\":\"turn.completed\"}\n";
        assert_eq!(codex(0, events, "Done.", true, false).unwrap().kind, FailKind::NoImage);
        let refused = "{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"I can't help with that request.\"}}\n";
        assert_eq!(codex(0, refused, "", true, false).unwrap().kind, FailKind::Refused);
    }

    #[test]
    fn codex_timeout_and_plain_exit() {
        let mut out = output(0, "", "");
        out.outcome = Outcome::TimedOut;
        let f = classify_codex(&out, &[], "", true, false).unwrap();
        assert_eq!((f.kind, f.exit_code), (FailKind::Timeout, None));
        let f = classify_codex(&output(2, "", "error: unexpected argument '--nope'"), &[], "", false, false).unwrap();
        assert_eq!(f.kind, FailKind::Exit);
        assert!(f.message.contains("unexpected argument"));
    }

    fn claude(code: i32, stdout: &str, stderr: &str) -> Option<Failure> {
        let out = output(code, stdout, stderr);
        let json: Option<Value> = serde_json::from_str(stdout.trim()).ok();
        classify_claude(&out, json.as_ref())
    }

    #[test]
    fn claude_ok() {
        assert!(claude(0, fixture!("claude-ok.json"), "").is_none());
    }

    #[test]
    fn claude_auth() {
        let f = claude(1, fixture!("claude-auth.json"), "").unwrap();
        assert_eq!(f.kind, FailKind::Auth);
        assert!(f.message.contains("Failed to authenticate"));
    }

    #[test]
    fn claude_bad_model() {
        let f = claude(1, fixture!("claude-model.out"), fixture!("claude-model.err")).unwrap();
        assert_eq!(f.kind, FailKind::Model);
    }

    #[test]
    fn claude_no_structured_output() {
        let j = "{\"type\":\"result\",\"is_error\":false,\"terminal_reason\":\"completed\",\"result\":\"Here is the prompt: ...\"}";
        assert_eq!(claude(0, j, "").unwrap().kind, FailKind::BadJson);
        assert_eq!(claude(0, "not json at all", "").unwrap().kind, FailKind::BadJson);
        let limit = "{\"type\":\"result\",\"is_error\":true,\"terminal_reason\":\"api_error\",\"result\":\"Claude AI usage limit reached. Your limit will reset at 5pm.\"}";
        assert_eq!(claude(1, limit, "").unwrap().kind, FailKind::Limit);
    }

    #[test]
    fn messages_are_masked() {
        let f = Failure::new(FailKind::Exit, "codex", "bad token=abcdef123456 here");
        assert!(!f.message.contains("abcdef123456"), "{}", f.message);
    }
}
