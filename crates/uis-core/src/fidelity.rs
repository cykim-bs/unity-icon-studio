//! Fidelity of a generated picture to its capture: did the model keep every part where and as big
//! as it was, and add nothing?
//!
//! Both silhouettes (alpha over one half) are put on a 256 x 256 grid: the capture's long side is
//! 86 % of the grid, the generated picture is scaled to the same area, and both centroids go to the
//! middle (loose bits under 2 % of the main part do not move the placement but are compared).
//!
//! - `profile`: mean difference of the row widths and column heights of the two silhouettes, as a
//!   share of the grid. Grown or shrunk parts, thicker bases and changed proportions raise it.
//! - `foreignBlobs` / `foreign`: on a 64 x 64 grid, colorful cells (chroma over 25) of the generated
//!   picture whose color is farther than 28 (CIE76, lightness weighted one half) from every capture
//!   color within 9 % of the grid around them: things painted that the capture does not have there
//!   (sparkles, hearts, new flowers). Pieces of at least 3 cells are counted.
//! - `outsideParts` / `outside`: generated area outside the capture widened by 3 % of the grid
//!   (protrusions, floating pieces); pieces over 0.15 % of the capture area are counted.
//! - `missing`: capture area outside the widened generated silhouette.
//!
//! The limits come from the pack's `checks.strict` and were set on 17 real pictures: 11 made with
//! charms and exaggeration all fail, 6 made with the strict sentences pass (one only "suspect").

use serde::Serialize;
use serde_json::Value;

use crate::checks::{CheckItem, CheckReport};
use crate::post::mask;
use crate::post::raster::{lab, Raster};
use crate::post::resize;

const N: usize = 256;
const G: usize = 64;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FidelityMetrics {
    pub profile: f32,
    pub foreign: f32,
    pub foreign_blobs: usize,
    pub outside: f32,
    pub outside_parts: usize,
    pub missing: f32,
}

/// The picture scaled to `w` x `h` (premultiplied Lanczos3).
fn scaled(img: &Raster, w: usize, h: usize) -> Raster {
    let pm = resize::resample(&img.premultiplied(), img.w, img.h, w.max(1), h.max(1));
    Raster::from_premultiplied(w.max(1), h.max(1), &pm, 0.0)
}

