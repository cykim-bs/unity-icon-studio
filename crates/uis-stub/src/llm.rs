//! The `codex` and `claude` roles: made-up LLM command lines that spend no limits.
//!
//! `stub-scenario.txt` next to the program, `key=value` per line:
//! - `codex`: `ok` (default), `no_image` (exit 0, nothing saved), `timeout` (sleeps 30 s), `limit`,
//!   `auth`, `model`, `no_tool` (no image runner), `refused`, `bad_json` (text answer is not JSON)
//! - `codex_harvest`: how an `ok` picture is reported: `1` a `saved_path` event, `2` the path in
//!   the last message (default), `3` only the file in `generated_images\<thread>\`
//! - `codex_login`: `no` makes `login status` fail
//! - `claude`: `ok` (default), `auth`, `model`, `bad_json`, `timeout`
//! - `claude_login`: `no` makes `auth status` say `loggedIn: false`
//!
//! Text answers: `reply-<n>.json` next to the program for the n-th call of that role (1-based),
//! else `reply.json`, else a small edit list (a review: `{pass: true, issues: []}`). Every call appends one line to `calls.log` next to
//! the program (`role<TAB>kind<TAB>stdin bytes<TAB>names of CLAUDE*, ANTHROPIC*, OPENAI* variables`)
//! and writes its stdin to `<role>-stdin-<n>.txt` (Codex also its arguments to `codex-args-<n>.txt`).

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn here() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default()
}

fn scenario() -> HashMap<String, String> {
    let text = std::fs::read_to_string(here().join("stub-scenario.txt")).unwrap_or_default();
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

fn value_after(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn values_after(args: &[String], name: &str) -> Vec<String> {
    args.iter().enumerate().filter(|(_, a)| *a == name).filter_map(|(i, _)| args.get(i + 1).cloned()).collect()
}

fn read_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    buf
}

/// Logs the call; returns its number for this role (1-based).
fn log_call(role: &str, kind: &str, stdin: &[u8]) -> usize {
    let log = here().join("calls.log");
    let n = std::fs::read_to_string(&log).map(|t| t.lines().filter(|l| l.starts_with(&format!("{role}\t"))).count()).unwrap_or(0) + 1;
    let mut names: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .filter(|k| {
            let u = k.to_uppercase();
            u.starts_with("CLAUDE") || u.starts_with("ANTHROPIC") || u.starts_with("OPENAI")
        })
        .collect();
    names.sort();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
        let _ = writeln!(f, "{role}\t{kind}\t{}\t{}", stdin.len(), names.join(","));
    }
    let _ = std::fs::write(here().join(format!("{role}-stdin-{n}.txt")), stdin);
    n
}

/// The "repainted" picture of `codex_image=copy`: input image 1 (the capture on white) with its
/// white keyed out; with `sparkle`, a yellow four-point star drawn apart from it at the top right.
fn copy_picture(input: &str, sparkle: bool) -> image::RgbaImage {
    let Ok(src) = image::open(input) else { return image::RgbaImage::new(64, 64) };
    let mut img = src.to_rgba8();
    // Small made-up captures are scaled up: a harvested picture must be at least 256 px
    let k = 400u32.div_ceil(img.width().max(1)).max(1);
    if k > 1 {
        img = image::imageops::resize(&img, img.width() * k, img.height() * k, image::imageops::FilterType::Triangle);
    }
    for p in img.pixels_mut() {
        if p.0[0] > 248 && p.0[1] > 248 && p.0[2] > 248 {
            p.0 = [0, 0, 0, 0];
        }
    }
    if sparkle {
        let (w, h) = (img.width() as f32, img.height() as f32);
        let (cx, cy, r) = (w * 0.86, h * 0.14, w * 0.13);
        for y in 0..img.height() {
            for x in 0..img.width() {
                let (dx, dy) = ((x as f32 - cx).abs(), (y as f32 - cy).abs());
                if dx * dy < r * r * 0.08 && dx + dy < r {
                    img.put_pixel(x, y, image::Rgba([255, 214, 30, 255]));
                }
            }
        }
    }
    img
}

