//! The fidelity review: an LLM (Claude, Codex or both) compares the capture and a generated
//! picture and lists what was added, resized, dropped or re-patterned. Code, not the answer's
//! `pass`, decides from those lists; a failed picture gets a correction sentence for one more try.
//!
//! Judging (each item has a severity, `minor` or `major`):
//! - fails: anything added that the note did not ask for; a part missing (`major`, or two or more);
//!   a part grown, shrunk, thickened, thinned or moved (`major`, or three or more).
//! - notes only: `reshaped` parts and changed patterns.

use serde::Serialize;
use serde_json::Value;
use std::path::Path;

use crate::cli::errors::{FailKind, Failure};
use crate::cli::{self, Call};
use crate::home::Home;
use crate::post::raster::Raster;
use crate::settings::Settings;

const BODY: &str = "A is a raw 3D capture of an object. B was made from A by repainting it in a casual game-icon style. \
Only the rendering may differ: smoother materials, softer shading, gloss, slightly brighter or softer colors, smoother \
edges (an outline is added later, ignore outlines). Everything else must match A: every part, how many there are, where \
they are, how big each one is relative to the whole object, and their patterns.\n\
Compare carefully and list only clear differences you can point to:\n\
- added: things in B that are not in A (for example sparkles, stars, hearts, flowers, leaves, extra props, emblems, \
text, effects), with where they are;\n\
- resized: parts whose size or thickness relative to the whole object clearly differs, or that moved or changed shape;\n\
- missing: parts of A that are not in B;\n\
- changedPattern: patterns, grids, stripes, planks or window panes that were simplified or changed.\n\
Give each item a severity: major when a player would notice it at icon size, minor when it is a small difference.\n\
Ignore brightness, saturation, lighting, gloss and edge smoothness. pass is true only when all four lists are empty. \
Write in English. Answer with the JSON object only.";

/// The instruction for `cli` (`claude` reads `./a.png` and `./b.png`; `codex` gets them as input
/// images). `allowed`: things the note asked to add.
pub fn prompt(cli: &str, allowed: &[String]) -> String {
    let lead = if cli == "claude" {
        "Open ./a.png (picture A) and ./b.png (picture B) with the Read tool. "
    } else {
        "Input image 1 is picture A and input image 2 is picture B. "
    };
    let mut out = format!("{lead}{BODY}");
    if !allowed.is_empty() {
        out.push_str(&format!(
            "\nThe person asked for these additions; they are allowed and are not differences: {}.",
            allowed.join("; ")
        ));
    }
    out
}

/// True for an answer with the four lists.
pub fn is_fidelity_answer(v: &Value) -> bool {
    ["added", "resized", "missing"].iter().all(|k| v.get(*k).is_some_and(Value::is_array))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// `added`, `resized`, `missing` or `pattern`
    pub kind: String,
    pub what: String,
    /// Where (added, missing) or how (resized)
    pub detail: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FidelityReview {
    pub cli: String,
    /// The call answered (a failed call is not a pass)
    pub ok: bool,
    pub pass: bool,
    pub failing: Vec<Finding>,
    pub notes: Vec<Finding>,
    pub summary: String,
    pub answer: Value,
    pub call: Option<Call>,
    pub failure: Option<Failure>,
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 4).map(str::to_lowercase).collect()
}

/// True when `what` is one of the allowed additions (shares a word of four letters or more).
fn allowed_by(what: &str, allowed: &[String]) -> bool {
    let w = words(what);
    allowed.iter().any(|a| words(a).iter().any(|x| w.contains(x)))
}

