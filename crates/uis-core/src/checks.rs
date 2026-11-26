//! Pixel checks of icons. Every measure is taken on a 256 x 256 picture (others are reduced to it
//! first), so the pixel ranges of a style pack mean the same thing at every output size.
//!
//! - Distances from the edge use alternating 3x3 / plus-shaped erosion (an octagon), like the
//!   measurements the default ranges came from.
//! - Stroke color: the most frequent color of the outer three bands; uniformity is the share of
//!   that band within 3 levels of it.
//! - Stroke width: the share of stroke pixels (dark, luminance under 0.35, and within 12 levels of
//!   the stroke color) in each one-pixel band from the edge inwards, added up until a band is
//!   under 12 %. `darkBandPx` counts every dark pixel instead (a dark object edge adds to it).
//! - Inside colors: pixels at least 8 px from the edge.

use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

use crate::post::mask;
use crate::post::raster::{delta_e, hex, parse_hex, Raster};
use crate::post::resize;

pub const SIDE: usize = 256;

/// Everything measured on one picture.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
    pub corner_alpha_max: u8,
    /// Box of alpha over 8 (left, top, right, bottom), on 256
    pub bbox: [usize; 4],
    pub margin_min_px: f32,
    pub margins: [usize; 4],
    pub size: f32,
    pub fill: f32,
    pub center_x_px: f32,
    pub largest_part: f32,
    pub parts: usize,
    pub outline_px: f32,
    /// Every dark pixel per band, the stroke's color or not
    pub dark_band_px: f32,
    /// Like `outline_px` for near-white pixels (a sticker border drawn by the model)
    pub light_rim_px: f32,
    pub outline_color: String,
    pub outline_delta_e: f32,
    pub outline_uniform: f32,
    pub edge_softness: f32,
    pub saturation: f32,
    pub value: f32,
    pub inner_dark: f32,
    pub highlight: f32,
}

fn erode(m: &[bool], w: usize, h: usize, square: bool) -> Vec<bool> {
    let get = |x: isize, y: isize| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && m[y as usize * w + x as usize];
    let mut out = vec![false; w * h];
    for y in 0..h as isize {
        for x in 0..w as isize {
            if !get(x, y) {
                continue;
            }
            out[y as usize * w + x as usize] = if square {
                (-1..=1).all(|dy| (-1..=1).all(|dx| get(x + dx, y + dy)))
            } else {
                get(x - 1, y) && get(x + 1, y) && get(x, y - 1) && get(x, y + 1)
            };
        }
    }
    out
}

/// Band number (1 = outermost pixels) of each pixel of `m`; 0 outside, `maxd + 1` deeper.
pub fn bands(m: &[bool], w: usize, h: usize, maxd: u32) -> Vec<u32> {
    let mut d = vec![0u32; w * h];
    let mut cur = m.to_vec();
    let mut n = 0u32;
    while cur.iter().any(|&c| c) && n < maxd {
        n += 1;
        for (i, &c) in cur.iter().enumerate() {
            if c {
                d[i] = n;
            }
        }
        cur = erode(&cur, w, h, n % 2 == 1);
    }
    for (i, &c) in cur.iter().enumerate() {
        if c {
            d[i] = maxd + 1;
        }
    }
    d
}

fn lum(p: [f32; 4]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}

fn sv(p: [f32; 4]) -> (f32, f32) {
    let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
    (if mx > 0.0 { (mx - mn) / mx } else { 0.0 }, mx)
}

/// The picture at 256 x 256 (premultiplied Lanczos3; non-square pictures are centered on a
/// square first).
pub fn to_256(img: &Raster) -> Raster {
    if img.w == SIDE && img.h == SIDE {
        return img.clone();
    }
    let side = img.w.max(img.h);
    let mut sq = Raster::new(side, side);
    let (ox, oy) = ((side - img.w) / 2, (side - img.h) / 2);
    for y in 0..img.h {
        for x in 0..img.w {
            sq.px[(y + oy) * side + x + ox] = img.at(x, y);
        }
    }
    let pm = resize::resample(&sq.premultiplied(), side, side, SIDE, SIDE);
    Raster::from_premultiplied(SIDE, SIDE, &pm, 0.5 / 255.0)
}

