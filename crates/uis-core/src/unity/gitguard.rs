//! Tracked files around a capture. Opening a project (even in batch mode) makes Unity rewrite
//! some serialized files: importer blocks added to `.meta`, font atlases, settings assets. The
//! tool must leave the working copy as it found it.
//!
//! 1. Before: `git status --porcelain=v1 -z` is kept ([`snapshot`]).
//! 2. After: files that were clean before and are modified now, and whose path matches one of the
//!    restore patterns (`*.mat`, `*.asset`, `*.meta`, `ProjectSettings/*`), are backed up as they are
//!    (`git diff --binary`, bytes untouched) to `<home>\backups\<run>.patch`, then put back with
//!    `git checkout --`.
//! 3. Files that were clean before and now differ from HEAD only in their line endings (a tool
//!    or plugin rewrote them with CRLF; `git diff --ignore-cr-at-eol` is empty) are put back too,
//!    whatever their name: nothing in them changed. Their bytes are copied next to the patch first
//!    (`<run>.files\<path>`), since a diff of line endings alone may come out empty.
//! 4. Any other change (scripts, scenes, prefabs, new files) is only listed, in `left` when it
//!    appeared during the run (the screen and the command line warn about those). Files that were
//!    already changed before the run are never touched.
//!
//! Outside git nothing can be put back; the result says so (`notGit`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::packs::glob_match;
use crate::proc::{run, Cmd};

