//! Stand-in for the programs Icon Studio runs, for tests. Copied to a temporary folder under the
//! name of the program it plays; the file name picks the role. Run under its own name
//! (`uis-stub`), the first argument is the role.
//!
//! Roles:
//! - `echo`: copies stdin to stdout byte for byte.
//! - `env [PREFIX...]`: prints `NAME=value` for each variable whose name starts with a prefix
//!   (case-insensitive), sorted.
//! - `sleep <secs> [--child]`: prints its process id (and with `--child` first starts a copy of
//!   itself that sleeps as long and prints that one's id too), then sleeps.
//! - `exit <code> [message]`: writes the message to stderr and ends with that code.
//! - `args <log file> <code> [arg...]`: appends the arguments as one tab-separated line to the log
//!   file, prints `args ok`, ends with that code (a capture hook).
//! - `unity`: the Unity editor when started with `-batchmode`, else the `unity` CLI
//!   (`--version`, `status --json`, `command "script run" ...`). See [`unity`].
//! - `codex`, `claude`: made-up LLM command lines (text answers, pictures, failures). See [`llm`].
//!
//! The Unity roles read `stub-scenario.txt` next to the program (`key=value` lines), not the
//! environment: the batch editor is started through WMI and gets the user's environment, not the
//! test's.

use std::io::{Read, Write};
use std::time::Duration;

mod llm;
mod unity;

fn own_name() -> bool {
    let stem = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    stem.starts_with("uis-stub") || stem.starts_with("uis_stub")
}

fn role_and_args() -> (String, Vec<String>) {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if own_name() {
        let role = if args.is_empty() { String::new() } else { args.remove(0) };
        (role, args)
    } else {
        let stem = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        (stem, args)
    }
}

fn main() {
    let (role, args) = role_and_args();
    let code = match role.as_str() {
        "echo" => echo(),
        "env" => env(&args),
        "sleep" => sleep(&args),
        "exit" => exit(&args),
        "args" => args_log(&args),
        "unity" => unity::main(&args),
        "codex" => llm::codex(&args),
        "claude" => llm::claude(&args),
        other => {
            eprintln!("uis-stub: unknown role {other:?}");
            64
        }
    };
    std::process::exit(code);
}

fn echo() -> i32 {
    let mut buf = Vec::new();
    if std::io::stdin().read_to_end(&mut buf).is_err() {
        return 1;
    }
    let mut out = std::io::stdout();
    let _ = out.write_all(&buf);
    let _ = out.flush();
    0
}

fn env(prefixes: &[String]) -> i32 {
    let mut lines: Vec<String> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let name = k.to_string_lossy().into_owned();
            let upper = name.to_uppercase();
            prefixes
                .iter()
                .any(|p| upper.starts_with(&p.to_uppercase()))
                .then(|| format!("{name}={}", v.to_string_lossy()))
        })
        .collect();
    lines.sort();
    for line in lines {
        println!("{line}");
    }
    0
}

fn sleep(args: &[String]) -> i32 {
    let secs: f64 = args.first().and_then(|a| a.parse().ok()).unwrap_or(30.0);
    let mut out = std::io::stdout();
    if args.iter().any(|a| a == "--child") {
        let me = std::env::current_exe().expect("current exe");
        let mut cmd = std::process::Command::new(me);
        if own_name() {
            cmd.arg("sleep");
        }
        cmd.arg(secs.to_string());
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        match cmd.spawn() {
            Ok(child) => {
                let _ = writeln!(out, "child {}", child.id());
            }
            Err(e) => {
                eprintln!("spawn child: {e}");
                return 1;
            }
        }
    }
    let _ = writeln!(out, "pid {}", std::process::id());
    let _ = out.flush();
    std::thread::sleep(Duration::from_secs_f64(secs.max(0.0)));
    0
}

fn exit(args: &[String]) -> i32 {
    let code: i32 = args.first().and_then(|a| a.parse().ok()).unwrap_or(1);
    if let Some(message) = args.get(1) {
        eprintln!("{message}");
    }
    code
}

fn args_log(args: &[String]) -> i32 {
    let Some(log) = args.first() else { return 64 };
    let code: i32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(0);
    let line = args.iter().skip(2).cloned().collect::<Vec<_>>().join("\t");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(f, "{line}");
    }
    println!("args ok");
    code
}
