//! `uis`: the command line of Icon Studio. It uses the same home folder and functions as the app.
//! Commands print JSON with `--json` (a short text summary otherwise).

use serde_json::{json, Value};
use std::path::PathBuf;
use uis_core::home::Home;

mod post_cmds;

const USAGE: &str = "usage: uis <command> [options]

commands:
  scan <project> [--force] [--models] [--pack <id>] [--json]
                       list and classify the prefabs of a Unity project (no Unity needed)
  probe <project> [--json]
                       how a capture would run now (lock file, version, packages)
  capture --project <folder> --prefab <asset> [<asset>...] [--mode auto|attach|batch]
          [--out <folder>] [--pack <id>] [--thumb] [--json]
                       capture prefabs (or model files) to transparent PNGs: in the open editor
                       (unity CLI) or with Unity in batch mode when the project is closed
  recover [--status] [--json]
                       finish batch captures left by an earlier run (remove the temporary
                       folder, put rewritten files back, run the after-capture hooks)
  readiness [--json]   Codex, Claude and the unity CLI: found, version, signed in, image runner
  codex-copy [--json]  copy the Codex desktop app's codex*.exe to the tool's folder (about 0.5 GB)
  prompt --kind <kind> [--refs N] [--yaw D] [--pitch D] [--outline code|model|none]
         [--background transparent|flat] [--pack <id>] [--json]
                       the fixed prompt (no LLM)
  rewrite --kind <kind> --note <text> [--cli claude|codex] [--refs N] [--yaw D] [--pitch D]
          [--pack <id>] [--out <folder>] [--json]
                       work a note into the fixed prompt (an empty note calls nothing)
  generate --input <capture.png> --kind <kind> [--note <text>] [--cli claude|codex]
           [--ref <png>...] [--no-pack-refs] [--candidates N] [--ignore-user-config true|false]
           [--name <prefab>] [--pack <id>] [--out <job folder>] [--prompt-only] [--post] [--json]
                       make raw icon pictures with Codex (counts against the image limit);
                       --post also post-processes and checks each picture
  run --project <folder> --prefab <asset> [<asset>...] [--note <text>] [--cli claude|codex]
      [--mode auto|attach|batch] [--candidates N] [--outline code|model|none]
      [--background auto|key] [--outline-off-ok] [--no-pack-refs] [--to <folder> [--sizes 256,512] [--export-failing]]
      [--pack <id>] [--json]
                       the whole run: capture → prompt → rewrite → pictures → post-processing →
                       checks [→ export]; steps go to stderr (counts against the image limit)
  review --capture <png> --candidate <png> --kind <kind> [--cli claude|codex] [--json]
                       ask an LLM whether a picture is usable ({pass, issues})
  post --job <folder> [--cand N] [--set key=value...] [--json]
  post --raw <png> --out <folder> [--kind <kind>] [--capture <png>] [--pack <id>] [--set key=value...]
                       outline, background, layout and sizes of a raw picture, then the checks
                       (keys: outline widthPct aaPx workScale fillPct layout background compose ...)
  check <png>... [--kind <kind>] [--pack <id>] [--no-outline] [--json]
                       pixel checks of finished icons against the pack's ranges
  board --out <png> [--title <text>] [--spec <rows.json>] [--job <folder> [--ref <png>]]...
                       a comparison board: capture | raw | icon | small sizes | on white and dark | reference
  export --job <folder> --to <folder> [--cand N] [--sizes 256,512,1024] [--name <prefab>] [--rule <rule>]
                       copy finished icons out (never into a Unity project)
  jobs [--limit N]     job folders, newest first
  hub [--json]         the projects Unity Hub lists
  packs [--json]       style packs (the default one and local ones)
  home [--json]        the tool's home folder and settings file
  settings             the settings in effect (JSON)
  --version            the version
";

