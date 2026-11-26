//! Post-processing: from a raw generated picture to the icon.
//!
//! 1. Alpha: the picture's own alpha when the corners are clear and enough of it is transparent,
//!    otherwise the flat background is keyed out (only what is connected to the border).
//! 2. Clean: faint pixels (alpha under 8/255) and parts much smaller than the main one are removed.
//! 3. Layout: the box of the opaque pixels (alpha over 127/255) is scaled so that the long side,
//!    stroke included, is `fillPct` of the canvas; centered across; buildings sit lower
//!    (bottom margin : top margin = `bottomTopRatio`).
//! 4. Outline (`code`): on a canvas `workScale` times the output, the distance to the object gives
//!    a stroke of `widthPct` of the canvas width under the object. Holes smaller than `holeFillPct`
//!    of the canvas are filled; larger holes get the stroke around them.
//! 5. The working canvas is reduced to each output size with premultiplied Lanczos3.

pub mod alpha;
pub mod mask;
pub mod raster;
pub mod resize;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::packs::Pack;
use raster::{parse_hex, Raster};

/// Options of one post-processing run (what `options.json` holds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostOptions {
    /// Output sizes (the first is the main one)
    pub sizes: Vec<u32>,
    /// `code` (drawn here), `model` (the picture has its own) or `none`
    pub outline: String,
    pub outline_color: String,
    /// Stroke width as a share of the canvas width, in percent, as the checks measure it (the
    /// band of fully dark pixels; the pixel where stroke and object mix is not counted)
    pub width_pct: f32,
    /// Added to the drawn stroke, in output pixels, for that mixed pixel
    pub edge_comp_px: f32,
    /// Soft edge of the stroke on the working canvas, in pixels
    pub aa_px: f32,
    /// Working canvas = output size times this (1: draw at the output size)
    pub work_scale: u32,
    /// Long side of the object with its stroke, in percent of the canvas
    pub fill_pct: f32,
    /// `bottom` (buildings) or `center`
    pub layout: String,
    pub bottom_top_ratio: f32,
    /// `auto` (native alpha, keying when it fails), `native` or `key`
    pub background: String,
    pub key_tol: f32,
    pub key_soft: f32,
    /// Parts smaller than this share of the largest part are removed, in percent
    pub min_part_pct: f32,
    /// Holes smaller than this share of the canvas are filled with the stroke, in percent
    pub hole_fill_pct: f32,
    /// The object's alpha is sharpened between these two values before the stroke goes under it
    /// (faint halo pixels disappear into the stroke)
    pub edge_lo: f32,
    pub edge_hi: f32,
    pub saturation_boost: bool,
    /// `output` (object and stroke reduced apart, put together at the output size) or `work`
    /// (put together on the working canvas, then reduced)
    pub compose: String,
}

impl Default for PostOptions {
    fn default() -> Self {
        PostOptions {
            sizes: vec![256],
            outline: "code".into(),
            outline_color: "#753719".into(),
            width_pct: 1.45,
            edge_comp_px: 0.5,
            aa_px: 1.0,
            work_scale: 4,
            fill_pct: 88.0,
            layout: "center".into(),
            bottom_top_ratio: 0.6,
            background: "auto".into(),
            key_tol: 12.0,
            key_soft: 25.0,
            min_part_pct: 0.5,
            hole_fill_pct: 0.3,
            edge_lo: 0.25,
            edge_hi: 0.75,
            saturation_boost: false,
            compose: "output".into(),
        }
    }
}

