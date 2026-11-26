//! Writing finished icons out of the job folder. The tool never writes into a Unity project: a
//! folder inside one (a folder with `Assets` and `ProjectSettings` above it) or inside a project
//! listed in the settings is refused.

use serde::Serialize;
use serde_json::json;
use std::path::{Component, Path, PathBuf};

use crate::settings::Settings;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub size: u32,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub ok: bool,
    pub folder: String,
    pub files: Vec<Exported>,
    pub error: Option<String>,
}

/// `.` and `..` resolved without touching the disk (the folder may not exist yet).
fn lexical(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn same_or_inside(path: &Path, root: &Path) -> bool {
    let norm = |p: &Path| lexical(p).to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let (p, r) = (norm(path), norm(root));
    !r.is_empty() && (p == r || p.starts_with(&format!("{r}\\")))
}

/// Why `folder` may not receive icons, if it may not: `insideProject:<root>`.
pub fn refusal(folder: &Path, settings: &Settings) -> Option<String> {
    let target = lexical(folder);
    // Any existing folder above (or the folder itself) that looks like a Unity project
    let mut cur = Some(target.as_path());
    while let Some(dir) = cur {
        if dir.join("Assets").is_dir() && dir.join("ProjectSettings").is_dir() {
            return Some(format!("insideProject:{}", dir.display()));
        }
        cur = dir.parent();
    }
    settings
        .projects
        .iter()
        .find(|p| same_or_inside(&target, Path::new(&p.path)))
        .map(|p| format!("insideProject:{}", p.path))
}

/// The file name of an icon: the name rule with `{prefab}` replaced, characters Windows does not
/// allow replaced by `_`, and `_<size>` added for sizes after the first.
pub fn file_name(rule: &str, prefab: &str, size: u32, first: bool) -> String {
    let base = rule.replace("{prefab}", prefab).replace("{size}", &size.to_string());
    let mut clean: String = base.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    clean = clean.trim().trim_end_matches('.').to_string();
    if clean.is_empty() {
        clean = "icon".into();
    }
    if first || rule.contains("{size}") {
        format!("{clean}.png")
    } else {
        format!("{clean}_{size}.png")
    }
}

/// Copies `icons` (size, PNG file) to `folder` under the name rule, after the refusal check.
/// Existing files are replaced. Writes `export.json` into `job_dir` when given.
pub fn export(icons: &[(u32, PathBuf)], folder: &Path, rule: &str, prefab: &str, settings: &Settings, job_dir: Option<&Path>) -> ExportReport {
    let mut report = ExportReport { ok: false, folder: lexical(folder).display().to_string(), files: Vec::new(), error: None };
    if let Some(why) = refusal(folder, settings) {
        report.error = Some(why);
        return report;
    }
    if let Err(e) = std::fs::create_dir_all(folder) {
        report.error = Some(format!("write:{e}"));
        return report;
    }
    for (n, (size, src)) in icons.iter().enumerate() {
        let dst = folder.join(file_name(rule, prefab, *size, n == 0));
        let tmp = dst.with_extension("png.part");
        let done = std::fs::copy(src, &tmp).and_then(|_| std::fs::rename(&tmp, &dst));
        if let Err(e) = done {
            let _ = std::fs::remove_file(&tmp);
            report.error = Some(format!("write:{}:{e}", dst.display()));
            return report;
        }
        report.files.push(Exported { size: *size, path: dst.display().to_string() });
    }
    report.ok = true;
    if let Some(job) = job_dir {
        let doc = json!({
            "folder": report.folder,
            "files": report.files,
            "at": crate::capture::new_run_id(),
        });
        let _ = std::fs::write(job.join("export.json"), serde_json::to_string_pretty(&doc).unwrap_or_default());
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("uis-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn refuses_folders_inside_a_project() {
        let root = temp("refuse");
        let proj = root.join("Game");
        std::fs::create_dir_all(proj.join("Assets/Icons")).unwrap();
        std::fs::create_dir_all(proj.join("ProjectSettings")).unwrap();
        let s = Settings::default();
        assert!(refusal(&proj.join("Assets/Icons"), &s).unwrap().starts_with("insideProject:"));
        assert!(refusal(&proj.join("Assets/Icons/new/deeper"), &s).is_some(), "folders that do not exist yet too");
        assert!(refusal(&proj, &s).is_some());
        assert!(refusal(&root.join("Out/../Game/Assets"), &s).is_some(), "dots are resolved");
        assert!(refusal(&root.join("Out"), &s).is_none());
        // A listed project counts even if its folders are not there
        let mut listed = Settings::default();
        listed.projects.push(crate::settings::ProjectEntry { id: "p".into(), path: root.join("Listed").display().to_string(), pack_id: "default".into() });
        assert!(refusal(&root.join("listed/sub"), &listed).is_some(), "case-insensitive");
        assert!(refusal(&root.join("Listed2"), &listed).is_none());
        let src = root.join("icon.png");
        std::fs::write(&src, b"png").unwrap();
        let r = export(&[(256, src.clone())], &proj.join("Assets"), "icon_{prefab}", "crate", &s, None);
        assert!(!r.ok && r.files.is_empty());
        assert!(!proj.join("Assets/icon_crate.png").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn writes_named_files() {
        let root = temp("write");
        let (a, b) = (root.join("a.png"), root.join("b.png"));
        std::fs::write(&a, b"256").unwrap();
        std::fs::write(&b, b"512").unwrap();
        let out = root.join("Out");
        let r = export(&[(256, a), (512, b)], &out, "icon_{prefab}", "Lamp:Post", &Settings::default(), Some(&root));
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(std::fs::read(out.join("icon_Lamp_Post.png")).unwrap(), b"256");
        assert_eq!(std::fs::read(out.join("icon_Lamp_Post_512.png")).unwrap(), b"512");
        assert!(root.join("export.json").is_file());
        assert_eq!(file_name("{prefab}-{size}", "x", 512, false), "x-512.png");
        let _ = std::fs::remove_dir_all(&root);
    }
}
