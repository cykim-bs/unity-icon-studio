//! Getting the alpha: the picture's own (native) alpha when it is clean enough, otherwise keying
//! the flat background out (only the region connected to the border), with a one-pixel unmix.

use serde::Serialize;

use super::mask;
use super::raster::{delta_e, hex, Raster};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeCheck {
    pub ok: bool,
    /// Highest alpha (0..255) in the four corner boxes
    pub corner_max: u8,
    /// Share of fully transparent pixels
    pub transparent: f32,
    pub reason: Option<String>,
}

/// Native alpha is usable when the four corner boxes (1/32 of the side, at least 8 px) are fully
/// transparent and at least `min_transparent` of the picture is.
pub fn native_check(img: &Raster, min_transparent: f32) -> NativeCheck {
    let b = (img.w.min(img.h) / 32).max(8).min(img.w.min(img.h));
    let mut corner_max = 0u8;
    for (x0, y0) in [(0, 0), (img.w - b, 0), (0, img.h - b), (img.w - b, img.h - b)] {
        for y in y0..y0 + b {
            for x in x0..x0 + b {
                corner_max = corner_max.max((img.at(x, y)[3] * 255.0 + 0.5) as u8);
            }
        }
    }
    let transparent = img.px.iter().filter(|p| p[3] <= 0.5 / 255.0).count() as f32 / img.px.len().max(1) as f32;
    let reason = if corner_max > 0 {
        Some("corners".to_string())
    } else if transparent < min_transparent {
        Some("opaque".to_string())
    } else {
        None
    };
    NativeCheck { ok: reason.is_none(), corner_max, transparent, reason }
}

/// The picture over a flat color (what an opaque generated picture looks like anyway).
pub fn flatten(img: &Raster, under: [f32; 3]) -> Raster {
    let px = img
        .px
        .iter()
        .map(|p| {
            let a = p[3];
            [p[0] * a + under[0] * (1.0 - a), p[1] * a + under[1] * (1.0 - a), p[2] * a + under[2] * (1.0 - a), 1.0]
        })
        .collect();
    Raster { w: img.w, h: img.h, px }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyReport {
    /// The background color sampled on the border
    pub key: String,
    /// Share of the picture removed as background
    pub removed: f32,
    /// Share of border pixels within the tolerance of the key (low: the border is not flat)
    pub border_flat: f32,
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// Removes the background of an opaque picture: samples the border ring (2 px), floods from the
/// border through pixels within `tol` (CIE76) of that color, and unmixes the one-pixel ring of the
/// object next to the removed region (alpha from the distance to the key, `soft` wide).
pub fn key_out(img: &Raster, tol: f32, soft: f32) -> (Raster, KeyReport) {
    let (w, h) = (img.w, img.h);
    let ring: Vec<[f32; 4]> = (0..w * h)
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            x < 2 || y < 2 || x + 2 >= w || y + 2 >= h
        })
        .map(|i| img.px[i])
        .collect();
    let key = [median(ring.iter().map(|p| p[0]).collect()), median(ring.iter().map(|p| p[1]).collect()), median(ring.iter().map(|p| p[2]).collect())];
    let rgb = |p: [f32; 4]| [p[0], p[1], p[2]];
    let near: Vec<bool> = img.px.iter().map(|&p| delta_e(rgb(p), key) <= tol).collect();
    let border_flat = ring.iter().filter(|&&p| delta_e(rgb(p), key) <= tol).count() as f32 / ring.len().max(1) as f32;
    // Background = parts of `near` (4-connected) that touch the border
    let (label, _) = mask::components4(w, h, &near);
    let mut keep_label = std::collections::HashSet::new();
    for (i, &l) in label.iter().enumerate() {
        let (x, y) = (i % w, i / w);
        if l != 0 && (x == 0 || y == 0 || x + 1 == w || y + 1 == h) {
            keep_label.insert(l);
        }
    }
    let bg: Vec<bool> = label.iter().map(|l| *l != 0 && keep_label.contains(l)).collect();
    let mut out = img.clone();
    let mut removed = 0usize;
    for i in 0..w * h {
        if bg[i] {
            out.px[i] = [0.0; 4];
            removed += 1;
            continue;
        }
        // Unmix pixels next to the background
        let (x, y) = (i % w, i / w);
        let touches = (y.saturating_sub(1)..(y + 2).min(h)).any(|yy| (x.saturating_sub(1)..(x + 2).min(w)).any(|xx| bg[yy * w + xx]));
        if touches {
            let p = img.px[i];
            let de = delta_e(rgb(p), key);
            let a = ((de - tol) / soft).clamp(0.0, 1.0).max(1.0 / 255.0) * p[3];
            let un = |c: f32, k: f32| ((c - (1.0 - a) * k) / a).clamp(0.0, 1.0);
            out.px[i] = [un(p[0], key[0]), un(p[1], key[1]), un(p[2], key[2]), a];
        }
    }
    let report = KeyReport { key: hex(key), removed: removed as f32 / (w * h).max(1) as f32, border_flat };
    (out, report)
}

