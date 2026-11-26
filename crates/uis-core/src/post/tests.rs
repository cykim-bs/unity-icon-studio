//! Post-processing on shapes drawn here (no generated pictures).

use super::raster::Raster;
use super::*;
use crate::checks;

const BROWN: [f32; 3] = [117.0 / 255.0, 55.0 / 255.0, 25.0 / 255.0];

/// A canvas with shapes: each pixel's coverage comes from 4x4 samples of `inside(x, y)`.
fn draw(n: usize, bg: [f32; 4], color: [f32; 3], inside: impl Fn(f32, f32) -> bool) -> Raster {
    let mut r = Raster::new(n, n);
    for y in 0..n {
        for x in 0..n {
            let mut cov = 0.0;
            for sy in 0..4 {
                for sx in 0..4 {
                    if inside(x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0) {
                        cov += 1.0 / 16.0;
                    }
                }
            }
            // Over the background (premultiplied mix, straight result)
            let a = cov + bg[3] * (1.0 - cov);
            let mix = |c: f32, b: f32| if a > 0.0 { (c * cov + b * bg[3] * (1.0 - cov)) / a } else { 0.0 };
            r.px[y * n + x] = [mix(color[0], bg[0]), mix(color[1], bg[1]), mix(color[2], bg[2]), a];
        }
    }
    r
}

const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
const PEACH: [f32; 3] = [0.98, 0.82, 0.69];

fn house(x: f32, y: f32) -> bool {
    // A box with a triangular roof, on a 1000 canvas
    let body = (300.0..=700.0).contains(&x) && (450.0..=800.0).contains(&y);
    let roof = (250.0..=750.0).contains(&x) && (250.0..=450.0).contains(&y) && (x - 500.0).abs() <= (y - 250.0) * 1.25;
    body || roof
}

fn run(img: &Raster, f: impl FnOnce(&mut PostOptions)) -> PostOutput {
    let mut o = PostOptions::default();
    f(&mut o);
    process(img, &o).expect("post")
}

fn icon(out: &PostOutput) -> &Raster {
    &out.icons[0].1
}

fn measure(r: &Raster) -> checks::Metrics {
    checks::measure(r, BROWN)
}

#[test]
fn stroke_is_about_3_7_px_at_256() {
    let img = draw(1000, CLEAR, PEACH, house);
    let out = run(&img, |_| {});
    let m = measure(icon(&out));
    // Measured like the reference table (every dark pixel per band) and as the checks do (dark
    // pixels of the stroke color only)
    assert!((3.5..=3.9).contains(&m.dark_band_px), "stroke {}", m.dark_band_px);
    assert!((3.2..=3.9).contains(&m.outline_px), "stroke color {}", m.outline_px);
    assert!(m.outline_delta_e < 2.0, "color {}", m.outline_color);
    assert!(m.outline_uniform > 0.85, "uniform {}", m.outline_uniform);
    assert!((out.report.layout.stroke_px - 4.212).abs() < 0.01, "{}", out.report.layout.stroke_px);
    assert_eq!(out.icons[0].0, 256);
    assert_eq!((icon(&out).w, out.master.w), (256, 1024));
}

#[test]
fn fill_is_88_percent_and_buildings_sit_low() {
    let img = draw(1000, CLEAR, PEACH, house);
    let out = run(&img, |o| o.layout = "bottom".into());
    let r = icon(&out);
    let (x0, y0, x1, y1) = r.bbox(0.5).unwrap();
    let long = (x1 - x0 + 1).max(y1 - y0 + 1) as f32 / 256.0;
    assert!((long - 0.88).abs() <= 0.01, "long side {long}");
    let (top, bottom) = (y0 as f32, 255.0 - y1 as f32);
    let ratio = bottom / top;
    assert!((ratio - 0.6).abs() <= 0.05, "bottom/top {ratio} ({bottom}/{top})");
    let center = (x0 + x1) as f32 / 2.0 - 127.5;
    assert!(center.abs() <= 1.0, "centered across {center}");
    // Centered layout: equal margins
    let c = run(&img, |o| o.layout = "center".into());
    let (_, y0, _, y1) = icon(&c).bbox(0.5).unwrap();
    assert!(((255 - y1) as i32 - y0 as i32).abs() <= 1);
}

