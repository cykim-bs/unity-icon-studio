//! The `unity` role: a made-up Unity editor (batch mode) and `unity` CLI.
//!
//! `stub-scenario.txt` next to the program, `key=value` per line:
//! - `scenario`: `ok` (default), `hang` (sleeps `hang_ms` before working), `crash` (exit 4, no
//!   result), `compile_error` (CLI: the script does not compile)
//! - `delay_ms`: batch start-up time (default 1500)
//! - `hang_ms`: how long `hang` sleeps (default 8000)
//! - `touch_crlf`: a tracked text file the batch editor writes again with CRLF line ends and the
//!   same content (like an editor plugin regenerating a resource file)
//! - `touch_edit`: a tracked file the batch editor really changes (a line added at the end)
//! - `touch`: a tracked file (from the project) the batch editor rewrites, like Unity adding an
//!   importer block to a `.meta`
//! - `instances`: what `status --json` lists: `<project path>|<state>` separated by `;`
//!
//! The capture itself is faked: an item whose asset exists in the project gives a 64 px PNG (an
//! opaque square in a transparent field), one that does not gives `prefab_not_found`.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ENTRY: &str = "Assets/__UnityIconStudioCapture/Editor";

fn scenario() -> HashMap<String, String> {
    let file = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("stub-scenario.txt")));
    let text = file.and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

fn value_after(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

pub fn main(args: &[String]) -> i32 {
    let sc = scenario();
    if args.iter().any(|a| a == "-batchmode") {
        return editor(args, &sc);
    }
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("1.0.0-stub");
            0
        }
        Some("status") => status(&sc),
        Some("command") => command(args, &sc),
        _ => {
            eprintln!("unity stub: unknown command");
            2
        }
    }
}

fn status(sc: &HashMap<String, String>) -> i32 {
    let instances: Vec<Value> = sc
        .get("instances")
        .map(|s| {
            s.split(';')
                .filter(|x| !x.trim().is_empty())
                .map(|x| {
                    let (path, state) = x.split_once('|').unwrap_or((x, "ready"));
                    json!({ "port": 7800, "projectPath": path, "version": "6000.0.1f1", "pid": 1, "state": state, "role": "main" })
                })
                .collect()
        })
        .unwrap_or_default();
    let ok = !instances.is_empty();
    println!("{}", json!({ "success": ok, "command": "status", "data": { "count": instances.len(), "instances": instances }, "errors": [], "warnings": [] }));
    eprintln!("A new version of the unity CLI is available (stub notice on stderr)");
    if ok {
        0
    } else {
        1
    }
}

fn command(args: &[String], sc: &HashMap<String, String>) -> i32 {
    if args.get(1).map(String::as_str) != Some("script run") {
        eprintln!("unity stub: only \"script run\"");
        return 2;
    }
    let project = value_after(args, "--project-path").map(PathBuf::from);
    let file = value_after(args, "--file").map(PathBuf::from);
    let entry = value_after(args, "--entry").unwrap_or_default();
    let job: Option<String> = value_after(args, "--args")
        .and_then(|a| serde_json::from_str::<Vec<String>>(&a).ok())
        .and_then(|v| v.into_iter().next());
    let (Some(project), Some(file), Some(job)) = (project, file, job) else {
        println!("{}", json!({ "success": false, "diagnostics": [], "error": "INVALID_COMMAND_ARGS" }));
        return 1;
    };
    let source = std::fs::read_to_string(&file).unwrap_or_default();
    if sc.get("scenario").map(String::as_str) == Some("compile_error") || !source.contains("RunJobFile") || entry != "UnityIconStudio.PrefabIconCapture.RunJobFile" {
        println!(
            "{}",
            json!({ "diagnostics": [{ "line": 1, "column": 1, "message": "CS0103: stub compile error" }], "compileMs": 5, "executeMs": 0, "success": false, "result": null })
        );
        return 0;
    }
    let t = Instant::now();
    let (result, _failed) = fake_capture(&project, Path::new(&job), false);
    println!(
        "{}",
        json!({ "diagnostics": [], "compileMs": 7, "executeMs": t.elapsed().as_millis() as u64, "assemblyName": "PipelineRunScript_stub", "success": true, "result": result.to_string() })
    );
    0
}

fn log_line(log: &mut Option<std::fs::File>, line: &str) {
    if let Some(f) = log.as_mut() {
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
    }
}

