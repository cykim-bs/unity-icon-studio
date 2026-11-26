//! Which way a capture runs now: the file checks of [`crate::project::probe`] plus, when an editor
//! holds the project, whether that editor answers the `unity` CLI (`unity status --json`).
//!
//! | files                        | `unity status`               | result                         |
//! |------------------------------|------------------------------|--------------------------------|
//! | not locked                   | -                            | `batch` (`closed`)             |
//! | locked, no pipeline package  | -                            | `blocked` (`noPipelinePackage`)|
//! | locked                       | the project, `ready`         | `attach` (`open`)              |
//! | locked                       | the project, another state   | wait 5 s, ask again, else `blocked` (`editorBusy:<state>`) |
//! | locked                       | not listed                   | `blocked` (`notResponding`: starting, or a batch run) |
//!
//! `capture.mode` narrows it: `attach` turns `batch` into `blocked` (`closedAttachOnly`), `batch`
//! turns `attach` into `blocked` (`openBatchOnly`).

use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

use crate::proc::{run, Cmd};
use crate::project::{self, Probe};
use crate::settings::Capture;

use super::find::find_unity_cli;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorInstance {
    pub project_path: String,
    pub state: String,
    pub version: Option<String>,
    pub port: Option<u64>,
    pub pid: Option<u64>,
}

/// Editors the `unity` CLI sees (`unity status --json`, stdout only). `Err` when the CLI is missing
/// or its output is not JSON.
pub fn editor_instances(cli: &Path) -> Result<Vec<EditorInstance>, String> {
    let out = run(&Cmd::new(cli).args(["status", "--json"]).timeout(Duration::from_secs(30)))?;
    let text = out.stdout_text();
    let v: Value = serde_json::from_str(text.trim().trim_start_matches('\u{feff}')).map_err(|e| format!("status:{e}"))?;
    let list = v.pointer("/data/instances").and_then(|i| i.as_array()).cloned().unwrap_or_default();
    Ok(list
        .iter()
        .map(|i| EditorInstance {
            project_path: i.get("projectPath").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            state: i.get("state").and_then(|x| x.as_str()).unwrap_or("unknown").to_string(),
            version: i.get("version").and_then(|x| x.as_str()).map(str::to_string),
            port: i.get("port").and_then(|x| x.as_u64()),
            pid: i.get("pid").and_then(|x| x.as_u64()),
        })
        .collect())
}

/// Same folder, ignoring case, separators and a trailing separator.
pub fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').trim_start_matches(r"\\?\").to_lowercase();
    norm(a) == norm(b)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureProbe {
    #[serde(flatten)]
    pub probe: Probe,
    /// The open editor that answers for this project (attach)
    pub editor: Option<EditorInstance>,
    /// How many editors the CLI sees
    pub editors_seen: usize,
}

/// The file probe, then the CLI check for an open editor, then `capture.mode`. `retry_wait` is
/// how long to wait before asking a busy editor again (5 s in the app; tests pass less).
pub fn capture_probe(project_dir: &Path, settings: &Capture, retry_wait: Duration) -> CaptureProbe {
    let mut probe = project::probe(project_dir);
    let mut editor = None;
    let mut seen = 0;
    if probe.mode == "attach" && settings.mode == "batch" {
        probe.mode = "blocked".into();
        probe.reason = "openBatchOnly".into();
        return CaptureProbe { probe, editor, editors_seen: seen };
    }
    if probe.mode == "attach" {
        match find_unity_cli(settings) {
            Err(e) => {
                probe.mode = "blocked".into();
                probe.reason = e;
            }
            Ok(cli) => {
                let mut attempt = 0;
                loop {
                    attempt += 1;
                    match editor_instances(&cli) {
                        Err(e) => {
                            probe.mode = "blocked".into();
                            probe.reason = format!("unityCli:{e}");
                            break;
                        }
                        Ok(list) => {
                            seen = list.len();
                            match list.into_iter().find(|i| same_path(&i.project_path, &probe.facts.path)) {
                                None => {
                                    probe.mode = "blocked".into();
                                    probe.reason = "notResponding".into();
                                    break;
                                }
                                Some(i) if i.state == "ready" => {
                                    editor = Some(i);
                                    break;
                                }
                                Some(i) => {
                                    if attempt >= 2 {
                                        probe.mode = "blocked".into();
                                        probe.reason = format!("editorBusy:{}", i.state);
                                        editor = Some(i);
                                        break;
                                    }
                                    std::thread::sleep(retry_wait);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    match (settings.mode.as_str(), probe.mode.as_str()) {
        ("attach", "batch") => {
            probe.mode = "blocked".into();
            probe.reason = "closedAttachOnly".into();
        }
        ("batch", "attach") => {
            probe.mode = "blocked".into();
            probe.reason = "openBatchOnly".into();
        }
        _ => {}
    }
    CaptureProbe { probe, editor, editors_seen: seen }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compare_loosely() {
        assert!(same_path(r"C:\repo\Game-2", "C:/repo/game-2/"));
        assert!(!same_path(r"C:\repo\Game-2", r"C:\repo\Game-20"));
    }
}
