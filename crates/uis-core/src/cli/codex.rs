//! Codex CLI: finding it, copying it out of the desktop app, readiness, text answers and pictures.
//!
//! - **Finding** ([`find`]): the settings' `codex.path`, then the tool's copy
//!   (`<home>\codex-bin\current.json`), then `codex.exe` / `codex.cmd` on PATH (for an npm install,
//!   the `codex.exe` it ships is used when found).
//! - **The desktop app's Codex** cannot be started from its install folder, and the image tool needs
//!   `codex-code-mode-host.exe` next to `codex.exe`. [`copy_from_app`] copies the app's
//!   `codex*.exe` files to `<home>\codex-bin\<app version>\` (a copy on this PC, no download). An
//!   older copy is never removed here.
//! - **CODEX_HOME** stays the user's (`%CODEX_HOME%` or `~\.codex`): sign-in is read from there,
//!   nothing is copied out of it. The only thing the tool takes out of it is the picture of its own
//!   run (`generated_images\<thread id>\`), which it removes after copying when
//!   `codex.cleanGeneratedImages` is on.
//! - **Pictures**: exit code 0 does not mean a picture was saved. The picture is looked for in three
//!   steps ([`harvest`]): a `saved_path` in the events, a path in the last message, a new PNG in
//!   the run's `generated_images\<thread id>\` folder.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::errors::{self, FailKind, Failure};
use super::{Call, CliState, Found};
use crate::home::Home;
use crate::proc::{self, Cmd, CODEX_ENV_REMOVE};
use crate::settings::Settings;

const FIND_APP_PS1: &str = include_str!("../../resources/scripts/find-codex-app.ps1");
const HOST_EXE: &str = "codex-code-mode-host.exe";
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// The tool's copy of the desktop app's Codex (`<home>\codex-bin\current.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyInfo {
    pub app_version: String,
    pub cli_version: Option<String>,
    pub dir: String,
    pub bytes: u64,
    pub copied_at: String,
}

pub fn current_copy(home: &Home) -> Option<CopyInfo> {
    let text = std::fs::read_to_string(home.sub("codex-bin").join("current.json")).ok()?;
    let info: CopyInfo = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    Path::new(&info.dir).join("codex.exe").is_file().then_some(info)
}

/// `CODEX_HOME`, or `~\.codex`.
pub fn codex_home() -> PathBuf {
    if let Some(v) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(v);
    }
    let profile = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
    profile.join(".codex")
}

pub fn find(home: &Home, settings: &Settings) -> Option<Found> {
    if let Some(p) = settings.codex.path.as_ref().map(PathBuf::from).filter(|p| p.is_file()) {
        let via_cmd = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd"));
        return Some(Found { path: p, source: "settings".into(), via_cmd });
    }
    if let Some(c) = current_copy(home) {
        return Some(Found { path: Path::new(&c.dir).join("codex.exe"), source: "copy".into(), via_cmd: false });
    }
    if let Some(p) = super::on_path(&["codex.exe"]) {
        return Some(Found { path: p, source: "path".into(), via_cmd: false });
    }
    if let Some(cmd) = super::on_path(&["codex.cmd"]) {
        // npm puts a shim next to node_modules; the real exe ships in the package
        if let Some(dir) = cmd.parent() {
            let vendor = dir.join("node_modules").join("@openai").join("codex").join("vendor");
            if let Ok(entries) = std::fs::read_dir(&vendor) {
                for e in entries.flatten() {
                    let exe = e.path().join("codex").join("codex.exe");
                    if exe.is_file() {
                        return Some(Found { path: exe, source: "npm".into(), via_cmd: false });
                    }
                }
            }
        }
        return Some(Found { path: cmd, source: "path".into(), via_cmd: true });
    }
    None
}

fn command(found: &Found) -> Cmd {
    if found.via_cmd {
        Cmd::new(proc::system_exe("cmd.exe")).args(["/d", "/c"]).arg(found.path.clone())
    } else {
        Cmd::new(&found.path)
    }
}

fn env(cmd: Cmd, settings: &Settings, codex_home_override: Option<&Path>) -> Cmd {
    let mut cmd = cmd;
    if !settings.codex.keep_api_key_env {
        for n in CODEX_ENV_REMOVE {
            cmd = cmd.remove_env(*n);
        }
    }
    if let Some(h) = codex_home_override {
        cmd = cmd.env("CODEX_HOME", h.display().to_string());
    }
    cmd
}