/// The key color for a flat background: of magenta, green and cyan, the one whose hue is least
/// used in the capture (counting colorful pixels only).
pub fn auto_key_color(capture: &Raster) -> &'static str {
    const KEYS: [(&str, f32); 3] = [("#FF00FF", 300.0), ("#00FF00", 120.0), ("#00FFFF", 180.0)];
    let mut counts = [0usize; 3];
    for p in &capture.px {
        if p[3] < 0.5 {
            continue;
        }
        let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
        if mx <= 0.0 || (mx - mn) / mx < 0.25 {
            continue;
        }
        let hue = hue_of(p[0], p[1], p[2]);
        for (k, (_, kh)) in KEYS.iter().enumerate() {
            let d = (hue - kh).abs();
            if d.min(360.0 - d) < 40.0 {
                counts[k] += 1;
            }
        }
    }
    let best = (0..3).min_by_key(|&k| counts[k]).unwrap_or(0);
    KEYS[best].0
}

pub fn hue_of(r: f32, g: f32, b: f32) -> f32 {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    if d <= 1e-6 {
        return 0.0;
    }
    let h = if mx == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    if h < 0.0 {
        h + 360.0
    } else {
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(n: usize, bg: [f32; 4], fg: [f32; 4]) -> Raster {
        let mut r = Raster::new(n, n);
        let c = n as f32 / 2.0;
        for y in 0..n {
            for x in 0..n {
                let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
                r.px[y * n + x] = if d < n as f32 * 0.3 { fg } else { bg };
            }
        }
        r
    }

    #[test]
    fn native_alpha_passes_and_opaque_fails() {
        let clear = disk(64, [0.0; 4], [1.0, 0.0, 0.0, 1.0]);
        assert!(native_check(&clear, 0.05).ok);
        let opaque = disk(64, [1.0, 1.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0]);
        let c = native_check(&opaque, 0.05);
        assert!(!c.ok && c.reason.as_deref() == Some("corners"));
        let mut speck = clear.clone();
        speck.px[0] = [1.0, 1.0, 1.0, 0.2];
        assert!(!native_check(&speck, 0.05).ok, "a speck in a corner fails");
    }

    #[test]
    fn keying_removes_only_the_outer_background() {
        // Magenta background, white disk with a magenta "hole" inside (not connected to the border)
        let mut img = disk(64, [1.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
        img.px[32 * 64 + 32] = [1.0, 0.0, 1.0, 1.0];
        let (out, rep) = key_out(&img, 12.0, 25.0);
        assert_eq!(rep.key, "#FF00FF");
        assert!(rep.border_flat > 0.99);
        assert_eq!(out.at(0, 0)[3], 0.0);
        assert_eq!(out.at(32, 20)[3], 1.0, "inside stays");
        assert_eq!(out.at(32, 32)[3], 1.0, "an enclosed key-colored pixel stays");
        assert!(rep.removed > 0.6 && rep.removed < 0.8);
    }

    #[test]
    fn key_color_avoids_the_capture_hue() {
        let green = disk(32, [0.0; 4], [0.1, 0.9, 0.1, 1.0]);
        assert_ne!(auto_key_color(&green), "#00FF00");
        let pink = disk(32, [0.0; 4], [0.95, 0.2, 0.8, 1.0]);
        assert_ne!(auto_key_color(&pink), "#FF00FF");
    }
}
