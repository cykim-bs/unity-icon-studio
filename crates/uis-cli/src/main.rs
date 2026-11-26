//! `uis`: the command line of Icon Studio. It uses the same home folder and functions as the app.
//! Commands print JSON with `--json` (a short text summary otherwise).

use serde_json::{json, Value};
use std::path::PathBuf;
use uis_core::home::Home;

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
                "--pack" | "--project" | "--mode" | "--out" => {
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

fn print_value(value: &Value, json_out: bool) {
    if json_out {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    } else if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            println!("{k}: {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
        }
    }
}
