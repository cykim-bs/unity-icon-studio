//! Unity project folders: what counts as one, what it says about itself (Unity version, render
//! pipeline, packages, git branch), whether an editor holds it, and the projects Unity Hub knows.
//! Everything here only reads files; nothing starts Unity or git.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Why a folder is not a Unity project, as a code (`assets`, `version`, `manifest`, `missing`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotProject(pub &'static str);

/// A Unity project folder has `Assets/`, `ProjectSettings/ProjectVersion.txt` and
/// `Packages/manifest.json`.
pub fn check_project(path: &Path) -> Result<(), NotProject> {
    if !path.is_dir() {
        return Err(NotProject("missing"));
    }
    if !path.join("Assets").is_dir() {
        return Err(NotProject("assets"));
    }
    if !path.join("ProjectSettings").join("ProjectVersion.txt").is_file() {
        return Err(NotProject("version"));
    }
    if !path.join("Packages").join("manifest.json").is_file() {
        return Err(NotProject("manifest"));
    }
    Ok(())
}

/// Walks up from `path` to the nearest folder that is a Unity project (a folder dropped from
/// inside a project, such as its Assets folder, still finds it).
pub fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut cur = Some(path);
    while let Some(dir) = cur {
        if check_project(dir).is_ok() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFacts {
    pub path: String,
    /// Folder name
    pub name: String,
    pub unity_version: Option<String>,
    pub unity_revision: Option<String>,
    /// `com.unity.render-pipelines.universal` in the manifest
    pub urp: bool,
    /// `com.unity.pipeline` in the manifest (needed to capture in an open editor)
    pub pipeline_package: bool,
    /// Current branch, or `(detached <sha>)`; None outside git
    pub branch: Option<String>,
    pub is_git: bool,
}

/// `m_EditorVersion` and the revision from ProjectVersion.txt.
pub fn read_version(project: &Path) -> (Option<String>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(project.join("ProjectSettings").join("ProjectVersion.txt")) else {
        return (None, None);
    };
    let mut version = None;
    let mut revision = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("m_EditorVersion:") {
            version = Some(v.trim().to_string()).filter(|v| !v.is_empty());
        } else if let Some(v) = line.strip_prefix("m_EditorVersionWithRevision:") {
            // 6000.6.0f1 (abcdef123456)
            revision = v.split('(').nth(1).map(|r| r.trim_end_matches(')').trim().to_string()).filter(|r| !r.is_empty());
        }
    }
    (version, revision)
}

/// Package names in `Packages/manifest.json` `dependencies`.
pub fn read_packages(project: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(project.join("Packages").join("manifest.json")) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
        return Vec::new();
    };
    value
        .get("dependencies")
        .and_then(|d| d.as_object())
        .map(|d| d.keys().cloned().collect())
        .unwrap_or_default()
}

/// The git folder of a working copy: `.git` itself, or where a `.git` file points (`gitdir:`).
fn git_dir(project: &Path) -> Option<PathBuf> {
    let dot = project.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let path = PathBuf::from(target);
    Some(if path.is_absolute() { path } else { project.join(path) })
}

/// The branch from `.git/HEAD`, read as a file (no git process, no lock).
pub fn read_branch(project: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(project)?.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(r) = head.strip_prefix("ref:") {
        let r = r.trim();
        return Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_string());
    }
    Some(format!("(detached {})", &head[..head.len().min(9)]))
}

pub fn read_facts(project: &Path) -> ProjectFacts {
    let (unity_version, unity_revision) = read_version(project);
    let packages = read_packages(project);
    let branch = read_branch(project);
    ProjectFacts {
        path: project.display().to_string(),
        name: project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        unity_version,
        unity_revision,
        urp: packages.iter().any(|p| p == "com.unity.render-pipelines.universal"),
        pipeline_package: packages.iter().any(|p| p == "com.unity.pipeline"),
        is_git: git_dir(project).is_some(),
        branch,
    }
}

/// `Temp/UnityLockfile`: an editor (or a batch run) holds it open while the project is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Lock {
    /// Held: an editor or a batch run has the project open
    Locked,
    /// Left behind (the file is there, nobody holds it)
    Stale,
    None,
}

/// Whether the lock file is held. It is opened for reading with full sharing for a moment; if
/// Unity holds it, that open fails with a sharing violation. Nothing is written.
pub fn lock_state(project: &Path) -> Lock {
    let file = project.join("Temp").join("UnityLockfile");
    if !file.exists() {
        return Lock::None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        options.share_mode(0x1 | 0x2 | 0x4);
    }
    match options.open(&file) {
        Ok(_) => Lock::Stale,
        // 32 = ERROR_SHARING_VIOLATION, 33 = ERROR_LOCK_VIOLATION
        Err(e) if matches!(e.raw_os_error(), Some(32) | Some(33)) => Lock::Locked,
        Err(_) => Lock::Locked,
    }
}

/// How a capture would run now. This reads files only: whether the open editor answers
/// (`unity status`) is checked by the capture step.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    /// `attach` (open editor), `batch` (closed project) or `blocked`
    pub mode: String,
    /// Code for the screen: `open` (an editor holds it), `closed`, `noPipelinePackage`, `noVersion`, `notProject:<why>`
    pub reason: String,
    pub lock: Lock,
    #[serde(flatten)]
    pub facts: ProjectFacts,
}

