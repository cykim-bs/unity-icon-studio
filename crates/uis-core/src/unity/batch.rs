//! Capture in a closed project (path D): Unity in batch mode with the capture script copied into a
//! temporary folder of the project.
//!
//! - The entry folder is `Assets/__UnityIconStudioCapture/Editor/` with the script and its own
//!   editor-only asmdef. It is untracked; it is removed (with the `.meta` files Unity made) as soon
//!   as Unity has ended. While Unity runs it is never touched.
//! - Unity is started by `start-batch.ps1` through WMI (`Win32_Process.Create`): not a child of the
//!   tool, the user's own environment, never in a Job Object. `-nographics` is never passed (renders
//!   come back as a constant colour without a device).
//! - The tool only waits for the process. It never ends Unity: when the wait runs out, it stops
//!   waiting and the journal keeps the run for later.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::proc::{run, Cmd};

/// Folder under `Assets/` that holds the entry script during a batch run.
pub const ENTRY_FOLDER: &str = "__UnityIconStudioCapture";
pub const SCRIPT_NAME: &str = "PrefabIconCapture.cs";
pub const ASMDEF_NAME: &str = "UnityIconStudio.Capture.Editor.asmdef";
pub const METHOD: &str = "UnityIconStudio.PrefabIconCapture.RunFromCommandLine";
pub const LOG_PREFIX: &str = "[IconStudio] ";

pub const SCRIPT: &str = include_str!("../../resources/capture/PrefabIconCapture.cs");
pub const ASMDEF: &str = include_str!("../../resources/capture/UnityIconStudio.Capture.Editor.asmdef");
pub const START_BATCH: &str = include_str!("../../resources/scripts/start-batch.ps1");

pub fn entry_root(project: &Path) -> PathBuf {
    project.join("Assets").join(ENTRY_FOLDER)
}

