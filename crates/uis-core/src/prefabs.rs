//! The prefab list of a Unity project, made without Unity.
//!
//! 1. Walk `Assets/` (names Unity hides are skipped: a leading `.` or a trailing `~`), collecting
//!    `.prefab` files, model files (`.fbx .obj .blend .dae`) and every `.meta`.
//! 2. guid index: the `guid:` line of each `.meta` (asset path by guid). Model `.meta` files also
//!    say whether the model has a humanoid rig (`animationType: 3`).
//! 3. Each prefab is scanned line by line: `--- !u!<classID>` documents (stripped ones too) and the
//!    references that carry its look: `m_SourcePrefab` (nested prefabs and variants) and `m_Mesh`
//!    of mesh filters and skinned renderers. A file that does not start with `%YAML` is binary
//!    serialization: listed as `unknown`.
//! 4. Class, first that applies: `ui` (RectTransform 224 of its own) > `3d` (MeshRenderer 23 or
//!    SkinnedMeshRenderer 137, or a model file) > `2d` (SpriteRenderer 212) > `fx` (ParticleSystem
//!    198/199) > `empty`. Except for `ui`, what it nests counts too: references are followed up to
//!    8 levels deep with a cycle guard (a wrapper or variant of a 3d or fx prefab is 3d or fx).
//! 5. Kind: the pack's `kindRules` (glob on the path, first match) > `character` when its
//!    references reach a skinned renderer or a humanoid model > `generic`. A person's choice
//!    (project state) wins over all of them.
//!
//! Cache (`<home>\cache\<project hash>\scan-cache.json`): per file, the size and modified time with
//! what was read from it. Files that did not change are not read again; classes and kinds are
//! worked out anew each time (cheap, in memory).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use crate::home::Home;
use crate::packs::{KindRule, Pack};

pub const SCAN_SCHEMA: &str = "unity-icon-studio.scan/1";
const CACHE_SCHEMA: &str = "unity-icon-studio.scan-cache/1";
pub const MODEL_EXTS: &[&str] = &["fbx", "obj", "blend", "dae"];
const MAX_DEPTH: usize = 8;

