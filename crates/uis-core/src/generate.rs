//! From a capture to raw generated pictures: the job folder, the capture on white, the fixed
//! prompt, the rewrite when there is a note, and one Codex run per candidate. Post-processing comes
//! after (a later stage reads `gen\cand-NN\raw.png`).
//!
//! Job folder (`job.json` schema `unity-icon-studio.job/1`):
//!
//! ```text
//! capture\capture.png, capture-onwhite.png, ref-NN.png
//! prompt\fixed.txt, rewrite-request.txt, rewrite-response.json, final.txt, diff.json
//! gen\cand-NN\raw.png, events.jsonl, last-message.txt, stderr.txt, prompt.txt, meta.json
//! ```

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::cli::codex::{self, ImageRequest, ImageResult};
use crate::cli::{self, Clis, RewriteReport, TextLlm};
use crate::home::Home;
use crate::packs::{self, Pack};
use crate::prompt::{self, PromptContext};
use crate::settings::Settings;

pub const JOB_SCHEMA: &str = "unity-icon-studio.job/1";

#[derive(Debug, Clone, Default)]
pub struct GenerateRequest {
    /// The capture (transparent PNG)
    pub input: PathBuf,
    pub kind: String,
    pub pack_id: Option<String>,
    /// The prefab's name: style references whose file name holds it are left out
    pub name: Option<String>,
    pub note: String,
    /// `claude` or `codex` (None: settings `llm.rewriter`)
    pub rewriter: Option<String>,
    /// Extra style references (the pack's come first when `generate.styleRefs` is on)
    pub refs: Vec<PathBuf>,
    pub no_pack_refs: bool,
    pub yaw: Option<f64>,
    pub pitch: Option<f64>,
    /// None: settings `generate.candidates`
    pub candidates: Option<u32>,
    /// None: settings `codex.ignoreUserConfig`
    pub ignore_user_config: Option<bool>,
    /// The job folder (None: `<work>\jobs\<run id>-<name>`)
    pub out_dir: Option<PathBuf>,
    /// Stop after the prompt (no picture)
    pub prompt_only: bool,
    /// Tests: a made-up CODEX_HOME for the child
    pub codex_home: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateReport {
    pub ok: bool,
    pub job_dir: String,
    pub kind: String,
    pub pack_id: String,
    pub template_version: String,
    pub refs: Vec<String>,
    pub fixed: String,
    pub final_text: String,
    pub rewrite: Option<RewriteReport>,
    pub candidates: Vec<ImageResult>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}

/// `src` (transparent) over white, written as an opaque PNG.
pub fn on_white(src: &Path, dst: &Path) -> Result<(), String> {
    let img = ::image::open(src).map_err(|e| format!("image:{e}"))?.to_rgba8();
    let mut out = ::image::RgbImage::new(img.width(), img.height());
    for (x, y, p) in img.enumerate_pixels() {
        let a = p.0[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        out.put_pixel(x, y, ::image::Rgb([mix(p.0[0]), mix(p.0[1]), mix(p.0[2])]));
    }
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("write:{e}"))?;
    }
    out.save_with_format(dst, ::image::ImageFormat::Png).map_err(|e| format!("image:{e}"))
}

/// A style reference path of a pack: relative, no `..`, and still inside the pack folder once
/// links are resolved. A pack is a file people pass around; its pictures are sent to Codex, so a
/// path must not reach other files on the PC.
fn pack_ref_path(dir: &Path, rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    let plain = p.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
    if rel.trim().is_empty() || p.is_absolute() || !plain {
        return None;
    }
    let full = dir.join(p);
    let (real, root) = (std::fs::canonicalize(&full).ok()?, std::fs::canonicalize(dir).ok()?);
    (real.starts_with(&root) && real.is_file()).then_some(full)
}