#[test]
fn big_holes_get_a_ring_and_small_gaps_are_filled() {
    // A thick ring (big hole) and, beside it, a block with a narrow slit
    let shape = |x: f32, y: f32| {
        let d = ((x - 400.0).powi(2) + (y - 500.0).powi(2)).sqrt();
        let ring = (120.0..=300.0).contains(&d);
        let block = (720.0..=900.0).contains(&x) && (380.0..=620.0).contains(&y) && !((800.0..=812.0).contains(&x) && y > 450.0);
        let bar = (650.0..=760.0).contains(&x) && (480.0..=520.0).contains(&y);
        ring || block || bar
    };
    let img = draw(1000, CLEAR, PEACH, shape);
    let out = run(&img, |o| o.layout = "center".into());
    let m = &out.master; // 1024 working canvas
    // Map a source point to the master canvas
    let (bx0, by0, _, _) = img.bbox(0.5).unwrap();
    let s = out.report.layout.scale * 4.0;
    let (mx, my) = (out.report.layout.margins[0] * 4.0, out.report.layout.margins[2] * 4.0);
    let stroke = out.report.layout.stroke_px * 4.0;
    let at = |x: f32, y: f32| m.at(((x - bx0 as f32) * s + mx + stroke) as usize, ((y - by0 as f32) * s + my + stroke) as usize);
    assert_eq!(at(400.0, 500.0)[3], 0.0, "the big hole stays open");
    let inner_edge = at(400.0, 500.0 - 120.0 + 6.0);
    assert!(inner_edge[3] > 0.99 && (inner_edge[0] - BROWN[0]).abs() < 0.02, "stroke around the hole {inner_edge:?}");
    let slit = at(806.0, 560.0);
    assert!(slit[3] > 0.99 && (slit[0] - BROWN[0]).abs() < 0.02, "the slit is filled {slit:?}");
}

#[test]
fn native_alpha_is_used_when_clean_and_keying_otherwise() {
    let oval = |x: f32, y: f32| ((x - 300.0) / 200.0).powi(2) + ((y - 300.0) / 150.0).powi(2) < 1.0;
    let clear = draw(600, CLEAR, PEACH, oval);
    let out = run(&clear, |_| {});
    assert_eq!(out.report.alpha_source, "native");
    // The same object on a flat magenta background
    let flat = draw(600, [1.0, 0.0, 1.0, 1.0], PEACH, oval);
    let keyed = run(&flat, |_| {});
    assert_eq!(keyed.report.alpha_source, "key");
    assert_eq!(keyed.report.key.as_ref().unwrap().key, "#FF00FF");
    let m = measure(icon(&keyed));
    assert_eq!(m.corner_alpha_max, 0);
    assert!((3.5..=3.9).contains(&m.dark_band_px), "{}", m.dark_band_px);
    // No magenta left at the edge: the band next to the stroke is peach or brown
    let r = icon(&keyed);
    let pinkish = r.px.iter().filter(|p| p[3] > 0.1 && p[0] > 0.8 && p[1] < 0.5 && p[2] > 0.8).count();
    assert_eq!(pinkish, 0, "magenta spill");
    // Forced native on the opaque one fails its corner check
    let forced = run(&flat, |o| o.background = "native".into());
    assert!(!forced.report.native.ok);
}