const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// `XY` status by path (from the project folder, `/` separators).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub is_git: bool,
    pub entries: BTreeMap<String, String>,
    /// Why git could not be read, when it could not
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl Snapshot {
    /// `git status --short` lines, sorted (for reports).
    pub fn short_lines(&self) -> Vec<String> {
        self.entries.iter().map(|(p, xy)| format!("{xy} {p}")).collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardReport {
    pub is_git: bool,
    /// Status after the run (and after putting files back) is the one before
    pub same: bool,
    /// Files put back
    pub restored: Vec<String>,
    /// The patch holding what was put back
    pub backup: Option<String>,
    /// Changes that appeared during the run and were left as they are
    pub other: Vec<String>,
    /// Put back because only their line endings changed (also in `restored`)
    #[serde(default)]
    pub line_endings: Vec<String>,
    /// Changes that appeared during the run and are still there (a person should look)
    #[serde(default)]
    pub left: Vec<String>,
    pub before: Vec<String>,
    pub after: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

fn git(project: &Path) -> Cmd {
    Cmd::new("git")
        .arg("-C")
        .arg(project.as_os_str())
        .args(["--no-optional-locks", "-c", "core.quotepath=false"])
        .timeout(GIT_TIMEOUT)
}

/// True when `project` is inside a git working copy.
pub fn is_git(project: &Path) -> bool {
    run(&git(project).args(["rev-parse", "--is-inside-work-tree"]))
        .map(|o| o.success() && o.stdout_text().trim() == "true")
        .unwrap_or(false)
}

/// Parses `git status --porcelain=v1 -z`: `XY path\0`, renames and copies carry the old path as
/// the next field.
pub fn parse_status_z(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut fields = bytes.split(|b| *b == 0).filter(|f| !f.is_empty());
    while let Some(f) = fields.next() {
        if f.len() < 4 {
            continue;
        }
        let xy = String::from_utf8_lossy(&f[..2]).into_owned();
        let path = String::from_utf8_lossy(&f[3..]).into_owned();
        if xy.starts_with('R') || xy.starts_with('C') {
            let _old = fields.next();
        }
        out.insert(path, xy);
    }
    out
}

pub fn snapshot(project: &Path) -> Snapshot {
    if !is_git(project) {
        return Snapshot { is_git: false, entries: BTreeMap::new(), problem: Some("notGit".into()) };
    }
    match run(&git(project).args(["status", "--porcelain=v1", "-z"])) {
        Ok(o) if o.success() => Snapshot { is_git: true, entries: parse_status_z(&o.stdout), problem: None },
        Ok(o) => Snapshot { is_git: true, entries: BTreeMap::new(), problem: Some(format!("status:{}", o.code())) },
        Err(e) => Snapshot { is_git: true, entries: BTreeMap::new(), problem: Some(format!("status:{e}")) },
    }
}

/// `pattern` with a `/` matches the whole path, otherwise the file name. Case-insensitive.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    let p = pattern.trim().replace('\\', "/").to_lowercase();
    let path = path.to_lowercase();
    if p.is_empty() {
        return false;
    }
    if p.contains('/') {
        glob_match(&p, &path)
    } else {
        let name = path.rsplit('/').next().unwrap_or(&path);
        glob_match(&p, name)
    }
}

/// Modified in the working tree (or index) and tracked: ` M`, `M `, `MM`, ` T`.
fn is_tracked_change(xy: &str) -> bool {
    let b = xy.as_bytes();
    b.len() == 2 && b[0] != b'?' && b[0] != b'!' && (b[0] == b'M' || b[1] == b'M' || b[1] == b'T' || b[0] == b'T') && b[0] != b'D' && b[1] != b'D'
}

/// Compares with `before`, puts restorable files back (when `restore`), and reports.
pub fn restore_and_compare(project: &Path, before: &Snapshot, patterns: &[String], restore: bool, backup: &Path) -> GuardReport {
    let mut report = GuardReport { is_git: before.is_git, before: before.short_lines(), ..GuardReport::default() };
    if !before.is_git {
        report.problem = Some("notGit".into());
        return report;
    }
    let now = snapshot(project);
    if let Some(p) = &now.problem {
        report.problem = Some(p.clone());
        return report;
    }
    let mut to_restore: Vec<String> = Vec::new();
    for (path, xy) in &now.entries {
        if before.entries.get(path) == Some(xy) {
            continue;
        }
        if before.entries.contains_key(path) {
            // It was already changed before the run: never touched, only listed
            report.other.push(format!("{xy} {path}"));
            continue;
        }
        if restore && is_tracked_change(xy) && patterns.iter().any(|p| matches_pattern(p, path)) {
            to_restore.push(path.clone());
        } else if restore && is_tracked_change(xy) && line_endings_only(project, path) {
            report.line_endings.push(path.clone());
            to_restore.push(path.clone());
        } else {
            report.other.push(format!("{xy} {path}"));
        }
    }
    if !to_restore.is_empty() {
        match save_patch_and_checkout(project, &to_restore, &report.line_endings, backup) {
            Ok(()) => {
                report.restored = to_restore;
                report.backup = Some(backup.display().to_string());
            }
            Err(e) => {
                report.problem = Some(e);
                report.other.extend(to_restore.iter().map(|p| format!(" M {p}")));
                report.line_endings.clear();
            }
        }
    }
    let after = snapshot(project);
    report.after = after.short_lines();
    // What the run left behind: new or changed since the start, still there now
    report.left = after
        .entries
        .iter()
        .filter(|(p, xy)| before.entries.get(*p) != Some(*xy))
        .map(|(p, xy)| format!("{xy} {p}"))
        .collect();
    report.same = after.problem.is_none() && after.entries == before.entries;
    report
}

/// True when the working file differs from the index only in CR at line ends (or not at all).
pub fn line_endings_only(project: &Path, path: &str) -> bool {
    run(&git(project).args(["diff", "--ignore-cr-at-eol", "--quiet", "--"]).arg(path)).map(|o| o.success()).unwrap_or(false)
}

fn save_patch_and_checkout(project: &Path, paths: &[String], raw_copies: &[String], backup: &Path) -> Result<(), String> {
    let diff = run(&git(project).args(["diff", "--binary", "--"]).args(paths.iter())).map_err(|e| format!("diff:{e}"))?;
    if !diff.success() {
        return Err(format!("diff:{}", diff.code()));
    }
    if let Some(dir) = backup.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("backup:{e}"))?;
    }
    std::fs::write(backup, &diff.stdout).map_err(|e| format!("backup:{e}"))?;
    // Line-ending-only files: their bytes as they are (the diff can be empty for them)
    if !raw_copies.is_empty() {
        let dir = backup.with_extension("files");
        for rel in raw_copies {
            let dst = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(d) = dst.parent() {
                std::fs::create_dir_all(d).map_err(|e| format!("backup:{e}"))?;
            }
            std::fs::copy(project.join(rel), &dst).map_err(|e| format!("backup:{rel}:{e}"))?;
        }
    }
    let co = run(&git(project).args(["checkout", "--"]).args(paths.iter())).map_err(|e| format!("checkout:{e}"))?;
    if !co.success() {
        return Err(format!("checkout:{}", co.code()));
    }
    Ok(())
}

