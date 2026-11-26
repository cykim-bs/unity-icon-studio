//! `settings.json` in the tool's home (schema `unity-icon-studio.settings/1`).
//!
//! - Reading is lenient: a missing key, or a value of the wrong kind, takes the default for that key
//!   only; the rest of the file still counts. Values are then fitted (ranges, known choices).
//! - Writing merges: `update` takes a patch of changed keys (top-level names or dotted paths such
//!   as `capture.mode`), keeps every other key of the file as it is (unknown keys too) and writes
//!   the file atomically (`.tmp` + rename, retried while someone reads it).
//! - The first load writes the defaults, so a person can find and edit the file.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use crate::home::Home;

pub const SCHEMA: &str = "unity-icon-studio.settings/1";

pub const LANGUAGES: &[&str] = &["auto", "ko", "en"];
pub const THEMES: &[&str] = &["system", "light", "dark"];
pub const CAPTURE_MODES: &[&str] = &["auto", "attach", "batch"];
pub const REWRITERS: &[&str] = &["claude", "codex"];
pub const REVIEWERS: &[&str] = &["off", "claude", "codex"];
pub const OUTLINES: &[&str] = &["code", "model", "none"];
pub const BACKGROUNDS: &[&str] = &["transparent", "flat"];
pub const EXPORT_SIZES: &[u32] = &[256, 512, 1024];
pub const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema: String,
    pub language: String,
    pub theme: String,
    pub work_dir: Option<String>,
    pub export_dir: Option<String>,
    pub export_name: String,
    pub export_sizes: Vec<u32>,
    pub projects: Vec<ProjectEntry>,
    pub capture: Capture,
    pub codex: Codex,
    pub claude: Claude,
    pub llm: Llm,
    pub generate: Generate,
    pub thumbnails: Thumbnails,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectEntry {
    pub id: String,
    pub path: String,
    pub pack_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capture {
    pub mode: String,
    pub unity_cli: Option<String>,
    /// Unity version -> Unity.exe
    pub editors: BTreeMap<String, String>,
    pub batch_timeout_min: u32,
    pub attach_timeout_sec: u32,
    pub restore_tracked_files: bool,
    pub restore_patterns: Vec<String>,
    pub hooks: Hooks,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hooks {
    pub before_capture: Vec<Hook>,
    pub after_capture: Vec<Hook>,
}

/// A command run before or after a capture: `run` is the program and its arguments, with
/// placeholders such as `{projectPath}` filled in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hook {
    pub name: String,
    pub run: Vec<String>,
    pub timeout_sec: u32,
    /// Capture modes the hook runs for (`attach`, `batch`)
    pub when: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Codex {
    pub path: Option<String>,
    pub image_timeout_sec: u32,
    pub text_timeout_sec: u32,
    pub ignore_user_config: bool,
    pub clean_generated_images: bool,
    pub keep_api_key_env: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Claude {
    pub enabled: bool,
    pub path: Option<String>,
    pub model: String,
    pub effort: String,
    pub timeout_sec: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Llm {
    pub rewriter: String,
    pub review: String,
    pub confirm_rewrite: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Generate {
    pub candidates: u32,
    pub outline: String,
    pub background: String,
    pub key_color: String,
    pub saturation_boost: bool,
    pub style_refs: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thumbnails {
    pub size: u32,
    pub max_per_run: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: SCHEMA.into(),
            language: "auto".into(),
            theme: "system".into(),
            work_dir: None,
            export_dir: None,
            export_name: "icon_{prefab}".into(),
            export_sizes: vec![256],
            projects: Vec::new(),
            capture: Capture::default(),
            codex: Codex::default(),
            claude: Claude::default(),
            llm: Llm::default(),
            generate: Generate::default(),
            thumbnails: Thumbnails::default(),
        }
    }
}

impl Default for ProjectEntry {
    fn default() -> Self {
        ProjectEntry { id: String::new(), path: String::new(), pack_id: "default".into() }
    }
}

impl Default for Capture {
    fn default() -> Self {
        Capture {
            mode: "auto".into(),
            unity_cli: None,
            editors: BTreeMap::new(),
            batch_timeout_min: 30,
            attach_timeout_sec: 300,
            restore_tracked_files: true,
            restore_patterns: ["*.mat", "*.asset", "*.meta", "ProjectSettings/*"].map(String::from).to_vec(),
            hooks: Hooks::default(),
        }
    }
}

impl Default for Hook {
    fn default() -> Self {
        Hook { name: String::new(), run: Vec::new(), timeout_sec: 120, when: vec!["attach".into(), "batch".into()] }
    }
}

impl Default for Codex {
    fn default() -> Self {
        Codex {
            path: None,
            image_timeout_sec: 300,
            text_timeout_sec: 90,
            ignore_user_config: true,
            clean_generated_images: true,
            keep_api_key_env: false,
        }
    }
}

impl Default for Claude {
    fn default() -> Self {
        Claude { enabled: true, path: None, model: "sonnet".into(), effort: "low".into(), timeout_sec: 90 }
    }
}

impl Default for Llm {
    fn default() -> Self {
        Llm { rewriter: "claude".into(), review: "off".into(), confirm_rewrite: false }
    }
}

impl Default for Generate {
    fn default() -> Self {
        Generate {
            candidates: 1,
            outline: "code".into(),
            background: "transparent".into(),
            key_color: "auto".into(),
            saturation_boost: false,
            style_refs: true,
        }
    }
}

impl Default for Thumbnails {
    fn default() -> Self {
        Thumbnails { size: 256, max_per_run: 200 }
    }
}

fn pick(value: &mut String, allowed: &[&str], fallback: &str) {
    if !allowed.contains(&value.as_str()) {
        *value = fallback.into();
    }
}

fn clamp(value: &mut u32, min: u32, max: u32) {
    *value = (*value).clamp(min, max);
}

fn blank_to_none(value: &mut Option<String>) {
    if value.as_deref().is_some_and(|v| v.trim().is_empty()) {
        *value = None;
    }
}

impl Settings {
    /// Fits every value to its range or its known choices (what a person may have typed in the file).
    pub fn normalize(mut self) -> Settings {
        self.schema = SCHEMA.into();
        pick(&mut self.language, LANGUAGES, "auto");
        pick(&mut self.theme, THEMES, "system");
        blank_to_none(&mut self.work_dir);
        blank_to_none(&mut self.export_dir);
        if self.export_name.trim().is_empty() {
            self.export_name = "icon_{prefab}".into();
        }
        self.export_sizes.retain(|s| EXPORT_SIZES.contains(s));
        self.export_sizes.sort_unstable();
        self.export_sizes.dedup();
        if self.export_sizes.is_empty() {
            self.export_sizes = vec![256];
        }
        self.projects.retain(|p| !p.id.trim().is_empty() && !p.path.trim().is_empty());
        for p in &mut self.projects {
            if p.pack_id.trim().is_empty() {
                p.pack_id = "default".into();
            }
        }
        let c = &mut self.capture;
        pick(&mut c.mode, CAPTURE_MODES, "auto");
        blank_to_none(&mut c.unity_cli);
        clamp(&mut c.batch_timeout_min, 1, 240);
        clamp(&mut c.attach_timeout_sec, 10, 3600);
        for hook in c.hooks.before_capture.iter_mut().chain(c.hooks.after_capture.iter_mut()) {
            clamp(&mut hook.timeout_sec, 1, 3600);
            hook.when.retain(|w| w == "attach" || w == "batch");
        }
        blank_to_none(&mut self.codex.path);
        clamp(&mut self.codex.image_timeout_sec, 30, 1800);
        clamp(&mut self.codex.text_timeout_sec, 10, 600);
        blank_to_none(&mut self.claude.path);
        if self.claude.model.trim().is_empty() {
            self.claude.model = "sonnet".into();
        }
        pick(&mut self.claude.effort, CLAUDE_EFFORTS, "low");
        clamp(&mut self.claude.timeout_sec, 10, 600);
        pick(&mut self.llm.rewriter, REWRITERS, "claude");
        pick(&mut self.llm.review, REVIEWERS, "off");
        clamp(&mut self.generate.candidates, 1, 4);
        pick(&mut self.generate.outline, OUTLINES, "code");
        pick(&mut self.generate.background, BACKGROUNDS, "transparent");
        if self.generate.key_color.trim().is_empty() {
            self.generate.key_color = "auto".into();
        }
        clamp(&mut self.thumbnails.size, 64, 512);
        clamp(&mut self.thumbnails.max_per_run, 1, 2000);
        self
    }
}

/// `file` laid over `default`: objects key by key; a value only when it has the kind of the
/// default (a null default takes anything but an object or array of the wrong kind).
fn overlay(default: &Value, file: &Value) -> Value {
    match (default, file) {
        // a map with free keys (capture.editors): the file's keys
        (Value::Object(d), Value::Object(_)) if d.is_empty() => file.clone(),
        (Value::Object(d), Value::Object(f)) => {
            let mut out = d.clone();
            for (key, dv) in d {
                if let Some(fv) = f.get(key) {
                    out.insert(key.clone(), overlay(dv, fv));
                }
            }
            out.into()
        }
        (Value::Null, Value::String(_) | Value::Null) => file.clone(),
        (Value::String(_), Value::String(_) | Value::Null) => {
            if file.is_null() {
                default.clone()
            } else {
                file.clone()
            }
        }
        (Value::Bool(_), Value::Bool(_)) => file.clone(),
        (Value::Number(_), Value::Number(n)) => {
            // whole numbers only (every number setting is a u32)
            match n.as_f64() {
                Some(x) if x.is_finite() && x >= 0.0 => Value::from(x.round().min(u32::MAX as f64) as u64),
                _ => default.clone(),
            }
        }
        (Value::Array(_), Value::Array(_)) => file.clone(),
        _ => default.clone(),
    }
}

/// Settings from any JSON (the file, a test value). Never fails: what cannot be read is the default.
pub fn from_value(file: &Value) -> Settings {
    let defaults = serde_json::to_value(Settings::default()).unwrap_or(Value::Null);
    let mut merged = overlay(&defaults, file);
    // Lists keep only the items that read (a project or hook with a wrong field is dropped)
    keep_items::<ProjectEntry>(&mut merged, &["projects"]);
    keep_items::<Hook>(&mut merged, &["capture", "hooks", "beforeCapture"]);
    keep_items::<Hook>(&mut merged, &["capture", "hooks", "afterCapture"]);
    keep_items::<u32>(&mut merged, &["exportSizes"]);
    keep_items::<String>(&mut merged, &["capture", "restorePatterns"]);
    if let Some(Value::Object(editors)) = get_path_mut(&mut merged, &["capture", "editors"]) {
        editors.retain(|_, v| v.is_string());
    }
    serde_json::from_value::<Settings>(merged).unwrap_or_default().normalize()
}

fn get_path_mut<'a>(value: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    path.iter().try_fold(value, |v, key| v.get_mut(*key))
}

fn keep_items<T: serde::de::DeserializeOwned>(value: &mut Value, path: &[&str]) {
    if let Some(Value::Array(items)) = get_path_mut(value, path) {
        items.retain(|item| serde_json::from_value::<T>(item.clone()).is_ok());
    }
}

fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let text = text.trim_start_matches('\u{feff}');
            match serde_json::from_str::<Value>(text) {
                Ok(Value::Object(obj)) => Ok(Some(obj)),
                Ok(_) => Err("parse:not an object".into()),
                Err(e) => Err(format!("parse:{e}")),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read:{e}")),
    }
}

/// Writes `text` next to `path` and renames it over, retrying while the file is held open.
pub fn write_replace(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("write:{e}"))?;
    }
    let mut tmp_name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, text).map_err(|e| format!("write:{e}"))?;
    let mut last = None;
    for _ in 0..5 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(format!("write:{}", last.map(|e| e.to_string()).unwrap_or_default()))
}

