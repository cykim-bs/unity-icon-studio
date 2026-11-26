//! The projects a person added: the list in settings.json (`projects`: id, path, pack) and each
//! project's own state in `<home>\projects\<id>.json` (kinds and capture values a person chose for
//! single prefabs, the last note).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::home::Home;
use crate::packs::KINDS;
use crate::prefabs::project_hash;
use crate::project::{find_project_root, probe, Probe};
use crate::settings::{self, ProjectEntry};

pub const STATE_SCHEMA: &str = "unity-icon-studio.project/1";

/// What a person changed for one prefab. Absent keys keep the pack's value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Override {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 1 or -1: which side faces the viewer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yaw_sign: Option<i32>,
    /// Root rotation, Euler degrees
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f64; 3]>,
    /// `+z`, `-z`, `+x`, `-x`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front: Option<String>,
}

impl Override {
    fn is_empty(&self) -> bool {
        self == &Override::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectState {
    pub schema: String,
    pub path: String,
    pub pack_id: String,
    pub overrides: BTreeMap<String, Override>,
    pub last_note: String,
}

/// A project of the list with what its folder says now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub pack_id: String,
    pub exists: bool,
    #[serde(flatten)]
    pub probe: Probe,
}

fn state_file(home: &Home, id: &str) -> PathBuf {
    home.projects_dir().join(format!("{id}.json"))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Same folder, same id: `p` + 12 hex digits of the folder's hash.
pub fn id_for(path: &Path) -> String {
    format!("p{}", &project_hash(path)[..12])
}

fn same_path(a: &str, b: &Path) -> bool {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(&b.display().to_string())
}

pub fn load_state(home: &Home, id: &str) -> ProjectState {
    if !valid_id(id) {
        return ProjectState::default();
    }
    std::fs::read(state_file(home, id))
        .ok()
        .and_then(|b| serde_json::from_slice::<ProjectState>(&b).ok())
        .unwrap_or_default()
}

fn save_state(home: &Home, id: &str, state: &ProjectState) -> Result<(), String> {
    let mut state = state.clone();
    state.schema = STATE_SCHEMA.into();
    let text = serde_json::to_string_pretty(&state).map_err(|e| format!("write:{e}"))? + "\n";
    settings::write_replace(&state_file(home, id), &text)
}

/// Kinds chosen by a person, by guid (for the scan).
pub fn kind_overrides(home: &Home, id: &str) -> HashMap<String, String> {
    load_state(home, id).overrides.into_iter().filter_map(|(g, o)| o.kind.map(|k| (g, k))).collect()
}

pub fn summary(home: &Home, entry: &ProjectEntry) -> ProjectSummary {
    let path = PathBuf::from(&entry.path);
    let _ = home;
    ProjectSummary { id: entry.id.clone(), pack_id: entry.pack_id.clone(), exists: path.is_dir(), probe: probe(&path) }
}

pub fn list(home: &Home) -> Vec<ProjectSummary> {
    settings::load(home).projects.iter().map(|e| summary(home, e)).collect()
}

pub fn entry(home: &Home, id: &str) -> Result<ProjectEntry, String> {
    settings::load(home).projects.into_iter().find(|p| p.id == id).ok_or_else(|| format!("noProject:{id}"))
}

/// Adds the Unity project at `path` (or the one it is inside). Adding it again returns the same
/// entry. Errors: `notProject:<why>` with the reason of the folder itself.
pub fn add(home: &Home, path: &Path) -> Result<ProjectSummary, String> {
    let root = match find_project_root(path) {
        Some(root) => root,
        None => {
            let why = crate::project::check_project(path).err().map(|e| e.0).unwrap_or("missing");
            return Err(format!("notProject:{why}"));
        }
    };
    let root = std::fs::canonicalize(&root).map(|p| strip_verbatim(&p)).unwrap_or(root);
    let current = settings::load(home);
    if let Some(found) = current.projects.iter().find(|p| same_path(&p.path, &root)) {
        return Ok(summary(home, found));
    }
    let entry = ProjectEntry { id: id_for(&root), path: root.display().to_string(), pack_id: crate::packs::DEFAULT_ID.into() };
    let mut projects = current.projects.clone();
    projects.push(entry.clone());
    let mut patch = Map::new();
    patch.insert("projects".into(), serde_json::to_value(&projects).map_err(|e| format!("write:{e}"))?);
    settings::update(home, &patch)?;
    let mut state = load_state(home, &entry.id);
    state.path = entry.path.clone();
    state.pack_id = entry.pack_id.clone();
    save_state(home, &entry.id, &state)?;
    Ok(summary(home, &entry))
}

/// `\\?\C:\x` -> `C:\x` (what canonicalize gives on Windows).
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

/// Takes the project off the list. Its state file stays (adding it again brings the choices back).
pub fn remove(home: &Home, id: &str) -> Result<(), String> {
    let current = settings::load(home);
    let projects: Vec<ProjectEntry> = current.projects.into_iter().filter(|p| p.id != id).collect();
    let mut patch = Map::new();
    patch.insert("projects".into(), serde_json::to_value(&projects).map_err(|e| format!("write:{e}"))?);
    settings::update(home, &patch)?;
    Ok(())
}

/// Applies `patch` to one prefab's override (`null` for a key clears it) and returns the result.
pub fn set_override(home: &Home, id: &str, guid: &str, patch: &Map<String, Value>) -> Result<Override, String> {
    entry(home, id)?;
    if guid.len() != 32 || !guid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("badGuid".into());
    }
    let mut state = load_state(home, id);
    let mut current = serde_json::to_value(state.overrides.get(guid).cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
    if let Value::Object(obj) = &mut current {
        for (k, v) in patch {
            if v.is_null() {
                obj.remove(k);
            } else {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    let next: Override = serde_json::from_value(current).map_err(|e| format!("badPatch:{e}"))?;
    if let Some(kind) = &next.kind {
        if !KINDS.contains(&kind.as_str()) {
            return Err(format!("badKind:{kind}"));
        }
    }
    if let Some(sign) = next.yaw_sign {
        if sign != 1 && sign != -1 {
            return Err("badYawSign".into());
        }
    }
    if let Some(front) = &next.front {
        if !["+z", "-z", "+x", "-x"].contains(&front.as_str()) {
            return Err(format!("badFront:{front}"));
        }
    }
    if next.is_empty() {
        state.overrides.remove(guid);
    } else {
        state.overrides.insert(guid.to_ascii_lowercase(), next.clone());
    }
    save_state(home, id, &state)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fake(tag: &str) -> (Home, PathBuf) {
        let base = std::env::temp_dir().join(format!("uis-projects-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let proj = base.join("Proj");
        std::fs::create_dir_all(proj.join("Assets/Deep")).unwrap();
        std::fs::create_dir_all(proj.join("ProjectSettings")).unwrap();
        std::fs::create_dir_all(proj.join("Packages")).unwrap();
        std::fs::write(proj.join("ProjectSettings/ProjectVersion.txt"), "m_EditorVersion: 6000.0.1f1\n").unwrap();
        std::fs::write(proj.join("Packages/manifest.json"), r#"{ "dependencies": {} }"#).unwrap();
        (Home::at(base.join("home")), proj)
    }

    #[test]
    fn add_is_idempotent_and_finds_the_root() {
        let (home, proj) = fake("add");
        let a = add(&home, &proj.join("Assets/Deep")).unwrap();
        let b = add(&home, &proj).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(list(&home).len(), 1);
        assert_eq!(a.probe.mode, "batch");
        assert!(add(&home, proj.parent().unwrap()).unwrap_err().starts_with("notProject:"));
        remove(&home, &a.id).unwrap();
        assert!(list(&home).is_empty());
        let _ = std::fs::remove_dir_all(home.root().parent().unwrap());
    }

    #[test]
    fn overrides_merge_and_clear() {
        let (home, proj) = fake("ovr");
        let id = add(&home, &proj).unwrap().id;
        let guid = "0123456789abcdef0123456789abcdef";
        let patch = |v: Value| v.as_object().unwrap().clone();
        set_override(&home, &id, guid, &patch(json!({ "kind": "prop", "rotation": [270, 0, 0] }))).unwrap();
        let o = set_override(&home, &id, guid, &patch(json!({ "yawSign": -1 }))).unwrap();
        assert_eq!(o.kind.as_deref(), Some("prop"));
        assert_eq!(o.rotation, Some([270.0, 0.0, 0.0]));
        assert_eq!(kind_overrides(&home, &id).get(guid).map(String::as_str), Some("prop"));
        assert!(set_override(&home, &id, guid, &patch(json!({ "kind": "dragon" }))).is_err());
        assert!(set_override(&home, &id, guid, &patch(json!({ "yawSign": 2 }))).is_err());
        set_override(&home, &id, guid, &patch(json!({ "kind": null, "rotation": null, "yawSign": null }))).unwrap();
        assert!(load_state(&home, &id).overrides.is_empty());
        let _ = std::fs::remove_dir_all(home.root().parent().unwrap());
    }
}