/// Code's verdict on an answer: (pass, failing findings, notes).
pub fn judge(answer: &Value, allowed: &[String]) -> (bool, Vec<Finding>, Vec<Finding>) {
    let list = |k: &str| answer.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let sev = |v: &Value| if s(v, "severity") == "minor" { "minor".to_string() } else { "major".to_string() };
    let (mut failing, mut notes) = (Vec::new(), Vec::new());
    for a in list("added") {
        let f = Finding { kind: "added".into(), what: s(&a, "what"), detail: s(&a, "where"), severity: sev(&a) };
        if allowed_by(&f.what, allowed) {
            notes.push(f);
        } else {
            failing.push(f);
        }
    }
    let missing: Vec<Finding> = list("missing").iter().map(|m| Finding { kind: "missing".into(), what: s(m, "what"), detail: s(m, "where"), severity: sev(m) }).collect();
    if missing.iter().any(|m| m.severity == "major") || missing.len() >= 2 {
        failing.extend(missing);
    } else {
        notes.extend(missing);
    }
    let resized: Vec<Finding> = list("resized").iter().map(|r| Finding { kind: "resized".into(), what: s(r, "what"), detail: s(r, "change"), severity: sev(r) }).collect();
    let (sized, reshaped): (Vec<Finding>, Vec<Finding>) = resized.into_iter().partition(|r| r.detail != "reshaped");
    if sized.iter().any(|r| r.severity == "major") || sized.len() >= 3 {
        failing.extend(sized);
    } else {
        notes.extend(sized);
    }
    notes.extend(reshaped);
    for p in list("changedPattern") {
        notes.push(Finding { kind: "pattern".into(), what: p.as_str().unwrap_or("").to_string(), detail: String::new(), severity: "minor".into() });
    }
    (failing.is_empty(), failing, notes)
}

