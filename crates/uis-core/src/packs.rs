//! Style packs (`unity-icon-studio.pack/1`): capture presets per kind, kind rules, outline and
//! canvas values, check ranges and the prompt blocks.
//!
//! - The default pack ships in the program (`resources/packs/default/pack.json`) and holds no
//!   project's names.
//! - Local packs live in `<home>\packs\<id>\pack.json` (outside any repository). A local pack with
//!   `"extends": "<id>"` is laid over that pack: objects key by key, other values replaced, and its
//!   `kindRules` go before the base pack's rules.

use serde::Serialize;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

use crate::home::Home;

pub const PACK_SCHEMA: &str = "unity-icon-studio.pack/1";
pub const DEFAULT_ID: &str = "default";
pub const KINDS: &[&str] = &["building", "prop", "character", "generic"];
const DEFAULT_PACK: &str = include_str!("../resources/packs/default/pack.json");
const MAX_EXTENDS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub id: String,
    /// `builtin` or `local`
    pub source: String,
    pub dir: Option<PathBuf>,
    /// The pack after `extends` was applied
    pub doc: Value,
}

/// `{ "match": "**/Props/*.prefab", "kind": "prop", ... }`: the first rule whose glob matches the
/// path (from the project folder, `/` separators, case-insensitive) gives the kind. Other keys of
/// the rule (`capture`, `hideObjectsNamed`) are capture overrides for what it matches.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KindRule {
    pub pattern: String,
    pub kind: String,
    #[serde(skip)]
    pub extra: Value,
}

impl KindRule {
    pub fn matches(&self, path: &str) -> bool {
        glob_match(&self.pattern.to_lowercase(), &path.replace('\\', "/").to_lowercase())
    }
}

/// Glob on `/`-separated paths: `**` any number of folders (also none), `*` any run inside one
/// name, `?` one character inside a name. Both sides already lower case.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let s: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match_parts(&p, &s)
}

fn match_parts(p: &[&str], s: &[&str]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((&"**", rest)) => (0..=s.len()).any(|i| match_parts(rest, &s[i..])),
        Some((first, rest)) => !s.is_empty() && match_name(first.as_bytes(), s[0].as_bytes()) && match_parts(rest, &s[1..]),
    }
}

fn match_name(p: &[u8], s: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| match_name(rest, &s[i..])),
        Some((b'?', rest)) => !s.is_empty() && match_name(rest, &s[1..]),
        Some((c, rest)) => s.first() == Some(c) && match_name(rest, &s[1..]),
    }
}

