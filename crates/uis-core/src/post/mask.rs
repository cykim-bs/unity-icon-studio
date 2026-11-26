//! Binary masks: connected parts and the exact Euclidean distance transform.

/// Labels of the 8-connected parts of `mask` (0 = outside the mask, parts from 1) and the size of
/// each part (index = label - 1).
pub fn components(w: usize, h: usize, mask: &[bool]) -> (Vec<u32>, Vec<usize>) {
    let mut label = vec![0u32; w * h];
    let mut sizes = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !mask[start] || label[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32 + 1;
        let mut n = 0usize;
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            n += 1;
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let (nx, ny) = (x + dx, y + dy);
                    if (dx == 0 && dy == 0) || nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if mask[j] && label[j] == 0 {
                        label[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        sizes.push(n);
    }
    (label, sizes)
}

/// Like `components` but 4-connected (for the background: a diagonal gap between two parts of an
/// object does not let the background through).
pub fn components4(w: usize, h: usize, mask: &[bool]) -> (Vec<u32>, Vec<usize>) {
    let mut label = vec![0u32; w * h];
    let mut sizes = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !mask[start] || label[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32 + 1;
        let mut n = 0usize;
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            n += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if mask[j] && label[j] == 0 {
                    label[j] = id;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        sizes.push(n);
    }
    (label, sizes)
}

const INF: f32 = 1e20;

/// 1-D squared distance transform (Felzenszwalb & Huttenlocher) of `f` into `d`.
fn dt1(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0: replace the first parabola
                v[0] = q;
                z[0] = -INF;
                z[1] = INF;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = INF;
            break;
        }
    }
    k = 0;
    for (q, dq) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let dx = q as f32 - p as f32;
        *dq = dx * dx + f[p];
    }
}

/// Euclidean distance (in pixels) from each pixel to the nearest pixel of `mask` (0 inside).
pub fn distance_to(w: usize, h: usize, mask: &[bool]) -> Vec<f32> {
    let mut grid: Vec<f32> = mask.iter().map(|&m| if m { 0.0 } else { INF }).collect();
    let n = w.max(h);
    let (mut f, mut d, mut v, mut z) = (vec![0.0f32; n], vec![0.0f32; n], vec![0usize; n], vec![0.0f32; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        dt1(&f[..h], &mut d[..h], &mut v[..h], &mut z[..h + 1]);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..(y + 1) * w]);
        dt1(&f[..w], &mut d[..w], &mut v[..w], &mut z[..w + 1]);
        for x in 0..w {
            grid[y * w + x] = d[x].sqrt();
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_matches_brute_force() {
        let (w, h) = (23, 17);
        let mut mask = vec![false; w * h];
        for &(x, y) in &[(3usize, 4usize), (15, 12), (20, 2), (7, 15)] {
            mask[y * w + x] = true;
        }
        let d = distance_to(w, h, &mask);
        for y in 0..h {
            for x in 0..w {
                let best = mask
                    .iter()
                    .enumerate()
                    .filter(|(_, &m)| m)
                    .map(|(i, _)| (((i % w) as f32 - x as f32).powi(2) + ((i / w) as f32 - y as f32).powi(2)).sqrt())
                    .fold(f32::MAX, f32::min);
                assert!((d[y * w + x] - best).abs() < 1e-3, "({x},{y}) {} vs {best}", d[y * w + x]);
            }
        }
    }

    #[test]
    fn parts_are_counted() {
        // Two blobs touching only diagonally are one 8-connected part and two 4-connected parts
        let (w, h) = (4, 4);
        let mut mask = vec![false; 16];
        mask[0] = true;
        mask[5] = true;
        mask[15] = true;
        let (_, s8) = components(w, h, &mask);
        let (_, s4) = components4(w, h, &mask);
        assert_eq!(s8.len(), 2);
        assert_eq!(s4.len(), 3);
    }
}
