//! The tool's home folder: `%USERPROFILE%\.unity-icon-studio`, or the folder in the environment
//! variable `UNITY_ICON_STUDIO_HOME` (tests and trial runs use a temporary one).
//!
//! The home is under the user profile, not AppData: a program started from a packaged shell gets a
//! private, redirected copy of AppData, so two programs would not see the same files.

use std::path::{Path, PathBuf};

pub const HOME_ENV: &str = "UNITY_ICON_STUDIO_HOME";
const HOME_DIR_NAME: &str = ".unity-icon-studio";

/// Sub folders, created on first use.
pub const SUB_DIRS: &[&str] = &[
    "projects", "packs", "cache", "codex-bin", "runtime", "journal", "backups", "hook-state", "logs", "work",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    /// The folder in `UNITY_ICON_STUDIO_HOME` when set (and not blank), else the default one.
    pub fn resolve() -> Home {
        if let Some(dir) = std::env::var_os(HOME_ENV).filter(|v| !v.to_string_lossy().trim().is_empty()) {
            return Home::at(PathBuf::from(dir));
        }
        let profile = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Home::at(profile.join(HOME_DIR_NAME))
    }

    pub fn at(root: impl Into<PathBuf>) -> Home {
        Home { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// True when the folder came from the environment variable.
    pub fn is_override() -> bool {
        std::env::var_os(HOME_ENV).is_some_and(|v| !v.to_string_lossy().trim().is_empty())
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// `<home>\<name>` for one of [`SUB_DIRS`].
    pub fn sub(&self, name: &str) -> PathBuf {
        debug_assert!(SUB_DIRS.contains(&name), "unknown sub folder {name}");
        self.root.join(name)
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.sub("projects")
    }
    pub fn packs_dir(&self) -> PathBuf {
        self.sub("packs")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.sub("cache")
    }
    pub fn runtime_dir(&self) -> PathBuf {
        self.sub("runtime")
    }
    pub fn journal_dir(&self) -> PathBuf {
        self.sub("journal")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.sub("backups")
    }
    pub fn hook_state_dir(&self) -> PathBuf {
        self.sub("hook-state")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.sub("logs")
    }
    /// Default work folder for runs (settings `workDir` replaces it).
    pub fn default_work_dir(&self) -> PathBuf {
        self.sub("work")
    }

    /// Creates the home and its sub folders.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        for name in SUB_DIRS {
            std::fs::create_dir_all(self.root.join(name))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_folders_live_under_the_root() {
        let home = Home::at("C:/fake/home");
        assert_eq!(home.settings_file(), PathBuf::from("C:/fake/home/settings.json"));
        assert_eq!(home.journal_dir(), PathBuf::from("C:/fake/home/journal"));
        assert_eq!(home.default_work_dir(), PathBuf::from("C:/fake/home/work"));
    }

    #[test]
    fn ensure_creates_every_sub_folder() {
        let dir = std::env::temp_dir().join(format!("uis-home-{}", std::process::id()));
        let home = Home::at(&dir);
        home.ensure().unwrap();
        for name in SUB_DIRS {
            assert!(dir.join(name).is_dir(), "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
