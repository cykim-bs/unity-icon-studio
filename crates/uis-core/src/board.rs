//! Comparison boards: one row per item, columns capture | raw | icon on checker | small sizes |
//! icon on white and dark | reference, with a line of check results above each row.
//! Labels use a built-in 5x7 font (ASCII letters, digits and a few signs).

use std::path::{Path, PathBuf};

use crate::post::raster::Raster;
use crate::post::resize;

const CELL: usize = 256;
const GAP: usize = 8;
const LABEL: usize = 26;
const BG: [u8; 3] = [246, 244, 240];
const INK: [u8; 3] = [40, 40, 40];

/// One row of a board.
#[derive(Debug, Clone, Default)]
pub struct BoardRow {
    pub label: String,
    pub capture: Option<PathBuf>,
    pub raw: Option<PathBuf>,
    pub icon: Option<PathBuf>,
    pub reference: Option<PathBuf>,
    /// Shown in red when the row did not pass
    pub pass: bool,
}

pub const COLUMNS: [&str; 6] = ["CAPTURE", "RAW", "ICON 256", "128 / 64", "WHITE / DARK", "REFERENCE"];

fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ',' => [0, 0, 0, 0, 12, 4, 8],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '=' => [0, 0, 31, 0, 31, 0, 0],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '%' => [24, 25, 2, 4, 8, 19, 3],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '#' => [10, 10, 31, 10, 31, 10, 10],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        _ => [0; 7],
    }
}

/// Text at (x, y), 2x scale (each glyph 12 px wide with spacing, 14 px high).
fn text(img: &mut ::image::RgbImage, x: usize, y: usize, s: &str, color: [u8; 3]) {
    let mut cx = x;
    for ch in s.chars() {
        let g = glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let (px, py) = (cx + col * 2 + dx, y + row * 2 + dy);
                            if px < img.width() as usize && py < img.height() as usize {
                                img.put_pixel(px as u32, py as u32, ::image::Rgb(color));
                            }
                        }
                    }
                }
            }
        }
        cx += 12;
    }
}

fn checker(x: usize, y: usize) -> [f32; 3] {
    if (x / 8 + y / 8).is_multiple_of(2) {
        [0.80, 0.80, 0.80]
    } else {
        [1.0, 1.0, 1.0]
    }
}

/// A picture scaled to fit `side` (premultiplied Lanczos3), centered.
fn fit(img: &Raster, side: usize) -> Raster {
    let s = side as f32 / img.w.max(img.h) as f32;
    let (w, h) = (((img.w as f32 * s).round() as usize).clamp(1, side), ((img.h as f32 * s).round() as usize).clamp(1, side));
    let pm = resize::resample(&img.premultiplied(), img.w, img.h, w, h);
    let small = Raster::from_premultiplied(w, h, &pm, 0.0);
    let mut out = Raster::new(side, side);
    let (ox, oy) = ((side - w) / 2, (side - h) / 2);
    for y in 0..h {
        for x in 0..w {
            out.px[(y + oy) * side + x + ox] = small.at(x, y);
        }
    }
    out
}

/// Draws `img` (already `side` square) at (x, y) over a background (`None` = checker).
fn put(board: &mut ::image::RgbImage, img: &Raster, x: usize, y: usize, under: Option<[f32; 3]>) {
    for py in 0..img.h {
        for px in 0..img.w {
            let p = img.at(px, py);
            let b = under.unwrap_or_else(|| checker(px, py));
            let c = [0, 1, 2].map(|k| ((p[k] * p[3] + b[k] * (1.0 - p[3])).clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            board.put_pixel((x + px) as u32, (y + py) as u32, ::image::Rgb(c));
        }
    }
}

fn open(p: &Option<PathBuf>) -> Option<Raster> {
    p.as_ref().and_then(|p| Raster::open(p).ok())
}

/// Builds the board and writes it as PNG.
pub fn board(title: &str, rows: &[BoardRow], out: &Path) -> Result<(), String> {
    let width = GAP + COLUMNS.len() * (CELL + GAP);
    let head = GAP + 16 + GAP + 16 + GAP;
    let row_h = LABEL + CELL + GAP;
    let height = head + rows.len() * row_h + GAP;
    let mut img = ::image::RgbImage::from_pixel(width as u32, height as u32, ::image::Rgb(BG));
    text(&mut img, GAP, GAP, title, INK);
    for (i, c) in COLUMNS.iter().enumerate() {
        text(&mut img, GAP + i * (CELL + GAP), GAP + 16 + GAP, c, [110, 110, 110]);
    }
    for (r, row) in rows.iter().enumerate() {
        let y = head + r * row_h;
        let color = if row.pass { INK } else { [190, 40, 40] };
        text(&mut img, GAP, y + 4, &row.label, color);
        let y = y + LABEL;
        let col = |i: usize| GAP + i * (CELL + GAP);
        if let Some(c) = open(&row.capture) {
            put(&mut img, &fit(&c, CELL), col(0), y, None);
        }
        if let Some(c) = open(&row.raw) {
            put(&mut img, &fit(&c, CELL), col(1), y, None);
        }
        if let Some(icon) = open(&row.icon) {
            let full = fit(&icon, CELL);
            put(&mut img, &full, col(2), y, None);
            let (s128, s64) = (fit(&icon, 128), fit(&icon, 64));
            put(&mut img, &s128, col(3), y, None);
            put(&mut img, &s64, col(3) + 136, y, None);
            put(&mut img, &s64, col(3) + 136, y + 72, Some([1.0, 1.0, 1.0]));
            put(&mut img, &s64, col(3) + 136, y + 144, Some([0.16, 0.17, 0.2]));
            put(&mut img, &s128, col(4), y, Some([1.0, 1.0, 1.0]));
            put(&mut img, &s128, col(4), y + 128, Some([0.16, 0.17, 0.2]));
            put(&mut img, &s128, col(4) + 128, y, Some([0.98, 0.88, 0.75]));
            put(&mut img, &s128, col(4) + 128, y + 128, Some([0.5, 0.5, 0.5]));
        }
        if let Some(c) = open(&row.reference) {
            put(&mut img, &fit(&c, CELL), col(5), y, None);
        }
    }
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("write:{e}"))?;
    }
    img.save_with_format(out, ::image::ImageFormat::Png).map_err(|e| format!("image:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_has_a_row_per_item() {
        let dir = std::env::temp_dir().join(format!("uis-board-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut r = Raster::new(64, 32);
        r.px.iter_mut().for_each(|p| *p = [0.2, 0.6, 0.9, 1.0]);
        let png = dir.join("a.png");
        r.save(&png).unwrap();
        let rows = vec![
            BoardRow { label: "box PASS 16/16".into(), capture: Some(png.clone()), raw: Some(png.clone()), icon: Some(png.clone()), reference: None, pass: true },
            BoardRow { label: "lamp FAIL size".into(), icon: Some(png), ..Default::default() },
        ];
        let out = dir.join("board.png");
        board("test board", &rows, &out).unwrap();
        let img = ::image::open(&out).unwrap();
        assert_eq!(img.width() as usize, GAP + 6 * (CELL + GAP));
        assert!(img.height() as usize > 2 * CELL);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