/// `<home>\backups\<run id>.patch`
pub fn backup_path(backups_dir: &Path, run_id: &str) -> PathBuf {
    backups_dir.join(format!("{run_id}.patch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_z() {
        let raw = b" M Assets/a.meta\0?? Assets/New/\0R  new.txt\0old.txt\0MM ProjectSettings/x.asset\0";
        let m = parse_status_z(raw);
        assert_eq!(m.get("Assets/a.meta").map(String::as_str), Some(" M"));
        assert_eq!(m.get("Assets/New/").map(String::as_str), Some("??"));
        assert_eq!(m.get("new.txt").map(String::as_str), Some("R "));
        assert!(!m.contains_key("old.txt"));
        assert_eq!(m.get("ProjectSettings/x.asset").map(String::as_str), Some("MM"));
    }

    #[test]
    fn patterns() {
        assert!(matches_pattern("*.meta", "Assets/Deep/a.txt.meta"));
        assert!(matches_pattern("*.asset", "Assets/Fonts/X SDF.asset"));
        assert!(matches_pattern("ProjectSettings/*", "ProjectSettings/ProjectSettings.asset"));
        assert!(!matches_pattern("ProjectSettings/*", "Assets/ProjectSettings/x.txt"));
        assert!(!matches_pattern("*.mat", "Assets/a.prefab"));
    }

    fn git_in(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=test", "-c", "user.name=test"])
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn line_ending_only_changes_are_put_back_and_others_are_reported() {
        let d = std::env::temp_dir().join(format!("uis-eol-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Assets/Plugins/res")).unwrap();
        // LF in the repository, normalized like a project with `eol=lf`
        std::fs::write(d.join(".gitattributes"), "*.xml text eol=lf\n*.txt text eol=lf\n").unwrap();
        std::fs::write(d.join("Assets/Plugins/res/values.xml"), "<a>\n  <b/>\n</a>\n").unwrap();
        std::fs::write(d.join("Assets/notes.txt"), "one\ntwo\n").unwrap();
        std::fs::write(d.join("Assets/kept.txt"), "x\n").unwrap();
        assert!(git_in(&d, &["init", "-q"]));
        assert!(git_in(&d, &["config", "core.autocrlf", "false"]));
        assert!(git_in(&d, &["add", "-A"]));
        assert!(git_in(&d, &["commit", "-q", "-m", "x"]));
        // Already changed before the run: never touched
        std::fs::write(d.join("Assets/kept.txt"), "changed before\n").unwrap();
        let before = snapshot(&d);
        // The run: one file rewritten with CRLF only, one with a real change
        std::fs::write(d.join("Assets/Plugins/res/values.xml"), "<a>\r\n  <b/>\r\n</a>\r\n").unwrap();
        std::fs::write(d.join("Assets/notes.txt"), "one\ntwo\nthree\n").unwrap();
        let mid = snapshot(&d);
        assert!(mid.entries.contains_key("Assets/Plugins/res/values.xml"), "git sees the CRLF file as changed: {:?}", mid.entries);
        assert!(line_endings_only(&d, "Assets/Plugins/res/values.xml"));
        assert!(!line_endings_only(&d, "Assets/notes.txt"));
        let backup = d.with_extension("bk").join("run1.patch");
        let r = restore_and_compare(&d, &before, &["*.meta".to_string()], true, &backup);
        assert_eq!(r.line_endings, ["Assets/Plugins/res/values.xml"]);
        assert_eq!(r.restored, ["Assets/Plugins/res/values.xml"]);
        assert_eq!(std::fs::read_to_string(d.join("Assets/Plugins/res/values.xml")).unwrap(), "<a>\n  <b/>\n</a>\n");
        // the CRLF bytes were kept
        assert!(std::fs::read(d.with_extension("bk").join("run1.files/Assets/Plugins/res/values.xml")).unwrap().windows(2).any(|w| w == b"\r\n"));
        assert_eq!(r.left, [" M Assets/notes.txt"], "the real change is left and reported");
        assert_eq!(std::fs::read_to_string(d.join("Assets/kept.txt")).unwrap(), "changed before\n");
        assert!(!r.same);
        // Not asked to restore: nothing put back, both reported
        std::fs::write(d.join("Assets/Plugins/res/values.xml"), "<a>\r\n  <b/>\r\n</a>\r\n").unwrap();
        let r2 = restore_and_compare(&d, &before, &[], false, &d.with_extension("bk").join("run2.patch"));
        assert!(r2.restored.is_empty() && r2.left.len() == 2);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(d.with_extension("bk"));
    }

    #[test]
    fn tracked_changes() {
        assert!(is_tracked_change(" M"));
        assert!(is_tracked_change("M "));
        assert!(!is_tracked_change("??"));
        assert!(!is_tracked_change(" D"));
        assert!(!is_tracked_change("A "));
    }
}
