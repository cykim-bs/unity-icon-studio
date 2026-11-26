//! Finding the programs a capture needs.
//!
//! - `Unity.exe` for batch captures: only the exact version in `ProjectVersion.txt`. Looked up in
//!   the settings (`capture.editors["<version>"]`), Unity Hub's install list
//!   (`%APPDATA%\UnityHub\editors-v2.json`, `secondaryInstallPath.json`) and the default Hub folder
//!   (`C:\Program Files\Unity\Hub\Editor\<version>\Editor\Unity.exe`). Another version is refused:
//!   batch mode would upgrade the project without asking.
//! - the official `unity` CLI for captures in an open editor: settings `capture.unityCli`,
//!   `%LOCALAPPDATA%\Unity\bin\unity.exe`, then `PATH`.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::settings::Capture;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorFound {
    pub version: String,
    pub path: String,
    /// `settings`, `hub`, `hubSecondary`, `default`
    pub source: String,
}

/// Versions found under a Hub editor folder (`<root>\<version>\Editor\Unity.exe`).
fn versions_under(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join("Editor").join("Unity.exe").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn hub_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("UnityHub"))
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Editors listed in Hub's `editors-v2.json` (`data[]` with a version and a location). The file's
/// exact shape changes between Hub versions, so a few names are tried.
fn hub_listed() -> Vec<(String, PathBuf)> {
    let Some(dir) = hub_dir() else { return Vec::new() };
    let Some(v) = read_json(&dir.join("editors-v2.json")) else { return Vec::new() };
    let items = v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    items
        .iter()
        .filter_map(|e| {
            let version = e.get("version").and_then(|x| x.as_str())?.to_string();
            let loc = ["location", "path", "editorPath"]
                .iter()
                .find_map(|k| match e.get(*k) {
                    Some(serde_json::Value::String(s)) => Some(s.clone()),
                    Some(serde_json::Value::Array(a)) => a.first().and_then(|x| x.as_str()).map(str::to_string),
                    _ => None,
                })?;
            let p = PathBuf::from(loc);
            let exe = if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) { p } else { p.join("Editor").join("Unity.exe") };
            Some((version, exe))
        })
        .collect()
}

fn hub_secondary_root() -> Option<PathBuf> {
    let v = read_json(&hub_dir()?.join("secondaryInstallPath.json"))?;
    let s = v.as_str()?.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

fn default_root() -> PathBuf {
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    pf.join("Unity").join("Hub").join("Editor")
}

/// `Unity.exe` of exactly `version`, or `editorMissing:<version>:<versions found>`.
pub fn find_editor(settings: &Capture, version: &str) -> Result<EditorFound, String> {
    let found = |path: PathBuf, source: &str| EditorFound { version: version.into(), path: path.display().to_string(), source: source.into() };
    if let Some(p) = settings.editors.get(version) {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(found(path, "settings"));
        }
    }
    for (v, exe) in hub_listed() {
        if v == version && exe.is_file() {
            return Ok(found(exe, "hub"));
        }
    }
    if let Some(root) = hub_secondary_root() {
        let exe = root.join(version).join("Editor").join("Unity.exe");
        if exe.is_file() {
            return Ok(found(exe, "hubSecondary"));
        }
    }
    let exe = default_root().join(version).join("Editor").join("Unity.exe");
    if exe.is_file() {
        return Ok(found(exe, "default"));
    }
    let mut known: Vec<String> = settings.editors.keys().cloned().collect();
    known.extend(hub_listed().into_iter().map(|(v, _)| v));
    if let Some(root) = hub_secondary_root() {
        known.extend(versions_under(&root));
    }
    known.extend(versions_under(&default_root()));
    known.sort();
    known.dedup();
    Err(format!("editorMissing:{version}:{}", known.join(",")))
}

/// The `unity` CLI, or `unityCliMissing`.
pub fn find_unity_cli(settings: &Capture) -> Result<PathBuf, String> {
    if let Some(p) = &settings.unity_cli {
        let path = PathBuf::from(p);
        return if path.is_file() { Ok(path) } else { Err(format!("unityCliMissing:{p}")) };
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("Unity").join("bin").join("unity.exe");
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["unity.exe", "unity"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    Err("unityCliMissing".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_editor_needs_the_exact_version() {
        let dir = std::env::temp_dir().join(format!("uis-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Unity.exe");
        std::fs::write(&exe, b"x").unwrap();
        let mut c = Capture::default();
        c.editors.insert("1.2.3f1".into(), exe.display().to_string());
        assert_eq!(find_editor(&c, "1.2.3f1").unwrap().source, "settings");
        let err = find_editor(&c, "9.9.9f9").unwrap_err();
        assert!(err.starts_with("editorMissing:9.9.9f9:"), "{err}");
        assert!(err.contains("1.2.3f1"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