pub fn probe(project: &Path) -> Probe {
    if let Err(NotProject(why)) = check_project(project) {
        return Probe {
            mode: "blocked".into(),
            reason: format!("notProject:{why}"),
            lock: Lock::None,
            facts: ProjectFacts { path: project.display().to_string(), ..ProjectFacts::default() },
        };
    }
    let facts = read_facts(project);
    let lock = lock_state(project);
    let (mode, reason) = if facts.unity_version.is_none() {
        ("blocked", "noVersion")
    } else {
        match lock {
            Lock::Locked if facts.pipeline_package => ("attach", "open"),
            Lock::Locked => ("blocked", "noPipelinePackage"),
            Lock::Stale | Lock::None => ("batch", "closed"),
        }
    };
    Probe { mode: mode.into(), reason: reason.into(), lock, facts }
}

/// A project in Unity Hub's list (read only).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubProject {
    pub path: String,
    pub title: String,
    pub version: Option<String>,
    /// Milliseconds since 1970
    pub last_modified: Option<i64>,
    pub favorite: bool,
    /// Still a Unity project folder on this PC
    pub exists: bool,
}

/// Unity Hub's project list: `%APPDATA%\UnityHub\projects-v1.json` (`data` keyed by path).
pub fn hub_projects() -> Vec<HubProject> {
    let Some(appdata) = std::env::var_os("APPDATA") else { return Vec::new() };
    hub_projects_from(&PathBuf::from(appdata).join("UnityHub").join("projects-v1.json"))
}

pub fn hub_projects_from(file: &Path) -> Vec<HubProject> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
        return Vec::new();
    };
    let data = value.get("data").unwrap_or(&value);
    let Some(map) = data.as_object() else { return Vec::new() };
    let mut out: Vec<HubProject> = map
        .iter()
        .filter_map(|(key, v)| {
            let path = v.get("path").and_then(|p| p.as_str()).unwrap_or(key).to_string();
            if path.is_empty() {
                return None;
            }
            Some(HubProject {
                title: v
                    .get("title")
                    .and_then(|t| t.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                version: v.get("version").and_then(|t| t.as_str()).map(str::to_string),
                last_modified: v.get("lastModified").and_then(|t| t.as_f64()).map(|t| t as i64),
                favorite: v.get("isFavorite").and_then(|t| t.as_bool()).unwrap_or(false),
                exists: check_project(Path::new(&path)).is_ok(),
                path,
            })
        })
        .collect();
    out.sort_by(|a, b| b.last_modified.cmp(&a.last_modified).then_with(|| a.title.cmp(&b.title)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uis-project-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Assets/Sub")).unwrap();
        std::fs::create_dir_all(dir.join("ProjectSettings")).unwrap();
        std::fs::create_dir_all(dir.join("Packages")).unwrap();
        std::fs::write(
            dir.join("ProjectSettings/ProjectVersion.txt"),
            "m_EditorVersion: 6000.0.1f1\nm_EditorVersionWithRevision: 6000.0.1f1 (0123456789ab)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("Packages/manifest.json"),
            r#"{ "dependencies": { "com.unity.render-pipelines.universal": "17.0.0", "com.unity.pipeline": "1.0.0" } }"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_folder_needs_all_three_marks() {
        let dir = fake_project("marks");
        assert_eq!(check_project(&dir), Ok(()));
        std::fs::remove_file(dir.join("Packages/manifest.json")).unwrap();
        assert_eq!(check_project(&dir), Err(NotProject("manifest")));
        assert_eq!(check_project(&dir.join("nope")), Err(NotProject("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn facts_and_root_from_inside() {
        let dir = fake_project("facts");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        let f = read_facts(&dir);
        assert_eq!(f.unity_version.as_deref(), Some("6000.0.1f1"));
        assert_eq!(f.unity_revision.as_deref(), Some("0123456789ab"));
        assert!(f.urp && f.pipeline_package && f.is_git);
        assert_eq!(f.branch.as_deref(), Some("feature/x"));
        assert_eq!(find_project_root(&dir.join("Assets/Sub")), Some(dir.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_without_lock_is_batch_and_stale_lock_too() {
        let dir = fake_project("probe");
        let p = probe(&dir);
        assert_eq!((p.mode.as_str(), p.reason.as_str(), p.lock), ("batch", "closed", Lock::None));
        std::fs::create_dir_all(dir.join("Temp")).unwrap();
        std::fs::write(dir.join("Temp/UnityLockfile"), "").unwrap();
        assert_eq!(probe(&dir).lock, Lock::Stale);
        assert_eq!(probe(&dir).mode, "batch");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_held_lock_file_means_open() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = fake_project("held");
        std::fs::create_dir_all(dir.join("Temp")).unwrap();
        // what an editor does: keep it open with no sharing
        let held = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .share_mode(0)
            .open(dir.join("Temp/UnityLockfile"))
            .unwrap();
        let p = probe(&dir);
        assert_eq!((p.mode.as_str(), p.lock), ("attach", Lock::Locked));
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hub_list_reads_paths_titles_and_order() {
        let dir = std::env::temp_dir().join(format!("uis-hub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("projects-v1.json");
        std::fs::write(
            &file,
            r#"{ "schema_version": "v1", "data": {
                "C:\\fake\\A": { "title": "A", "path": "C:\\fake\\A", "version": "6000.0.1f1", "lastModified": 10 },
                "C:\\fake\\B": { "title": "B", "path": "C:\\fake\\B", "lastModified": 20, "isFavorite": true } } }"#,
        )
        .unwrap();
        let list = hub_projects_from(&file);
        assert_eq!(list.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["B", "A"]);
        assert!(list[0].favorite && !list[0].exists);
        assert_eq!(list[1].version.as_deref(), Some("6000.0.1f1"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