/// Writes the files the capture uses to `<runtime>\<hash>\` (once per content) and returns that
/// folder. The hash keeps an old copy from being used by a newer tool.
pub fn runtime_files(runtime_dir: &Path) -> Result<PathBuf, String> {
    let hash = fnv(&[SCRIPT, ASMDEF, START_BATCH].concat());
    let dir = runtime_dir.join(format!("capture-{hash:016x}"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("runtime:{e}"))?;
    for (name, text) in [(SCRIPT_NAME, SCRIPT), (ASMDEF_NAME, ASMDEF), ("start-batch.ps1", START_BATCH)] {
        let p = dir.join(name);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(text) {
            std::fs::write(&p, text).map_err(|e| format!("runtime:{e}"))?;
        }
    }
    Ok(dir)
}

fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Copies the script and asmdef into the entry folder. Refuses (`entryExists`) when the folder is
/// already there: someone else's run, or one left behind that the journal does not know.
pub fn place_entry(project: &Path, runtime: &Path) -> Result<(), String> {
    let root = entry_root(project);
    if root.exists() {
        return Err("entryExists".into());
    }
    let dir = root.join("Editor");
    std::fs::create_dir_all(&dir).map_err(|e| format!("entry:{e}"))?;
    for name in [SCRIPT_NAME, ASMDEF_NAME] {
        std::fs::copy(runtime.join(name), dir.join(name)).map_err(|e| format!("entry:{e}"))?;
    }
    Ok(())
}

/// Removes the entry folder and its `.meta`. Returns true when something was removed.
pub fn remove_entry(project: &Path) -> Result<bool, String> {
    let root = entry_root(project);
    let meta = project.join("Assets").join(format!("{ENTRY_FOLDER}.meta"));
    let mut removed = false;
    if root.exists() {
        let mut last = None;
        for _ in 0..10 {
            match std::fs::remove_dir_all(&root) {
                Ok(()) => {
                    last = None;
                    break;
                }
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        if let Some(e) = last {
            return Err(format!("entryRemove:{e}"));
        }
        removed = true;
    }
    if meta.exists() {
        std::fs::remove_file(&meta).map_err(|e| format!("entryRemove:{e}"))?;
        removed = true;
    }
    Ok(removed)
}

/// Starts Unity through `start-batch.ps1` and returns its process id.
pub fn start(runtime: &Path, unity_exe: &Path, project: &Path, log_file: &Path, job_file: &Path) -> Result<u32, String> {
    let ps = std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    let cmd = Cmd::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(runtime.join("start-batch.ps1").as_os_str())
        .arg("-UnityExe")
        .arg(unity_exe.as_os_str())
        .arg("-ProjectPath")
        .arg(project.as_os_str())
        .arg("-LogFile")
        .arg(log_file.as_os_str())
        .arg("-JobFile")
        .arg(job_file.as_os_str())
        .timeout(Duration::from_secs(90));
    let out = run(&cmd).map_err(|e| format!("start:{e}"))?;
    let text = out.stdout_text();
    for line in text.lines() {
        if let Some(pid) = line.trim().strip_prefix("pid=") {
            return pid.trim().parse::<u32>().map_err(|e| format!("start:pid {e}"));
        }
        if let Some(err) = line.trim().strip_prefix("error=") {
            return Err(format!("start:{err}"));
        }
    }
    Err(format!("start:{}", out.code()))
}

/// What the capture script wrote to the Unity log so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogProgress {
    pub total: Option<u32>,
    pub done: u32,
    pub failed: u32,
    /// Last `[IconStudio]` line without the prefix
    pub last: Option<String>,
    /// From the `exit <code>` line
    pub exit_code: Option<i32>,
}

pub fn read_progress(log_file: &Path) -> LogProgress {
    let mut p = LogProgress::default();
    let Ok(bytes) = std::fs::read(log_file) else { return p };
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix(LOG_PREFIX) else { continue };
        if let Some(n) = rest.strip_prefix("items ") {
            p.total = n.trim().parse().ok();
        } else if rest.starts_with("ok ") {
            p.done += 1;
        } else if rest.starts_with("FAIL ") {
            p.done += 1;
            p.failed += 1;
        } else if let Some(c) = rest.strip_prefix("exit ") {
            p.exit_code = c.trim().parse().ok();
        }
        p.last = Some(rest.chars().take(300).collect());
    }
    p
}

/// The last `n` lines of the Unity log (for error reports).
pub fn log_tail(log_file: &Path, n: usize) -> Vec<String> {
    let Ok(bytes) = std::fs::read(log_file) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| crate::hooks::mask(l)).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitResult {
    pub exited: bool,
    pub exit_code: Option<i32>,
    pub waited_ms: u64,
}

/// Waits for the Unity process `pid` (started elsewhere) up to `limit`, calling `tick` with the log
/// progress about once a second. Never ends the process.
pub fn wait(pid: u32, limit: Duration, log_file: &Path, tick: &mut dyn FnMut(&LogProgress)) -> WaitResult {
    let started = Instant::now();
    let watch = watch::Watch::open(pid);
    let mut last = LogProgress::default();
    loop {
        let exited = match &watch {
            Some(w) => w.wait(Duration::from_millis(1000)),
            None => {
                std::thread::sleep(Duration::from_millis(1000));
                !crate::proc::pid_alive(pid)
            }
        };
        let p = read_progress(log_file);
        if p != last {
            tick(&p);
            last = p;
        }
        if exited {
            let code = watch.as_ref().and_then(|w| w.exit_code()).or(last.exit_code);
            return WaitResult { exited: true, exit_code: code, waited_ms: started.elapsed().as_millis() as u64 };
        }
        if started.elapsed() >= limit {
            return WaitResult { exited: false, exit_code: None, waited_ms: started.elapsed().as_millis() as u64 };
        }
    }
}

/// True while `pid` runs and is the program at `exe` (an id the system gave to another program
/// after Unity ended does not count).
pub fn unity_running(pid: u32, exe: &str) -> bool {
    if pid == 0 || !crate::proc::pid_alive(pid) {
        return false;
    }
    match watch::image_path(pid) {
        Some(path) => crate::unity::probe::same_path(&path, exe),
        None => true,
    }
}

#[cfg(windows)]
mod watch {
    use std::time::Duration;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    /// A handle to watch a process the tool did not start: wait for it and read its exit code.
    pub struct Watch(HANDLE);

    impl Watch {
        pub fn open(pid: u32) -> Option<Watch> {
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok().map(Watch) }
        }
        /// True when the process has ended (within `limit`).
        pub fn wait(&self, limit: Duration) -> bool {
            unsafe { WaitForSingleObject(self.0, limit.as_millis() as u32) == WAIT_OBJECT_0 }
        }
        pub fn exit_code(&self) -> Option<i32> {
            let mut code = 0u32;
            unsafe {
                GetExitCodeProcess(self.0, &mut code).ok()?;
            }
            (code != STILL_ACTIVE.0 as u32).then_some(code as i32)
        }
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn image_path(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = vec![0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }
}

#[cfg(not(windows))]
mod watch {
    use std::time::Duration;
    pub struct Watch;
    impl Watch {
        pub fn open(_pid: u32) -> Option<Watch> {
            None
        }
        pub fn wait(&self, _limit: Duration) -> bool {
            true
        }
        pub fn exit_code(&self) -> Option<i32> {
            None
        }
    }
    pub fn image_path(_pid: u32) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_progress_lines() {
        let dir = std::env::temp_dir().join(format!("uis-batchlog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("unity.log");
        std::fs::write(
            &log,
            "noise\n[IconStudio] entry x\n[IconStudio] items 3\n[IconStudio] ok a 10ms C:/x/a.png\n[IconStudio] FAIL b 5ms prefab_not_found\n[IconStudio] exit 2\n",
        )
        .unwrap();
        let p = read_progress(&log);
        assert_eq!((p.total, p.done, p.failed, p.exit_code), (Some(3), 2, 1, Some(2)));
        assert_eq!(log_tail(&log, 2), vec!["[IconStudio] FAIL b 5ms prefab_not_found", "[IconStudio] exit 2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_files_are_ascii() {
        for (name, text) in [(SCRIPT_NAME, SCRIPT), (ASMDEF_NAME, ASMDEF), ("start-batch.ps1", START_BATCH)] {
            assert!(text.is_ascii(), "{name} must be ASCII");
        }
        assert!(SCRIPT.contains("RunFromCommandLine") && SCRIPT.contains("RunJobFile"));
    }
}
