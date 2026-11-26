//! The process runner (uis-core proc.rs) against the stub program.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use uis_core::proc::{pid_alive, run, Cmd, Outcome, CLAUDE_ENV_PREFIXES};

/// Tests that change this process's environment run one at a time.
static ENV: Mutex<()> = Mutex::new(());

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_uis-stub"))
}

/// The stub copied as `<name>.exe` into a fresh folder (the role comes from the name).
fn stub_as(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uis-stub-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(stub(), &path).unwrap();
    path
}

#[test]
fn stdin_korean_text_round_trips_as_utf8_bytes() {
    let text = "지붕을 파란색으로, 겨울 느낌 \u{2014} snow \u{2603}\r\n둘째 줄";
    let out = run(&Cmd::new(stub()).arg("echo").stdin(text.as_bytes().to_vec()).timeout(Duration::from_secs(20))).unwrap();
    assert!(out.success(), "{}", out.code());
    assert_eq!(out.stdout, text.as_bytes());
}

#[test]
fn role_comes_from_the_file_name() {
    let echo = stub_as("echo");
    let out = run(&Cmd::new(&echo).stdin(b"abc".to_vec())).unwrap();
    assert_eq!(out.stdout_text(), "abc");
}

#[test]
fn claude_and_anthropic_variables_are_removed() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let vars = [
        ("CLAUDECODE", "1"),
        ("CLAUDE_CODE_ENTRYPOINT", "cli"),
        ("ANTHROPIC_API_KEY", "secret"),
        ("anthropic_base_url", "x"),
        ("UIS_KEEP_ME", "yes"),
    ];
    let saved: Vec<(&str, Option<std::ffi::OsString>)> = vars.iter().map(|(k, _)| (*k, std::env::var_os(k))).collect();
    for (k, v) in vars {
        std::env::set_var(k, v);
    }
    let out = run(&Cmd::new(stub()).args(["env", "CLAUDE", "ANTHROPIC", "UIS_KEEP"]).remove_env_prefixes(CLAUDE_ENV_PREFIXES)).unwrap();
    for (k, old) in saved {
        match old {
            Some(v) => std::env::set_var(k, v),
            None => std::env::remove_var(k),
        }
    }
    let text = out.stdout_text();
    for line in text.lines() {
        let upper = line.to_uppercase();
        for prefix in CLAUDE_ENV_PREFIXES {
            assert!(!upper.starts_with(prefix), "{line}");
        }
    }
    assert!(text.contains("UIS_KEEP_ME=yes"), "{text}");
}

#[test]
fn exact_names_are_removed_too() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("OPENAI_API_KEY_UIS_TEST", "k");
    let out = run(&Cmd::new(stub()).args(["env", "OPENAI_API_KEY_UIS"]).remove_env("openai_api_key_uis_test")).unwrap();
    std::env::remove_var("OPENAI_API_KEY_UIS_TEST");
    assert_eq!(out.stdout_text(), "");
}

#[test]
fn set_variables_reach_the_child() {
    let out = run(&Cmd::new(stub()).args(["env", "UIS_SET_"]).env("UIS_SET_ONE", "값")).unwrap();
    assert_eq!(out.stdout_text().trim(), "UIS_SET_ONE=값");
}

#[test]
fn time_out_ends_the_stub_and_its_child() {
    let out = run(&Cmd::new(stub()).args(["sleep", "30", "--child"]).timeout(Duration::from_millis(1500))).unwrap();
    assert_eq!(out.outcome, Outcome::TimedOut);
    assert_eq!(out.code(), "timeout");
    assert!(out.elapsed_ms < 10_000, "{}", out.elapsed_ms);
    let text = out.stdout_text();
    let child: u32 = text
        .lines()
        .find_map(|l| l.strip_prefix("child "))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("no child id in {text:?}"));
    // The job ends the tree; give Windows a moment to finish it
    let mut alive = true;
    for _ in 0..50 {
        alive = pid_alive(out.pid) || pid_alive(child);
        if !alive {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!alive, "stub {} or child {child} still runs", out.pid);
}

#[test]
fn exit_code_and_first_line() {
    let out = run(&Cmd::new(stub()).args(["exit", "4", "leased by someone"])).unwrap();
    assert_eq!(out.outcome, Outcome::Exited(4));
    assert_eq!(out.code(), "exit:4:leased by someone");
}

#[test]
fn a_quick_run_is_not_cut() {
    let out = run(&Cmd::new(stub()).args(["sleep", "0.2"]).timeout(Duration::from_secs(20))).unwrap();
    assert!(out.success(), "{}", out.code());
    assert!(out.stdout_text().starts_with("pid "));
}
