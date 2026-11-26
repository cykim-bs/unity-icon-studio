//! The prefab scan against tests/fixtures/SampleProject (a made-up project; see its README).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uis_core::home::Home;
use uis_core::packs::{self, Pack};
use uis_core::prefabs::{scan, PrefabItem, ScanOptions, ScanResult};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/SampleProject")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A copy of the fixture and an empty home, both in a fresh temp folder.
fn setup(tag: &str) -> (Home, PathBuf) {
    let base = std::env::temp_dir().join(format!("uis-scan-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("SampleProject");
    copy_dir(&fixture(), &project);
    (Home::at(base.join("home")), project)
}

fn by_name(result: &ScanResult) -> HashMap<String, PrefabItem> {
    result.prefabs.iter().map(|p| (p.name.clone(), p.clone())).collect()
}

#[test]
fn classes_follow_nested_prefabs_and_models() {
    let (home, project) = setup("classes");
    let r = scan(&home, &project, &Pack::builtin(), &ScanOptions::default()).unwrap();
    let items = by_name(&r);
    let class = |n: &str| items.get(n).unwrap_or_else(|| panic!("{n} missing")).class.as_str();
    assert_eq!(class("Crate"), "3d");
    assert_eq!(class("CrateRed"), "3d", "variant of a 3d prefab");
    assert_eq!(class("Robot"), "3d", "nests a model file");
    assert_eq!(class("HouseArt"), "3d");
    assert_eq!(class("House"), "3d", "wrapper around a 3d prefab");
    assert_eq!(class("Tower"), "3d", "two levels down");
    assert_eq!(class("Button"), "ui");
    assert_eq!(class("Sparkle"), "fx");
    assert_eq!(class("SparkleBig"), "fx", "variant of an fx prefab");
    assert_eq!(class("Coin"), "2d");
    assert_eq!(class("Empty"), "empty");
    assert_eq!(class("Binary"), "unknown");
    assert_eq!(class("LoopA"), "empty", "a cycle ends");
    assert_eq!(class("LoopB"), "empty");
    assert!(!items.contains_key("Ghost") && !items.contains_key(".Secret"), "hidden names are skipped");
    assert_eq!(r.total, 14);
    assert_eq!(r.counts.get("3d"), Some(&6));
    assert_eq!(r.models, 2);
    assert_eq!(items["Crate"].guid, "b2000000000000000000000000000001");
    assert_eq!(items["Crate"].folder, "Assets/Props");
    assert_eq!(items["Crate"].path, "Assets/Props/Crate.prefab");
    let _ = std::fs::remove_dir_all(home.root().parent().unwrap());
}

#[test]
fn kinds_come_from_rules_then_rig_then_default() {
    let (home, project) = setup("kinds");
    // without rules: a humanoid model makes a character, the rest is generic
    let r = scan(&home, &project, &Pack::builtin(), &ScanOptions::default()).unwrap();
    let items = by_name(&r);
    assert_eq!((items["Robot"].kind.as_str(), items["Robot"].kind_source.as_str()), ("character", "skinned"));
    assert!(items["Robot"].skinned);
    assert_eq!(items["House"].kind, "generic");

    // a local pack's rules, first match wins
    std::fs::create_dir_all(home.packs_dir().join("sample")).unwrap();
    std::fs::write(
        home.packs_dir().join("sample/pack.json"),
        r#"{ "extends": "default", "kindRules": [
              { "match": "**/Buildings/*.prefab", "kind": "building" },
              { "match": "**/props/**", "kind": "prop" },
              { "match": "**/*.prefab", "kind": "generic" } ] }"#,
    )
    .unwrap();
    let pack = packs::load(&home, "sample").unwrap();
    let mut options = ScanOptions::default();
    options.kind_overrides.insert("b2000000000000000000000000000006".into(), "prop".into());
    let r = scan(&home, &project, &pack, &options).unwrap();
    let items = by_name(&r);
    assert_eq!((items["House"].kind.as_str(), items["House"].kind_source.as_str()), ("building", "rule"));
    assert_eq!(items["House"].rule.as_deref(), Some("**/Buildings/*.prefab"));
    assert_eq!(items["Crate"].kind, "prop", "rules ignore case");
    assert_eq!(items["Robot"].kind_source, "rule", "a rule wins over the rig");
    assert_eq!((items["Tower"].kind.as_str(), items["Tower"].kind_source.as_str()), ("prop", "override"));
    let _ = std::fs::remove_dir_all(home.root().parent().unwrap());
}

#[test]
fn cache_reads_only_what_changed() {
    let (home, project) = setup("cache");
    let pack = Pack::builtin();
    let first = scan(&home, &project, &pack, &ScanOptions::default()).unwrap();
    assert!(!first.cache.used_cache_file);
    assert_eq!(first.cache.prefabs_read, 14);

    let second = scan(&home, &project, &pack, &ScanOptions::default()).unwrap();
    assert!(second.cache.used_cache_file);
    assert_eq!((second.cache.prefabs_read, second.cache.metas_read), (0, 0));
    assert_eq!(second.prefabs, first.prefabs);

    // Empty gets a renderer: only it is read again, and it turns 3d
    let file = project.join("Assets/Misc/Empty.prefab");
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("--- !u!23 &103\nMeshRenderer:\n  m_GameObject: {fileID: 100}\n");
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, text).unwrap();
    let third = scan(&home, &project, &pack, &ScanOptions::default()).unwrap();
    assert_eq!(third.cache.prefabs_read, 1);
    assert_eq!(by_name(&third)["Empty"].class, "3d");

    // a new prefab and a removed one
    std::fs::copy(project.join("Assets/Props/Crate.prefab"), project.join("Assets/Props/Crate2.prefab")).unwrap();
    std::fs::remove_file(project.join("Assets/Sprites/Coin.prefab")).unwrap();
    let fourth = scan(&home, &project, &pack, &ScanOptions::default()).unwrap();
    assert_eq!(fourth.cache.prefabs_read, 1);
    let items = by_name(&fourth);
    assert!(items.contains_key("Crate2") && !items.contains_key("Coin"));
    assert_eq!(items["Crate2"].guid, "", "no .meta yet: no guid");

    // force reads everything
    let forced = scan(&home, &project, &pack, &ScanOptions { force: true, ..ScanOptions::default() }).unwrap();
    assert_eq!(forced.cache.prefabs_read as usize, forced.total);
    let _ = std::fs::remove_dir_all(home.root().parent().unwrap());
}

#[test]
fn models_can_be_listed() {
    let (home, project) = setup("models");
    let r = scan(&home, &project, &Pack::builtin(), &ScanOptions { include_models: true, ..ScanOptions::default() }).unwrap();
    let models: Vec<&PrefabItem> = r.prefabs.iter().filter(|p| p.class == "model").collect();
    assert_eq!(models.len(), 2);
    let robot = models.iter().find(|m| m.name == "Robot").unwrap();
    assert_eq!((robot.kind.as_str(), robot.skinned), ("character", true));
    assert_eq!(r.total, 14, "total counts prefabs only");
    let _ = std::fs::remove_dir_all(home.root().parent().unwrap());
}

#[test]
fn not_a_project_is_refused() {
    let home = Home::at(std::env::temp_dir().join(format!("uis-scan-none-{}", std::process::id())));
    let err = scan(&home, &fixture().join("Assets"), &Pack::builtin(), &ScanOptions::default()).unwrap_err();
    assert!(err.starts_with("notProject:"), "{err}");
}
