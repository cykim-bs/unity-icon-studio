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
    /// `strict` or `stylized` (empty without a capture)
    #[serde(default)]
    pub fidelity_mode: String,
    /// Fidelity to the capture (strict: its hard items decide with the sizes' checks)
    pub fidelity: Option<crate::fidelity::FidelityReport>,
    /// Fidelity reviews (filled by the run after post-processing)
    #[serde(default)]
    pub reviews: Vec<crate::review::FidelityReview>,
    /// Every size passed its checks (strict: and the fidelity checks, and the reviews)
    pub pass: bool,
    pub elapsed_ms: u64,
}

impl PostJobReport {
    /// Strict and suspect (foreign colors or outside parts) with no review that answered.
    pub fn needs_review(&self) -> bool {
        self.fidelity_mode == "strict" && self.fidelity.as_ref().is_some_and(|f| f.suspect) && !self.reviews.iter().any(|r| r.ok)
    }

    /// The pass after the reviews: sizes, the fidelity's hard items, every review that answered,
    /// and a suspect picture needs at least one review that answered.
    pub fn settle(&mut self) {
        let sizes = self.sizes.iter().all(|s| s.checks.pass);
        if self.fidelity_mode != "strict" {
            self.pass = sizes;
            return;
        }
        let hard = self.fidelity.as_ref().is_none_or(|f| f.report.pass);
        let answered: Vec<&crate::review::FidelityReview> = self.reviews.iter().filter(|r| r.ok).collect();
        let suspect = self.fidelity.as_ref().is_some_and(|f| f.suspect);
        self.pass = sizes && hard && answered.iter().all(|r| r.pass) && (!suspect || !answered.is_empty());
    }

    /// Items that failed: sizes' checks and the fidelity checks (ids).
    pub fn failed_checks(&self) -> Vec<String> {
        let mut out: Vec<String> = self.sizes.iter().flat_map(|s| s.checks.failed.iter().cloned()).collect();
        if let Some(f) = &self.fidelity {
            out.extend(f.report.failed.iter().cloned());
        }
        out
    }

    /// Writes the report back to `checks.json` (after reviews).
    pub fn save(&self) -> Result<(), String> {
        let dir = Path::new(&self.master).parent().map(Path::to_path_buf).unwrap_or_default();
        write_json(&dir.join("checks.json"), self)
    }
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
    /// `strict`, `stylized` or empty (no fidelity checks)
    pub fidelity: String,
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
    let mut ranges = pack.checks_for(&req.kind);
    let color = post::raster::parse_hex(&opts.outline_color).unwrap_or([0.459, 0.216, 0.098]);
    let capture = req.capture.as_ref().filter(|p| p.is_file()).map(|p| Raster::open(p)).transpose()?;
    let strict = req.fidelity == "strict" && capture.is_some();
    let limits = pack.doc.pointer("/checks/strict").cloned().unwrap_or(Value::Null);
    let mut gen_ranges = ranges.clone();
    if strict {
        // The capture's own colors are the target: absolute color ranges only show values, the
        // fidelity checks compare with the capture; the silhouette must match closely
        if let Some(o) = ranges.as_object_mut() {
            o.remove("saturation");
            o.remove("value");
        }
        if let (Some(o), Some(iou)) = (gen_ranges.as_object_mut(), limits.get("iouMin")) {
            o.insert("iouMin".into(), iou.clone());
            o.remove("saturation");
            o.remove("value");
        }
    }
    let generated = checks::check_generated(&out.source, out.report.alpha_source == "native", capture.as_ref(), &gen_ranges, color);
    let mut sizes = Vec::new();
    for (size, img) in &out.icons {
        let path = req.out_dir.join(format!("icon-{size}.png"));
        img.save(&path)?;
        // Checked from the written file (8-bit values, like any icon on disk)
        let report = checks::check_file(&path, &ranges, &opts.outline_color, opts.outline == "code")?;
        sizes.push(SizeResult { size: *size, path: path.display().to_string(), checks: report });
    }
    let fidelity = match (&capture, req.fidelity.is_empty()) {
        (Some(cap), false) => {
            let (m, map) = crate::fidelity::measure(cap, &out.source);
            let sv = |img: &Raster| {
                let x = checks::measure(img, color);
                [x.saturation, x.value]
            };
            let (cap_c, gen_c) = (sv(cap), sv(&out.source));
            let (report, suspect) = crate::fidelity::check(&m, cap_c, gen_c, &limits);
            let map_path = req.out_dir.join("fidelity-map.png");
            let map_saved = map.save(&map_path).ok().map(|_| map_path.display().to_string());
            Some(crate::fidelity::FidelityReport { metrics: m, report, suspect, capture_color: cap_c, generated_color: gen_c, map: map_saved })
        }
        _ => None,
    };
    let pass = sizes.iter().all(|s| s.checks.pass);
    let mut report = PostJobReport {
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
        fidelity_mode: if capture.is_some() { req.fidelity.clone() } else { String::new() },
        fidelity,
        reviews: Vec::new(),
        pass,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };
    report.settle();
    if strict && report.fidelity.as_ref().is_some_and(|f| f.suspect) {
        // Without a review yet a suspect picture is not a pass
        report.pass = false;
    }
    write_json(&req.out_dir.join("options.json"), &opts)?;
    write_json(&req.out_dir.join("checks.json"), &report)?;
    Ok(report)
}

