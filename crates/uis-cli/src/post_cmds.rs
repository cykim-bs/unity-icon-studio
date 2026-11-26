//! `uis post`, `uis check`, `uis board`, `uis export` and `uis jobs`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use uis_core::home::Home;

use crate::{fail, print_value, Args, USAGE};

/// `--set key=value` pairs as an object (values parsed as JSON when they are JSON).
fn overrides(args: &Args) -> Value {
    let mut obj = serde_json::Map::new();
    for kv in args.all("--set") {
        if let Some((k, v)) = kv.split_once('=') {
            let val = serde_json::from_str::<Value>(v).unwrap_or_else(|_| Value::String(v.to_string()));
            obj.insert(k.trim().to_string(), val);
        }
    }
    Value::Object(obj)
}

fn print_post(r: &uis_core::jobs::PostJobReport, json_out: bool) {
    if json_out {
        print_value(&serde_json::to_value(r).unwrap_or_default(), true);
        return;
    }
    println!(
        "{}  alpha {}  removed {}  stroke {:.2}px  {} ms",
        r.job_dir, r.post.alpha_source, r.post.removed_parts, r.post.layout.stroke_px, r.elapsed_ms
    );
    let g = &r.generated;
    println!(
        "  generated: pass {}  iou {}  model outline {}  failed {:?}",
        g.report.pass,
        g.iou.map(|v| format!("{v:.3}")).unwrap_or_else(|| "-".into()),
        g.model_outline,
        g.report.failed
    );
    for s in &r.sizes {
        println!(
            "  {}: pass {}  stroke {:.2}px  failed {:?}  {}",
            s.size, s.checks.pass, s.checks.metrics.outline_px, s.checks.failed, s.path
        );
    }
}

pub fn post_cmd(args: &Args) -> i32 {
    let home = Home::resolve();
    let _ = home.ensure();
    let s = uis_core::settings::load(&home);
    let over = overrides(args);
    if let Some(job) = args.value("--job") {
        let job = PathBuf::from(job);
        let cands: Vec<u32> = match args.value("--cand").and_then(|v| v.parse().ok()) {
            Some(c) => vec![c],
            None => uis_core::jobs::candidates(&job),
        };
        if cands.is_empty() {
            return fail(args, "noRaw");
        }
        let mut code = 0;
        for c in cands {
            match uis_core::jobs::post_candidate(&home, &s, &job, c, &over) {
                Ok(r) => print_post(&r, args.json),
                Err(e) => code = fail(args, &e),
            }
        }
        return code;
    }
    let (Some(raw), Some(out)) = (args.value("--raw"), args.value("--out")) else {
        eprintln!("uis post: give --job <folder>, or --raw <png> --out <folder>\n\n{USAGE}");
        return 2;
    };
    let req = uis_core::jobs::PostRequest {
        raw: PathBuf::from(raw),
        capture: args.value("--capture").map(PathBuf::from),
        kind: args.value("--kind").unwrap_or("generic").into(),
        pack_id: args.value("--pack").unwrap_or(uis_core::packs::DEFAULT_ID).into(),
        out_dir: PathBuf::from(out),
        overrides: over,
        fidelity: args.value("--fidelity").unwrap_or("").to_string(),
    };
    match uis_core::jobs::post_run(&home, &s, &req) {
        Ok(r) => {
            print_post(&r, args.json);
            0
        }
        Err(e) => fail(args, &e),
    }
}