fn editor(args: &[String], sc: &HashMap<String, String>) -> i32 {
    let project = value_after(args, "-projectPath").map(PathBuf::from).unwrap_or_default();
    let mut log = value_after(args, "-logFile").and_then(|p| std::fs::File::create(p).ok());
    log_line(&mut log, "Unity stub (batch mode)");
    let entry = project.join(ENTRY);
    if !entry.join("PrefabIconCapture.cs").is_file() || !entry.join("UnityIconStudio.Capture.Editor.asmdef").is_file() {
        log_line(&mut log, "error: executeMethod class not found (entry script missing)");
        return 1;
    }
    // Like Unity: .meta files for the new folder and files
    for rel in ["Assets/__UnityIconStudioCapture.meta", "Assets/__UnityIconStudioCapture/Editor.meta", &format!("{ENTRY}/PrefabIconCapture.cs.meta")] {
        let _ = std::fs::write(project.join(rel), "fileFormatVersion: 2\nguid: 00000000000000000000000000000000\n");
    }
    let num = |k: &str, d: u64| sc.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
    std::thread::sleep(Duration::from_millis(num("delay_ms", 1500)));
    if let Some(rel) = sc.get("touch") {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(project.join(rel)) {
            let _ = f.write_all(b"  stubImporter:\n    serializedVersion: 2\n");
        }
    }
    if let Some(rel) = sc.get("touch_crlf") {
        if let Ok(text) = std::fs::read_to_string(project.join(rel)) {
            let crlf = text.replace("\r\n", "\n").replace('\n', "\r\n");
            let _ = std::fs::write(project.join(rel), crlf);
        }
    }
    if let Some(rel) = sc.get("touch_edit") {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(project.join(rel)) {
            let _ = f.write_all(b"stub edit\n");
        }
    }
    let kind = sc.get("scenario").map(String::as_str).unwrap_or("ok");
    if kind == "hang" {
        std::thread::sleep(Duration::from_millis(num("hang_ms", 8000)));
    }
    log_line(&mut log, "[IconStudio] entry graphics=Direct3D12");
    if kind == "crash" {
        log_line(&mut log, "[IconStudio] exception: stub crash");
        log_line(&mut log, "[IconStudio] exit 4");
        return 4;
    }
    let Some(job) = value_after(args, "-iconStudioJob") else {
        log_line(&mut log, "[IconStudio] missing -iconStudioJob <path>");
        return 3;
    };
    log_line(&mut log, "[IconStudio] flushed delayCall: 0 call(s) in 0 round(s)");
    let (result, failed) = fake_capture(&project, Path::new(&job), true);
    let items = result["items"].as_array().cloned().unwrap_or_default();
    log_line(&mut log, &format!("[IconStudio] items {}", items.len()));
    for it in &items {
        let id = it["id"].as_str().unwrap_or("");
        if it["ok"].as_bool() == Some(true) {
            log_line(&mut log, &format!("[IconStudio] ok {id} 1ms {}", it["output"].as_str().unwrap_or("")));
        } else {
            log_line(&mut log, &format!("[IconStudio] FAIL {id} 1ms {}", it["error"].as_str().unwrap_or("")));
        }
    }
    let code = if failed == 0 { 0 } else { 2 };
    log_line(&mut log, &format!("[IconStudio] exit {code}"));
    code
}

/// Writes the PNGs and result.json the job asks for. Returns the result and the failed count.
fn fake_capture(project: &Path, job_file: &Path, batch: bool) -> (Value, usize) {
    let job: Value = std::fs::read_to_string(job_file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let base = job_file.parent().map(Path::to_path_buf).unwrap_or_default();
    let resolve = |p: &str| if Path::new(p).is_absolute() { PathBuf::from(p) } else { base.join(p) };
    let out_dir = resolve(job["outputDir"].as_str().unwrap_or("."));
    let _ = std::fs::create_dir_all(&out_dir);
    let result_path = resolve(job["resultPath"].as_str().unwrap_or("result.json"));
    let mut items = Vec::new();
    let mut failed = 0;
    for it in job["items"].as_array().cloned().unwrap_or_default() {
        let id = it["id"].as_str().unwrap_or("item").to_string();
        let prefab = it["prefab"].as_str().unwrap_or("").to_string();
        if !project.join(&prefab).is_file() {
            failed += 1;
            items.push(json!({ "id": id, "prefab": prefab, "ok": false, "error": "prefab_not_found", "coverage": 0 }));
            continue;
        }
        let out = out_dir.join(it["output"].as_str().map(str::to_string).unwrap_or(format!("{id}.png")));
        let mut img = image::RgbaImage::new(64, 64);
        for y in 16..48 {
            for x in 16..48 {
                img.put_pixel(x, y, image::Rgba([200, 120, 60, 255]));
            }
        }
        let _ = img.save(&out);
        items.push(json!({
            "id": id, "prefab": prefab, "ok": true, "output": out.display().to_string().replace('\\', "/"),
            "size": [64, 64], "coverage": 0.25, "partialAlpha": 0, "notes": format!("stub yaw:{}", it["yaw"]),
        }));
    }
    let dirty = json!([{ "path": "Assets/Scenes/Main.unity", "dirty": false }]);
    let result = json!({
        "schema": "unity-icon-studio.capture/1", "unity": "6000.0.1f1", "batchMode": batch, "graphics": "Direct3D12",
        "sceneDirtyBefore": dirty, "items": items, "failed": failed, "sceneDirtyAfter": dirty, "overrides": "", "totalMs": 1, "ok": failed == 0,
    });
    if let Some(dir) = result_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&result_path, result.to_string());
    (result, failed)
}