/// Readiness: version, sign-in, the image runner next to it.
pub fn state(home: &Home, settings: &Settings) -> CliState {
    let Some(found) = find(home, settings) else {
        return CliState { found: false, enabled: true, problem: Some("notFound".into()), ..CliState::default() };
    };
    let mut st = CliState {
        found: true,
        path: Some(found.path.display().to_string()),
        source: Some(found.source.clone()),
        enabled: true,
        ..CliState::default()
    };
    let version = proc::run(&env(command(&found).arg("--version"), settings, None).timeout(PROBE_TIMEOUT));
    match version {
        Ok(out) if out.success() => st.version = super::version_line(&out.stdout_text()),
        Ok(out) => st.problem = Some(format!("version:{}", out.code())),
        Err(e) => {
            st.problem = Some(e);
            return st;
        }
    }
    if let Ok(out) = proc::run(&env(command(&found).args(["login", "status"]), settings, None).timeout(PROBE_TIMEOUT)) {
        let text = format!("{}\n{}", out.stdout_text(), out.stderr_text()).to_lowercase();
        st.logged_in = Some(out.success() && text.contains("logged in") && !text.contains("not logged in"));
    }
    st.image_tool = Some(!found.via_cmd && found.path.parent().is_some_and(|d| d.join(HOST_EXE).is_file()));
    st
}

// ------------------------------------------------------------------ the desktop app

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppFile {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub resources: String,
    pub files: Vec<AppFile>,
    pub bytes: u64,
}

fn script_path(home: &Home) -> Result<PathBuf, String> {
    // Content-named, so an edited script gets a new file
    let hash = FIND_APP_PS1.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let dir = home.runtime_dir().join(format!("codex-{:012x}", hash & 0xffff_ffff_ffff));
    let file = dir.join("find-codex-app.ps1");
    if !file.is_file() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("write:{e}"))?;
        std::fs::write(&file, FIND_APP_PS1).map_err(|e| format!("write:{e}"))?;
    }
    Ok(file)
}

/// The installed Codex desktop app and its `codex*.exe` files (Err `noApp` when not installed).
pub fn app_info(home: &Home) -> Result<AppInfo, String> {
    let script = script_path(home)?;
    let out = proc::run(
        &Cmd::new(proc::system_exe("WindowsPowerShell/v1.0/powershell.exe"))
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .cwd(script.parent().map(Path::to_path_buf).unwrap_or_else(|| home.runtime_dir()))
            .timeout(PROBE_TIMEOUT),
    )?;
    if !out.success() {
        return Err(format!("findApp:{}", out.code()));
    }
    let v: Value = serde_json::from_str(out.stdout_text().trim()).map_err(|e| format!("findApp:{e}"))?;
    let Some(version) = v.get("version").and_then(Value::as_str) else { return Err("noApp".into()) };
    let files: Vec<AppFile> = v
        .get("files")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|f| serde_json::from_value(f.clone()).ok()).collect())
        .unwrap_or_default();
    if !files.iter().any(|f| f.name.eq_ignore_ascii_case("codex.exe")) {
        return Err("appNoCodex".into());
    }
    Ok(AppInfo {
        version: version.into(),
        resources: v.get("resources").and_then(Value::as_str).unwrap_or("").into(),
        bytes: files.iter().map(|f| f.size).sum(),
        files,
    })
}

/// Copies the app's `codex*.exe` to `<home>\codex-bin\<version>\` and makes it the current copy
/// once its `--version` answers. `progress(done, total)` in bytes.
pub fn copy_from_app(home: &Home, progress: &mut dyn FnMut(u64, u64)) -> Result<CopyInfo, String> {
    let app = app_info(home)?;
    let safe: String = app.version.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-').collect();
    let dest = home.sub("codex-bin").join(&safe);
    std::fs::create_dir_all(&dest).map_err(|e| format!("write:{e}"))?;
    let mut done = 0u64;
    progress(0, app.bytes);
    for f in &app.files {
        let src = Path::new(&app.resources).join(&f.name);
        let dst = dest.join(&f.name);
        if dst.metadata().is_ok_and(|m| m.len() == f.size) {
            done += f.size;
            progress(done, app.bytes);
            continue;
        }
        let tmp = dest.join(format!("{}.part", f.name));
        copy_with_progress(&src, &tmp, &mut |n| progress(done + n, app.bytes)).map_err(|e| format!("copy:{}:{e}", f.name))?;
        std::fs::rename(&tmp, &dst).map_err(|e| format!("copy:{}:{e}", f.name))?;
        done += f.size;
    }
    let exe = dest.join("codex.exe");
    let out = proc::run(&Cmd::new(&exe).arg("--version").timeout(PROBE_TIMEOUT))?;
    if !out.success() {
        return Err(format!("copyCheck:{}", out.code()));
    }
    let info = CopyInfo {
        app_version: app.version.clone(),
        cli_version: super::version_line(&out.stdout_text()),
        dir: dest.display().to_string(),
        bytes: app.bytes,
        copied_at: crate::capture::new_run_id(),
    };
    let text = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
    crate::settings::write_replace(&home.sub("codex-bin").join("current.json"), &text)?;
    Ok(info)
}