pub fn check_cmd(args: &Args) -> i32 {
    let files: Vec<&String> = args.positional.iter().skip(1).collect();
    if files.is_empty() {
        eprintln!("uis check: give one or more PNG files\n\n{USAGE}");
        return 2;
    }
    let home = Home::resolve();
    let pack = match uis_core::packs::load(&home, args.value("--pack").unwrap_or(uis_core::packs::DEFAULT_ID)) {
        Ok(p) => p,
        Err(e) => return fail(args, &format!("pack:{e}")),
    };
    let kind = args.value("--kind").unwrap_or("building");
    let ranges = pack.checks_for(kind);
    let color = pack.doc.pointer("/outline/color").and_then(Value::as_str).unwrap_or("#753719").to_string();
    let with_outline = !args.has("--no-outline");
    let mut out = Vec::new();
    let mut all_pass = true;
    for f in files {
        match uis_core::checks::check_file(Path::new(f), &ranges, &color, with_outline) {
            Ok(r) => {
                all_pass &= r.pass;
                if !args.json {
                    let m = &r.metrics;
                    println!(
                        "{}  {}  stroke {:.2}px {} dE {:.1} uni {:.2} soft {:.2}  S {:.2} V {:.2}  fill {:.2} size {:.2}  failed {:?}",
                        if r.pass { "PASS" } else { "FAIL" },
                        Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        m.outline_px,
                        m.outline_color,
                        m.outline_delta_e,
                        m.outline_uniform,
                        m.edge_softness,
                        m.saturation,
                        m.value,
                        m.fill,
                        m.size,
                        r.failed
                    );
                }
                out.push(json!({ "file": f, "report": r }));
            }
            Err(e) => {
                all_pass = false;
                if !args.json {
                    println!("ERROR {f}: {e}");
                }
                out.push(json!({ "file": f, "error": e }));
            }
        }
    }
    if args.json {
        print_value(&json!({ "kind": kind, "pack": pack.id, "results": out }), true);
    }
    if all_pass {
        0
    } else {
        1
    }
}