/// Places a picture on the grid (see the module text). `area`: the capture's silhouette area on the
/// grid (None for the capture itself).
fn align(img: &Raster, area: Option<f32>) -> Raster {
    let small_w = 512usize;
    let small_h = ((small_w as f32) * img.h as f32 / img.w as f32).round().max(1.0) as usize;
    let mut small = scaled(img, small_w, small_h);
    let solid: Vec<bool> = small.px.iter().map(|p| p[3] > 0.5).collect();
    let (label, sizes) = mask::components(small.w, small.h, &solid);
    let big = sizes.iter().copied().max().unwrap_or(0).max(1);
    // Specks (under 0.05 % of the main part) are dropped; bits under 2 % do not move the placement
    for (i, l) in label.iter().enumerate() {
        if *l != 0 && (sizes[*l as usize - 1] as f32) < 0.0005 * big as f32 {
            small.px[i][3] = 0.0;
        }
    }
    let main: Vec<bool> = label.iter().map(|l| *l != 0 && sizes[*l as usize - 1] as f32 >= 0.02 * big as f32).collect();
    let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0usize);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for (i, &m) in main.iter().enumerate() {
        if m {
            let (x, y) = (i % small.w, i / small.w);
            sx += x as f64;
            sy += y as f64;
            n += 1;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    let mut out = Raster::new(N, N);
    if n == 0 {
        return out;
    }
    let s = match area {
        None => 0.86 * N as f32 / ((x1 - x0 + 1).max(y1 - y0 + 1) as f32),
        Some(a) => (a / n as f32).sqrt(),
    };
    let (cx, cy) = ((sx / n as f64) as f32, (sy / n as f64) as f32);
    let (w2, h2) = (((small.w as f32) * s).round() as usize, ((small.h as f32) * s).round() as usize);
    let big_img = scaled(&small, w2, h2);
    let ox = (N as f32 / 2.0 - cx * s).round() as isize;
    let oy = (N as f32 / 2.0 - cy * s).round() as isize;
    for y in 0..h2 {
        for x in 0..w2 {
            let (tx, ty) = (ox + x as isize, oy + y as isize);
            if tx >= 0 && ty >= 0 && (tx as usize) < N && (ty as usize) < N {
                out.px[ty as usize * N + tx as usize] = big_img.at(x, y);
            }
        }
    }
    out
}

fn widen(m: &[bool], w: usize, h: usize, r: f32) -> Vec<bool> {
    mask::distance_to(w, h, m).iter().map(|&d| d <= r).collect()
}

/// Measures `generated` against `capture`; also returns a map (capture | generated | marks: grey
/// capture, red outside, blue missing, yellow foreign colors).
pub fn measure(capture: &Raster, generated: &Raster) -> (FidelityMetrics, Raster) {
    let cap = align(capture, None);
    let cm: Vec<bool> = cap.px.iter().map(|p| p[3] > 0.5).collect();
    let area = cm.iter().filter(|&&b| b).count().max(1) as f32;
    let gen = align(generated, Some(area));
    let gm: Vec<bool> = gen.px.iter().map(|p| p[3] > 0.5).collect();
    let r = 0.03 * N as f32;
    let cw = widen(&cm, N, N, r);
    let gw = widen(&gm, N, N, r);
    let out_mask: Vec<bool> = gm.iter().zip(&cw).map(|(g, c)| *g && !*c).collect();
    let miss: Vec<bool> = cm.iter().zip(&gw).map(|(c, g)| *c && !*g).collect();
    let (_, out_sizes) = mask::components(N, N, &out_mask);
    // Foreign colors on the coarse grid
    let cell = |img: &Raster, m: &[bool]| -> (Vec<[f32; 3]>, Vec<bool>) {
        let mut masked = img.clone();
        for (p, &k) in masked.px.iter_mut().zip(m) {
            if !k {
                *p = [0.0; 4];
            }
        }
        let s = scaled(&masked, G, G);
        (s.px.iter().map(|p| lab([p[0], p[1], p[2]])).collect(), s.px.iter().map(|p| p[3] > 0.5).collect())
    };
    let (cl, cmk) = cell(&cap, &cm);
    let (gl, gmk) = cell(&gen, &gm);
    let rad = (0.09 * G as f32).round() as isize;
    let mut foreign = vec![false; G * G];
    for y in 0..G as isize {
        for x in 0..G as isize {
            let i = y as usize * G + x as usize;
            if !gmk[i] {
                continue;
            }
            let g = gl[i];
            if (g[1] * g[1] + g[2] * g[2]).sqrt() <= 25.0 {
                continue;
            }
            let mut best = f32::MAX;
            for dy in -rad..=rad {
                for dx in -rad..=rad {
                    if dx * dx + dy * dy > rad * rad {
                        continue;
                    }
                    let (xx, yy) = ((x + dx).rem_euclid(G as isize) as usize, (y + dy).rem_euclid(G as isize) as usize);
                    let j = yy * G + xx;
                    if !cmk[j] {
                        continue;
                    }
                    let c = cl[j];
                    let d = ((0.5 * (g[0] - c[0])).powi(2) + (g[1] - c[1]).powi(2) + (g[2] - c[2]).powi(2)).sqrt();
                    best = best.min(d);
                }
            }
            foreign[i] = best > 28.0;
        }
    }
    let (_, fsizes) = mask::components(G, G, &foreign);
    let gcells = gmk.iter().filter(|&&b| b).count().max(1) as f32;
    // Row and column profiles
    let profile = {
        let mut d = 0.0f32;
        for y in 0..N {
            let (a, b) = ((0..N).filter(|&x| cm[y * N + x]).count(), (0..N).filter(|&x| gm[y * N + x]).count());
            d += (a as f32 - b as f32).abs() / N as f32;
        }
        let rows = d / N as f32;
        let mut d = 0.0f32;
        for x in 0..N {
            let (a, b) = ((0..N).filter(|&y| cm[y * N + x]).count(), (0..N).filter(|&y| gm[y * N + x]).count());
            d += (a as f32 - b as f32).abs() / N as f32;
        }
        (rows + d / N as f32) / 2.0
    };
    let metrics = FidelityMetrics {
        profile,
        foreign: foreign.iter().filter(|&&b| b).count() as f32 / gcells,
        foreign_blobs: fsizes.iter().filter(|&&s| s >= 3).count(),
        outside: out_mask.iter().filter(|&&b| b).count() as f32 / area,
        outside_parts: out_sizes.iter().filter(|&&s| s as f32 >= 0.0015 * area).count(),
        missing: miss.iter().filter(|&&b| b).count() as f32 / area,
    };
    // Map
    let mut map = Raster::new(N * 3, N);
    let k = N / G;
    for y in 0..N {
        for x in 0..N {
            let on_white = |p: [f32; 4]| [p[0] * p[3] + 1.0 - p[3], p[1] * p[3] + 1.0 - p[3], p[2] * p[3] + 1.0 - p[3], 1.0];
            map.px[y * N * 3 + x] = on_white(cap.at(x, y));
            map.px[y * N * 3 + N + x] = on_white(gen.at(x, y));
            let i = y * N + x;
            let c = if foreign[(y / k) * G + x / k] && gm[i] {
                [1.0, 0.8, 0.0, 1.0]
            } else if out_mask[i] {
                [1.0, 0.2, 0.2, 1.0]
            } else if miss[i] {
                [0.2, 0.4, 1.0, 1.0]
            } else if cm[i] {
                [0.6, 0.6, 0.6, 1.0]
            } else {
                [0.88, 0.88, 0.88, 1.0]
            };
            map.px[y * N * 3 + 2 * N + x] = c;
        }
    }
    (metrics, map)
}

fn range(v: &Value, key: &str) -> (Option<f64>, Option<f64>) {
    match v.get(key) {
        Some(Value::Array(a)) if a.len() == 2 => (a[0].as_f64(), a[1].as_f64()),
        _ => (None, None),
    }
}

/// Fidelity report: hard items (`fidelityProfile`, colors against the capture) decide the pass;
/// `foreignBlobs` and `outsideParts` over their limits make the picture "suspect" (an LLM review
/// looks at it; without one it fails).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FidelityReport {
    pub metrics: FidelityMetrics,
    pub report: CheckReport,
    pub suspect: bool,
    /// Saturation and brightness of the capture and the generated picture (inside, on 256)
    pub capture_color: [f32; 2],
    pub generated_color: [f32; 2],
    pub map: Option<String>,
}