/// One writer at a time inside this process.
static WRITE: Mutex<()> = Mutex::new(());

fn write_object(path: &Path, obj: &Map<String, Value>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&Value::Object(obj.clone())).map_err(|e| format!("write:{e}"))? + "\n";
    write_replace(path, &text)
}

/// The settings in effect. A missing file is created with the defaults; a broken one is left
/// alone (the defaults apply until it is fixed or a change rewrites it).
pub fn load(home: &Home) -> Settings {
    load_at(&home.settings_file())
}

pub fn load_at(path: &Path) -> Settings {
    match read_object(path) {
        Ok(Some(obj)) => from_value(&Value::Object(obj)),
        Ok(None) => {
            let settings = Settings::default();
            let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(Value::Object(obj)) = serde_json::to_value(&settings) {
                let _ = write_object(path, &obj);
            }
            settings
        }
        Err(_) => Settings::default(),
    }
}

/// Error text of a file that cannot be read (shown in the settings popup), or None.
pub fn file_problem(home: &Home) -> Option<String> {
    read_object(&home.settings_file()).err()
}

fn set_path(obj: &mut Map<String, Value>, path: &[&str], value: Value) {
    let Some((first, rest)) = path.split_first() else { return };
    if rest.is_empty() {
        obj.insert((*first).into(), value);
        return;
    }
    let entry = obj.entry((*first).to_string()).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(inner) = entry {
        set_path(inner, rest, value);
    }
}

