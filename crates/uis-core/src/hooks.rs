//! Commands run before and after a capture (`capture.hooks.beforeCapture` / `afterCapture`).
//!
//! A hook is `{ "name", "run": ["program", "arg", ...], "timeoutSec", "when": ["attach", "batch"] }`.
//! Placeholders in `run` are filled in: `{mode}` (attach | batch), `{projectPath}`, `{projectName}`
//! (folder name), `{runId}`, `{stateDir}` (`<home>\hook-state`) and `{owner}` (`unity-icon-studio`).
//!
//! Exit codes: 0 goes on; 4 refuses (its last output line is shown as it is); anything else, and a
//! time-out, is an error. In both of those cases nothing is captured. Hooks are the tool's own CLI
//! children: they run through the process runner, so a hook that runs too long is ended with its job.
//!
//! What hooks print goes to the run's log with secrets masked ([`mask`]).

use serde::Serialize;
use std::path::Path;
use std::time::Duration;

use crate::proc::{run, Cmd, Outcome};
use crate::settings::Hook;

/// The `{owner}` placeholder: who holds what a hook takes (a lease, a lock).
pub const OWNER: &str = "unity-icon-studio";

/// Values for the placeholders.
#[derive(Debug, Clone)]
pub struct HookContext {
    pub mode: String,
    pub project_path: String,
    pub run_id: String,
    pub state_dir: String,
}