/// A fidelity review answer. Scenario `fidelity=clean` (default), `added` (always a sparkle) or
/// `added_once` (a sparkle on the first fidelity call of this CLI, clean after: a retry fixes it).
fn fidelity_answer(sc: &std::collections::HashMap<String, String>, role: &str) -> Value {
    let log = here().join(format!("{role}-fidelity.count"));
    let n = std::fs::read_to_string(&log).ok().and_then(|t| t.trim().parse::<usize>().ok()).unwrap_or(0) + 1;
    let _ = std::fs::write(&log, n.to_string());
    let added = match sc.get("fidelity").map(String::as_str).unwrap_or("clean") {
        "added" => true,
        "added_once" => n == 1,
        _ => false,
    };
    let list = if added { json!([{ "what": "Yellow four-point sparkle", "where": "above the roof", "severity": "major" }]) } else { json!([]) };
    json!({ "pass": !added, "added": list, "resized": [], "missing": [], "changedPattern": [], "summary": if added { "A sparkle was added." } else { "Same parts." } })
}

fn reply(n: usize, default: Value) -> Value {
    for name in [format!("reply-{n}.json"), "reply.json".to_string()] {
        if let Ok(t) = std::fs::read_to_string(here().join(name)) {
            if let Ok(v) = serde_json::from_str(&t) {
                return v;
            }
        }
    }
    default
}

fn default_edits() -> Value {
    json!({
        "replace": [{ "id": "STYLE.color", "text": "Color: cool winter palette with a sky-blue roof and snow-white accents; no haze, no fog, no bloom." }],
        "add": [{ "block": "SUBJECT", "text": "A thin layer of snow rests on the roof." }],
        "avoidAdd": [],
        "outline": "keep",
        "camera": "keep",
        "rejected": [],
        "summaryKo": "지붕을 하늘색으로 바꾸고 눈을 얹었습니다."
    })
}

fn line(v: Value) {
    println!("{v}");
}

pub fn codex(args: &[String]) -> i32 {
    let sc = scenario();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("codex-cli 0.0.0-stub");
            return 0;
        }
        Some("login") => {
            if sc.get("codex_login").map(String::as_str) == Some("no") {
                eprintln!("Not logged in");
                return 1;
            }
            println!("Logged in using ChatGPT");
            return 0;
        }
        Some("exec") => {}
        _ => {
            eprintln!("codex stub: unknown command");
            return 2;
        }
    }
    let images = values_after(args, "-i");
    let image = !images.is_empty() && value_after(args, "--output-schema").is_none();
    let stdin = read_stdin();
    let n = log_call("codex", if image { "image" } else { "text" }, &stdin);
    let _ = std::fs::write(here().join(format!("codex-args-{n}.txt")), args.join("
"));
    let thread = format!("00000000-0000-7000-8000-{n:012}");
    let out_file = value_after(args, "-o").map(PathBuf::from);
    let write_last = |text: &str| {
        if let Some(f) = &out_file {
            let _ = std::fs::write(f, text);
        }
    };
    let msg = |text: &str| line(json!({ "type": "item.completed", "item": { "id": "item_1", "type": "agent_message", "text": text } }));
    line(json!({ "type": "thread.started", "thread_id": thread }));
    line(json!({ "type": "item.completed", "item": { "id": "item_0", "type": "error", "message": "Codex is ignoring 1 unrecognized configuration setting." } }));
    line(json!({ "type": "turn.started" }));
    let failed = |m: &str| {
        line(json!({ "type": "error", "message": m }));
        line(json!({ "type": "turn.failed", "error": { "message": m } }));
        1
    };
    match sc.get("codex").map(String::as_str).unwrap_or("ok") {
        "timeout" => {
            std::thread::sleep(std::time::Duration::from_secs(30));
            0
        }
        "limit" => failed("You've hit your usage limit. Try again later."),
        "auth" => failed("unexpected status 401 Unauthorized: Missing bearer or basic authentication in header"),
        "model" => failed("The 'x' model is not supported when using Codex with a ChatGPT account."),
        "no_tool" if image => {
            msg("tools.image_gen is not a function");
            write_last("tools.image_gen is not a function");
            line(json!({ "type": "turn.completed" }));
            0
        }
        "refused" if image => {
            msg("I can't help with that request.");
            write_last("I can't help with that request.");
            line(json!({ "type": "turn.completed" }));
            0
        }
        "no_image" if image => {
            msg("Done.");
            write_last("Done.");
            line(json!({ "type": "turn.completed" }));
            0
        }
        "bad_json" if !image => {
            msg("Here is your prompt, rewritten.");
            write_last("Here is your prompt, rewritten.");
            line(json!({ "type": "turn.completed" }));
            0
        }
        _ if image => {
            let chome = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| here().join("codex-home"));
            let dir = chome.join("generated_images").join(&thread);
            let _ = std::fs::create_dir_all(&dir);
            let pic = dir.join("exec-stub.png");
            let img = match sc.get("codex_image").map(String::as_str) {
                Some(mode @ ("copy" | "copy_sparkle" | "copy_sparkle_once")) => {
                    let images_before = std::fs::read_to_string(here().join("codex-images.count")).ok().and_then(|t| t.trim().parse::<usize>().ok()).unwrap_or(0);
                    let _ = std::fs::write(here().join("codex-images.count"), (images_before + 1).to_string());
                    let sparkle = mode == "copy_sparkle" || (mode == "copy_sparkle_once" && images_before == 0);
                    copy_picture(images.first().map(String::as_str).unwrap_or(""), sparkle)
                }
                _ => image::RgbaImage::from_fn(320, 320, |x, y| {
                    let inside = (40..280).contains(&x) && (40..280).contains(&y);
                    image::Rgba([(x % 256) as u8, (y % 256) as u8, 120, if inside { 255 } else { 0 }])
                }),
            };
            let _ = img.save(&pic);
            let path = pic.display().to_string();
            match sc.get("codex_harvest").map(String::as_str).unwrap_or("2") {
                "1" => {
                    line(json!({ "type": "item.completed", "item": { "id": "item_2", "type": "image_generation_end", "saved_path": path } }));
                    msg("Saved.");
                    write_last("Saved.");
                }
                "3" => {
                    msg("Saved.");
                    write_last("Saved.");
                }
                _ => {
                    msg(&path);
                    write_last(&path);
                }
            }
            line(json!({ "type": "turn.completed", "usage": { "input_tokens": 1, "output_tokens": 1 } }));
            0
        }
        _ => {
            let schema = value_after(args, "--output-schema").and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
            let default = if schema.contains("changedPattern") {
                fidelity_answer(&sc, "codex")
            } else if schema.contains("\"pass\"") {
                json!({ "pass": true, "issues": [] })
            } else {
                default_edits()
            };
            // Fidelity reviews answer from the scenario (reply files are for rewrites)
            let answer = if schema.contains("changedPattern") { default } else { reply(n, default) }.to_string();
            msg(&answer);
            write_last(&answer);
            line(json!({ "type": "turn.completed" }));
            0
        }
    }
}

