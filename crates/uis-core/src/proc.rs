//! One runner for the external programs the tool starts (Codex, Claude, the `unity` CLI, hooks).
//!
//! - No console window (`CREATE_NO_WINDOW`); stdin is written as the given bytes (UTF-8 text stays
//!   UTF-8, whatever the console code page); stdout and stderr are collected as bytes.
//! - Environment: variables can be removed by exact name or by prefix (case-insensitive, as
//!   Windows treats names), and set.
//! - Time limit: the child is put in a Job Object that ends its whole process tree when the job is
//!   closed. On a time-out, or when the run is over, the job is closed. This is only for the
//!   tool's own CLI children: Unity is never started through this runner and never ended by the tool.
//! - Busy guard: one run at a time for a given kind of work ([`Busy`]).
//!
//! Result codes, for logs and the screen: `ok`, `timeout`, `spawn:<text>`, `exit:<code>:<first line>`.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prefixes removed from a Claude child: a session variable (`CLAUDECODE`, `CLAUDE_CODE_*`,
/// `CLAUDE_AGENT_SDK_*`, `CLAUDE_EFFORT` ...) would tie it to the session that started the tool,
/// and an API key would quietly switch it to API billing. The Claude runner sets
/// `CLAUDE_CONFIG_DIR` again when the parent has it (where a person keeps their login).
pub const CLAUDE_ENV_PREFIXES: &[&str] = &["CLAUDE", "ANTHROPIC_"];
/// Removed from a Codex child unless the settings keep it (`codex.keepApiKeyEnv`).
pub const CODEX_ENV_REMOVE: &[&str] = &["OPENAI_API_KEY"];

#[derive(Debug, Clone, Default)]
pub struct Cmd {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// Written to stdin, then stdin is closed. None: stdin is empty (closed at once).
    pub stdin: Option<Vec<u8>>,
    /// None: no limit.
    pub timeout: Option<Duration>,
    pub env_remove: Vec<String>,
    pub env_remove_prefixes: Vec<String>,
    pub env_set: Vec<(String, String)>,
}

impl Cmd {
    pub fn new(program: impl Into<PathBuf>) -> Cmd {
        Cmd { program: program.into(), ..Cmd::default() }
    }
    pub fn arg(mut self, arg: impl Into<OsString>) -> Cmd {
        self.args.push(arg.into());
        self
    }
    pub fn args<I, S>(mut self, args: I) -> Cmd
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Cmd {
        self.cwd = Some(dir.into());
        self
    }
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Cmd {
        self.stdin = Some(bytes.into());
        self
    }
    pub fn timeout(mut self, limit: Duration) -> Cmd {
        self.timeout = Some(limit);
        self
    }
    pub fn remove_env(mut self, name: impl Into<String>) -> Cmd {
        self.env_remove.push(name.into());
        self
    }
    pub fn remove_env_prefixes(mut self, prefixes: &[&str]) -> Cmd {
        self.env_remove_prefixes.extend(prefixes.iter().map(|p| p.to_string()));
        self
    }
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Cmd {
        self.env_set.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Exited(i32),
    /// Ran past the limit; its process tree was ended.
    TimedOut,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub outcome: Outcome,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub elapsed_ms: u64,
    pub pid: u32,
}

impl Output {
    pub fn success(&self) -> bool {
        self.outcome == Outcome::Exited(0)
    }
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
    /// `ok`, `timeout` or `exit:<code>:<first line of stderr, else stdout>`.
    pub fn code(&self) -> String {
        match self.outcome {
            Outcome::Exited(0) => "ok".into(),
            Outcome::TimedOut => "timeout".into(),
            Outcome::Exited(n) => {
                let line = first_line(&self.stderr).or_else(|| first_line(&self.stdout)).unwrap_or_default();
                format!("exit:{n}:{line}")
            }
        }
    }
}

fn first_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(200).collect())
}

/// Names of the current environment that `cmd` removes.
fn removed_names(cmd: &Cmd) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    for (name, _) in std::env::vars_os() {
        let upper = name.to_string_lossy().to_uppercase();
        let exact = cmd.env_remove.iter().any(|n| n.to_uppercase() == upper);
        let prefixed = cmd.env_remove_prefixes.iter().any(|p| upper.starts_with(&p.to_uppercase()));
        if exact || prefixed {
            out.push(name);
        }
    }
    for name in &cmd.env_remove {
        out.push(name.into());
    }
    out
}

fn reader<R: Read + Send + 'static>(source: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = source {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    })
}