#[test]
fn small_specks_are_removed_and_halo_is_hidden() {
    let mut img = draw(800, CLEAR, PEACH, |x, y| ((x - 400.0).powi(2) + (y - 420.0).powi(2)).sqrt() < 300.0);
    // Specks far from the object, and a faint white halo around it
    for (cx, cy) in [(40usize, 40usize), (760, 60), (700, 760)] {
        for y in cy..cy + 4 {
            for x in cx..cx + 4 {
                img.px[y * 800 + x] = [0.2, 0.2, 0.2, 1.0];
            }
        }
    }
    for y in 0..800 {
        for x in 0..800 {
            let d = ((x as f32 - 400.0).powi(2) + (y as f32 - 420.0).powi(2)).sqrt();
            if (300.5..306.0).contains(&d) {
                img.px[y * 800 + x] = [1.0, 1.0, 1.0, 0.12];
            }
        }
    }
    let out = run(&img, |_| {});
    assert_eq!(out.report.removed_parts, 3);
    let m = measure(icon(&out));
    assert!((m.largest_part - 1.0).abs() < 1e-6, "one part");
    assert!(m.edge_softness < 1.0, "softness {}", m.edge_softness);
    // No light pixels in the outer band: the halo is under the stroke
    assert!(m.outline_uniform > 0.85, "uniform {}", m.outline_uniform);
}

#[test]
fn premultiplied_reduction_keeps_edge_colors() {
    // White disk, transparent outside with black RGB: straight filtering would grey the edge
    let img = draw(1024, CLEAR, [1.0, 1.0, 1.0], |x, y| ((x - 512.0).powi(2) + (y - 512.0).powi(2)).sqrt() < 400.0);
    let out = run(&img, |o| o.outline = "none".into());
    let r = icon(&out);
    let edge: Vec<[f32; 4]> = r.px.iter().copied().filter(|p| p[3] > 0.02 && p[3] < 0.98).collect();
    assert!(!edge.is_empty());
    let darkest = edge.iter().map(|p| p[0].min(p[1]).min(p[2])).fold(1.0f32, f32::min);
    assert!(darkest > 0.97, "edge darkened to {darkest}");
}

#[test]
fn checks_pass_after_post_and_flag_missing_stroke() {
    let img = draw(1000, CLEAR, PEACH, house);
    let pack = crate::packs::Pack::builtin();
    let ranges = pack.checks_for("building");
    let out = run(&img, |o| o.layout = "bottom".into());
    let r = checks::check(&measure(icon(&out)), &ranges, true);
    let frame_and_outline: Vec<_> = r.items.iter().filter(|i| i.group != "color" && !i.pass).map(|i| &i.id).collect();
    assert!(frame_and_outline.is_empty(), "{frame_and_outline:?}");
    let plain = run(&img, |o| o.outline = "none".into());
    let r = checks::check(&measure(icon(&plain)), &ranges, true);
    assert!(r.failed.iter().any(|f| f == "outlineWidth"), "{:?}", r.failed);
    assert!(r.items.iter().filter(|i| !i.pass).all(|i| i.group != "frame"), "{:?}", r.failed);
}

#[test]
fn drawing_at_output_size_is_possible_and_stroke_matches() {
    let img = draw(1000, CLEAR, PEACH, house);
    let out = run(&img, |o| o.work_scale = 1);
    let m = measure(icon(&out));
    assert!((3.2..=4.2).contains(&m.outline_px), "{}", m.outline_px);
    assert_eq!(out.master.w, 1024);
}

#[test]
fn larger_sizes_keep_the_same_proportions() {
    let img = draw(1000, CLEAR, PEACH, house);
    let out = run(&img, |o| o.sizes = vec![256, 512]);
    assert_eq!(out.icons.iter().map(|(s, r)| (*s, r.w)).collect::<Vec<_>>(), [(256, 256), (512, 512)]);
    let m = measure(&out.icons[1].1); // measured at 256
    assert!((3.4..=4.0).contains(&m.outline_px), "{}", m.outline_px);
}

#[test]
fn options_from_the_pack_and_overrides() {
    let pack = crate::packs::Pack::builtin();
    let o = PostOptions::from_pack(&pack, "building");
    assert_eq!(o.layout, "bottom");
    assert_eq!(o.outline_color, "#753719");
    assert!((o.width_pct - 1.45).abs() < 1e-6);
    let mut p = PostOptions::from_pack(&pack, "prop");
    assert_eq!(p.layout, "center");
    p.apply(&serde_json::json!({ "widthPct": 2.0, "outline": "bogus", "unknown": 1 }));
    assert!((p.width_pct - 2.0).abs() < 1e-6);
    assert_eq!(p.outline, "code", "unknown values fall back");
}

