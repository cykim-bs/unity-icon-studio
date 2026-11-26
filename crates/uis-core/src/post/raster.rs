//! A float RGBA picture (straight alpha, 0..1, sRGB values) and the conversions to and from PNG.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub w: usize,
    pub h: usize,
    /// Row-major `[r, g, b, a]`, straight (not premultiplied) alpha
    pub px: Vec<[f32; 4]>,
}

impl Raster {
    pub fn new(w: usize, h: usize) -> Raster {
        Raster { w, h, px: vec![[0.0; 4]; w * h] }
    }

    pub fn from_rgba8(img: &::image::RgbaImage) -> Raster {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img.pixels().map(|p| [p.0[0] as f32 / 255.0, p.0[1] as f32 / 255.0, p.0[2] as f32 / 255.0, p.0[3] as f32 / 255.0]).collect();
        Raster { w, h, px }
    }

    /// 8-bit RGBA; a pixel with no alpha gets RGB 0 (no color hidden under transparent pixels).
    pub fn to_rgba8(&self) -> ::image::RgbaImage {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let mut out = ::image::RgbaImage::new(self.w as u32, self.h as u32);
        for (i, p) in out.pixels_mut().enumerate() {
            let s = self.px[i];
            let a = q(s[3]);
            p.0 = if a == 0 { [0, 0, 0, 0] } else { [q(s[0]), q(s[1]), q(s[2]), a] };
        }
        out
    }

    pub fn open(path: &Path) -> Result<Raster, String> {
        let img = ::image::open(path).map_err(|e| format!("image:{}:{e}", path.display()))?;
        Ok(Raster::from_rgba8(&img.to_rgba8()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("write:{e}"))?;
        }
        self.to_rgba8().save_with_format(path, ::image::ImageFormat::Png).map_err(|e| format!("image:{}:{e}", path.display()))
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> [f32; 4] {
        self.px[y * self.w + x]
    }

    pub fn alpha(&self) -> Vec<f32> {
        self.px.iter().map(|p| p[3]).collect()
    }

    /// Premultiplied copy (RGB times alpha).
    pub fn premultiplied(&self) -> Vec<[f32; 4]> {
        self.px.iter().map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]).collect()
    }

    /// From premultiplied values: alpha clamped to 0..1, RGB divided back and clamped. Alpha under
    /// `floor` becomes 0 (filter ringing far from the edge).
    pub fn from_premultiplied(w: usize, h: usize, pm: &[[f32; 4]], floor: f32) -> Raster {
        let px = pm
            .iter()
            .map(|p| {
                let a = p[3].clamp(0.0, 1.0);
                if a <= floor {
                    [0.0; 4]
                } else {
                    [(p[0] / a).clamp(0.0, 1.0), (p[1] / a).clamp(0.0, 1.0), (p[2] / a).clamp(0.0, 1.0), a]
                }
            })
            .collect();
        Raster { w, h, px }
    }

    /// The box of pixels with alpha above `min` as (x0, y0, x1, y1), inclusive.
    pub fn bbox(&self, min: f32) -> Option<(usize, usize, usize, usize)> {
        bbox_of(self.w, self.h, |i| self.px[i][3] > min)
    }
}

/// The box of the pixels where `inside(i)` holds, inclusive.
pub fn bbox_of(w: usize, h: usize, inside: impl Fn(usize) -> bool) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if inside(y * w + x) {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != usize::MAX).then_some((x0, y0, x1, y1))
}

/// `#RRGGBB` to 0..1 RGB.
pub fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let t = s.trim().trim_start_matches('#');
    if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    Some([v(0)?, v(2)?, v(4)?])
}

pub fn hex(rgb: [f32; 3]) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    format!("#{:02X}{:02X}{:02X}", q(rgb[0]), q(rgb[1]), q(rgb[2]))
}

/// CIE L*a*b* (D65) of an sRGB color in 0..1.
pub fn lab(rgb: [f32; 3]) -> [f32; 3] {
    let lin = |c: f32| if c > 0.04045 { ((c + 0.055) / 1.055).powf(2.4) } else { c / 12.92 };
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIE76 color difference.
pub fn delta_e(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (p, q) = (lab(a), lab(b));
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_delta_e() {
        let c = parse_hex("#753719").unwrap();
        assert_eq!(hex(c), "#753719");
        assert!(delta_e(c, c) < 1e-4);
        assert!(delta_e([1.0, 1.0, 1.0], [0.0, 0.0, 0.0]) > 99.0);
        assert!(parse_hex("#12345").is_none());
    }

    #[test]
    fn transparent_pixels_lose_their_color() {
        let mut r = Raster::new(2, 1);
        r.px[0] = [1.0, 0.5, 0.0, 0.0];
        r.px[1] = [1.0, 0.5, 0.0, 1.0];
        let img = r.to_rgba8();
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(img.get_pixel(1, 0).0, [255, 128, 0, 255]);
    }
}