/// What the app compares for a candidate's fidelity: the job's capture and the candidate's raw
/// picture after its alpha was made (the same preparation as post-processing, with the options
/// the candidate was finished with), against the pack's strict limits. Not the finished icon: its
/// outline and new layout would count as differences.
pub struct CandidateFidelity {
    pub capture: PathBuf,
    pub raw: PathBuf,
    pub report: crate::fidelity::FidelityReport,
}

pub fn candidate_fidelity(home: &Home, settings: &Settings, job_dir: &Path, cand: u32, map: Option<&Path>) -> Result<CandidateFidelity, String> {
    let job: Value = std::fs::read(job_dir.join("job.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| "noJob".to_string())?;
    let kind = job.get("kind").and_then(Value::as_str).unwrap_or("generic").to_string();
    let pack_id = job.get("packId").and_then(Value::as_str).unwrap_or(packs::DEFAULT_ID);
    let pack = packs::load(home, pack_id).map_err(|e| format!("pack:{e}"))?;
    let name = format!("cand-{cand:02}");
    let raw = job_dir.join("gen").join(&name).join("raw.png");
    let capture = job_dir.join("capture").join("capture.png");
    if !raw.is_file() {
        return Err(format!("noRaw:{name}"));
    }
    if !capture.is_file() {
        return Err("noCapture".into());
    }
    // The options the candidate was finished with, else the ones it would get now
    let saved = std::fs::read(job_dir.join("post").join(&name).join("options.json")).ok().and_then(|b| serde_json::from_slice::<PostOptions>(&b).ok());
    let opts = saved.unwrap_or_else(|| options_for(&pack, settings, &kind, &Value::Null));
    let out = post::process(&Raster::open(&raw)?, &opts)?;
    let cap = Raster::open(&capture)?;
    let color = post::raster::parse_hex(&opts.outline_color).unwrap_or([0.459, 0.216, 0.098]);
    let limits = pack.doc.pointer("/checks/strict").cloned().unwrap_or(Value::Null);
    let (m, fmap) = crate::fidelity::measure(&cap, &out.source);
    let sv = |img: &Raster| {
        let x = checks::measure(img, color);
        [x.saturation, x.value]
    };
    let (cap_c, gen_c) = (sv(&cap), sv(&out.source));
    let (report, suspect) = crate::fidelity::check(&m, cap_c, gen_c, &limits);
    let map_saved = map.and_then(|p| fmap.save(p).ok().map(|_| p.display().to_string()));
    Ok(CandidateFidelity {
        capture,
        raw,
        report: crate::fidelity::FidelityReport { metrics: m, report, suspect, capture_color: cap_c, generated_color: gen_c, map: map_saved },
    })
}

/// The job folder and candidate of a finished icon inside one (`<job>\post\cand-NN\…png`).
pub fn job_of_finished_picture(path: &Path) -> Option<(PathBuf, u32)> {
    let cand_dir = path.parent()?;
    let n: u32 = cand_dir.file_name()?.to_str()?.strip_prefix("cand-")?.parse().ok()?;
    let post = cand_dir.parent()?;
    if !post.file_name()?.to_str()?.eq_ignore_ascii_case("post") {
        return None;
    }
    let job = post.parent()?;
    job.join("job.json").is_file().then(|| (job.to_path_buf(), n))
}

/// Post-processes candidate `cand` of a job folder made by `generate` (kind, pack and capture
/// come from `job.json`).
pub fn post_candidate(home: &Home, settings: &Settings, job_dir: &Path, cand: u32, overrides: &Value) -> Result<PostJobReport, String> {
    let job: Value = std::fs::read(job_dir.join("job.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| "noJob".to_string())?;
    let kind = job.get("kind").and_then(Value::as_str).unwrap_or("generic").to_string();
    let fidelity = job.get("fidelity").and_then(Value::as_str).unwrap_or("stylized").to_string();
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
        fidelity,
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
            let best = job.get("best").and_then(Value::as_u64).map(|b| b as u32).or_else(|| posted.first().copied());
            let icon = best
                .map(|b| d.join("post").join(format!("cand-{b:02}")).join("icon-256.png"))
                .filter(|p| p.is_file())
                .map(|p| p.display().to_string());
            let at = d.file_name().map(|n| n.to_string_lossy().chars().take(15).collect::<String>()).unwrap_or_default();
            json!({
                "dir": d.display().to_string(),
                "at": at,
                "name": job.get("name"),
                "kind": job.get("kind"),
                "packId": job.get("packId"),
                "note": job.get("note"),
                "project": job.get("project"),
                "projectName": job.get("projectName"),
                "asset": job.get("asset"),
                "ok": job.get("ok"),
                "state": job.get("state"),
                "pass": job.get("pass"),
                "failedStage": job.get("failedStage"),
                "error": job.get("runError").filter(|v| !v.is_null()).or_else(|| job.get("error")),
                "candidates": candidates(&d),
                "posted": posted,
                "best": best,
                "icon": icon,
                "exported": d.join("export.json").is_file(),
            })
        })
        .collect()
}
