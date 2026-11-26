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

use std::io::{Read, Write};
use std::time::Duration;

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