fn item(id: &str, value: f64, min: Option<f64>, max: Option<f64>) -> CheckItem {
    let pass = min.is_none_or(|lo| value >= lo - 1e-6) && max.is_none_or(|hi| value <= hi + 1e-6);
    CheckItem { id: id.into(), group: "fidelity".into(), value: (value * 1000.0).round() / 1000.0, min, max, pass }
}

/// Checks against the pack's `checks.strict` values.
pub fn check(metrics: &FidelityMetrics, cap_color: [f32; 2], gen_color: [f32; 2], limits: &Value) -> (CheckReport, bool) {
    let num = |k: &str, d: f64| limits.get(k).and_then(Value::as_f64).unwrap_or(d);
    let (slo, shi) = range(limits, "deltaSaturation");
    let (vlo, vhi) = range(limits, "deltaValue");
    let items = vec![
        item("fidelityProfile", metrics.profile as f64, None, Some(num("fidelityProfile", 0.012))),
        item("deltaSaturation", (gen_color[0] - cap_color[0]) as f64, slo.or(Some(-0.05)), shi.or(Some(0.15))),
        item("deltaValue", (gen_color[1] - cap_color[1]) as f64, vlo.or(Some(-0.08)), vhi.or(Some(0.15))),
    ];
    let suspect = metrics.foreign_blobs as f64 >= num("foreignBlobs", 2.0) || metrics.outside_parts as f64 >= num("outsideParts", 1.0);
    let mut all = items;
    all.push(CheckItem {
        id: "foreignBlobs".into(),
        group: "fidelity".into(),
        value: metrics.foreign_blobs as f64,
        min: None,
        max: Some(num("foreignBlobs", 2.0) - 1.0),
        pass: (metrics.foreign_blobs as f64) < num("foreignBlobs", 2.0),
    });
    all.push(CheckItem {
        id: "outsideParts".into(),
        group: "fidelity".into(),
        value: metrics.outside_parts as f64,
        min: None,
        max: Some(num("outsideParts", 1.0) - 1.0),
        pass: (metrics.outside_parts as f64) < num("outsideParts", 1.0),
    });
    // The hard items decide; the two suspect items are shown but left to the review
    let hard_failed: Vec<String> = all.iter().take(3).filter(|i| !i.pass).map(|i| i.id.clone()).collect();
    let failed: Vec<String> = all.iter().filter(|i| !i.pass).map(|i| i.id.clone()).collect();
    let report = CheckReport { pass: hard_failed.is_empty(), failed, outline_only: false, items: all, metrics: Default::default() };
    (report, suspect)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A house: a body, a roof and a chimney, drawn with soft edges on `n` x `n`.
    fn house(n: usize, chimney_scale: f32, star: bool) -> Raster {
        let mut r = Raster::new(n, n);
        let f = n as f32 / 400.0;
        for y in 0..n {
            for x in 0..n {
                let (px, py) = (x as f32 / f, y as f32 / f);
                let body = (100.0..=300.0).contains(&px) && (200.0..=330.0).contains(&py);
                let roof = (80.0..=320.0).contains(&px) && (110.0..=200.0).contains(&py) && (px - 200.0).abs() <= (py - 110.0) * 1.33 + 1.0;
                let cw = 22.0 * chimney_scale;
                let chimney = (250.0..=250.0 + cw).contains(&px) && (110.0 - 40.0 * chimney_scale..=170.0).contains(&py);
                let door = (180.0..=220.0).contains(&px) && (270.0..=330.0).contains(&py);
                let sparkle = star && {
                    let (dx, dy) = ((px - 330.0).abs(), (py - 120.0).abs());
                    dx * dy < 30.0 && dx + dy < 26.0
                };
                let c = if sparkle {
                    [1.0, 0.85, 0.1, 1.0]
                } else if door {
                    [0.45, 0.3, 0.2, 1.0]
                } else if chimney {
                    [0.8, 0.4, 0.2, 1.0]
                } else if roof {
                    [0.25, 0.45, 0.85, 1.0]
                } else if body {
                    [0.95, 0.85, 0.7, 1.0]
                } else {
                    [0.0; 4]
                };
                r.px[y * n + x] = c;
            }
        }
        r
    }

    #[test]
    fn same_shape_at_another_size_is_faithful() {
        let (m, map) = measure(&house(400, 1.0, false), &house(600, 1.0, false));
        assert!(m.profile < 0.006, "{m:?}");
        assert_eq!((m.foreign_blobs, m.outside_parts), (0, 0), "{m:?}");
        assert!(m.missing < 0.01);
        assert_eq!((map.w, map.h), (N * 3, N));
    }

    #[test]
    fn an_added_sparkle_is_foreign_and_outside() {
        let (m, _) = measure(&house(400, 1.0, false), &house(500, 1.0, true));
        assert!(m.foreign_blobs >= 1, "{m:?}");
        assert!(m.outside_parts >= 1, "{m:?}");
    }

    #[test]
    fn an_enlarged_part_raises_the_profile() {
        let (same, _) = measure(&house(400, 1.0, false), &house(400, 1.0, false));
        let (big, _) = measure(&house(400, 1.0, false), &house(400, 2.4, false));
        assert!(big.profile > same.profile + 0.006, "same {} big {}", same.profile, big.profile);
        let limits = crate::packs::Pack::builtin().doc["checks"]["strict"].clone();
        let (r, _) = check(&big, [0.4, 0.8], [0.45, 0.85], &limits);
        assert!(r.failed.contains(&"fidelityProfile".to_string()), "{:?} {}", r.failed, big.profile);
        let (r, _) = check(&same, [0.4, 0.8], [0.45, 0.85], &limits);
        assert!(r.pass, "{:?}", r.failed);
        // Colors pushed far from the capture fail
        let (r, _) = check(&same, [0.4, 0.8], [0.7, 0.8], &limits);
        assert_eq!(r.failed, ["deltaSaturation"]);
    }
}