/// Measures a picture (reduced to 256 first). `outline` is the expected stroke color.
pub fn measure(img: &Raster, outline: [f32; 3]) -> Metrics {
    let src = to_256(img);
    let (w, h) = (src.w, src.h);
    let a8: Vec<u8> = src.px.iter().map(|p| (p[3] * 255.0 + 0.5) as u8).collect();
    let rgb8: Vec<[u8; 3]> = src.px.iter().map(|p| [0, 1, 2].map(|c| (p[c] * 255.0 + 0.5) as u8)).collect();
    let mut m = Metrics { width: img.w as u32, height: img.h as u32, has_alpha: true, ..Default::default() };
    let b = 8usize.min(w);
    for (x0, y0) in [(0, 0), (w - b, 0), (0, h - b), (w - b, h - b)] {
        for y in y0..y0 + b {
            for x in x0..x0 + b {
                m.corner_alpha_max = m.corner_alpha_max.max(a8[y * w + x]);
            }
        }
    }
    let Some((x0, y0, x1, y1)) = crate::post::raster::bbox_of(w, h, |i| a8[i] > 8) else {
        return m;
    };
    m.bbox = [x0, y0, x1, y1];
    m.margins = [x0, w - 1 - x1, y0, h - 1 - y1];
    m.margin_min_px = *m.margins.iter().min().unwrap_or(&0) as f32;
    m.size = (x1 - x0 + 1).max(y1 - y0 + 1) as f32 / w as f32;
    m.fill = a8.iter().filter(|&&a| a > 8).count() as f32 / (w * h) as f32;
    m.center_x_px = (x0 + x1) as f32 / 2.0 - (w as f32 - 1.0) / 2.0;

    let solid: Vec<bool> = a8.iter().map(|&a| a > 127).collect();
    let d = bands(&solid, w, h, 40);
    let dark = |i: usize| lum(src.px[i]) < 0.35;
    // Stroke color and uniformity
    let band: Vec<[u8; 3]> = (0..w * h).filter(|&i| (1..=3).contains(&d[i])).map(|i| rgb8[i]).collect();
    let mut mode = [0u8; 3];
    if !band.is_empty() {
        let mut counts = std::collections::HashMap::new();
        for c in &band {
            *counts.entry(*c).or_insert(0usize) += 1;
        }
        mode = counts.iter().max_by_key(|(c, n)| (**n, std::cmp::Reverse(**c))).map(|(c, _)| *c).unwrap_or([0; 3]);
        let mf = mode.map(|v| v as f32 / 255.0);
        m.outline_color = hex(mf);
        m.outline_delta_e = delta_e(mf, outline);
        let near = band.iter().filter(|c| (0..3).all(|k| (c[k] as i32 - mode[k] as i32).abs() <= 3)).count();
        m.outline_uniform = near as f32 / band.len() as f32;
    }
    // Stroke width: dark pixels of the stroke's color (all dark pixels for darkBandPx)
    let stroke_px = |i: usize| dark(i) && (0..3).all(|k| (rgb8[i][k] as i32 - mode[k] as i32).abs() <= 12);
    let band_sum = |pick: &dyn Fn(usize) -> bool| {
        let mut th = 0.0f32;
        for band in 1..=10u32 {
            let sel: Vec<usize> = (0..w * h).filter(|&i| d[i] == band).collect();
            if sel.is_empty() {
                break;
            }
            let frac = sel.iter().filter(|&&i| pick(i)).count() as f32 / sel.len() as f32;
            if frac < 0.12 {
                break;
            }
            th += frac;
        }
        th
    };
    m.outline_px = band_sum(&stroke_px);
    m.dark_band_px = band_sum(&dark);
    // A white sticker border: bands that are mostly near-white
    let mut rim = 0.0f32;
    for band in 1..=10u32 {
        let sel: Vec<usize> = (0..w * h).filter(|&i| d[i] == band).collect();
        if sel.is_empty() {
            break;
        }
        let frac = sel.iter().filter(|&&i| {
            let (s, v) = sv(src.px[i]);
            v > 0.9 && s < 0.15
        }).count() as f32 / sel.len() as f32;
        if frac < 0.5 {
            break;
        }
        rim += frac;
    }
    m.light_rim_px = rim;
    // Faint pixels just outside the edge, per edge pixel
    let outside: Vec<bool> = solid.iter().map(|s| !s).collect();
    let dout = bands(&outside, w, h, 3);
    let edge = d.iter().filter(|&&v| v == 1).count().max(1);
    let halo = (0..w * h).filter(|&i| a8[i] > 8 && a8[i] <= 127 && dout[i] >= 1 && dout[i] <= 2).count();
    m.edge_softness = halo as f32 / edge as f32;
    // Parts
    let (_, sizes) = mask::components(w, h, &solid);
    let total: usize = sizes.iter().sum();
    m.parts = sizes.len();
    m.largest_part = sizes.iter().max().map(|&s| s as f32 / total.max(1) as f32).unwrap_or(0.0);
    // Inside colors
    let mut inner: Vec<usize> = (0..w * h).filter(|&i| d[i] >= 8).collect();
    if inner.len() < 200 {
        let dmax = d.iter().max().copied().unwrap_or(0);
        let lim = (dmax / 2).max(1);
        inner = (0..w * h).filter(|&i| d[i] >= lim).collect();
    }
    if !inner.is_empty() {
        let n = inner.len() as f32;
        let (mut s, mut v, mut dk, mut hl) = (0.0f32, 0.0f32, 0usize, 0usize);
        for &i in &inner {
            // The 8-bit values, like the reference measurements
            let p = [rgb8[i][0] as f32 / 255.0, rgb8[i][1] as f32 / 255.0, rgb8[i][2] as f32 / 255.0, 1.0];
            let (si, vi) = sv(p);
            s += si;
            v += vi;
            if lum(p) < 0.25 {
                dk += 1;
            }
            if vi > 0.93 && si < 0.18 {
                hl += 1;
            }
        }
        m.saturation = s / n;
        m.value = v / n;
        m.inner_dark = dk as f32 / n;
        m.highlight = hl as f32 / n;
    }
    m
}