/// The pack's style references (paths from the pack folder, kept inside it), without those named
/// like `name`.
pub fn pack_refs(pack: &Pack, name: Option<&str>) -> Vec<PathBuf> {
    let Some(dir) = &pack.dir else { return Vec::new() };
    let skip = name.map(|n| n.to_lowercase()).filter(|n| !n.is_empty());
    pack.doc
        .get("styleRefs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().or_else(|| v.get("path").and_then(Value::as_str)))
                .filter_map(|p| pack_ref_path(dir, p))
                .filter(|p| {
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
                    skip.as_ref().is_none_or(|n| !stem.contains(n.as_str()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("write:{e}"))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("write:{}:{e}", path.display()))
}

fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    t.chars().take(60).collect()
}

/// The prompt context for a request: the pack's angles for the kind, then the request's.
pub fn context(pack: &Pack, settings: &Settings, kind: &str, refs: usize, yaw: Option<f64>, pitch: Option<f64>) -> PromptContext {
    let mut ctx = PromptContext::for_kind(pack, kind);
    if let Some(y) = yaw {
        ctx.yaw = y;
    }
    if let Some(p) = pitch {
        ctx.pitch = p;
    }
    ctx.refs = refs;
    ctx.outline = settings.generate.outline.clone();
    ctx.background = settings.generate.background.clone();
    ctx.key_color = if settings.generate.key_color.starts_with('#') { settings.generate.key_color.clone() } else { "#FF00FF".into() };
    ctx
}

/// Runs the whole step with the real CLIs.
pub fn generate(home: &Home, settings: &Settings, req: &GenerateRequest) -> GenerateReport {
    let clis = Clis { home, settings };
    generate_with(home, settings, req, &clis)
}

pub fn generate_with(home: &Home, settings: &Settings, req: &GenerateRequest, llm: &dyn TextLlm) -> GenerateReport {
    let started = std::time::Instant::now();
    let pack_id = req.pack_id.clone().unwrap_or_else(|| packs::DEFAULT_ID.into());
    let name = req
        .name
        .clone()
        .unwrap_or_else(|| req.input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "item".into()));
    let job_dir = req.out_dir.clone().unwrap_or_else(|| {
        let work = settings.work_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| home.default_work_dir());
        work.join("jobs").join(format!("{}-{}", crate::capture::new_run_id(), safe_name(&name)))
    });
    let mut report = GenerateReport {
        ok: false,
        job_dir: job_dir.display().to_string(),
        kind: req.kind.clone(),
        pack_id: pack_id.clone(),
        template_version: String::new(),
        refs: Vec::new(),
        fixed: String::new(),
        final_text: String::new(),
        rewrite: None,
        candidates: Vec::new(),
        error: None,
        elapsed_ms: 0,
    };
    let result = run(home, settings, req, llm, &pack_id, &name, &job_dir, &mut report);
    if let Err(e) = result {
        report.error = Some(e);
    }
    report.ok = report.error.is_none() && (req.prompt_only || (!report.candidates.is_empty() && report.candidates.iter().all(|c| c.ok)));
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    let _ = write_job(&job_dir, req, &report);
    report
}

#[allow(clippy::too_many_arguments)]
fn run(
    home: &Home,
    settings: &Settings,
    req: &GenerateRequest,
    llm: &dyn TextLlm,
    pack_id: &str,
    name: &str,
    job_dir: &Path,
    report: &mut GenerateReport,
) -> Result<(), String> {
    if !packs::KINDS.contains(&req.kind.as_str()) {
        return Err(format!("badKind:{}", req.kind));
    }
    let pack = packs::load(home, pack_id).map_err(|e| format!("pack:{e}"))?;
    std::fs::create_dir_all(job_dir).map_err(|e| format!("write:{e}"))?;
    let cap_dir = job_dir.join("capture");
    std::fs::create_dir_all(&cap_dir).map_err(|e| format!("write:{e}"))?;
    let capture = cap_dir.join("capture.png");
    if crate::capture::absolute(&req.input) != crate::capture::absolute(&capture) {
        std::fs::copy(&req.input, &capture).map_err(|e| format!("input:{e}"))?;
    }
    let onwhite = cap_dir.join("capture-onwhite.png");
    on_white(&capture, &onwhite)?;

    let mut refs: Vec<PathBuf> = Vec::new();
    if settings.generate.style_refs && !req.no_pack_refs {
        refs.extend(pack_refs(&pack, Some(name)));
    }
    refs.extend(req.refs.iter().cloned());
    let mut inputs = vec![onwhite.clone()];
    for (i, r) in refs.iter().enumerate() {
        let dst = cap_dir.join(format!("ref-{:02}.png", i + 1));
        on_white(r, &dst).map_err(|e| format!("ref:{}:{e}", r.display()))?;
        inputs.push(dst);
        report.refs.push(r.display().to_string());
    }

    let ctx = context(&pack, settings, &req.kind, refs.len(), req.yaw, req.pitch);
    let fixed = prompt::assemble(&pack, &ctx)?;
    report.template_version = fixed.template_version.clone();
    report.fixed = fixed.text.clone();
    let pdir = job_dir.join("prompt");
    write(&pdir.join("fixed.txt"), fixed.text.as_bytes())?;

    let rewriter = req.rewriter.clone().unwrap_or_else(|| settings.llm.rewriter.clone());
    let rw = cli::rewrite(&pack, &fixed, &req.note, &rewriter, llm);
    if let Some(r) = &rw.request {
        write(&pdir.join("rewrite-request.txt"), r.as_bytes())?;
    }
    if let Some(r) = &rw.response {
        write(&pdir.join("rewrite-response.json"), serde_json::to_string_pretty(r).unwrap_or_default().as_bytes())?;
    }
    write(&pdir.join("diff.json"), serde_json::to_string_pretty(&rw.diff).unwrap_or_default().as_bytes())?;
    let ok = rw.ok;
    let failure = rw.failure.clone();
    report.final_text = rw.final_text.clone();
    report.rewrite = Some(rw);
    if !ok {
        return Err(format!("rewrite:{}", failure.map(|f| f.code()).unwrap_or_default()));
    }
    write(&pdir.join("final.txt"), report.final_text.as_bytes())?;
    if req.prompt_only {
        return Ok(());
    }

    let text = prompt::generator_text(&pack, &ctx.background, &report.final_text);
    let n = req.candidates.unwrap_or(settings.generate.candidates).clamp(1, 4);
    for i in 1..=n {
        let image = ImageRequest {
            inputs: inputs.clone(),
            prompt: text.clone(),
            dest: job_dir.join("gen").join(format!("cand-{i:02}")),
            codex_home: req.codex_home.clone(),
            ignore_user_config: req.ignore_user_config,
        };
        let r = codex::generate_image(home, settings, &image);
        let stop = r.failure.as_ref().is_some_and(|f| {
            matches!(f.kind, crate::cli::errors::FailKind::Limit | crate::cli::errors::FailKind::Auth | crate::cli::errors::FailKind::NotFound)
        });
        report.candidates.push(r);
        if stop {
            break;
        }
    }
    Ok(())
}

