//! Journal of batch captures: `<home>\journal\<run id>.json`, written before anything touches the
//! project and removed when the run is cleaned up. If the tool stops in between (closed, crashed,
//! or it stopped waiting for a long Unity run), the next start finds the journal and finishes the
//! run: it removes the temporary entry folder, puts re-serialized tracked files back and runs the
//! after-capture hooks (see `capture::recover`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::home::Home;
use crate::settings::write_replace;
use crate::unity::gitguard::Snapshot;

pub const JOURNAL_SCHEMA: &str = "unity-icon-studio.journal/1";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Journal {
    pub schema: String,
    pub run_id: String,
    pub mode: String,
    pub project: String,
    /// The temporary entry folder (`<project>\Assets\__UnityIconStudioCapture`)
    pub entry_dir: String,
    pub unity_exe: String,
    /// 0 until Unity started
    pub pid: u32,
    /// `prepared` (hooks and entry), `running` (Unity started), `timedOut` (stopped waiting)
    pub state: String,
    pub out_dir: String,
    pub job_file: String,
    pub log_file: String,
    pub started_ms: i64,
    /// Every before-capture hook went on (so the after-capture hooks are due)
    pub hooks_before_ok: bool,
    pub git_before: Snapshot,
    pub restore_tracked_files: bool,
    pub restore_patterns: Vec<String>,
}

fn file(home: &Home, run_id: &str) -> PathBuf {
    home.journal_dir().join(format!("{run_id}.json"))
}

pub fn write(home: &Home, j: &Journal) -> Result<(), String> {
    std::fs::create_dir_all(home.journal_dir()).map_err(|e| format!("journal:{e}"))?;
    let mut j = j.clone();
    j.schema = JOURNAL_SCHEMA.into();
    let text = serde_json::to_string_pretty(&j).map_err(|e| format!("journal:{e}"))?;
    write_replace(&file(home, &j.run_id), &text).map_err(|e| format!("journal:{e}"))
}

pub fn remove(home: &Home, run_id: &str) -> Result<(), String> {
    match std::fs::remove_file(file(home, run_id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("journal:{e}")),
    }
}

/// Every journal entry, oldest first. Files that cannot be read are skipped (and kept).
pub fn list(home: &Home) -> Vec<Journal> {
    let Ok(entries) = std::fs::read_dir(home.journal_dir()) else { return Vec::new() };
    let mut out: Vec<Journal> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read(&e.path()))
        .collect();
    out.sort_by(|a, b| a.started_ms.cmp(&b.started_ms).then_with(|| a.run_id.cmp(&b.run_id)));
    out
}

fn read(path: &Path) -> Option<Journal> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// A journal entry for this project folder (any state).
pub fn for_project(home: &Home, project: &str) -> Option<Journal> {
    list(home).into_iter().find(|j| crate::unity::probe::same_path(&j.project, project))
}