/// One check: its value and range.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    /// `frame`, `outline`, `color` or `generate`
    pub group: String,
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub pass: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub pass: bool,
    pub failed: Vec<String>,
    /// Failed, and every failed item is a stroke item
    pub outline_only: bool,
    pub items: Vec<CheckItem>,
    pub metrics: Metrics,
}

fn range(v: &Value, key: &str) -> (Option<f64>, Option<f64>) {
    match v.get(key) {
        Some(Value::Array(a)) if a.len() == 2 => (a[0].as_f64(), a[1].as_f64()),
        _ => (None, None),
    }
}

fn item(id: &str, group: &str, value: f64, min: Option<f64>, max: Option<f64>) -> CheckItem {
    let pass = min.is_none_or(|lo| value >= lo - 1e-6) && max.is_none_or(|hi| value <= hi + 1e-6);
    let r = |x: f64| (x * 1000.0).round() / 1000.0;
    CheckItem { id: id.into(), group: group.into(), value: r(value), min, max, pass }
}

fn finish(items: Vec<CheckItem>, metrics: Metrics) -> CheckReport {
    let failed: Vec<String> = items.iter().filter(|i| !i.pass).map(|i| i.id.clone()).collect();
    let outline_only = !failed.is_empty() && items.iter().filter(|i| !i.pass).all(|i| i.group == "outline");
    CheckReport { pass: failed.is_empty(), failed, outline_only, items, metrics }
}