/// Rows from `--spec <rows.json>` (`[{label, capture, raw, icon, reference, pass}]`) or from
/// post-processed jobs (`--job <folder>` repeated, `--ref <png>` repeated in the same order).
pub fn board_cmd(args: &Args) -> i32 {
    let Some(out) = args.value("--out") else {
        eprintln!("uis board: give --out <png>\n\n{USAGE}");
        return 2;
    };
    let mut rows = Vec::new();
    if let Some(spec) = args.value("--spec") {
        let v: Value = match std::fs::read(spec).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(v) => v,
            None => return fail(args, "spec"),
        };
        for r in v.as_array().cloned().unwrap_or_default() {
            let p = |k: &str| r.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(PathBuf::from);
            rows.push(uis_core::board::BoardRow {
                label: r.get("label").and_then(Value::as_str).unwrap_or("").into(),
                capture: p("capture"),
                raw: p("raw"),
                icon: p("icon"),
                reference: p("reference"),
                pass: r.get("pass").and_then(Value::as_bool).unwrap_or(true),
            });
        }
    }
    let refs = args.all("--ref");
    for (i, job) in args.all("--job").iter().enumerate() {
        let job = PathBuf::from(job);
        for c in uis_core::jobs::candidates(&job) {
            let dir = job.join("post").join(format!("cand-{c:02}"));
            let checks: Value = std::fs::read(dir.join("checks.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
            let size = checks.pointer("/sizes/0");
            let pass = size.and_then(|s| s.pointer("/checks/pass")).and_then(Value::as_bool).unwrap_or(false);
            let failed: Vec<String> = size
                .and_then(|s| s.pointer("/checks/failed"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let name = job.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let label = format!(
                "{name} {} STROKE {:.1} IOU {:.2} {}",
                if pass { "PASS" } else { "FAIL" },
                size.and_then(|s| s.pointer("/checks/metrics/outlinePx")).and_then(Value::as_f64).unwrap_or(0.0),
                checks.pointer("/generated/iou").and_then(Value::as_f64).unwrap_or(0.0),
                failed.join(" ")
            );
            rows.push(uis_core::board::BoardRow {
                label,
                capture: Some(job.join("capture").join("capture.png")),
                raw: Some(job.join("gen").join(format!("cand-{c:02}")).join("raw.png")),
                icon: size.and_then(|s| s.get("path")).and_then(Value::as_str).map(PathBuf::from),
                reference: refs.get(i).map(PathBuf::from),
                pass,
            });
        }
    }
    if rows.is_empty() {
        return fail(args, "noRows");
    }
    match uis_core::board::board(args.value("--title").unwrap_or("board"), &rows, Path::new(out)) {
        Ok(()) => {
            print_value(&json!({ "board": out, "rows": rows.len() }), args.json);
            0
        }
        Err(e) => fail(args, &e),
    }
}

pub fn export_cmd(args: &Args) -> i32 {
    let (Some(job), Some(to)) = (args.value("--job"), args.value("--to")) else {
        eprintln!("uis export: give --job <folder> --to <folder>\n\n{USAGE}");
        return 2;
    };
    let home = Home::resolve();
    let s = uis_core::settings::load(&home);
    let job = PathBuf::from(job);
    let cand: u32 = args.value("--cand").and_then(|v| v.parse().ok()).unwrap_or(1);
    let dir = job.join("post").join(format!("cand-{cand:02}"));
    let sizes: Vec<u32> = args
        .value("--sizes")
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| s.export_sizes.clone());
    let mut icons = Vec::new();
    for size in sizes {
        let f = if size == 1024 { dir.join("master-1024.png") } else { dir.join(format!("icon-{size}.png")) };
        if !f.is_file() {
            return fail(args, &format!("noIcon:{size}"));
        }
        icons.push((size, f));
    }
    let name = args.value("--name").map(str::to_string).unwrap_or_else(|| {
        let job_doc: Value = std::fs::read(job.join("job.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        job_doc
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| job.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "icon".into()))
    });
    let rule = args.value("--rule").map(str::to_string).unwrap_or_else(|| s.export_name.clone());
    let r = uis_core::export::export(&icons, Path::new(to), &rule, &name, &s, Some(&job));
    print_value(&serde_json::to_value(&r).unwrap_or_default(), true);
    if r.ok {
        0
    } else {
        1
    }
}

/// `uis fidelity <capture.png> <generated.png> [--map <png>]`: how faithful a picture is.
pub fn fidelity_cmd(args: &Args) -> i32 {
    let (Some(cap), Some(gen)) = (args.positional.get(1), args.positional.get(2)) else {
        eprintln!("uis fidelity: give a capture and a generated picture\n\n{USAGE}");
        return 2;
    };
    let open = |p: &str| uis_core::post::raster::Raster::open(Path::new(p));
    let (cap, gen) = match (open(cap), open(gen)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return fail(args, &e),
    };
    let (m, map) = uis_core::fidelity::measure(&cap, &gen);
    let limits = uis_core::packs::Pack::builtin().doc["checks"]["strict"].clone();
    let c = |img: &uis_core::post::raster::Raster| {
        let x = uis_core::checks::measure(img, [0.459, 0.216, 0.098]);
        [x.saturation, x.value]
    };
    let (report, suspect) = uis_core::fidelity::check(&m, c(&cap), c(&gen), &limits);
    if let Some(p) = args.value("--map") {
        if let Err(e) = map.save(Path::new(p)) {
            return fail(args, &e);
        }
    }
    let reviews = match args.value("--review") {
        Some(which) => {
            let home = Home::resolve();
            let _ = home.ensure();
            let s = uis_core::settings::load(&home);
            let allowed = args.all("--allow");
            uis_core::review::reviews(&home, &s, which, Path::new(&args.positional[1]), Path::new(&args.positional[2]), &allowed)
        }
        None => Vec::new(),
    };
    if !args.json {
        for r in &reviews {
            let ms = r.call.as_ref().map(|c| c.elapsed_ms).unwrap_or(0);
            let list = |v: &[uis_core::review::Finding]| v.iter().map(|f| format!("{} {} [{}]", f.kind, f.what, f.severity)).collect::<Vec<_>>().join("; ");
            println!(
                "review {}: {} {} ms  failing: {}  notes: {}{}",
                r.cli,
                if !r.ok { "ERROR" } else if r.pass { "pass" } else { "fail" },
                ms,
                list(&r.failing),
                list(&r.notes),
                r.failure.as_ref().map(|f| format!("  ({})", f.code())).unwrap_or_default()
            );
        }
    }
    if args.json {
        print_value(&json!({ "metrics": m, "pass": report.pass, "suspect": suspect, "failed": report.failed, "items": report.items, "reviews": reviews }), true);
    } else {
        println!(
            "{}{}  profile {:.4}  foreignBlobs {}  foreign {:.3}  outsideParts {}  outside {:.3}  missing {:.3}  failed {:?}",
            if report.pass { "PASS" } else { "FAIL" },
            if suspect { " (suspect)" } else { "" },
            m.profile,
            m.foreign_blobs,
            m.foreign,
            m.outside_parts,
            m.outside,
            m.missing,
            report.failed
        );
    }
    if report.pass {
        0
    } else {
        1
    }
}

pub fn jobs_cmd(args: &Args) -> i32 {
    let home = Home::resolve();
    let s = uis_core::settings::load(&home);
    let limit = args.value("--limit").and_then(|v| v.parse().ok()).unwrap_or(50);
    print_value(&Value::Array(uis_core::jobs::list(&home, &s, limit)), true);
    0
}