impl HookContext {
    fn project_name(&self) -> String {
        Path::new(&self.project_path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// `text` with every placeholder filled in. Unknown `{...}` stay as they are.
    pub fn fill(&self, text: &str) -> String {
        text.replace("{mode}", &self.mode)
            .replace("{projectPath}", &self.project_path)
            .replace("{projectName}", &self.project_name())
            .replace("{runId}", &self.run_id)
            .replace("{stateDir}", &self.state_dir)
            .replace("{owner}", OWNER)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRun {
    /// `before` or `after`
    pub phase: String,
    pub name: String,
    /// `ok`, `refused`, `error`, `timeout`, `spawn`
    pub state: String,
    pub exit_code: Option<i32>,
    /// Last non-empty output line (stdout, else stderr), masked
    pub last_line: String,
    /// Everything it printed (stdout then stderr), masked
    pub output: String,
    pub elapsed_ms: u64,
}

impl HookRun {
    pub fn ok(&self) -> bool {
        self.state == "ok"
    }
}

/// Runs one hook (whatever its `when`).
pub fn run_hook(phase: &str, hook: &Hook, ctx: &HookContext) -> HookRun {
    let name = if hook.name.trim().is_empty() { hook.run.first().cloned().unwrap_or_default() } else { hook.name.clone() };
    let mut result = HookRun {
        phase: phase.into(),
        name,
        state: "error".into(),
        exit_code: None,
        last_line: String::new(),
        output: String::new(),
        elapsed_ms: 0,
    };
    let Some((program, args)) = hook.run.split_first() else {
        result.last_line = "empty run".into();
        return result;
    };
    let cmd = Cmd::new(ctx.fill(program))
        .args(args.iter().map(|a| ctx.fill(a)))
        .timeout(Duration::from_secs(hook.timeout_sec.max(1) as u64));
    match run(&cmd) {
        Err(e) => {
            result.state = "spawn".into();
            result.last_line = mask(&e);
        }
        Ok(out) => {
            result.elapsed_ms = out.elapsed_ms;
            let stdout = mask(&out.stdout_text());
            let stderr = mask(&out.stderr_text());
            result.last_line = last_line(&stdout).or_else(|| last_line(&stderr)).unwrap_or_default();
            result.output = [stdout.trim_end(), stderr.trim_end()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n");
            match out.outcome {
                Outcome::TimedOut => result.state = "timeout".into(),
                Outcome::Exited(code) => {
                    result.exit_code = Some(code);
                    result.state = match code {
                        0 => "ok",
                        4 => "refused",
                        _ => "error",
                    }
                    .into();
                }
            }
        }
    }
    result
}

/// Runs the hooks of a phase that apply to the mode, in order. Before-hooks stop at the first one
/// that does not go on; after-hooks all run.
pub fn run_phase(phase: &str, hooks: &[Hook], ctx: &HookContext) -> Vec<HookRun> {
    let mut out = Vec::new();
    for hook in hooks.iter().filter(|h| h.when.iter().any(|w| w == &ctx.mode)) {
        let r = run_hook(phase, hook, ctx);
        let stop = phase == "before" && !r.ok();
        out.push(r);
        if stop {
            break;
        }
    }
    out
}

/// Error code for a before-phase that did not go on: `hookRefused:<last line>` or
/// `hookFailed:<name>:<state>[:<code>]`. None when every hook went on.
pub fn phase_error(runs: &[HookRun]) -> Option<String> {
    let bad = runs.iter().find(|r| !r.ok())?;
    Some(match bad.state.as_str() {
        "refused" => format!("hookRefused:{}", bad.last_line),
        _ => match bad.exit_code {
            Some(code) => format!("hookFailed:{}:{}:{}", bad.name, bad.state, code),
            None => format!("hookFailed:{}:{}", bad.name, bad.state),
        },
    })
}

fn last_line(text: &str) -> Option<String> {
    text.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(300).collect())
}

/// Hides values that look like secrets: the value after `-accessToken`, `-hubSessionId`,
/// `-cbmEditorLicense`, `-cbmMachineId`, `--token`, `token=`, `password=`, `secret=`, `key=`, an
/// `Authorization: Bearer` value and `sk-...` keys.
pub fn mask(text: &str) -> String {
    const FLAGS: &[&str] = &["-accesstoken", "-hubsessionid", "-cbmeditorlicense", "-cbmmachineid", "--token", "--api-key"];
    const KEYS: &[&str] = &["token", "password", "secret", "apikey", "api_key", "key"];
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut words: Vec<String> = Vec::new();
        let mut hide_next = false;
        for word in line.split(' ') {
            let lower = word.to_lowercase();
            if hide_next && !word.is_empty() {
                words.push("***".into());
                hide_next = false;
                continue;
            }
            if FLAGS.contains(&lower.as_str()) || lower == "bearer" {
                hide_next = true;
                words.push(word.into());
                continue;
            }
            if let Some(eq) = word.find(['=', ':']) {
                let key = lower[..eq].trim_start_matches(['-', '"', '\'']).trim_end_matches(['"', '\'']);
                if KEYS.iter().any(|k| key == *k || key.ends_with(&format!("_{k}")) || key.ends_with(&format!("-{k}"))) && eq + 1 < word.len() {
                    words.push(format!("{}***", &word[..=eq]));
                    continue;
                }
            }
            if lower.starts_with("sk-") && word.len() > 12 {
                words.push("sk-***".into());
                continue;
            }
            words.push(word.into());
        }
        out.push_str(&words.join(" "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> HookContext {
        HookContext {
            mode: "batch".into(),
            project_path: r"C:\fake\Game-2".into(),
            run_id: "20260101-000000-ab12".into(),
            state_dir: r"C:\fake\home\hook-state".into(),
        }
    }

    #[test]
    fn placeholders_are_filled() {
        let c = ctx();
        assert_eq!(c.fill("{mode}|{projectPath}|{projectName}|{runId}|{stateDir}|{owner}|{other}"),
            r"batch|C:\fake\Game-2|Game-2|20260101-000000-ab12|C:\fake\home\hook-state|unity-icon-studio|{other}");
    }

    #[test]
    fn masks_secrets() {
        assert_eq!(mask("Unity.exe -accessToken abc123 -projectPath X"), "Unity.exe -accessToken *** -projectPath X");
        assert_eq!(mask("token=xyz password:hunter2 plain=1"), "token=*** password:*** plain=1");
        assert_eq!(mask("Authorization: Bearer eyJhbGci"), "Authorization: Bearer ***");
        assert_eq!(mask("use sk-abcdefghijklmnop now"), "use sk-*** now");
        assert_eq!(mask("leased by agent:1\nok"), "leased by agent:1\nok");
    }

    #[test]
    fn phase_errors() {
        let mk = |state: &str, code: Option<i32>, line: &str| HookRun {
            phase: "before".into(),
            name: "lease".into(),
            state: state.into(),
            exit_code: code,
            last_line: line.into(),
            output: String::new(),
            elapsed_ms: 0,
        };
        assert_eq!(phase_error(&[mk("ok", Some(0), "")]), None);
        assert_eq!(phase_error(&[mk("refused", Some(4), "leased by someone")]).unwrap(), "hookRefused:leased by someone");
        assert_eq!(phase_error(&[mk("error", Some(1), "x")]).unwrap(), "hookFailed:lease:error:1");
        assert_eq!(phase_error(&[mk("timeout", None, "")]).unwrap(), "hookFailed:lease:timeout");
    }
}