/// Starts `cmd` and waits for it (or for its time limit). `Err` only when it could not start:
/// `spawn:<text>`. A time-out is an `Ok` with [`Outcome::TimedOut`] and what was written so far.
pub fn run(cmd: &Cmd) -> Result<Output, String> {
    let mut command = std::process::Command::new(&cmd.program);
    command.args(&cmd.args);
    if let Some(dir) = &cmd.cwd {
        command.current_dir(dir);
    }
    for name in removed_names(cmd) {
        command.env_remove(name);
    }
    for (name, value) in &cmd.env_set {
        command.env(name, value);
    }
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| format!("spawn:{e}"))?;
    let pid = child.id();
    let job = job::Job::for_child(&child);

    let stdin_bytes = cmd.stdin.clone();
    let mut stdin_pipe = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let (Some(pipe), Some(bytes)) = (stdin_pipe.as_mut(), stdin_bytes) {
            let _ = pipe.write_all(&bytes);
            let _ = pipe.flush();
        }
        drop(stdin_pipe);
    });
    let out = reader(child.stdout.take());
    let err = reader(child.stderr.take());

    let outcome = wait(&mut child, cmd.timeout, &job);
    // The run is over (or ran out of time): the job ends what is left of the tree
    drop(job);
    let _ = writer.join();
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    Ok(Output { outcome, stdout, stderr, elapsed_ms: started.elapsed().as_millis() as u64, pid })
}

fn wait(child: &mut Child, limit: Option<Duration>, job: &Option<job::Job>) -> Outcome {
    let deadline = limit.map(|l| Instant::now() + l);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Outcome::Exited(status.code().unwrap_or(-1)),
            Ok(None) => {}
            Err(_) => return Outcome::Exited(-1),
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            match job {
                Some(j) => j.end_tree(),
                // Not in a job (assignment failed): end the child itself. Only CLI children
                // come here; Unity is never run through this function.
                None => {
                    let _ = child.kill();
                }
            }
            let _ = child.wait();
            return Outcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// A Windows program by its full path under `%SystemRoot%\System32` (`cmd.exe`,
/// `WindowsPowerShell/v1.0/powershell.exe`), so a file of the same name in the working folder or
/// on PATH is never run instead.
pub fn system_exe(relative: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    relative.split(['\\', '/']).fold(root.join("System32"), |p, part| p.join(part))
}

/// True while a process with that id runs.
pub fn pid_alive(pid: u32) -> bool {
    job::pid_alive(pid)
}

/// One run at a time for a kind of work: `Busy::try_take(&FLAG)` gives a guard, or None (`busy`).
pub struct Busy(&'static AtomicBool);

impl Busy {
    pub fn try_take(flag: &'static AtomicBool) -> Option<Busy> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Busy(flag))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    /// A job that ends every process in it when closed (KILL_ON_JOB_CLOSE).
    pub struct Job(HANDLE);

    // The handle is only used to end or close the job
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn for_child(child: &Child) -> Option<Job> {
            unsafe {
                let handle = CreateJobObjectW(None, None).ok()?;
                let job = Job(handle);
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .ok()?;
                AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())).ok()?;
                Some(job)
            }
        }

        /// Ends every process in the job now (exit code 1).
        pub fn end_tree(&self) {
            unsafe {
                let _ = TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn pid_alive(pid: u32) -> bool {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code).is_ok();
            let _ = CloseHandle(handle);
            ok && code == STILL_ACTIVE.0 as u32
        }
    }
}

#[cfg(not(windows))]
mod job {
    use std::process::Child;
    pub struct Job;
    impl Job {
        pub fn for_child(_child: &Child) -> Option<Job> {
            None
        }
        pub fn end_tree(&self) {}
    }
    pub fn pid_alive(pid: u32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static FLAG: AtomicBool = AtomicBool::new(false);

    #[test]
    fn busy_guard_lets_one_run_at_a_time() {
        let first = Busy::try_take(&FLAG);
        assert!(first.is_some());
        assert!(Busy::try_take(&FLAG).is_none());
        drop(first);
        assert!(Busy::try_take(&FLAG).is_some());
    }

    #[test]
    fn codes() {
        let out = |outcome, stderr: &str, stdout: &str| Output {
            outcome,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            elapsed_ms: 0,
            pid: 0,
        };
        assert_eq!(out(Outcome::Exited(0), "", "").code(), "ok");
        assert_eq!(out(Outcome::TimedOut, "", "").code(), "timeout");
        assert_eq!(out(Outcome::Exited(2), "\n  bad thing\nmore", "x").code(), "exit:2:bad thing");
        assert_eq!(out(Outcome::Exited(3), "", "only out").code(), "exit:3:only out");
    }

    #[test]
    fn system_programs_have_full_paths() {
        let cmd = system_exe("cmd.exe");
        assert!(cmd.is_absolute() && cmd.ends_with("System32/cmd.exe"), "{}", cmd.display());
        let ps = system_exe("WindowsPowerShell/v1.0/powershell.exe");
        assert!(ps.ends_with("System32/WindowsPowerShell/v1.0/powershell.exe"));
    }

    #[test]
    fn spawn_failure_is_a_code() {
        let err = run(&Cmd::new("C:/fake/no-such-program.exe")).unwrap_err();
        assert!(err.starts_with("spawn:"), "{err}");
    }

    #[test]
    fn removed_names_match_prefixes_without_case() {
        std::env::set_var("Anthropic_Test_Var_Uis", "1");
        let cmd = Cmd::new("x").remove_env_prefixes(CLAUDE_ENV_PREFIXES);
        let names: Vec<String> = removed_names(&cmd).iter().map(|n| n.to_string_lossy().into_owned()).collect();
        assert!(names.iter().any(|n| n == "Anthropic_Test_Var_Uis"));
        std::env::remove_var("Anthropic_Test_Var_Uis");
    }
}