/// The capture and the picture on white, 768 px on the long side (what the reviewers see).
fn on_white(src: &Path, dst: &Path) -> Result<(), String> {
    let img = Raster::open(src)?;
    let s = 768.0 / img.w.max(img.h) as f32;
    let (w, h) = (((img.w as f32) * s).round() as usize, ((img.h as f32) * s).round() as usize);
    let pm = crate::post::resize::resample(&img.premultiplied(), img.w, img.h, w.max(1), h.max(1));
    let small = Raster::from_premultiplied(w.max(1), h.max(1), &pm, 0.0);
    let mut out = ::image::RgbImage::new(small.w as u32, small.h as u32);
    for (i, p) in out.pixels_mut().enumerate() {
        let q = small.px[i];
        let c = |v: f32| ((v * q[3] + 1.0 - q[3]).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        p.0 = [c(q[0]), c(q[1]), c(q[2])];
    }
    out.save_with_format(dst, ::image::ImageFormat::Png).map_err(|e| format!("image:{e}"))
}

/// One review by `cli` (`claude` or `codex`).
pub fn review(home: &Home, settings: &Settings, cli_name: &str, capture: &Path, raw: &Path, allowed: &[String]) -> FidelityReview {
    let mut r = FidelityReview { cli: cli_name.into(), ok: false, pass: false, failing: Vec::new(), notes: Vec::new(), summary: String::new(), answer: Value::Null, call: None, failure: None };
    let dir = match cli::temp_dir(home, "fidelity") {
        Ok(d) => d,
        Err(e) => {
            r.failure = Some(Failure::new(FailKind::Spawn, cli_name, e));
            return r;
        }
    };
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    if let Err(e) = on_white(capture, &a).and_then(|_| on_white(raw, &b)) {
        r.failure = Some(Failure::new(FailKind::Spawn, cli_name, e));
        cli::remove_temp(&dir);
        return r;
    }
    let text = prompt(cli_name, allowed);
    let schema = crate::prompt::FIDELITY_SCHEMA;
    let (result, call) = if cli_name == "claude" {
        cli::claude::review_as(home, settings, &a, &b, &text, schema, "fidelity", is_fidelity_answer)
    } else {
        cli::codex::review_as(home, settings, &a, &b, &text, schema, "fidelity", is_fidelity_answer)
    };
    cli::remove_temp(&dir);
    r.call = Some(call);
    match result {
        Ok(v) => {
            let (pass, failing, notes) = judge(&v, allowed);
            r.ok = true;
            r.pass = pass;
            r.failing = failing;
            r.notes = notes;
            r.summary = v.get("summary").and_then(Value::as_str).unwrap_or("").to_string();
            r.answer = v;
        }
        Err(f) => r.failure = Some(f),
    }
    r
}

/// Reviews by the settings' `llm.fidelityReview` (`both` asks Claude, then Codex).
pub fn reviews(home: &Home, settings: &Settings, which: &str, capture: &Path, raw: &Path, allowed: &[String]) -> Vec<FidelityReview> {
    let clis: &[&str] = match which {
        "claude" => &["claude"],
        "codex" => &["codex"],
        "both" => &["claude", "codex"],
        _ => &[],
    };
    clis.iter().map(|c| review(home, settings, c, capture, raw, allowed)).collect()
}

/// The correction sentence for one more try: only the parts the review named, then the pixel
/// checks' general requests. No color names (a fixed color recolors everything).
pub fn correction(reviews: &[FidelityReview], failed_checks: &[String]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for f in reviews.iter().flat_map(|r| r.failing.iter()) {
        let key = format!("{}:{}", f.kind, f.what.to_lowercase());
        if !seen.insert(key) || f.what.trim().is_empty() {
            continue;
        }
        let what = f.what.trim().trim_end_matches('.');
        parts.push(match f.kind.as_str() {
            "added" => format!("do not add {what}{}", if f.detail.is_empty() { String::new() } else { format!(" ({})", f.detail.trim()) }),
            "missing" => format!("keep {what} as in the capture"),
            _ => format!("keep {what} at the same size as in the capture"),
        });
    }
    let has = |id: &str| failed_checks.iter().any(|f| f == id);
    if parts.is_empty() && has("fidelityProfile") {
        parts.push("keep every part at exactly the size and thickness it has in the capture".into());
    }
    if parts.is_empty() && (has("foreignBlobs") || has("outsideParts")) {
        parts.push("add nothing that is not in the capture".into());
    }
    if has("deltaSaturation") || has("deltaValue") {
        parts.push("keep the colors close to the capture, only slightly brighter and cleaner".into());
    }
    if parts.is_empty() {
        return None;
    }
    parts.truncate(6);
    let mut text = format!("Corrections from the previous try: {}.", parts.join("; "));
    if text.chars().count() > 600 {
        text = text.chars().take(597).collect::<String>() + "...";
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn answer(added: Value, resized: Value, missing: Value) -> Value {
        json!({ "pass": false, "added": added, "resized": resized, "missing": missing, "changedPattern": ["window grid simpler"], "summary": "x" })
    }

    #[test]
    fn added_things_fail_unless_the_note_asked() {
        let a = answer(json!([{ "what": "Yellow sparkle", "where": "top right", "severity": "minor" }]), json!([]), json!([]));
        let (pass, failing, notes) = judge(&a, &[]);
        assert!(!pass && failing.len() == 1 && notes.len() == 1, "{failing:?} {notes:?}");
        let b = answer(json!([{ "what": "A layer of snow", "where": "roof", "severity": "major" }]), json!([]), json!([]));
        assert!(judge(&b, &["snow on the roof".into()]).0);
    }

    #[test]
    fn sizes_fail_when_major_or_many_and_reshapes_are_notes() {
        let minor = |w: &str, c: &str| json!({ "what": w, "change": c, "severity": "minor" });
        let a = answer(json!([]), json!([minor("bow", "reshaped"), minor("leaf", "bigger"), minor("stem", "thicker")]), json!([]));
        assert!(judge(&a, &[]).0, "two small size changes are notes");
        let b = answer(json!([]), json!([minor("leaf", "bigger"), minor("stem", "thicker"), minor("cap", "smaller")]), json!([]));
        assert!(!judge(&b, &[]).0, "three fail");
        let c = answer(json!([]), json!([{ "what": "chimney", "change": "bigger", "severity": "major" }]), json!([]));
        assert!(!judge(&c, &[]).0);
        let d = answer(json!([]), json!([]), json!([{ "what": "grass tufts", "where": "base", "severity": "minor" }]));
        assert!(judge(&d, &[]).0, "one small missing part is a note");
    }

    #[test]
    fn corrections_name_only_the_parts() {
        let r = FidelityReview {
            cli: "codex".into(),
            ok: true,
            pass: false,
            failing: vec![
                Finding { kind: "added".into(), what: "Two yellow sparkles".into(), detail: "above the roof".into(), severity: "major".into() },
                Finding { kind: "resized".into(), what: "chimney".into(), detail: "bigger".into(), severity: "major".into() },
            ],
            notes: vec![],
            summary: String::new(),
            answer: Value::Null,
            call: None,
            failure: None,
        };
        let t = correction(&[r], &["deltaValue".into()]).unwrap();
        assert_eq!(
            t,
            "Corrections from the previous try: do not add Two yellow sparkles (above the roof); keep chimney at the same size as in the capture; keep the colors close to the capture, only slightly brighter and cleaner."
        );
        assert!(correction(&[], &[]).is_none());
        assert!(correction(&[], &["fidelityProfile".into()]).unwrap().contains("exactly the size"));
        assert!(prompt("codex", &["snow on the roof".into()]).ends_with("snow on the roof."));
        assert!(prompt("claude", &[]).starts_with("Open ./a.png"));
    }
}