impl PostOptions {
    /// The pack's values for a kind: `outline`, `canvas`, `kinds.<kind>.layout` and `post`.
    pub fn from_pack(pack: &Pack, kind: &str) -> PostOptions {
        let mut o = PostOptions::default();
        let doc = &pack.doc;
        let f = |v: Option<&Value>| v.and_then(Value::as_f64).map(|x| x as f32);
        if let Some(c) = doc.pointer("/outline/color").and_then(Value::as_str) {
            o.outline_color = c.into();
        }
        if let Some(v) = f(doc.pointer("/outline/widthPct")) {
            o.width_pct = v;
        }
        if let Some(v) = f(doc.pointer("/canvas/fillPct")) {
            o.fill_pct = v;
        }
        if let Some(v) = f(doc.pointer("/canvas/bottomTopRatio")) {
            o.bottom_top_ratio = v;
        }
        if let Some(l) = pack.kind_preset(kind).get("layout").and_then(Value::as_str) {
            o.layout = l.into();
        }
        if let Some(p) = doc.get("post") {
            o.apply(p);
        }
        o
    }

    /// Lays the keys present in `v` (camelCase, as in `options.json`) over these options.
    pub fn apply(&mut self, v: &Value) {
        let Some(obj) = v.as_object() else { return };
        let mut cur = serde_json::to_value(&*self).unwrap_or_default();
        if let Some(c) = cur.as_object_mut() {
            for (k, val) in obj {
                if c.contains_key(k) {
                    c.insert(k.clone(), val.clone());
                }
            }
        }
        if let Ok(next) = serde_json::from_value::<PostOptions>(cur) {
            *self = next;
        }
        self.normalize();
    }

    pub fn normalize(&mut self) {
        self.sizes.retain(|s| (16..=2048).contains(s));
        if self.sizes.is_empty() {
            self.sizes = vec![256];
        }
        if !["code", "model", "none"].contains(&self.outline.as_str()) {
            self.outline = "code".into();
        }
        if parse_hex(&self.outline_color).is_none() {
            self.outline_color = "#753719".into();
        }
        if !["bottom", "center"].contains(&self.layout.as_str()) {
            self.layout = "center".into();
        }
        if !["output", "work"].contains(&self.compose.as_str()) {
            self.compose = "output".into();
        }
        if !["auto", "native", "key"].contains(&self.background.as_str()) {
            self.background = "auto".into();
        }
        self.width_pct = self.width_pct.clamp(0.0, 10.0);
        self.aa_px = self.aa_px.clamp(0.0, 8.0);
        self.edge_comp_px = self.edge_comp_px.clamp(-2.0, 4.0);
        self.work_scale = self.work_scale.clamp(1, 8);
        self.fill_pct = self.fill_pct.clamp(30.0, 100.0);
        self.bottom_top_ratio = self.bottom_top_ratio.clamp(0.1, 3.0);
        self.edge_hi = self.edge_hi.clamp(0.01, 1.0);
        self.edge_lo = self.edge_lo.clamp(0.0, self.edge_hi - 0.005);
    }