fn copy_with_progress(src: &Path, dst: &Path, tick: &mut dyn FnMut(u64)) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let mut r = std::fs::File::open(src)?;
    let mut w = std::fs::File::create(dst)?;
    let mut buf = vec![0u8; 4 << 20];
    let mut n = 0u64;
    loop {
        let k = r.read(&mut buf)?;
        if k == 0 {
            break;
        }
        w.write_all(&buf[..k])?;
        n += k as u64;
        tick(n);
    }
    w.flush()
}

// ------------------------------------------------------------------ runs

fn exec_args(cmd: Cmd, settings: &Settings, ignore_user_config: bool, dir: &Path) -> Cmd {
    let _ = settings;
    let mut cmd = cmd.args(["exec", "--ephemeral"]);
    if ignore_user_config {
        cmd = cmd.arg("--ignore-user-config");
    }
    cmd.args(["--ignore-rules", "--skip-git-repo-check", "-s", "read-only", "-C"]).arg(dir.as_os_str())
}

fn call(purpose: &str, out: Option<&proc::Output>, failure: Option<&Failure>) -> Call {
    Call {
        cli: "codex".into(),
        purpose: purpose.into(),
        attempt: 1,
        elapsed_ms: out.map_or(0, |o| o.elapsed_ms),
        code: failure.map(Failure::code).unwrap_or_else(|| "ok".into()),
        cost_usd: None,
    }
}

/// A JSON answer that matches `schema` (text only, low reasoning effort).
pub fn ask_json(home: &Home, settings: &Settings, prompt: &str, schema: &str) -> (Result<Value, Failure>, Call) {
    let Some(found) = find(home, settings) else {
        let f = Failure::new(FailKind::NotFound, "codex", "codex not found");
        return (Err(f.clone()), call("rewrite", None, Some(&f)));
    };
    let dir = match super::temp_dir(home, "codex") {
        Ok(d) => d,
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "codex", e);
            return (Err(f.clone()), call("rewrite", None, Some(&f)));
        }
    };
    let result = ask_json_in(&found, settings, &dir, prompt, schema);
    super::remove_temp(&dir);
    result
}

fn ask_json_in(found: &Found, settings: &Settings, dir: &Path, prompt: &str, schema: &str) -> (Result<Value, Failure>, Call) {
    let schema_file = dir.join("schema.json");
    let out_file = dir.join("out.json");
    if let Err(e) = std::fs::write(&schema_file, schema) {
        let f = Failure::new(FailKind::Spawn, "codex", format!("write:{e}"));
        return (Err(f.clone()), call("rewrite", None, Some(&f)));
    }
    let cmd = exec_args(command(found), settings, settings.codex.ignore_user_config, dir)
        .args(["-c", "model_reasoning_effort=\"low\"", "--output-schema"])
        .arg(schema_file.as_os_str())
        .arg("-o")
        .arg(out_file.as_os_str())
        .args(["--json", "-"])
        .cwd(dir)
        .stdin(prompt.as_bytes().to_vec())
        .timeout(Duration::from_secs(settings.codex.text_timeout_sec as u64));
    let cmd = env(cmd, settings, None);
    let out = match proc::run(&cmd) {
        Ok(o) => o,
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "codex", e);
            return (Err(f.clone()), call("rewrite", None, Some(&f)));
        }
    };
    let events = errors::parse_events(&out.stdout);
    let last = std::fs::read_to_string(&out_file).unwrap_or_default();
    if let Some(f) = errors::classify_codex(&out, &events, &last, false, false) {
        let c = call("rewrite", Some(&out), Some(&f));
        return (Err(f), c);
    }
    let text = if last.trim().is_empty() { errors::codex_agent_messages(&events).last().cloned().unwrap_or_default() } else { last };
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(v) if v.is_object() => (Ok(v), call("rewrite", Some(&out), None)),
        _ => {
            let f = Failure::new(FailKind::BadJson, "codex", text.lines().next().unwrap_or("empty answer").to_string());
            let c = call("rewrite", Some(&out), Some(&f));
            (Err(f), c)
        }
    }
}

