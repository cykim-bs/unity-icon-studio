//! Lanczos3 resampling of premultiplied pixels. Filtering straight alpha pulls the color of
//! transparent pixels into the edge (dark or light fringes); premultiplied values do not.
//! Pixels outside the picture count as transparent.

use std::f32::consts::PI;

fn lanczos3(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-6 {
        1.0
    } else if x < 3.0 {
        let p = PI * x;
        3.0 * p.sin() * (p / 3.0).sin() / (p * p)
    } else {
        0.0
    }
}

/// Taps for each output index: (first source index, weights). `scale` = source / destination.
fn taps(src: usize, dst: usize) -> Vec<(isize, Vec<f32>)> {
    let scale = src as f32 / dst as f32;
    let fw = scale.max(1.0);
    let support = 3.0 * fw;
    (0..dst)
        .map(|i| {
            let center = (i as f32 + 0.5) * scale - 0.5;
            let first = (center - support).floor() as isize;
            let last = (center + support).ceil() as isize;
            let mut w: Vec<f32> = (first..=last).map(|j| lanczos3((j as f32 - center) / fw)).collect();
            let sum: f32 = w.iter().sum();
            if sum.abs() > 1e-6 {
                w.iter_mut().for_each(|v| *v /= sum);
            }
            (first, w)
        })
        .collect()
}

/// Resamples premultiplied `src` (`sw` x `sh`) to `dw` x `dh`.
pub fn resample(src: &[[f32; 4]], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<[f32; 4]> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    // Horizontal pass: sh rows of dw
    let tx = taps(sw, dw);
    let mut mid = vec![[0.0f32; 4]; dw * sh];
    for y in 0..sh {
        let row = &src[y * sw..(y + 1) * sw];
        for (x, (first, w)) in tx.iter().enumerate() {
            let mut acc = [0.0f32; 4];
            for (k, wk) in w.iter().enumerate() {
                let j = first + k as isize;
                if j < 0 || j >= sw as isize {
                    continue;
                }
                let p = row[j as usize];
                for c in 0..4 {
                    acc[c] += p[c] * wk;
                }
            }
            mid[y * dw + x] = acc;
        }
    }
    // Vertical pass
    let ty = taps(sh, dh);
    let mut out = vec![[0.0f32; 4]; dw * dh];
    for (y, (first, w)) in ty.iter().enumerate() {
        for x in 0..dw {
            let mut acc = [0.0f32; 4];
            for (k, wk) in w.iter().enumerate() {
                let j = first + k as isize;
                if j < 0 || j >= sh as isize {
                    continue;
                }
                let p = mid[j as usize * dw + x];
                for c in 0..4 {
                    acc[c] += p[c] * wk;
                }
            }
            // Ringing: keep alpha in 0..1 and color under alpha
            let a = acc[3].clamp(0.0, 1.0);
            out[y * dw + x] = [acc[0].clamp(0.0, a), acc[1].clamp(0.0, a), acc[2].clamp(0.0, a), a];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one_and_flat_stays_flat() {
        for (s, d) in [(1024, 256), (1254, 1000), (100, 300)] {
            for (_, w) in taps(s, d) {
                assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4);
            }
        }
        let src = vec![[0.25, 0.5, 0.75, 1.0]; 40 * 40];
        let out = resample(&src, 40, 40, 10, 10);
        // Away from the border (which fades into the transparent outside) the color is unchanged
        let p = out[5 * 10 + 5];
        assert!((p[0] - 0.25).abs() < 1e-3 && (p[3] - 1.0).abs() < 1e-3, "{p:?}");
    }
}