/// The checks of a finished icon against a pack's ranges (`checks_for(kind)`). `with_outline`
/// false leaves out the stroke items (outline `none` or `model`).
pub fn check(metrics: &Metrics, ranges: &Value, with_outline: bool) -> CheckReport {
    let m = metrics;
    let num = |k: &str| ranges.get(k).and_then(Value::as_f64);
    let mut items = vec![
        item("canvas", "frame", if m.width == m.height && m.has_alpha { 1.0 } else { 0.0 }, Some(1.0), None),
        item("cornerAlpha", "frame", m.corner_alpha_max as f64, None, Some(num("cornerAlphaMax").unwrap_or(0.0))),
        item("margin", "frame", m.margin_min_px as f64, num("marginMinPx"), None),
    ];
    let (lo, hi) = range(ranges, "size");
    items.push(item("size", "frame", m.size as f64, lo, hi));
    let (lo, hi) = range(ranges, "fill");
    items.push(item("fill", "frame", m.fill as f64, lo, hi));
    let cx = num("centerXPx");
    items.push(item("centerX", "frame", m.center_x_px.abs() as f64, None, cx));
    items.push(item("largestPart", "frame", m.largest_part as f64, num("largestPart"), None));
    if with_outline {
        items.push(item("outlineColor", "outline", m.outline_delta_e as f64, None, num("outlineDeltaE")));
        items.push(item("outlineUniform", "outline", m.outline_uniform as f64, num("outlineUniform"), None));
        let (lo, hi) = range(ranges, "outlineWidthPx");
        items.push(item("outlineWidth", "outline", m.outline_px as f64, lo, hi));
        items.push(item("edgeSoftness", "outline", m.edge_softness as f64, None, num("edgeSoftness")));
    }
    let (lo, hi) = range(ranges, "saturation");
    items.push(item("saturation", "color", m.saturation as f64, lo, hi));
    let (lo, hi) = range(ranges, "value");
    items.push(item("value", "color", m.value as f64, lo, hi));
    items.push(item("innerDark", "color", m.inner_dark as f64, None, num("innerDark")));
    items.push(item("highlight", "color", m.highlight as f64, None, num("highlight")));
    finish(items, metrics.clone())
}

/// Checks of a raw generated picture, before post-processing: colors, one mass, nothing cut at
/// the border, the picture's own alpha, the silhouette against the capture, and no stroke drawn
/// by the model.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateChecks {
    pub report: CheckReport,
    pub iou: Option<f32>,
    pub model_outline: bool,
    pub native_alpha: bool,
}

/// `raw` is the picture after its alpha was made (post `prepare`), `native_ok` whether its own
/// alpha was used, `capture` the capture it was made from.
pub fn check_generated(raw: &Raster, native_ok: bool, capture: Option<&Raster>, ranges: &Value, outline: [f32; 3]) -> GenerateChecks {
    let m = measure(raw, outline);
    let num = |k: &str| ranges.get(k).and_then(Value::as_f64);
    let mut items = Vec::new();
    let (lo, hi) = range(ranges, "saturation");
    items.push(item("saturation", "color", m.saturation as f64, lo, hi));
    let (lo, hi) = range(ranges, "value");
    items.push(item("value", "color", m.value as f64, lo, hi));
    items.push(item("innerDark", "color", m.inner_dark as f64, None, num("innerDark")));
    // Raw pictures keep sparkles apart from the object; the stroke may join them later
    items.push(item("largestPart", "generate", m.largest_part as f64, num("rawLargestPart").or(num("largestPart")), None));
    let touching = raw.bbox(8.0 / 255.0).is_some_and(|(x0, y0, x1, y1)| x0 == 0 || y0 == 0 || x1 + 1 == raw.w || y1 + 1 == raw.h);
    items.push(item("notTouching", "generate", if touching { 0.0 } else { 1.0 }, Some(1.0), None));
    items.push(item("nativeAlpha", "generate", if native_ok { 1.0 } else { 0.0 }, Some(1.0), None));
    let iou = capture.map(|c| silhouette_iou(c, raw));
    if let Some(v) = iou {
        items.push(item("iou", "generate", v as f64, num("iouMin"), None));
    }
    let model_outline = m.outline_px >= num("modelOutlinePx").unwrap_or(1.5) as f32 && m.outline_uniform >= 0.5;
    items.push(item("noModelOutline", "generate", if model_outline { 0.0 } else { 1.0 }, Some(1.0), None));
    // A white border under the stroke shows as a double outline
    items.push(item("noStickerRim", "generate", m.light_rim_px as f64, None, Some(num("stickerRimPx").unwrap_or(1.0))));
    GenerateChecks { report: finish(items, m), iou, model_outline, native_alpha: native_ok }
}