/// One picture.
#[derive(Debug, Clone)]
pub struct ImageRequest {
    /// The capture (on white) first, then style references
    pub inputs: Vec<PathBuf>,
    /// What goes to stdin (the outer instruction + PROMPT block)
    pub prompt: String,
    /// The candidate folder: `raw.png`, `events.jsonl`, `last-message.txt`, `stderr.txt`, `meta.json`
    pub dest: PathBuf,
    /// Tests: a made-up CODEX_HOME passed to the child
    pub codex_home: Option<PathBuf>,
    /// None: the settings' `codex.ignoreUserConfig`
    pub ignore_user_config: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResult {
    pub ok: bool,
    pub raw: Option<String>,
    /// 1 `saved_path` event, 2 last message, 3 the thread folder
    pub harvest_stage: Option<u8>,
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
    pub thread_id: Option<String>,
    pub failure: Option<Failure>,
    pub call: Call,
    pub cli_path: String,
    pub ignore_user_config: bool,
    /// The picture was removed from CODEX_HOME after the copy
    pub cleaned: bool,
}

/// Runs Codex once for a picture and copies it to `dest\raw.png`.
pub fn generate_image(home: &Home, settings: &Settings, req: &ImageRequest) -> ImageResult {
    let ignore = req.ignore_user_config.unwrap_or(settings.codex.ignore_user_config);
    let mut result = ImageResult {
        ok: false,
        raw: None,
        harvest_stage: None,
        width: 0,
        height: 0,
        has_alpha: false,
        thread_id: None,
        failure: None,
        call: call("image", None, None),
        cli_path: String::new(),
        ignore_user_config: ignore,
        cleaned: false,
    };
    let fail = |mut r: ImageResult, f: Failure| {
        r.call.code = f.code();
        r.failure = Some(f);
        r
    };
    let Some(found) = find(home, settings) else {
        return fail(result, Failure::new(FailKind::NotFound, "codex", "codex not found"));
    };
    result.cli_path = found.path.display().to_string();
    if let Err(e) = std::fs::create_dir_all(&req.dest) {
        return fail(result, Failure::new(FailKind::Spawn, "codex", format!("write:{e}")));
    }
    let dir = match super::temp_dir(home, "codex-image") {
        Ok(d) => d,
        Err(e) => return fail(result, Failure::new(FailKind::Spawn, "codex", e)),
    };
    let last_file = dir.join("last.txt");
    let mut cmd = exec_args(command(&found), settings, ignore, &dir);
    for i in &req.inputs {
        cmd = cmd.arg("-i").arg(i.as_os_str());
    }
    let cmd = cmd
        .arg("-o")
        .arg(last_file.as_os_str())
        .args(["--json", "-"])
        .cwd(&dir)
        .stdin(req.prompt.as_bytes().to_vec())
        .timeout(Duration::from_secs(settings.codex.image_timeout_sec as u64));
    let cmd = env(cmd, settings, req.codex_home.as_deref());
    let started = SystemTime::now();
    let out = match proc::run(&cmd) {
        Ok(o) => o,
        Err(e) => {
            super::remove_temp(&dir);
            return fail(result, Failure::new(FailKind::Spawn, "codex", e));
        }
    };
    let events = errors::parse_events(&out.stdout);
    let last = std::fs::read_to_string(&last_file).unwrap_or_default();
    let thread = errors::codex_thread_id(&events);
    result.thread_id = thread.clone();
    let chome = req.codex_home.clone().unwrap_or_else(codex_home);
    let harvested = harvest(&events, &last, &chome, &dir, thread.as_deref(), started);

    // What the run said, kept next to the picture (secrets masked)
    let _ = std::fs::write(req.dest.join("events.jsonl"), crate::hooks::mask(&out.stdout_text()));
    let _ = std::fs::write(req.dest.join("last-message.txt"), &last);
    let _ = std::fs::write(req.dest.join("stderr.txt"), crate::hooks::mask(&out.stderr_text()));
    let _ = std::fs::write(req.dest.join("prompt.txt"), &req.prompt);

    let mut failure = errors::classify_codex(&out, &events, &last, true, harvested.is_ok());
    if let Ok(h) = &harvested {
        let raw = req.dest.join("raw.png");
        match std::fs::copy(&h.path, &raw) {
            Ok(_) => {
                result.raw = Some(raw.display().to_string());
                result.harvest_stage = Some(h.stage);
                result.width = h.width;
                result.height = h.height;
                result.has_alpha = h.has_alpha;
                if settings.codex.clean_generated_images {
                    result.cleaned = clean(&h.path, &chome, thread.as_deref());
                }
            }
            Err(e) => failure = Some(Failure::new(FailKind::NoImage, "codex", format!("copy:{e}"))),
        }
    }
    super::remove_temp(&dir);
    result.ok = failure.is_none() && result.raw.is_some();
    result.call = call("image", Some(&out), failure.as_ref());
    result.failure = failure;
    let why = harvested.err();
    let meta = json!({
        "threadId": result.thread_id, "harvestStage": result.harvest_stage, "harvestMiss": why,
        "elapsedMs": out.elapsed_ms, "code": result.call.code, "exit": out.code(),
        "ignoreUserConfig": ignore, "cli": result.cli_path, "width": result.width, "height": result.height,
        "hasAlpha": result.has_alpha, "cleaned": result.cleaned,
    });
    let _ = std::fs::write(req.dest.join("meta.json"), serde_json::to_string_pretty(&meta).unwrap_or_default());
    log_image(home, &result.call);
    result
}

fn log_image(home: &Home, call: &Call) {
    super::log_call(home, call);
}

#[derive(Debug, Clone, PartialEq)]
pub struct Harvest {
    pub path: PathBuf,
    pub stage: u8,
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
}

fn norm(p: &Path) -> String {
    crate::capture::absolute(p).display().to_string().replace('/', "\\").to_lowercase()
}

fn inside(p: &Path, root: &Path) -> bool {
    let (p, r) = (norm(p), norm(root));
    p.starts_with(&(r.trim_end_matches('\\').to_string() + "\\"))
}

/// A usable picture: a PNG at least 256 px on each side that is not one flat color.
pub fn check_picture(path: &Path) -> Result<(u32, u32, bool), String> {
    let img = ::image::open(path).map_err(|e| format!("decode:{e}"))?;
    let (w, h) = (img.width(), img.height());
    if w < 256 || h < 256 {
        return Err(format!("small:{w}x{h}"));
    }
    let has_alpha = img.color().has_alpha();
    let rgba = img.to_rgba8();
    let first = rgba.get_pixel(0, 0).0;
    if rgba.pixels().all(|p| p.0 == first) {
        return Err("uniform".into());
    }
    let alpha_used = has_alpha && rgba.pixels().any(|p| p.0[3] < 255);
    Ok((w, h, alpha_used))
}

/// Paths that look like a PNG file in a text (quotes, backticks and brackets around it are dropped).
fn png_paths(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines() {
        for raw in line.split(['`', '"', '\'', '<', '>', '(', ')', '[', ']', '*']) {
            let t = raw.trim().trim_end_matches(['.', ',', ';', ':']);
            let lower = t.to_lowercase();
            let Some(end) = lower.find(".png") else { continue };
            let cand = &t[..end + 4];
            // From the drive letter or a leading slash
            let start = cand.find(":\\").or_else(|| cand.find(":/")).map(|i| i.saturating_sub(1)).unwrap_or(0);
            let p = PathBuf::from(&cand[start..]);
            if p.is_absolute() {
                out.push(p);
            }
        }
    }
    out
}

fn saved_paths(v: &Value, out: &mut Vec<PathBuf>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k == "saved_path" || k == "savedPath" {
                    if let Some(s) = x.as_str() {
                        out.push(PathBuf::from(s));
                    }
                } else {
                    saved_paths(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| saved_paths(x, out)),
        _ => {}
    }
}

/// Finds the run's picture in three steps. Paths from the run are only taken inside CODEX_HOME or
/// the run's working folder. Err: why each step missed.
pub fn harvest(events: &[Value], last: &str, chome: &Path, work: &Path, thread: Option<&str>, started: SystemTime) -> Result<Harvest, String> {
    let mut misses: Vec<String> = Vec::new();
    let try_path = |p: &Path, stage: u8, misses: &mut Vec<String>| -> Option<Harvest> {
        if !(inside(p, chome) || inside(p, work)) {
            misses.push(format!("{stage}:outside"));
            return None;
        }
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) || !p.is_file() {
            misses.push(format!("{stage}:missing"));
            return None;
        }
        match check_picture(p) {
            Ok((width, height, has_alpha)) => Some(Harvest { path: p.to_path_buf(), stage, width, height, has_alpha }),
            Err(e) => {
                misses.push(format!("{stage}:{e}"));
                None
            }
        }
    };
    // 1: saved_path in the events
    let mut paths = Vec::new();
    events.iter().for_each(|e| saved_paths(e, &mut paths));
    for p in &paths {
        if let Some(h) = try_path(p, 1, &mut misses) {
            return Ok(h);
        }
    }
    if paths.is_empty() {
        misses.push("1:none".into());
    }
    // 2: a path in the last message (the -o file, else the last agent message)
    let said = if last.trim().is_empty() { errors::codex_agent_messages(events).last().cloned().unwrap_or_default() } else { last.to_string() };
    let paths = png_paths(&said);
    for p in paths.iter().rev() {
        if let Some(h) = try_path(p, 2, &mut misses) {
            return Ok(h);
        }
    }
    if paths.is_empty() {
        misses.push("2:none".into());
    }
    // 3: a new PNG in generated_images\<thread>
    if let Some(t) = thread.filter(|t| !t.is_empty() && !t.contains(['\\', '/', '.'])) {
        let dir = chome.join("generated_images").join(t);
        let since = started.checked_sub(Duration::from_secs(5)).unwrap_or(started);
        let mut found: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
                    .filter_map(|p| p.metadata().and_then(|m| m.modified()).ok().map(|t| (t, p)))
                    .filter(|(t, _)| *t >= since)
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        for (_, p) in found.iter().rev() {
            if let Some(h) = try_path(p, 3, &mut misses) {
                return Ok(h);
            }
        }
        if found.is_empty() {
            misses.push("3:none".into());
        }
    } else {
        misses.push("3:noThread".into());
    }
    Err(misses.join(" "))
}