fn remove_path(obj: &mut Map<String, Value>, path: &[&str]) {
    let Some((first, rest)) = path.split_first() else { return };
    if rest.is_empty() {
        obj.remove(*first);
    } else if let Some(Value::Object(inner)) = obj.get_mut(*first) {
        remove_path(inner, rest);
    }
}

fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |v, key| v.get(*key))
}

/// Merges `patch` (keys are top-level names or dotted paths) into the file and returns the
/// settings now in effect. An entry whose key is unknown or whose value does not fit is skipped.
/// Accepted entries are written as they are in effect (fitted); nothing else in the file changes.
pub fn update(home: &Home, patch: &Map<String, Value>) -> Result<Settings, String> {
    update_at(&home.settings_file(), patch)
}

pub fn update_at(path: &Path, patch: &Map<String, Value>) -> Result<Settings, String> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut obj = read_object(path)?.unwrap_or_default();
    let defaults = serde_json::to_value(Settings::default()).unwrap_or(Value::Null);
    let mut accepted: Vec<Vec<&str>> = Vec::new();
    for (key, value) in patch {
        let parts: Vec<&str> = key.split('.').collect();
        if parts[0] == "schema" || get_path(&defaults, &parts).is_none() {
            continue;
        }
        let mut trial = obj.clone();
        set_path(&mut trial, &parts, value.clone());
        // The entry must read back as given (after fitting) to count
        let settings = from_value(&Value::Object(trial.clone()));
        let effective = serde_json::to_value(&settings).unwrap_or(Value::Null);
        if get_path(&effective, &parts).is_some() && kind_fits(get_path(&defaults, &parts).unwrap(), value) {
            obj = trial;
            accepted.push(parts);
        }
    }
    let settings = from_value(&Value::Object(obj.clone()));
    let effective = serde_json::to_value(&settings).map_err(|e| format!("write:{e}"))?;
    for parts in &accepted {
        if let Some(v) = get_path(&effective, parts) {
            set_path(&mut obj, parts, v.clone());
        }
    }
    obj.insert("schema".into(), Value::String(SCHEMA.into()));
    write_object(path, &obj)?;
    Ok(settings)
}