fn write_job(job_dir: &Path, req: &GenerateRequest, report: &GenerateReport) -> Result<(), String> {
    let rw = report.rewrite.as_ref();
    let doc = json!({
        "schema": JOB_SCHEMA,
        "input": req.input.display().to_string(),
        "kind": report.kind,
        "packId": report.pack_id,
        "templateVersion": report.template_version,
        "note": req.note,
        "rewriter": rw.and_then(|r| r.cli.clone()),
        "rewriteFallbackFrom": rw.and_then(|r| r.fallback_from.clone()),
        "rewriteCalls": rw.map(|r| r.calls.clone()).unwrap_or_default(),
        "refs": report.refs,
        "candidates": report.candidates.iter().map(|c| json!({
            "ok": c.ok, "raw": c.raw, "harvestStage": c.harvest_stage, "threadId": c.thread_id,
            "elapsedMs": c.call.elapsed_ms, "code": c.call.code, "ignoreUserConfig": c.ignore_user_config,
            "width": c.width, "height": c.height, "hasAlpha": c.has_alpha,
        })).collect::<Vec<_>>(),
        "ok": report.ok,
        "error": report.error,
        "elapsedMs": report.elapsed_ms,
    });
    write(&job_dir.join("job.json"), serde_json::to_string_pretty(&doc).unwrap_or_default().as_bytes())
}

/// How many pictures the tool asked Codex for today (from `llm.log`).
pub fn images_today(home: &Home) -> usize {
    let today = crate::capture::new_run_id()[..8].to_string();
    std::fs::read_to_string(home.logs_dir().join("llm.log"))
        .map(|t| t.lines().filter(|l| l.starts_with(&today) && l.split('\t').nth(2) == Some("image")).count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_white_blends_alpha() {
        let d = std::env::temp_dir().join(format!("uis-onwhite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut img = ::image::RgbaImage::new(4, 1);
        img.put_pixel(0, 0, ::image::Rgba([0, 0, 0, 0]));
        img.put_pixel(1, 0, ::image::Rgba([0, 0, 0, 255]));
        img.put_pixel(2, 0, ::image::Rgba([255, 0, 0, 128]));
        img.put_pixel(3, 0, ::image::Rgba([10, 20, 30, 255]));
        img.save(d.join("a.png")).unwrap();
        on_white(&d.join("a.png"), &d.join("b.png")).unwrap();
        let b = ::image::open(d.join("b.png")).unwrap();
        assert!(!b.color().has_alpha());
        let b = b.to_rgb8();
        assert_eq!(b.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(b.get_pixel(1, 0).0, [0, 0, 0]);
        assert_eq!(b.get_pixel(2, 0).0, [255, 127, 127]);
        assert_eq!(b.get_pixel(3, 0).0, [10, 20, 30]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pack_refs_skip_the_items_own_icon() {
        let d = std::env::temp_dir().join(format!("uis-refs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("refs")).unwrap();
        for n in ["icon_bakery.png", "icon_mill.png"] {
            std::fs::write(d.join("refs").join(n), b"x").unwrap();
        }
        let mut pack = Pack::builtin();
        pack.dir = Some(d.clone());
        std::fs::write(d.join("outside.png"), b"x").unwrap();
        let abs = d.join("refs/icon_mill.png").display().to_string();
        pack.doc["styleRefs"] = json!([
            "refs/icon_bakery.png", { "path": "refs/icon_mill.png" }, "refs/missing.png",
            "../outside.png", "refs/../../outside.png", abs, ""
        ]);
        assert_eq!(pack_refs(&pack, None).len(), 2, "only the two inside the pack folder");
        let r = pack_refs(&pack, Some("Bakery"));
        assert_eq!(r, [d.join("refs/icon_mill.png")]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
