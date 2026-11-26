//! Capture in an open editor (path A): the official `unity` CLI compiles the capture script in
//! memory and calls it (`unity command "script run"`). The script and the job live outside the
//! project; nothing is written into it.
//!
//! Each `script run` leaves its assembly in the editor's domain until the next reload, so one
//! request is one call, whatever the number of items. When the CLI client runs past its time, it
//! is ended (it is the tool's child); the editor is left as it is.

use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

use crate::proc::{run, Cmd, Outcome};

pub const ENTRY: &str = "UnityIconStudio.PrefabIconCapture.RunJobFile";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachRun {
    /// The capture result JSON (`unity-icon-studio.capture/1`), when the script ran
    pub result: Option<Value>,
    pub compile_ms: Option<u64>,
    pub execute_ms: Option<u64>,
    pub cli_ms: u64,
    pub cli_version: Option<String>,
    /// `compile:<first diagnostic>`, `cli:<code>`, `timeout`, `output:<why>`
    pub error: Option<String>,
}

/// `unity --version` (first line of stdout).
pub fn cli_version(cli: &Path) -> Option<String> {
    let out = run(&Cmd::new(cli).arg("--version").timeout(Duration::from_secs(20))).ok()?;
    out.stdout_text().lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

/// Runs the script for `job` in the editor of `project`. `timeout` is the script's own limit;
/// the client gets 30 s more.
pub fn run_attach(cli: &Path, project: &Path, script: &Path, job: &Path, timeout: Duration) -> AttachRun {
    let mut r = AttachRun { cli_version: cli_version(cli), ..AttachRun::default() };
    let args_json = serde_json::to_string(&vec![job.display().to_string()]).unwrap_or_default();
    let cmd = Cmd::new(cli)
        .args(["command", "script run", "--project-path"])
        .arg(project.as_os_str())
        .args(["--result-only", "--timeout"])
        .arg(timeout.as_secs().max(1).to_string())
        .arg("--file")
        .arg(script.as_os_str())
        .args(["--entry", ENTRY, "--args"])
        .arg(args_json)
        .timeout(timeout + Duration::from_secs(30));
    let out = match run(&cmd) {
        Ok(o) => o,
        Err(e) => {
            r.error = Some(format!("cli:{e}"));
            return r;
        }
    };
    r.cli_ms = out.elapsed_ms;
    if out.outcome == Outcome::TimedOut {
        r.error = Some("timeout".into());
        return r;
    }
    match parse_output(&out.stdout_text()) {
        Ok((result, compile, execute)) => {
            r.result = Some(result);
            r.compile_ms = compile;
            r.execute_ms = execute;
        }
        Err(e) => {
            // The CLI may exit 0 with success:false; a non-zero exit with no JSON is its own error
            r.error = Some(if out.success() || e.starts_with("compile:") || e.starts_with("script:") { e } else { format!("cli:{}", out.code()) });
        }
    }
    r
}

/// `--result-only` output: `{diagnostics, compileMs, executeMs, success, result}` where `result` is
/// the string the entry returned (the capture result JSON).
pub fn parse_output(stdout: &str) -> Result<(Value, Option<u64>, Option<u64>), String> {
    let text = stdout.trim().trim_start_matches('\u{feff}');
    // Notices may come before the JSON; take the first line that starts an object
    let start = text.find('{').ok_or_else(|| "output:no json".to_string())?;
    let v: Value = serde_json::from_str(&text[start..]).map_err(|e| format!("output:{e}"))?;
    let v = v.get("data").filter(|d| d.get("success").is_some()).cloned().unwrap_or(v);
    let compile = v.get("compileMs").and_then(|x| x.as_u64());
    let execute = v.get("executeMs").and_then(|x| x.as_u64());
    if v.get("success").and_then(|s| s.as_bool()) != Some(true) {
        let diag = v
            .get("diagnostics")
            .and_then(|d| d.as_array())
            .and_then(|d| d.first())
            .map(|d| d.get("message").and_then(|m| m.as_str()).map(str::to_string).unwrap_or_else(|| d.to_string()))
            .or_else(|| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| "failed".into());
        let kind = if v.get("diagnostics").and_then(|d| d.as_array()).is_some_and(|d| !d.is_empty()) { "compile" } else { "script" };
        return Err(format!("{kind}:{}", diag.chars().take(300).collect::<String>()));
    }
    let result = match v.get("result") {
        Some(Value::String(s)) => serde_json::from_str(s).map_err(|e| format!("output:result {e}"))?,
        Some(obj @ Value::Object(_)) => obj.clone(),
        _ => return Err("output:no result".into()),
    };
    Ok((result, compile, execute))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_result_only_output() {
        let out = r#"{"diagnostics":[],"compileMs":386,"executeMs":1761,"success":true,"result":"{\"schema\":\"unity-icon-studio.capture/1\",\"failed\":0,\"ok\":true}"}"#;
        let (v, c, e) = parse_output(out).unwrap();
        assert_eq!(v["ok"], serde_json::json!(true));
        assert_eq!((c, e), (Some(386), Some(1761)));
    }

    #[test]
    fn compile_errors_are_codes() {
        let out = r#"{"diagnostics":[{"line":3,"message":"CS1002: ; expected"}],"success":false}"#;
        assert_eq!(parse_output(out).unwrap_err(), "compile:CS1002: ; expected");
        assert!(parse_output("nothing").unwrap_err().starts_with("output:"));
    }
}