const UI: u32 = 224;
const MESH_RENDERER: u32 = 23;
const MESH_FILTER: u32 = 33;
const SKINNED: u32 = 137;
const SPRITE: u32 = 212;
const PARTICLES: u32 = 198;
const PARTICLE_RENDERER: u32 = 199;
const PREFAB_INSTANCE: u32 = 1001;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stamp {
    size: u64,
    mtime_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetaEntry {
    #[serde(flatten)]
    stamp: Stamp,
    guid: Option<String>,
    /// Model files only: a humanoid rig
    #[serde(default)]
    humanoid: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrefabEntry {
    #[serde(flatten)]
    stamp: Stamp,
    text: bool,
    /// Document count by class id (stripped documents too)
    classes: BTreeMap<u32, u32>,
    /// guids of nested prefabs and models (m_SourcePrefab) and meshes (m_Mesh)
    refs: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cache {
    schema: String,
    project: String,
    metas: HashMap<String, MetaEntry>,
    prefabs: HashMap<String, PrefabEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefabItem {
    pub guid: String,
    /// From the project folder, with forward slashes (`Assets/...`)
    pub path: String,
    pub name: String,
    /// Folder of the file, from the project folder
    pub folder: String,
    /// `3d`, `ui`, `2d`, `fx`, `empty`, `unknown`; `model` for a model file
    pub class: String,
    /// `building`, `prop`, `character`, `generic`
    pub kind: String,
    /// `rule`, `skinned`, `default`, `override`
    pub kind_source: String,
    /// The pack rule that matched, when one did
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub skinned: bool,
    /// Nested prefabs or models it refers to directly
    pub nested: u32,
    pub mtime_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub metas_read: u32,
    pub metas_cached: u32,
    pub prefabs_read: u32,
    pub prefabs_cached: u32,
    pub used_cache_file: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub schema: String,
    pub project: String,
    pub pack_id: String,
    pub total: usize,
    pub counts: BTreeMap<String, usize>,
    pub kinds: BTreeMap<String, usize>,
    pub metas: usize,
    pub models: usize,
    pub elapsed_ms: u64,
    pub cache: CacheStats,
    pub prefabs: Vec<PrefabItem>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Read every file again, ignoring the cache
    pub force: bool,
    /// List model files too (class `model`)
    pub include_models: bool,
    /// Kind chosen by a person, by guid
    pub kind_overrides: HashMap<String, String>,
}

fn stamp_of(meta: &std::fs::Metadata) -> Stamp {
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Stamp { size: meta.len(), mtime_ms }
}

/// Unity skips these names when it imports.
fn hidden(name: &str) -> bool {
    name.starts_with('.') || name.ends_with('~')
}

fn ext_lower(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

struct Walk {
    prefabs: Vec<(String, Stamp)>,
    metas: Vec<(String, Stamp)>,
    models: Vec<String>,
}

/// Paths are relative to the project folder, with forward slashes.
fn walk(project: &Path) -> Walk {
    let mut out = Walk { prefabs: Vec::new(), metas: Vec::new(), models: Vec::new() };
    let mut stack = vec![(project.join("Assets"), String::from("Assets"))];
    while let Some((dir, rel)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(ft) = entry.file_type() else { continue };
            let child_rel = format!("{rel}/{name}");
            if ft.is_dir() {
                if !hidden(&name) {
                    stack.push((entry.path(), child_rel));
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let ext = ext_lower(&name);
            if ext == "meta" {
                // the .meta of a hidden asset belongs to nothing Unity shows
                let asset = &name[..name.len() - 5];
                if hidden(asset) {
                    continue;
                }
                if let Ok(m) = entry.metadata() {
                    out.metas.push((child_rel, stamp_of(&m)));
                }
            } else if hidden(&name) {
                continue;
            } else if ext == "prefab" {
                if let Ok(m) = entry.metadata() {
                    out.prefabs.push((child_rel, stamp_of(&m)));
                }
            } else if MODEL_EXTS.contains(&ext.as_str()) {
                out.models.push(child_rel);
            }
        }
    }
    out.prefabs.sort_by(|a, b| a.0.cmp(&b.0));
    out.metas.sort_by(|a, b| a.0.cmp(&b.0));
    out.models.sort();
    out
}

fn is_hex_guid(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn read_meta(project: &Path, rel: &str, model: bool) -> (Option<String>, bool) {
    let Ok(bytes) = std::fs::read(project.join(rel)) else { return (None, false) };
    let text = String::from_utf8_lossy(&bytes);
    let mut guid = None;
    let mut humanoid = false;
    for line in text.lines() {
        let line = line.trim_start();
        if guid.is_none() {
            if let Some(g) = line.strip_prefix("guid:") {
                let g = g.trim();
                if is_hex_guid(g) {
                    guid = Some(g.to_ascii_lowercase());
                }
                if !model {
                    break;
                }
            }
        } else if model {
            if let Some(v) = line.strip_prefix("animationType:") {
                humanoid = v.trim() == "3";
                break;
            }
        }
    }
    (guid, humanoid)
}

/// The guid in `{fileID: 100100000, guid: abc..., type: 3}`.
fn guid_in(line: &str) -> Option<String> {
    let at = line.find("guid:")?;
    let g: String = line[at + 5..].trim_start().chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    is_hex_guid(&g).then(|| g.to_ascii_lowercase())
}

fn read_prefab(project: &Path, rel: &str, stamp: Stamp) -> PrefabEntry {
    let mut entry = PrefabEntry { stamp, ..PrefabEntry::default() };
    let Ok(bytes) = std::fs::read(project.join(rel)) else { return entry };
    let text = String::from_utf8_lossy(&bytes);
    entry.text = text.trim_start_matches('\u{feff}').starts_with("%YAML");
    if !entry.text {
        return entry;
    }
    let mut current = 0u32;
    let mut refs: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("--- !u!") {
            let id: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            current = id.parse().unwrap_or(0);
            *entry.classes.entry(current).or_insert(0) += 1;
            continue;
        }
        let t = line.trim_start();
        let wanted = (current == PREFAB_INSTANCE && t.starts_with("m_SourcePrefab:"))
            || ((current == MESH_FILTER || current == SKINNED) && t.starts_with("m_Mesh:"));
        if wanted {
            if let Some(g) = guid_in(t) {
                if !refs.contains(&g) {
                    refs.push(g);
                }
            }
        }
    }
    entry.refs = refs;
    entry
}

/// Runs `f` over `items` on a few threads (reading thousands of small files).
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
    if items.len() < 64 || threads == 1 {
        return items.iter().map(&f).collect();
    }
    let chunk = items.len().div_ceil(threads);
    std::thread::scope(|s| {
        let handles: Vec<_> = items.chunks(chunk).map(|part| s.spawn(|| part.iter().map(&f).collect::<Vec<R>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

/// Stable short name for a project folder (cache folder name).
pub fn project_hash(project: &Path) -> String {
    let text = project.display().to_string().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    // FNV-1a, 64 bit
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn cache_file(home: &Home, project: &Path) -> PathBuf {
    home.cache_dir().join(project_hash(project)).join("scan-cache.json")
}

fn load_cache(file: &Path, project: &Path) -> Option<Cache> {
    let bytes = std::fs::read(file).ok()?;
    let cache: Cache = serde_json::from_slice(&bytes).ok()?;
    (cache.schema == CACHE_SCHEMA && cache.project == project.display().to_string()).then_some(cache)
}

struct Graph<'a> {
    prefab_by_guid: HashMap<&'a str, &'a PrefabEntry>,
    model_by_guid: HashMap<&'a str, bool>,
}

/// What a prefab draws, its own documents and what it nests together.
#[derive(Debug, Clone, Copy, Default)]
struct Looks {
    three: bool,
    skinned: bool,
    sprite: bool,
    fx: bool,
}

impl Looks {
    fn own(entry: &PrefabEntry) -> Looks {
        let has = |id| entry.classes.contains_key(&id);
        Looks {
            three: has(MESH_RENDERER) || has(SKINNED),
            skinned: has(SKINNED),
            sprite: has(SPRITE),
            fx: has(PARTICLES) || has(PARTICLE_RENDERER),
        }
    }
    fn add(&mut self, other: Looks) {
        self.three |= other.three;
        self.skinned |= other.skinned;
        self.sprite |= other.sprite;
        self.fx |= other.fx;
    }
}

impl Graph<'_> {
    /// What `entry` draws, following nested prefabs and models (MAX_DEPTH levels, each prefab once).
    fn look(&self, entry: &PrefabEntry, depth: usize, seen: &mut HashSet<String>) -> Looks {
        let mut looks = Looks::own(entry);
        if depth >= MAX_DEPTH {
            return looks;
        }
        for r in &entry.refs {
            if let Some(humanoid) = self.model_by_guid.get(r.as_str()) {
                looks.three = true;
                looks.skinned |= *humanoid;
            } else if let Some(child) = self.prefab_by_guid.get(r.as_str()) {
                if seen.insert(r.clone()) {
                    looks.add(self.look(child, depth + 1, seen));
                }
            }
        }
        looks
    }
}

fn classify(entry: &PrefabEntry, looks: Looks) -> &'static str {
    if !entry.text {
        "unknown"
    } else if entry.classes.contains_key(&UI) {
        "ui"
    } else if looks.three {
        "3d"
    } else if looks.sprite {
        "2d"
    } else if looks.fx {
        "fx"
    } else {
        "empty"
    }
}

fn kind_for(path: &str, skinned: bool, rules: &[KindRule]) -> (String, &'static str, Option<String>) {
    if let Some(rule) = rules.iter().find(|r| r.matches(path)) {
        return (rule.kind.clone(), "rule", Some(rule.pattern.clone()));
    }
    if skinned {
        return ("character".into(), "skinned", None);
    }
    ("generic".into(), "default", None)
}

fn split_path(rel: &str) -> (String, String) {
    let (folder, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let name = file.rsplit_once('.').map(|(n, _)| n).unwrap_or(file);
    (folder.to_string(), name.to_string())
}

/// Lists and classifies the prefabs of `project` (a Unity project folder) with `pack`'s rules.
pub fn scan(home: &Home, project: &Path, pack: &Pack, options: &ScanOptions) -> Result<ScanResult, String> {
    crate::project::check_project(project).map_err(|e| format!("notProject:{}", e.0))?;
    let started = Instant::now();
    let file = cache_file(home, project);
    let old = if options.force { None } else { load_cache(&file, project) };
    let mut stats = CacheStats { used_cache_file: old.is_some(), ..CacheStats::default() };
    let old = old.unwrap_or_default();

    let found = walk(project);
    let model_meta = |rel: &str| MODEL_EXTS.contains(&ext_lower(&rel[..rel.len() - 5]).as_str());

    // guid index: reuse what did not change
    let metas: Vec<(String, MetaEntry)> = par_map(&found.metas, |(rel, stamp)| {
        if let Some(e) = old.metas.get(rel).filter(|e| &e.stamp == stamp) {
            return (rel.clone(), e.clone());
        }
        let (guid, humanoid) = read_meta(project, rel, model_meta(rel));
        (rel.clone(), MetaEntry { stamp: stamp.clone(), guid, humanoid })
    });
    for (rel, e) in &metas {
        if old.metas.get(rel).is_some_and(|o| o.stamp == e.stamp) {
            stats.metas_cached += 1;
        } else {
            stats.metas_read += 1;
        }
    }
    let prefabs: Vec<(String, PrefabEntry)> = par_map(&found.prefabs, |(rel, stamp)| {
        if let Some(e) = old.prefabs.get(rel).filter(|e| &e.stamp == stamp) {
            return (rel.clone(), e.clone());
        }
        (rel.clone(), read_prefab(project, rel, stamp.clone()))
    });
    for (rel, e) in &prefabs {
        if old.prefabs.get(rel).is_some_and(|o| o.stamp == e.stamp) {
            stats.prefabs_cached += 1;
        } else {
            stats.prefabs_read += 1;
        }
    }

    let meta_map: HashMap<&str, &MetaEntry> = metas.iter().map(|(r, e)| (r.as_str(), e)).collect();
    let guid_of = |asset: &str| meta_map.get(format!("{asset}.meta").as_str()).and_then(|e| e.guid.clone());
    let mut prefab_by_guid: HashMap<&str, &PrefabEntry> = HashMap::new();
    let mut prefab_guids: Vec<Option<String>> = Vec::with_capacity(prefabs.len());
    for (rel, _) in &prefabs {
        prefab_guids.push(guid_of(rel));
    }
    for ((_, entry), guid) in prefabs.iter().zip(&prefab_guids) {
        if let Some(g) = guid {
            prefab_by_guid.insert(g.as_str(), entry);
        }
    }
    let mut model_by_guid: HashMap<&str, bool> = HashMap::new();
    let mut model_guids: Vec<(String, Option<String>, bool)> = Vec::new();
    for rel in &found.models {
        let meta = meta_map.get(format!("{rel}.meta").as_str());
        model_guids.push((rel.clone(), meta.and_then(|m| m.guid.clone()), meta.is_some_and(|m| m.humanoid)));
    }
    for (_, guid, humanoid) in &model_guids {
        if let Some(g) = guid {
            model_by_guid.insert(g.as_str(), *humanoid);
        }
    }
    let graph = Graph { prefab_by_guid, model_by_guid };

    let rules = pack.kind_rules();
    let mut items = Vec::with_capacity(prefabs.len());
    for ((rel, entry), guid) in prefabs.iter().zip(&prefab_guids) {
        let mut seen = HashSet::new();
        if let Some(g) = guid {
            seen.insert(g.clone());
        }
        let looks = graph.look(entry, 0, &mut seen);
        let class = classify(entry, looks);
        let skinned = looks.skinned;
        let guid = guid.clone().unwrap_or_default();
        let (mut kind, mut source, rule) = kind_for(rel, skinned, &rules);
        if let Some(k) = options.kind_overrides.get(&guid) {
            kind = k.clone();
            source = "override";
        }
        let (folder, name) = split_path(rel);
        items.push(PrefabItem {
            guid,
            path: rel.clone(),
            name,
            folder,
            class: class.into(),
            kind,
            kind_source: source.into(),
            rule,
            skinned,
            nested: entry.refs.iter().filter(|r| graph.prefab_by_guid.contains_key(r.as_str()) || graph.model_by_guid.contains_key(r.as_str())).count() as u32,
            mtime_ms: entry.stamp.mtime_ms,
        });
    }
    if options.include_models {
        for (rel, guid, humanoid) in &model_guids {
            let guid = guid.clone().unwrap_or_default();
            let (mut kind, mut source, rule) = kind_for(rel, *humanoid, &rules);
            if let Some(k) = options.kind_overrides.get(&guid) {
                kind = k.clone();
                source = "override";
            }
            let (folder, name) = split_path(rel);
            items.push(PrefabItem {
                guid,
                path: rel.clone(),
                name,
                folder,
                class: "model".into(),
                kind,
                kind_source: source.into(),
                rule,
                skinned: *humanoid,
                nested: 0,
                mtime_ms: 0,
            });
        }
    }

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for it in &items {
        *counts.entry(it.class.clone()).or_insert(0) += 1;
        *kinds.entry(it.kind.clone()).or_insert(0) += 1;
    }

    // Save the cache (a failure only costs time next scan)
    let cache = Cache {
        schema: CACHE_SCHEMA.into(),
        project: project.display().to_string(),
        metas: metas.into_iter().collect(),
        prefabs: prefabs.into_iter().collect(),
    };
    if let Ok(text) = serde_json::to_string(&cache) {
        let _ = crate::settings::write_replace(&file, &text);
    }

    Ok(ScanResult {
        schema: SCAN_SCHEMA.into(),
        project: project.display().to_string(),
        pack_id: pack.id.clone(),
        total: found.prefabs.len(),
        counts,
        kinds,
        metas: found.metas.len(),
        models: found.models.len(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        cache: stats,
        prefabs: items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_from_reference_lines() {
        assert_eq!(
            guid_in("  m_SourcePrefab: {fileID: 100100000, guid: 0123456789ABCDEF0123456789abcdef, type: 3}").as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(guid_in("m_Mesh: {fileID: 0}"), None);
    }

    #[test]
    fn hidden_names() {
        assert!(hidden(".git") && hidden("Backup~") && !hidden("Props"));
    }

    #[test]
    fn split_paths() {
        assert_eq!(split_path("Assets/Props/Crate.prefab"), ("Assets/Props".into(), "Crate".into()));
    }

    #[test]
    fn project_hash_ignores_case_and_slashes() {
        assert_eq!(project_hash(Path::new("C:/fake/Proj/")), project_hash(Path::new("c:\\FAKE\\proj")));
    }
}