/// Overlap of two silhouettes (alpha over 127) after fitting each one's box to the same square.
pub fn silhouette_iou(a: &Raster, b: &Raster) -> f32 {
    const N: usize = 128;
    let grid = |img: &Raster| -> Option<Vec<bool>> {
        let (x0, y0, x1, y1) = img.bbox(0.5)?;
        let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        let mut g = vec![false; N * N];
        for gy in 0..N {
            for gx in 0..N {
                // Majority of 2x2 samples in the cell
                let mut on = 0;
                for (sx, sy) in [(0.25f32, 0.25f32), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let x = x0 as f32 + (gx as f32 + sx) / N as f32 * bw;
                    let y = y0 as f32 + (gy as f32 + sy) / N as f32 * bh;
                    if img.at((x as usize).min(img.w - 1), (y as usize).min(img.h - 1))[3] > 0.5 {
                        on += 1;
                    }
                }
                g[gy * N + gx] = on >= 2;
            }
        }
        Some(g)
    };
    let (Some(ga), Some(gb)) = (grid(a), grid(b)) else { return 0.0 };
    let inter = ga.iter().zip(&gb).filter(|(p, q)| **p && **q).count();
    let union = ga.iter().zip(&gb).filter(|(p, q)| **p || **q).count();
    inter as f32 / union.max(1) as f32
}

/// Measures and checks a PNG file.
pub fn check_file(path: &Path, ranges: &Value, outline: &str, with_outline: bool) -> Result<CheckReport, String> {
    let dynimg = ::image::open(path).map_err(|e| format!("image:{}:{e}", path.display()))?;
    let has_alpha = dynimg.color().has_alpha();
    let img = Raster::from_rgba8(&dynimg.to_rgba8());
    let mut m = measure(&img, parse_hex(outline).unwrap_or([0.459, 0.216, 0.098]));
    m.has_alpha = has_alpha;
    Ok(check(&m, ranges, with_outline))
}

/// A short JSON summary of a report for listings.
pub fn summary(r: &CheckReport) -> Value {
    json!({ "pass": r.pass, "failed": r.failed, "outlinePx": r.metrics.outline_px, "outlineOnly": r.outline_only })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring_icon(stroke: f32) -> Raster {
        // A filled disk of light color with a dark ring `stroke` px wide, on 256
        let mut r = Raster::new(256, 256);
        for y in 0..256 {
            for x in 0..256 {
                let d = ((x as f32 + 0.5 - 128.0).powi(2) + (y as f32 + 0.5 - 128.0).powi(2)).sqrt();
                let a = (100.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    let c = if d > 100.0 - stroke { [0.459, 0.216, 0.098] } else { [0.95, 0.75, 0.6] };
                    r.px[y * 256 + x] = [c[0], c[1], c[2], a];
                }
            }
        }
        r
    }

    #[test]
    fn stroke_width_is_measured() {
        let m = measure(&ring_icon(4.0), [0.459, 0.216, 0.098]);
        assert!((m.outline_px - 4.0).abs() < 0.6, "{}", m.outline_px);
        assert!(m.outline_delta_e < 1.0, "{}", m.outline_color);
        assert!(m.outline_uniform > 0.9);
        let none = measure(&ring_icon(0.0), [0.459, 0.216, 0.098]);
        assert!(none.outline_px < 0.5);
    }

    #[test]
    fn a_white_sticker_border_is_found() {
        let mut r = ring_icon(0.0);
        for (i, p) in r.px.iter_mut().enumerate() {
            let (x, y) = ((i % 256) as f32 + 0.5 - 128.0, (i / 256) as f32 + 0.5 - 128.0);
            if p[3] > 0.0 && (x * x + y * y).sqrt() > 96.0 {
                *p = [1.0, 1.0, 1.0, p[3]];
            }
        }
        assert!(measure(&r, [0.459, 0.216, 0.098]).light_rim_px >= 3.0);
        assert!(measure(&ring_icon(0.0), [0.459, 0.216, 0.098]).light_rim_px < 0.5);
        let g = check_generated(&r, true, None, &crate::packs::Pack::builtin().checks_for("prop"), [0.459, 0.216, 0.098]);
        assert!(g.report.failed.contains(&"noStickerRim".to_string()), "{:?}", g.report.failed);
    }

    #[test]
    fn iou_of_same_and_different_shapes() {
        let a = ring_icon(2.0);
        assert!(silhouette_iou(&a, &a) > 0.99);
        let mut sq = Raster::new(100, 100);
        sq.px.iter_mut().for_each(|p| *p = [1.0, 1.0, 1.0, 1.0]);
        let v = silhouette_iou(&a, &sq);
        assert!(v > 0.7 && v < 0.85, "disk vs square {v}");
    }
}