/// `over` laid on `base`: objects merged key by key, anything else replaced. `kindRules` are put
/// before the base's.
fn merge(base: &Value, over: &Value, top: bool) -> Value {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            let mut out = b.clone();
            for (k, v) in o {
                let merged = if top && k == "kindRules" {
                    let mut rules = v.as_array().cloned().unwrap_or_default();
                    rules.extend(b.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default());
                    Value::Array(rules)
                } else if let Some(bv) = b.get(k) {
                    merge(bv, v, false)
                } else {
                    v.clone()
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        _ => over.clone(),
    }
}

impl Pack {
    pub fn builtin() -> Pack {
        let doc: Value = serde_json::from_str(DEFAULT_PACK).expect("default pack.json");
        Pack { id: DEFAULT_ID.into(), source: "builtin".into(), dir: None, doc }
    }

    pub fn kind_rules(&self) -> Vec<KindRule> {
        self.doc
            .get("kindRules")
            .and_then(|r| r.as_array())
            .map(|rules| {
                rules
                    .iter()
                    .filter_map(|r| {
                        let pattern = r.get("match")?.as_str()?.trim().to_string();
                        let kind = r.get("kind")?.as_str()?.to_string();
                        (!pattern.is_empty() && KINDS.contains(&kind.as_str())).then(|| KindRule { pattern, kind, extra: r.clone() })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The capture preset of a kind (`generic` when the pack has no such kind).
    pub fn kind_preset(&self, kind: &str) -> Value {
        let kinds = self.doc.get("kinds");
        kinds.and_then(|k| k.get(kind)).or_else(|| kinds.and_then(|k| k.get("generic"))).cloned().unwrap_or(Value::Null)
    }

    /// The check ranges for a kind: `checks.default` with `checks.<kind>` laid over it.
    pub fn checks_for(&self, kind: &str) -> Value {
        let checks = self.doc.get("checks");
        let base = checks.and_then(|c| c.get("default")).cloned().unwrap_or(Value::Object(Map::new()));
        match checks.and_then(|c| c.get(kind)) {
            Some(over) => merge(&base, over, false),
            None => base,
        }
    }

    pub fn name(&self, lang: &str) -> String {
        let name = self.doc.get("name");
        name.and_then(|n| n.get(lang))
            .or_else(|| name.and_then(|n| n.get("en")))
            .and_then(|n| n.as_str())
            .or_else(|| name.and_then(|n| n.as_str()))
            .unwrap_or(&self.id)
            .to_string()
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn read_local(dir: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(dir.join("pack.json")).map_err(|e| format!("read:{e}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("parse:{e}"))?;
    if !value.is_object() {
        return Err("parse:not an object".into());
    }
    Ok(value)
}

/// The pack `id`: the default one, or a local one laid over what it extends.
pub fn load(home: &Home, id: &str) -> Result<Pack, String> {
    load_depth(home, id, 0)
}

fn load_depth(home: &Home, id: &str, depth: usize) -> Result<Pack, String> {
    let dir = home.packs_dir().join(id);
    if id == DEFAULT_ID && !dir.join("pack.json").is_file() {
        return Ok(Pack::builtin());
    }
    if !valid_id(id) {
        return Err(format!("badId:{id}"));
    }
    if depth > MAX_EXTENDS {
        return Err("extendsTooDeep".into());
    }
    let own = read_local(&dir)?;
    let base = match own.get("extends").and_then(|e| e.as_str()) {
        Some(parent) if parent != id => load_depth(home, parent, depth + 1)?.doc,
        _ => Value::Object(Map::new()),
    };
    let mut doc = merge(&base, &own, true);
    if let Value::Object(o) = &mut doc {
        o.insert("id".into(), Value::String(id.into()));
    }
    Ok(Pack { id: id.into(), source: "local".into(), dir: Some(dir), doc })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub id: String,
    pub name: Value,
    pub source: String,
    pub extends: Option<String>,
    pub kind_rules: usize,
    pub style_refs: usize,
    pub dir: Option<String>,
    /// Why it cannot be used, when it cannot
    pub problem: Option<String>,
}

/// The default pack and every local pack folder.
pub fn list(home: &Home) -> Vec<PackSummary> {
    let summary = |id: &str, pack: Result<Pack, String>, dir: Option<PathBuf>| match pack {
        Ok(p) => PackSummary {
            id: id.into(),
            name: p.doc.get("name").cloned().unwrap_or(Value::Null),
            source: p.source.clone(),
            extends: p.doc.get("extends").and_then(|e| e.as_str()).map(str::to_string),
            kind_rules: p.kind_rules().len(),
            style_refs: p.doc.get("styleRefs").and_then(|r| r.as_array()).map_or(0, Vec::len),
            dir: p.dir.as_ref().map(|d| d.display().to_string()),
            problem: None,
        },
        Err(e) => PackSummary {
            id: id.into(),
            name: Value::Null,
            source: "local".into(),
            extends: None,
            kind_rules: 0,
            style_refs: 0,
            dir: dir.map(|d| d.display().to_string()),
            problem: Some(e),
        },
    };
    let mut out = vec![summary(DEFAULT_ID, load(home, DEFAULT_ID), None)];
    if let Ok(entries) = std::fs::read_dir(home.packs_dir()) {
        let mut ids: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join("pack.json").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|id| id != DEFAULT_ID)
            .collect();
        ids.sort();
        for id in ids {
            let dir = home.packs_dir().join(&id);
            out.push(summary(&id, load(home, &id), Some(dir)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn globs() {
        assert!(glob_match("**/props/*.prefab", "assets/game/props/crate.prefab"));
        assert!(glob_match("**/props/*.prefab", "props/crate.prefab"));
        assert!(!glob_match("**/props/*.prefab", "assets/props/sub/crate.prefab"));
        assert!(glob_match("**/characters/**", "assets/characters/a/b/hero.prefab"));
        assert!(glob_match("assets/?at.prefab", "assets/hat.prefab"));
        assert!(!glob_match("assets/*.prefab", "assets/a/b.prefab"));
    }

    #[test]
    fn default_pack_has_every_kind_and_no_rules() {
        let p = Pack::builtin();
        for k in KINDS {
            assert!(p.kind_preset(k).get("capture").is_some(), "{k}");
        }
        assert!(p.kind_rules().is_empty());
        assert_eq!(p.kind_preset("building")["capture"]["yaw"], json!(28));
        assert_eq!(p.checks_for("prop")["fill"], json!([0.25, 0.60]));
        assert_eq!(p.checks_for("prop")["saturation"], json!([0.35, 0.62]));
        assert_eq!(p.checks_for("building")["fill"], json!([0.38, 0.72]));
        assert_eq!(p.name("ko"), "캐주얼 두꺼운 외곽선");
    }

    #[test]
    fn local_pack_extends_and_its_rules_come_first() {
        let dir = std::env::temp_dir().join(format!("uis-packs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = Home::at(&dir);
        std::fs::create_dir_all(home.packs_dir().join("mine")).unwrap();
        std::fs::write(
            home.packs_dir().join("mine/pack.json"),
            r#"{ "extends": "default", "name": { "en": "Mine" },
                 "kinds": { "building": { "capture": { "front": "-z" } } },
                 "kindRules": [ { "match": "**/Props/*.prefab", "kind": "prop", "capture": { "rotation": [270, 0, 0] } },
                                { "match": "**/x", "kind": "dragon" } ] }"#,
        )
        .unwrap();
        let p = load(&home, "mine").unwrap();
        assert_eq!(p.kind_preset("building")["capture"]["front"], json!("-z"));
        assert_eq!(p.kind_preset("building")["capture"]["yaw"], json!(28));
        let rules = p.kind_rules();
        assert_eq!(rules.len(), 1, "unknown kinds are dropped");
        assert!(rules[0].matches("Assets/Game/Props/Crate.prefab"));
        assert_eq!(rules[0].extra["capture"]["rotation"], json!([270, 0, 0]));
        let all = list(&home);
        assert_eq!(all.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["default", "mine"]);
        assert_eq!(all[1].extends.as_deref(), Some("default"));
        assert!(load(&home, "../evil").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