fn kind_fits(default: &Value, value: &Value) -> bool {
    matches!(
        (default, value),
        (Value::Null, Value::String(_) | Value::Null)
            | (Value::String(_), Value::String(_))
            | (Value::Bool(_), Value::Bool(_))
            | (Value::Number(_), Value::Number(_))
            | (Value::Array(_), Value::Array(_))
            | (Value::Object(_), Value::Object(_))
    )
}

/// Removes the given keys (dotted paths) from the file, so they take their defaults; no keys
/// means every known key. Unknown keys of the file stay.
pub fn reset(home: &Home, keys: &[String]) -> Result<Settings, String> {
    let path = home.settings_file();
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut obj = read_object(&path)?.unwrap_or_default();
    let defaults = serde_json::to_value(Settings::default()).unwrap_or(Value::Null);
    if keys.is_empty() {
        if let Value::Object(d) = &defaults {
            for key in d.keys() {
                if key != "projects" {
                    obj.remove(key);
                }
            }
        }
    } else {
        for key in keys {
            let parts: Vec<&str> = key.split('.').collect();
            if get_path(&defaults, &parts).is_some() {
                remove_path(&mut obj, &parts);
            }
        }
    }
    let settings = from_value(&Value::Object(obj.clone()));
    // write the defaults back in full, so the file shows them
    let mut full = match serde_json::to_value(&settings) {
        Ok(Value::Object(o)) => o,
        _ => Map::new(),
    };
    for (k, v) in obj {
        full.entry(k).or_insert(v);
    }
    write_object(&path, &full)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_home(tag: &str) -> Home {
        let dir = std::env::temp_dir().join(format!("uis-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Home::at(dir)
    }

    #[test]
    fn first_load_writes_the_defaults() {
        let home = temp_home("first");
        let s = load(&home);
        assert_eq!(s, Settings::default());
        let text = std::fs::read_to_string(home.settings_file()).unwrap();
        assert!(text.contains("\"schema\": \"unity-icon-studio.settings/1\""));
        assert!(text.contains("\"exportName\": \"icon_{prefab}\""));
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn wrong_kinds_take_the_default_for_that_key_only() {
        let s = from_value(&json!({
            "language": 3, "theme": "dark", "exportSizes": [512, 7, 256, 512],
            "capture": { "mode": "warp", "batchTimeoutMin": 9999, "attachTimeoutSec": "x" },
            "generate": { "candidates": 2.6 }, "codex": "nope"
        }));
        assert_eq!(s.language, "auto");
        assert_eq!(s.theme, "dark");
        assert_eq!(s.export_sizes, vec![256, 512]);
        assert_eq!(s.capture.mode, "auto");
        assert_eq!(s.capture.batch_timeout_min, 240);
        assert_eq!(s.capture.attach_timeout_sec, 300);
        assert_eq!(s.generate.candidates, 3);
        assert_eq!(s.codex, Codex::default());
    }

    #[test]
    fn a_broken_projects_list_does_not_lose_the_rest() {
        let s = from_value(&json!({ "theme": "light", "projects": [{ "id": 5 }] }));
        assert_eq!(s.theme, "light");
        assert!(s.projects.is_empty());
    }

    #[test]
    fn update_merges_changed_keys_and_keeps_unknown_ones() {
        let home = temp_home("update");
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(home.settings_file(), r#"{ "theme": "dark", "mine": { "x": 1 }, "capture": { "mode": "batch" } }"#).unwrap();
        let mut patch = Map::new();
        patch.insert("language".into(), json!("ko"));
        patch.insert("capture.batchTimeoutMin".into(), json!(5000));
        patch.insert("nope".into(), json!(1));
        patch.insert("generate.outline".into(), json!(true));
        let s = update(&home, &patch).unwrap();
        assert_eq!(s.language, "ko");
        assert_eq!(s.theme, "dark");
        assert_eq!(s.capture.mode, "batch");
        assert_eq!(s.capture.batch_timeout_min, 240);
        assert_eq!(s.generate.outline, "code");
        let file: Value = serde_json::from_str(&std::fs::read_to_string(home.settings_file()).unwrap()).unwrap();
        assert_eq!(file["mine"], json!({ "x": 1 }));
        assert_eq!(file["capture"], json!({ "mode": "batch", "batchTimeoutMin": 240 }));
        assert!(file.get("nope").is_none());
        assert!(file["generate"].get("outline").is_none());
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn reset_brings_back_defaults_but_keeps_projects() {
        let home = temp_home("reset");
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(
            home.settings_file(),
            r#"{ "theme": "dark", "language": "en", "projects": [{ "id": "p1", "path": "C:/fake/p" }] }"#,
        )
        .unwrap();
        let s = reset(&home, &["theme".into()]).unwrap();
        assert_eq!(s.theme, "system");
        assert_eq!(s.language, "en");
        let s = reset(&home, &[]).unwrap();
        assert_eq!(s.language, "auto");
        assert_eq!(s.projects.len(), 1);
        assert_eq!(s.projects[0].pack_id, "default");
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn a_broken_file_reads_as_defaults_and_is_reported() {
        let home = temp_home("broken");
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(home.settings_file(), "{ not json").unwrap();
        assert_eq!(load(&home), Settings::default());
        assert!(file_problem(&home).unwrap().starts_with("parse:"));
        // still broken: the file is not replaced behind the person's back
        assert_eq!(std::fs::read_to_string(home.settings_file()).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn bom_is_accepted() {
        let home = temp_home("bom");
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(home.settings_file(), "\u{feff}{ \"theme\": \"light\" }").unwrap();
        assert_eq!(load(&home).theme, "light");
        let _ = std::fs::remove_dir_all(home.root());
    }
}