/// Removes the run's picture from `generated_images\<thread>\`, and the folder when it is empty.
fn clean(path: &Path, chome: &Path, thread: Option<&str>) -> bool {
    let Some(t) = thread else { return false };
    let dir = chome.join("generated_images").join(t);
    if !inside(path, &dir) {
        return false;
    }
    let removed = std::fs::remove_file(path).is_ok();
    if std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_none()) {
        let _ = std::fs::remove_dir(&dir);
    }
    removed
}

/// Review of a candidate against the capture (`{pass, issues}`), pictures given with `-i`.
pub fn review(home: &Home, settings: &Settings, capture: &Path, candidate: &Path, prompt: &str) -> (Result<Value, Failure>, Call) {
    let Some(found) = find(home, settings) else {
        let f = Failure::new(FailKind::NotFound, "codex", "codex not found");
        return (Err(f.clone()), call("review", None, Some(&f)));
    };
    let dir = match super::temp_dir(home, "codex-review") {
        Ok(d) => d,
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "codex", e);
            return (Err(f.clone()), call("review", None, Some(&f)));
        }
    };
    let schema_file = dir.join("schema.json");
    let out_file = dir.join("out.json");
    let _ = std::fs::write(&schema_file, crate::prompt::REVIEW_SCHEMA);
    let cmd = exec_args(command(&found), settings, settings.codex.ignore_user_config, &dir)
        .args(["-c", "model_reasoning_effort=\"low\"", "-i"])
        .arg(capture.as_os_str())
        .arg("-i")
        .arg(candidate.as_os_str())
        .arg("--output-schema")
        .arg(schema_file.as_os_str())
        .arg("-o")
        .arg(out_file.as_os_str())
        .args(["--json", "-"])
        .cwd(&dir)
        .stdin(prompt.as_bytes().to_vec())
        .timeout(Duration::from_secs(settings.codex.text_timeout_sec as u64));
    let cmd = env(cmd, settings, None);
    let result = match proc::run(&cmd) {
        Err(e) => {
            let f = Failure::new(FailKind::Spawn, "codex", e);
            (Err(f.clone()), call("review", None, Some(&f)))
        }
        Ok(out) => {
            let events = errors::parse_events(&out.stdout);
            let last = std::fs::read_to_string(&out_file).unwrap_or_default();
            match errors::classify_codex(&out, &events, &last, false, false) {
                Some(f) => {
                    let c = call("review", Some(&out), Some(&f));
                    (Err(f), c)
                }
                None => match serde_json::from_str::<Value>(last.trim()) {
                    Ok(v) if v.is_object() => (Ok(v), call("review", Some(&out), None)),
                    _ => {
                        let f = Failure::new(FailKind::BadJson, "codex", "no JSON answer");
                        let c = call("review", Some(&out), Some(&f));
                        (Err(f), c)
                    }
                },
            }
        }
    };
    super::remove_temp(&dir);
    super::log_call(home, &result.1);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(path: &Path, w: u32, h: u32, uniform: bool) {
        let img = ::image::RgbaImage::from_fn(w, h, |x, y| {
            if uniform {
                ::image::Rgba([200, 200, 200, 255])
            } else {
                ::image::Rgba([(x % 255) as u8, (y % 255) as u8, 90, if x < 10 { 0 } else { 255 }])
            }
        });
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        img.save(path).unwrap();
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("uis-harvest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn harvest_stage_one_saved_path() {
        let d = temp("one");
        let chome = d.join("codex");
        let pic = chome.join("generated_images/t1/a.png");
        png(&pic, 300, 300, false);
        let events = vec![json!({ "type": "item.completed", "item": { "type": "image_generation_end", "saved_path": pic.display().to_string() } })];
        let h = harvest(&events, "", &chome, &d.join("work"), Some("t1"), SystemTime::now()).unwrap();
        assert_eq!(h.stage, 1);
        assert!(h.has_alpha);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn harvest_stage_two_last_message_and_three_thread_folder() {
        let d = temp("two");
        let chome = d.join("codex");
        let pic = chome.join("generated_images/t2/exec-1.png");
        png(&pic, 256, 256, false);
        let said = format!("Saved: `{}`.", pic.display());
        let h = harvest(&[], &said, &chome, &d.join("work"), Some("t2"), SystemTime::now()).unwrap();
        assert_eq!(h.stage, 2);
        // No path said: found in the thread folder
        let h = harvest(&[], "Done.", &chome, &d.join("work"), Some("t2"), SystemTime::now()).unwrap();
        assert_eq!(h.stage, 3);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn harvest_refuses_small_uniform_outside_and_old() {
        let d = temp("bad");
        let chome = d.join("codex");
        let small = chome.join("generated_images/t3/small.png");
        png(&small, 100, 100, false);
        let flat = chome.join("generated_images/t3/flat.png");
        png(&flat, 300, 300, true);
        let outside = d.join("elsewhere/x.png");
        png(&outside, 300, 300, false);
        let said = format!("{}\n{}\n{}", small.display(), flat.display(), outside.display());
        let err = harvest(&[], &said, &chome, &d.join("work"), Some("t3"), SystemTime::now()).unwrap_err();
        assert!(err.contains("2:small") && err.contains("2:uniform") && err.contains("2:outside"), "{err}");
        // Started later than the files were written: stage 3 does not take them
        let later = SystemTime::now() + Duration::from_secs(60);
        let err = harvest(&[], "", &chome, &d.join("work"), Some("t3"), later).unwrap_err();
        assert!(err.contains("3:none"), "{err}");
        assert!(harvest(&[], "", &chome, &d.join("work"), None, SystemTime::now()).unwrap_err().contains("3:noThread"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn clean_removes_only_the_threads_picture() {
        let d = temp("clean");
        let chome = d.join("codex");
        let pic = chome.join("generated_images/t4/exec-1.png");
        png(&pic, 256, 256, false);
        let other = chome.join("generated_images/t5/keep.png");
        png(&other, 256, 256, false);
        assert!(!clean(&other, &chome, Some("t4")));
        assert!(clean(&pic, &chome, Some("t4")));
        assert!(!chome.join("generated_images/t4").exists());
        assert!(other.is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn paths_in_text() {
        let p = png_paths("Saved to C:\\fake\\home\\.codex\\generated_images\\t\\a.png.\n(see `C:/x/b.PNG`)");
        assert_eq!(p, [PathBuf::from("C:\\fake\\home\\.codex\\generated_images\\t\\a.png"), PathBuf::from("C:/x/b.PNG")]);
        assert!(png_paths("no picture here").is_empty());
    }
}