    fn draws_stroke(&self) -> bool {
        self.outline == "code" && self.width_pct > 0.0
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutReport {
    /// Box of the opaque pixels in the source picture (x0, y0, x1, y1)
    pub source_box: [usize; 4],
    pub scale: f32,
    /// Stroke width at the main output size, in pixels
    pub stroke_px: f32,
    /// Margins of the object with its stroke at the main output size (left, right, top, bottom)
    pub margins: [f32; 4],
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PostReport {
    /// `native` or `key`
    pub alpha_source: String,
    pub native: alpha::NativeCheck,
    pub key: Option<alpha::KeyReport>,
    pub removed_parts: usize,
    /// Share of the object's pixels in the removed parts
    pub removed_share: f32,
    pub layout: LayoutReport,
    pub saturation_boosted: bool,
}

pub struct PostOutput {
    /// Main output size first, then the other sizes
    pub icons: Vec<(u32, Raster)>,
    /// The 1024 picture kept with the job
    pub master: Raster,
    /// The cleaned source with its alpha (before layout), for the checks
    pub source: Raster,
    pub report: PostReport,
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Faint pixels to 0 and small parts removed. Returns (removed parts, removed share).
fn clean(img: &mut Raster, min_part_pct: f32) -> (usize, f32) {
    for p in img.px.iter_mut() {
        if p[3] < 8.0 / 255.0 {
            *p = [0.0; 4];
        }
    }
    let m: Vec<bool> = img.px.iter().map(|p| p[3] > 0.0).collect();
    let (label, sizes) = mask::components(img.w, img.h, &m);
    let Some(&largest) = sizes.iter().max() else { return (0, 0.0) };
    let min = (largest as f32 * min_part_pct / 100.0).ceil() as usize;
    let drop: Vec<bool> = sizes.iter().map(|&s| s < min).collect();
    let total: usize = sizes.iter().sum();
    let mut gone = 0usize;
    for (i, l) in label.iter().enumerate() {
        if *l != 0 && drop[*l as usize - 1] {
            img.px[i] = [0.0; 4];
            gone += 1;
        }
    }
    (drop.iter().filter(|d| **d).count(), gone as f32 / total.max(1) as f32)
}

/// Alpha and clean-up of a raw picture (steps 1 and 2).
pub fn prepare(raw: &Raster, opts: &PostOptions) -> (Raster, alpha::NativeCheck, Option<alpha::KeyReport>, usize, f32) {
    let mut native_img = raw.clone();
    let (parts, share) = clean(&mut native_img, opts.min_part_pct);
    let native = alpha::native_check(&native_img, 0.05);
    let use_native = match opts.background.as_str() {
        "native" => true,
        "key" => false,
        _ => native.ok,
    };
    if use_native {
        return (native_img, native, None, parts, share);
    }
    let (mut keyed, rep) = alpha::key_out(&alpha::flatten(raw, [1.0, 1.0, 1.0]), opts.key_tol, opts.key_soft);
    let (parts, share) = clean(&mut keyed, opts.min_part_pct);
    (keyed, native, Some(rep), parts, share)
}

/// Mean saturation of the opaque pixels.
fn mean_saturation(img: &Raster) -> f32 {
    let (mut s, mut n) = (0.0f32, 0usize);
    for p in &img.px {
        if p[3] > 0.5 {
            let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
            s += if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
            n += 1;
        }
    }
    s / n.max(1) as f32
}

fn boost_saturation(img: &mut Raster, k: f32) {
    for p in img.px.iter_mut() {
        let v = p[0].max(p[1]).max(p[2]);
        for c in p.iter_mut().take(3) {
            *c = (v - (v - *c) * k).clamp(0.0, 1.0);
        }
    }
}

/// The object and its stroke on a working canvas, before they are put together.
struct Layers {
    side: usize,
    /// The object (edge sharpened when a stroke goes under it)
    body: Raster,
    /// Stroke coverage per pixel (None: no stroke)
    stroke: Option<Vec<f32>>,
    color: [f32; 3],
    stroke_px: f32,
    margins: [f32; 4],
}

/// Places the object on a `side` x `side` canvas and works out the stroke under it.
fn render(src: &Raster, bbox: (usize, usize, usize, usize), side: usize, out_size: u32, opts: &PostOptions) -> Layers {
    let k = side as f32 / out_size as f32;
    let stroke = if opts.draws_stroke() { (opts.width_pct / 100.0 * out_size as f32 + opts.edge_comp_px).max(0.0) * k } else { 0.0 };
    let (bx0, by0, bx1, by1) = bbox;
    let (bw, bh) = ((bx1 - bx0 + 1) as f32, (by1 - by0 + 1) as f32);
    let long = opts.fill_pct / 100.0 * side as f32 - 2.0 * stroke;
    let s = long.max(1.0) / bw.max(bh);
    let (cw, ch) = (bw * s, bh * s);
    let x0 = (side as f32 - cw) / 2.0;
    let free = side as f32 - ch - 2.0 * stroke;
    let y0 = if opts.layout == "bottom" {
        let top = free / (1.0 + opts.bottom_top_ratio);
        top + stroke
    } else {
        free / 2.0 + stroke
    };
    // Crop with room for the filter, resample, paste
    let pad = ((3.0 / s).ceil() as usize + 2).max(2);
    let (sx0, sy0) = (bx0.saturating_sub(pad), by0.saturating_sub(pad));
    let (sx1, sy1) = ((bx1 + pad).min(src.w - 1), (by1 + pad).min(src.h - 1));
    let (cwi, chi) = (sx1 - sx0 + 1, sy1 - sy0 + 1);
    let mut crop = Vec::with_capacity(cwi * chi);
    for y in sy0..=sy1 {
        for x in sx0..=sx1 {
            let p = src.at(x, y);
            crop.push([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]);
        }
    }
    let (dw, dh) = (((cwi as f32) * s).round().max(1.0) as usize, ((chi as f32) * s).round().max(1.0) as usize);
    let scaled = resize::resample(&crop, cwi, chi, dw, dh);
    let ox = (x0 - (bx0 - sx0) as f32 * s).round() as isize;
    let oy = (y0 - (by0 - sy0) as f32 * s).round() as isize;
    let mut body_pm = vec![[0.0f32; 4]; side * side];
    for y in 0..dh {
        for x in 0..dw {
            let (tx, ty) = (ox + x as isize, oy + y as isize);
            if tx >= 0 && ty >= 0 && (tx as usize) < side && (ty as usize) < side {
                body_pm[ty as usize * side + tx as usize] = scaled[y * dw + x];
            }
        }
    }
    let mut body = Raster::from_premultiplied(side, side, &body_pm, 0.0);
    let margins = [x0 - stroke, side as f32 - x0 - cw - stroke, y0 - stroke, side as f32 - y0 - ch - stroke];
    let color = parse_hex(&opts.outline_color).unwrap_or([0.459, 0.216, 0.098]);
    if !opts.draws_stroke() {
        return Layers { side, body, stroke: None, color, stroke_px: 0.0, margins };
    }
    // Sharpen the object's edge: the faint fringe goes, the stroke takes its place
    for p in body.px.iter_mut() {
        p[3] = smoothstep(opts.edge_lo, opts.edge_hi, p[3]);
        if p[3] <= 0.0 {
            *p = [0.0; 4];
        }
    }
    let inside: Vec<bool> = body.px.iter().map(|p| p[3] >= 0.5).collect();
    let dist = mask::distance_to(side, side, &inside);
    let mut stroke_a: Vec<f32> = dist.iter().map(|&d| 1.0 - smoothstep(stroke, stroke + opts.aa_px, d)).collect();
    // Holes: regions without stroke that do not reach the canvas border
    let open: Vec<bool> = stroke_a.iter().map(|&a| a < 0.5).collect();
    let (label, sizes) = mask::components4(side, side, &open);
    let mut outer = vec![false; sizes.len()];
    for i in 0..side * side {
        let (x, y) = (i % side, i / side);
        if label[i] != 0 && (x == 0 || y == 0 || x + 1 == side || y + 1 == side) {
            outer[label[i] as usize - 1] = true;
        }
    }
    let small = (opts.hole_fill_pct / 100.0 * (side * side) as f32) as usize;
    for (i, a) in stroke_a.iter_mut().enumerate() {
        let l = label[i];
        if l != 0 && !outer[l as usize - 1] && sizes[l as usize - 1] < small {
            *a = 1.0;
        }
    }
    Layers { side, body, stroke: Some(stroke_a), color, stroke_px: stroke, margins }
}

/// The object over its stroke.
fn compose(body: &Raster, stroke: Option<&[f32]>, color: [f32; 3]) -> Raster {
    let Some(stroke) = stroke else { return body.clone() };
    let mut out = Raster::new(body.w, body.h);
    for (i, (&b, &st)) in body.px.iter().zip(stroke).enumerate() {
        let sa = st * (1.0 - b[3]);
        let a = b[3] + sa;
        if a <= 0.0 {
            continue;
        }
        let mix = |bc: f32, sc: f32| (bc * b[3] + sc * sa) / a;
        out.px[i] = [mix(b[0], color[0]), mix(b[1], color[1]), mix(b[2], color[2]), a];
    }
    out
}

/// Area average of a coverage map by an integer factor (no ringing: one flat color stays exact).
fn box_reduce(v: &[f32], side: usize, k: usize) -> Vec<f32> {
    let n = side / k;
    let mut out = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let mut s = 0.0;
            for yy in 0..k {
                for xx in 0..k {
                    s += v[(y * k + yy) * side + x * k + xx];
                }
            }
            out[y * n + x] = s / (k * k) as f32;
        }
    }
    out
}

fn reduce(img: &Raster, size: usize) -> Raster {
    if img.w == size {
        return img.clone();
    }
    let pm = resize::resample(&img.premultiplied(), img.w, img.h, size, size);
    Raster::from_premultiplied(size, size, &pm, 1.5 / 255.0)
}

impl Layers {
    /// The icon at `size`. `output`: the object reduced with Lanczos3, the stroke by area
    /// average, put together at the output size (the stroke color stays exact, like a stroke
    /// laid on the finished picture). `work`: put together first, then reduced.
    fn icon(&self, size: usize, opts: &PostOptions) -> Raster {
        if self.side == size {
            return compose(&self.body, self.stroke.as_deref(), self.color);
        }
        if opts.compose == "work" {
            return reduce(&compose(&self.body, self.stroke.as_deref(), self.color), size);
        }
        let body = reduce(&self.body, size);
        let stroke = self.stroke.as_ref().map(|s| box_reduce(s, self.side, self.side / size));
        let mut out = compose(&body, stroke.as_deref(), self.color);
        for p in out.px.iter_mut() {
            if p[3] < 1.5 / 255.0 {
                *p = [0.0; 4];
            }
        }
        out
    }
}

/// The whole post-processing of one raw picture.
pub fn process(raw: &Raster, opts: &PostOptions) -> Result<PostOutput, String> {
    let mut opts = opts.clone();
    opts.normalize();
    let (mut src, native, key, removed_parts, removed_share) = prepare(raw, &opts);
    let bbox = src.bbox(0.5).ok_or_else(|| "empty".to_string())?;
    let mut boosted = false;
    if opts.saturation_boost && mean_saturation(&src) < 0.38 {
        boost_saturation(&mut src, 1.15);
        boosted = true;
    }
    let mut icons = Vec::new();
    let mut master: Option<Raster> = None;
    let mut layout = None;
    for (n, &size) in opts.sizes.iter().enumerate() {
        let side = size as usize * opts.work_scale as usize;
        let layers = render(&src, bbox, side, size, &opts);
        let k = opts.work_scale as f32;
        if n == 0 {
            let long = (bbox.2 - bbox.0 + 1).max(bbox.3 - bbox.1 + 1) as f32;
            layout = Some(LayoutReport {
                source_box: [bbox.0, bbox.1, bbox.2, bbox.3],
                scale: (opts.fill_pct / 100.0 * size as f32 - 2.0 * layers.stroke_px / k) / long,
                stroke_px: layers.stroke_px / k,
                margins: layers.margins.map(|m| m / k),
            });
        }
        icons.push((size, layers.icon(size as usize, &opts)));
        if side == 1024 && master.is_none() {
            master = Some(compose(&layers.body, layers.stroke.as_deref(), layers.color));
        }
    }
    let master = match master {
        Some(m) => m,
        None => {
            let l = render(&src, bbox, 1024, opts.sizes[0], &opts);
            compose(&l.body, l.stroke.as_deref(), l.color)
        }
    };
    let report = PostReport {
        alpha_source: if key.is_some() { "key".into() } else { "native".into() },
        native,
        key,
        removed_parts,
        removed_share,
        layout: layout.ok_or("empty")?,
        saturation_boosted: boosted,
    };
    Ok(PostOutput { icons, master, source: src, report })
}

#[cfg(test)]
mod tests;
