//! Job folders after generation: post-processing a candidate (`post\cand-NN\`) and listing jobs.
//!
//! ```text
//! post\cand-NN\master-1024.png, icon-256.png[, icon-512.png], checks.json, options.json
//! ```

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::checks::{self, CheckReport, GenerateChecks};
use crate::home::Home;
use crate::packs;
use crate::post::{self, raster::Raster, PostOptions, PostReport};
use crate::settings::Settings;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeResult {
    pub size: u32,
    pub path: String,
    pub checks: CheckReport,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostJobReport {
    pub ok: bool,
    pub job_dir: String,
    pub candidate: u32,
    pub kind: String,
    pub pack_id: String,
    pub raw: String,
    pub master: String,
    pub options: PostOptions,
    pub post: PostReport,
    pub generated: GenerateChecks,
    pub sizes: Vec<SizeResult>,
    /// Every size passed its checks
    pub pass: bool,
    pub elapsed_ms: u64,
}

/// What a post-processing run needs when there is no job folder (tuning, the command line).
#[derive(Debug, Clone, Default)]
pub struct PostRequest {
    pub raw: PathBuf,
    pub capture: Option<PathBuf>,
    pub kind: String,
    pub pack_id: String,
    pub out_dir: PathBuf,
    /// Laid over the pack's and the settings' values
    pub overrides: Value,
}

/// The options for a kind: the pack's values, then the settings (outline, background, sizes,
/// saturation), then `overrides`.
pub fn options_for(pack: &packs::Pack, settings: &Settings, kind: &str, overrides: &Value) -> PostOptions {
    let mut o = PostOptions::from_pack(pack, kind);
    o.outline = settings.generate.outline.clone();
    o.background = if settings.generate.background == "flat" { "key".into() } else { "auto".into() };
    o.sizes = settings.export_sizes.iter().copied().filter(|s| *s != 1024).collect();
    o.saturation_boost = settings.generate.saturation_boost;
    o.apply(overrides);
    o.normalize();
    o
}

fn write_json(path: &Path, v: &impl Serialize) -> Result<(), String> {
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap_or_default()).map_err(|e| format!("write:{}:{e}", path.display()))
}

/// Post-processes one raw picture into `req.out_dir`.
pub fn post_run(home: &Home, settings: &Settings, req: &PostRequest) -> Result<PostJobReport, String> {
    let started = std::time::Instant::now();
    let pack = packs::load(home, &req.pack_id).map_err(|e| format!("pack:{e}"))?;
    let opts = options_for(&pack, settings, &req.kind, &req.overrides);
    let raw = Raster::open(&req.raw)?;
    let out = post::process(&raw, &opts)?;
    std::fs::create_dir_all(&req.out_dir).map_err(|e| format!("write:{e}"))?;
    let master = req.out_dir.join("master-1024.png");
    out.master.save(&master)?;
    let ranges = pack.checks_for(&req.kind);
    let color = post::raster::parse_hex(&opts.outline_color).unwrap_or([0.459, 0.216, 0.098]);
    let capture = req.capture.as_ref().filter(|p| p.is_file()).map(|p| Raster::open(p)).transpose()?;
    let generated = checks::check_generated(&out.source, out.report.alpha_source == "native", capture.as_ref(), &ranges, color);
    let mut sizes = Vec::new();
    for (size, img) in &out.icons {
        let path = req.out_dir.join(format!("icon-{size}.png"));
        img.save(&path)?;
        // Checked from the written file (8-bit values, like any icon on disk)
        let report = checks::check_file(&path, &ranges, &opts.outline_color, opts.outline == "code")?;
        sizes.push(SizeResult { size: *size, path: path.display().to_string(), checks: report });
    }
    let pass = sizes.iter().all(|s| s.checks.pass);
    let report = PostJobReport {
        ok: true,
        job_dir: req.out_dir.display().to_string(),
        candidate: 0,
        kind: req.kind.clone(),
        pack_id: pack.id.clone(),
        raw: req.raw.display().to_string(),
        master: master.display().to_string(),
        options: opts.clone(),
        post: out.report,
        generated,
        sizes,
        pass,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };
    write_json(&req.out_dir.join("options.json"), &opts)?;
    write_json(&req.out_dir.join("checks.json"), &report)?;
    Ok(report)
}

/// Post-processes candidate `cand` of a job folder made by `generate` (kind, pack and capture
/// come from `job.json`).
pub fn post_candidate(home: &Home, settings: &Settings, job_dir: &Path, cand: u32, overrides: &Value) -> Result<PostJobReport, String> {
    let job: Value = std::fs::read(job_dir.join("job.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| "noJob".to_string())?;
    let kind = job.get("kind").and_then(Value::as_str).unwrap_or("generic").to_string();
    let pack_id = job.get("packId").and_then(Value::as_str).unwrap_or(packs::DEFAULT_ID).to_string();
    let name = format!("cand-{cand:02}");
    let raw = job_dir.join("gen").join(&name).join("raw.png");
    if !raw.is_file() {
        return Err(format!("noRaw:{name}"));
    }
    let req = PostRequest {
        raw,
        capture: Some(job_dir.join("capture").join("capture.png")),
        kind,
        pack_id,
        out_dir: job_dir.join("post").join(&name),
        overrides: overrides.clone(),
    };
    let mut r = post_run(home, settings, &req)?;
    r.candidate = cand;
    r.job_dir = job_dir.display().to_string();
    write_json(&req.out_dir.join("checks.json"), &r)?;
    Ok(r)
}

/// Candidates of a job that have a raw picture.
pub fn candidates(job_dir: &Path) -> Vec<u32> {
    let mut out: Vec<u32> = std::fs::read_dir(job_dir.join("gen"))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_prefix("cand-")).and_then(|n| n.parse().ok()))
                .filter(|n| job_dir.join("gen").join(format!("cand-{n:02}")).join("raw.png").is_file())
                .collect()
        })
        .unwrap_or_default();
    out.sort_unstable();
    out
}

/// Job folders under the work folder (newest first), with their state.
pub fn list(home: &Home, settings: &Settings, limit: usize) -> Vec<Value> {
    let work = settings.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir()).join("jobs");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&work)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join("job.json").is_file()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs.reverse();
    dirs.into_iter()
        .take(limit)
        .map(|d| {
            let job: Value = std::fs::read(d.join("job.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
            let posted: Vec<u32> = candidates(&d).into_iter().filter(|c| d.join("post").join(format!("cand-{c:02}")).join("checks.json").is_file()).collect();
            json!({
                "dir": d.display().to_string(),
                "kind": job.get("kind"),
                "ok": job.get("ok"),
                "candidates": candidates(&d),
                "posted": posted,
                "exported": d.join("export.json").is_file(),
            })
        })
        .collect()
}