struct Args {
    json: bool,
    flags: Vec<String>,
    values: Vec<(String, String)>,
    positional: Vec<String>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Args {
        let mut out = Args { json: false, flags: Vec::new(), values: Vec::new(), positional: Vec::new() };
        let mut it = raw.into_iter().peekable();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--json" => out.json = true,
                "--pack" | "--project" | "--mode" | "--out" | "--kind" | "--note" | "--cli" | "--refs" | "--yaw"
                | "--pitch" | "--outline" | "--background" | "--input" | "--ref" | "--candidates"
                | "--ignore-user-config" | "--name" | "--capture" | "--candidate" | "--set" | "--raw" | "--job" | "--cand"
                | "--to" | "--sizes" | "--spec" | "--title" | "--rule" | "--limit" => {
                    if let Some(v) = it.next() {
                        out.values.push((a, v));
                    }
                }
                "--prefab" => {
                    // One or more assets, up to the next option
                    while let Some(v) = it.peek() {
                        if v.starts_with("--") {
                            break;
                        }
                        out.values.push((a.clone(), it.next().unwrap_or_default()));
                    }
                }
                _ if a.starts_with("--") => out.flags.push(a),
                _ => out.positional.push(a),
            }
        }
        out
    }
    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
    fn value(&self, name: &str) -> Option<&str> {
        self.values.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn all(&self, name: &str) -> Vec<String> {
        self.values.iter().filter(|(k, _)| k == name).map(|(_, v)| v.clone()).collect()
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "--version" || a == "-V") {
        println!("uis {}", uis_core::VERSION);
        return;
    }
    let args = Args::parse(raw);
    let code = match args.positional.first().map(String::as_str) {
        Some("scan") => scan(&args),
        Some("probe") => probe(&args),
        Some("capture") => capture(&args),
        Some("recover") => recover(&args),
        Some("readiness") => readiness(&args),
        Some("codex-copy") => codex_copy(&args),
        Some("prompt") => prompt_cmd(&args),
        Some("rewrite") => rewrite_cmd(&args),
        Some("generate") => generate_cmd(&args),
        Some("run") => run_cmd(&args),
        Some("review") => review_cmd(&args),
        Some("post") => post_cmds::post_cmd(&args),
        Some("check") => post_cmds::check_cmd(&args),
        Some("board") => post_cmds::board_cmd(&args),
        Some("export") => post_cmds::export_cmd(&args),
        Some("jobs") => post_cmds::jobs_cmd(&args),
        Some("hub") => {
            print_value(&serde_json::to_value(uis_core::project::hub_projects()).unwrap_or_default(), true);
            0
        }
        Some("packs") => {
            print_value(&serde_json::to_value(uis_core::packs::list(&Home::resolve())).unwrap_or_default(), true);
            0
        }
        Some("home") => {
            let home = Home::resolve();
            let value = json!({ "home": home.root(), "override": Home::is_override(), "settingsFile": home.settings_file() });
            print_value(&value, args.json);
            0
        }
        Some("settings") => {
            let settings = uis_core::settings::load(&Home::resolve());
            print_value(&serde_json::to_value(&settings).unwrap_or_default(), true);
            0
        }
        Some("help") | None => {
            print!("{USAGE}");
            0
        }
        _ if args.has("--help") || args.has("-h") => {
            print!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("uis: unknown command {other:?}\n\n{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

fn project_arg(args: &Args) -> Option<PathBuf> {
    args.positional.get(1).map(PathBuf::from)
}

fn fail(args: &Args, code: &str) -> i32 {
    if args.json {
        println!("{}", json!({ "error": code }));
    } else {
        eprintln!("uis: {code}");
    }
    1
}

fn scan(args: &Args) -> i32 {
    let Some(path) = project_arg(args) else {
        eprintln!("uis scan: give a project folder\n\n{USAGE}");
        return 2;
    };
    let home = Home::resolve();
    let path = uis_core::project::find_project_root(&path).unwrap_or(path);
    // A project on the list uses its pack and the kinds a person chose; --pack replaces the pack
    let listed = uis_core::settings::load(&home)
        .projects
        .into_iter()
        .find(|p| PathBuf::from(&p.path).canonicalize().ok() == path.canonicalize().ok());
    let pack_id = args
        .value("--pack")
        .map(str::to_string)
        .or_else(|| listed.as_ref().map(|p| p.pack_id.clone()))
        .unwrap_or_else(|| uis_core::packs::DEFAULT_ID.into());
    let pack = match uis_core::packs::load(&home, &pack_id) {
        Ok(p) => p,
        Err(e) => return fail(args, &format!("pack:{e}")),
    };
    let options = uis_core::prefabs::ScanOptions {
        force: args.has("--force"),
        include_models: args.has("--models"),
        kind_overrides: listed.map(|p| uis_core::projects::kind_overrides(&home, &p.id)).unwrap_or_default(),
    };
    match uis_core::prefabs::scan(&home, &path, &pack, &options) {
        Ok(result) => {
            if args.json {
                print_value(&serde_json::to_value(&result).unwrap_or_default(), true);
            } else {
                println!("{}  pack {}", result.project, result.pack_id);
                println!("prefabs {}  {:?}", result.total, result.counts);
                println!("kinds   {:?}", result.kinds);
                println!(
                    "metas {}  models {}  {} ms  (read {} prefabs, {} metas; cached {}, {})",
                    result.metas,
                    result.models,
                    result.elapsed_ms,
                    result.cache.prefabs_read,
                    result.cache.metas_read,
                    result.cache.prefabs_cached,
                    result.cache.metas_cached
                );
            }
            0
        }
        Err(e) => fail(args, &e),
    }
}

fn probe(args: &Args) -> i32 {
    let Some(path) = project_arg(args) else {
        eprintln!("uis probe: give a project folder\n\n{USAGE}");
        return 2;
    };
    let result = uis_core::project::probe(&path);
    let value = serde_json::to_value(&result).unwrap_or_default();
    if args.json {
        print_value(&value, true);
    } else {
        println!("{}  {} ({})", value["path"].as_str().unwrap_or(""), result.mode, result.reason);
        println!(
            "unity {}  branch {}  pipeline {}  urp {}",
            value["unityVersion"].as_str().unwrap_or("?"),
            value["branch"].as_str().unwrap_or("-"),
            value["pipelinePackage"],
            value["urp"]
        );
    }
    0
}

fn capture(args: &Args) -> i32 {
    let project = args.value("--project").map(PathBuf::from).or_else(|| project_arg(args));
    let Some(project) = project else {
        eprintln!("uis capture: give --project <folder>

{USAGE}");
        return 2;
    };
    let mut items = args.all("--prefab");
    if args.value("--project").is_some() {
        items.extend(args.positional.iter().skip(1).cloned());
    } else {
        items.extend(args.positional.iter().skip(2).cloned());
    }
    if items.is_empty() {
        eprintln!("uis capture: give --prefab <asset> [<asset>...]

{USAGE}");
        return 2;
    }
    let req = uis_core::capture::CaptureRequest {
        project: uis_core::project::find_project_root(&project).unwrap_or(project),
        items,
        pack_id: args.value("--pack").map(str::to_string),
        mode: args.value("--mode").map(str::to_string),
        out_dir: args.value("--out").map(PathBuf::from),
        purpose: if args.has("--thumb") { "thumb".into() } else { "capture".into() },
        ..Default::default()
    };
    let home = Home::resolve();
    let _ = home.ensure();
    let mut last = String::new();
    let report = uis_core::capture::capture(&home, &req, &mut |e| {
        let line = format!("[{:>6.1}s] {:<7} {}/{} {}", e.elapsed_ms as f64 / 1000.0, e.stage, e.done, e.total, e.message);
        if line != last {
            eprintln!("{line}");
            last = line;
        }
    });
    if args.json {
        print_value(&serde_json::to_value(&report).unwrap_or_default(), true);
    } else {
        println!("{}  {} ({})  run {}", report.project, report.mode, report.reason, report.run_id);
        for it in &report.items {
            println!("  {:<4} {:<9} {}  {}", if it.ok { "ok" } else { "FAIL" }, it.kind, it.prefab, it.error.as_deref().or(it.output.as_deref()).unwrap_or(""));
        }
        if let Some(g) = &report.git {
            println!("git same {}  restored {}  other {}", g.same, g.restored.len(), g.other.len());
        }
        if let Some(e) = &report.error {
            println!("error {e}");
        }
        println!("out {}  {} ms (unity {} ms)", report.out_dir, report.elapsed_ms, report.unity_ms);
    }
    if report.ok {
        0
    } else if report.error.is_none() {
        3
    } else {
        1
    }
}

fn recover(args: &Args) -> i32 {
    let home = Home::resolve();
    if args.has("--status") {
        print_value(&serde_json::to_value(uis_core::capture::recovery_status(&home)).unwrap_or_default(), true);
        return 0;
    }
    let done = uis_core::capture::recover(&home);
    let pending = uis_core::capture::recovery_status(&home);
    let value = json!({ "finished": done, "pending": pending });
    if args.json {
        print_value(&value, true);
    } else {
        for f in &done {
            println!("finished {}  {}  exit {:?}  git same {}  {}", f.run_id, f.project, f.exit_code, f.git.same, f.error.as_deref().unwrap_or("ok"));
        }
        for p in &pending {
            println!("pending  {}  {}  pid {}  running {}", p.run_id, p.project, p.pid, p.running);
        }
        if done.is_empty() && pending.is_empty() {
            println!("nothing to recover");
        }
    }
    0
}

// ------------------------------------------------------------------ prompts and CLIs

fn readiness(args: &Args) -> i32 {
    let home = Home::resolve();
    let _ = home.ensure();
    let s = uis_core::settings::load(&home);
    let r = uis_core::cli::readiness(&home, &s);
    if args.json {
        print_value(&serde_json::to_value(&r).unwrap_or_default(), true);
    } else {
        for (name, st) in [("codex", &r.codex), ("claude", &r.claude), ("unity", &r.unity_cli)] {
            println!(
                "{name:<7} found {:<5} logged in {:<5} image tool {:<5} {}  {}",
                st.found,
                st.logged_in.map_or("-".into(), |b| b.to_string()),
                st.image_tool.map_or("-".into(), |b| b.to_string()),
                st.version.as_deref().unwrap_or(""),
                st.problem.as_deref().unwrap_or("")
            );
        }
        if let Some(app) = &r.codex_app {
            println!("codex app {}  {} bytes  copy needed {}", app.version, app.bytes, r.codex_copy_needed);
        }
    }
    0
}

fn codex_copy(args: &Args) -> i32 {
    let home = Home::resolve();
    let _ = home.ensure();
    let mut last = 0u64;
    let r = uis_core::cli::codex::copy_from_app(&home, &mut |done, total| {
        if done == total || done >= last + (64 << 20) {
            eprintln!("copy {} / {} MB", done >> 20, total >> 20);
            last = done;
        }
    });
    match r {
        Ok(info) => {
            print_value(&serde_json::to_value(&info).unwrap_or_default(), args.json);
            0
        }
        Err(e) => fail(args, &e),
    }
}

fn pack_arg(args: &Args, home: &Home) -> Result<uis_core::packs::Pack, String> {
    uis_core::packs::load(home, args.value("--pack").unwrap_or(uis_core::packs::DEFAULT_ID)).map_err(|e| format!("pack:{e}"))
}

fn num(args: &Args, name: &str) -> Option<f64> {
    args.value(name).and_then(|v| v.parse().ok())
}

fn context_arg(args: &Args, pack: &uis_core::packs::Pack, s: &uis_core::settings::Settings) -> Result<uis_core::prompt::PromptContext, String> {
    let kind = args.value("--kind").unwrap_or("generic");
    if !uis_core::packs::KINDS.contains(&kind) {
        return Err(format!("badKind:{kind}"));
    }
    let refs = args.value("--refs").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut ctx = uis_core::generate::context(pack, s, kind, refs, num(args, "--yaw"), num(args, "--pitch"));
    if let Some(o) = args.value("--outline") {
        ctx.outline = o.into();
    }
    if let Some(b) = args.value("--background") {
        ctx.background = b.into();
    }
    Ok(ctx)
}

fn prompt_cmd(args: &Args) -> i32 {
    let home = Home::resolve();
    let s = uis_core::settings::load(&home);
    let built = pack_arg(args, &home).and_then(|pack| {
        let ctx = context_arg(args, &pack, &s)?;
        uis_core::prompt::assemble(&pack, &ctx)
    });
    match built {
        Ok(a) => {
            if args.json {
                print_value(&serde_json::to_value(&a).unwrap_or_default(), true);
            } else {
                print!("{}", a.text);
            }
            0
        }
        Err(e) => fail(args, &e),
    }
}

fn rewrite_cmd(args: &Args) -> i32 {
    let home = Home::resolve();
    let _ = home.ensure();
    let s = uis_core::settings::load(&home);
    let pack = match pack_arg(args, &home) {
        Ok(p) => p,
        Err(e) => return fail(args, &e),
    };
    let fixed = match context_arg(args, &pack, &s).and_then(|ctx| uis_core::prompt::assemble(&pack, &ctx)) {
        Ok(a) => a,
        Err(e) => return fail(args, &e),
    };
    let note = args.value("--note").unwrap_or("");
    let cli = args.value("--cli").map(str::to_string).unwrap_or_else(|| s.llm.rewriter.clone());
    let clis = uis_core::cli::Clis { home: &home, settings: &s };
    let started = std::time::Instant::now();
    let r = uis_core::cli::rewrite(&pack, &fixed, note, &cli, &clis);
    if let Some(dir) = args.value("--out").map(PathBuf::from) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("fixed.txt"), &r.fixed.text);
        let _ = std::fs::write(dir.join("final.txt"), &r.final_text);
        let _ = std::fs::write(dir.join("diff.json"), serde_json::to_string_pretty(&r.diff).unwrap_or_default());
        let _ = std::fs::write(dir.join("report.json"), serde_json::to_string_pretty(&r).unwrap_or_default());
        if let Some(q) = &r.request {
            let _ = std::fs::write(dir.join("rewrite-request.txt"), q);
        }
    }
    if args.json {
        let mut v = serde_json::to_value(&r).unwrap_or_default();
        v["elapsedMs"] = json!(started.elapsed().as_millis() as u64);
        print_value(&v, true);
    } else {
        println!(
            "cli {}  fallback from {}  calls {}  {} ms",
            r.cli.as_deref().unwrap_or("-"),
            r.fallback_from.as_deref().unwrap_or("-"),
            r.calls.len(),
            started.elapsed().as_millis()
        );
        if let Some(rw) = &r.rewritten {
            for st in rw.sentences.iter().filter(|x| x.status != uis_core::prompt::Status::Same) {
                println!("  {:?} {}: {}", st.status, st.id, st.text);
            }
            for x in &rw.rejected {
                println!("  rejected ({}): {} - {}", x.by, x.part, x.reason);
            }
            println!("  outline {}  camera {}  {}", rw.outline, rw.camera, rw.summary_ko);
        }
        if let Some(f) = &r.failure {
            println!("failed {}", f.code());
        }
    }
    if r.ok {
        0
    } else {
        1
    }
}

fn generate_cmd(args: &Args) -> i32 {
    let Some(input) = args.value("--input").map(PathBuf::from) else {
        eprintln!("uis generate: give --input <capture.png>\n\n{USAGE}");
        return 2;
    };
    let home = Home::resolve();
    let _ = home.ensure();
    let s = uis_core::settings::load(&home);
    let req = uis_core::generate::GenerateRequest {
        input,
        kind: args.value("--kind").unwrap_or("generic").into(),
        pack_id: args.value("--pack").map(str::to_string),
        name: args.value("--name").map(str::to_string),
        note: args.value("--note").unwrap_or("").into(),
        rewriter: args.value("--cli").map(str::to_string),
        refs: args.all("--ref").into_iter().map(PathBuf::from).collect(),
        no_pack_refs: args.has("--no-pack-refs"),
        yaw: num(args, "--yaw"),
        pitch: num(args, "--pitch"),
        candidates: args.value("--candidates").and_then(|v| v.parse().ok()),
        ignore_user_config: args.value("--ignore-user-config").map(|v| v == "true" || v == "1"),
        out_dir: args.value("--out").map(PathBuf::from),
        prompt_only: args.has("--prompt-only"),
        codex_home: None,
    };
    let r = uis_core::generate::generate(&home, &s, &req);
    let mut posted = Vec::new();
    if args.has("--post") {
        for (i, c) in r.candidates.iter().enumerate() {
            if c.ok {
                let job = PathBuf::from(&r.job_dir);
                match uis_core::jobs::post_candidate(&home, &s, &job, i as u32 + 1, &json!({})) {
                    Ok(p) => posted.push(serde_json::to_value(&p).unwrap_or_default()),
                    Err(e) => posted.push(json!({ "candidate": i + 1, "error": e })),
                }
            }
        }
    }
    if args.json && args.has("--post") {
        let mut v = serde_json::to_value(&r).unwrap_or_default();
        v["post"] = Value::Array(posted);
        print_value(&v, true);
    } else if args.json {
        print_value(&serde_json::to_value(&r).unwrap_or_default(), true);
    } else {
        println!("job {}  ok {}  {} ms", r.job_dir, r.ok, r.elapsed_ms);
        for c in &r.candidates {
            println!(
                "  {}  stage {:?}  {}x{} alpha {}  {} ms  {}",
                c.raw.as_deref().unwrap_or("-"),
                c.harvest_stage,
                c.width,
                c.height,
                c.has_alpha,
                c.call.elapsed_ms,
                c.call.code
            );
        }
        if let Some(e) = &r.error {
            println!("error {e}");
        }
        for p in &posted {
            println!(
                "  post cand {}: pass {}  failed {}",
                p["candidate"],
                p["pass"],
                p.pointer("/sizes/0/checks/failed").map(|v| v.to_string()).unwrap_or_else(|| p["error"].to_string())
            );
        }
    }
    if r.ok {
        0
    } else {
        1
    }
}

fn review_cmd(args: &Args) -> i32 {
    let (Some(capture), Some(candidate)) = (args.value("--capture"), args.value("--candidate")) else {
        eprintln!("uis review: give --capture <png> --candidate <png>\n\n{USAGE}");
        return 2;
    };
    let home = Home::resolve();
    let _ = home.ensure();
    let s = uis_core::settings::load(&home);
    let kind = args.value("--kind").unwrap_or("generic");
    let cli = args.value("--cli").unwrap_or(if s.llm.review == "codex" { "codex" } else { "claude" });
    let (capture, candidate) = (std::path::Path::new(capture), std::path::Path::new(candidate));
    let (result, call) = if cli == "codex" {
        uis_core::cli::codex::review(&home, &s, capture, candidate, &uis_core::cli::claude::review_prompt(kind, false))
    } else {
        uis_core::cli::claude::review(&home, &s, capture, candidate, &uis_core::cli::claude::review_prompt(kind, true))
    };
    let value = match &result {
        Ok(v) => json!({ "ok": true, "review": v, "call": call }),
        Err(f) => json!({ "ok": false, "failure": f, "call": call }),
    };
    print_value(&value, true);
    if result.is_ok() {
        0
    } else {
        1
    }
}

fn print_value(value: &Value, json_out: bool) {
    if json_out {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    } else if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            println!("{k}: {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
        }
    }
}

fn run_cmd(args: &Args) -> i32 {
    let Some(project) = args.value("--project").map(PathBuf::from) else {
        eprintln!("uis run: give --project and --prefab

{USAGE}");
        return 2;
    };
    let items = args.all("--prefab");
    if items.is_empty() {
        eprintln!("uis run: give one or more --prefab assets

{USAGE}");
        return 2;
    }
    let home = Home::resolve();
    let settings = uis_core::settings::load(&home);
    let project = uis_core::project::find_project_root(&project).unwrap_or(project);
    let listed = settings.projects.iter().find(|p| PathBuf::from(&p.path).canonicalize().ok() == project.canonicalize().ok());
    let mut post = serde_json::Map::new();
    if let Some(v) = args.value("--outline") {
        post.insert("outline".into(), json!(v));
    }
    if let Some(v) = args.value("--background") {
        post.insert("background".into(), json!(v));
    }
    let sizes = args.value("--sizes").map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect::<Vec<u32>>());
    let req = uis_core::pipeline::RunRequest {
        project,
        items: items.into_iter().map(|a| uis_core::pipeline::RunItemRequest { asset: a, ..Default::default() }).collect(),
        pack_id: args.value("--pack").map(str::to_string).or_else(|| listed.map(|p| p.pack_id.clone())),
        mode: args.value("--mode").map(str::to_string),
        note: args.value("--note").unwrap_or_default().to_string(),
        rewriter: args.value("--cli").map(str::to_string),
        candidates: args.value("--candidates").and_then(|v| v.parse().ok()),
        post: Value::Object(post),
        outline_off_ok: args.has("--outline-off-ok"),
        no_refs: args.has("--no-pack-refs"),
        export_dir: args.value("--to").map(PathBuf::from),
        export_sizes: sizes,
        export_failing: args.has("--export-failing"),
        ..Default::default()
    };
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let report = uis_core::pipeline::run(&home, &settings, &req, cancel, &mut |e| {
        if e.state != "progress" {
            let item = e.item.as_deref().map(|i| format!(" [{}/{}] {i}", e.index, e.total)).unwrap_or_default();
            let msg = if e.message.is_empty() { String::new() } else { format!(" {}", e.message) };
            eprintln!("{:>7.1}s {} {}{item}{msg}", e.elapsed_ms as f64 / 1000.0, e.stage, e.state);
        }
    });
    if args.json {
        print_value(&serde_json::to_value(&report).unwrap_or_default(), true);
    } else {
        for it in &report.items {
            let x = it.export.as_ref().map(|x| if x.ok { format!(" -> {}", x.folder) } else { format!(" export:{}", x.error.clone().unwrap_or_default()) }).unwrap_or_default();
            let why = it.error.as_deref().map(|e| format!(" ({}: {e})", it.stage.as_deref().unwrap_or("-"))).unwrap_or_default();
            println!("{} {} pass={}{why}{x}", it.state, it.asset, it.pass);
        }
    }
    if report.ok {
        0
    } else {
        1
    }
}