pub fn claude(args: &[String]) -> i32 {
    let sc = scenario();
    if args.first().map(String::as_str) == Some("--version") {
        println!("2.0.0 (Claude Code stub)");
        return 0;
    }
    if args.first().map(String::as_str) == Some("auth") {
        let ok = sc.get("claude_login").map(String::as_str) != Some("no");
        println!("{}", json!({ "loggedIn": ok, "authMethod": "stub" }));
        return 0;
    }
    if !args.iter().any(|a| a == "-p") {
        eprintln!("claude stub: unknown command");
        return 2;
    }
    let review = value_after(args, "--tools").as_deref() == Some("Read");
    let stdin = read_stdin();
    let n = log_call("claude", if review { "review" } else { "text" }, &stdin);
    let result = |is_error: bool, reason: &str, text: &str, structured: Option<Value>| {
        let mut v = json!({ "type": "result", "subtype": "success", "is_error": is_error, "terminal_reason": reason,
                            "result": text, "total_cost_usd": 0.001, "session_id": "00000000-0000-4000-8000-000000000000" });
        if let Some(s) = structured {
            v["structured_output"] = s;
        }
        println!("{v}");
    };
    match sc.get("claude").map(String::as_str).unwrap_or("ok") {
        "timeout" => {
            std::thread::sleep(std::time::Duration::from_secs(30));
            0
        }
        "auth" => {
            result(true, "api_error", "Failed to authenticate: OAuth session expired and could not be refreshed", None);
            1
        }
        "model" => {
            println!("There's an issue with the selected model (x). It may not exist or you may not have access to it.");
            1
        }
        "bad_json" => {
            result(false, "completed", "Sure! Here is the prompt.", None);
            0
        }
        _ => {
            let schema = value_after(args, "--json-schema").unwrap_or_default();
            let default = if schema.contains("changedPattern") {
                fidelity_answer(&sc, "claude")
            } else if review {
                json!({ "pass": true, "issues": [] })
            } else {
                default_edits()
            };
            let answer = if schema.contains("changedPattern") { default } else { reply(n, default) };
            result(false, "completed", &answer.to_string(), Some(answer));
            0
        }
    }
}
