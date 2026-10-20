//! Scaling the rendered frame to the window: area averaging when shrinking (supersampling
//! anti-aliasing), and nearest / bilinear / sharp-bilinear when enlarging.

use crate::config::Upscale;
use rayon::prelude::*;

/// Resamples `src` (sw x sh, 0RGB) into the `dw x dh` rectangle at (`ox`, `oy`) of `dst`
/// (row stride `stride`). Pixels of `dst` outside the rectangle are left alone.
#[allow(clippy::too_many_arguments)]
pub fn resample(
    src: &[u32],
    sw: usize,
    sh: usize,
    dst: &mut [u32],
    stride: usize,
    ox: usize,
    oy: usize,
    dw: usize,
    dh: usize,
    mode: Upscale,
) {
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return;
    }
    if sw == dw && sh == dh {
        dst.par_chunks_mut(stride)
            .skip(oy)
            .take(dh)
            .enumerate()
            .for_each(|(y, row)| {
                row[ox..ox + dw].copy_from_slice(&src[y * sw..y * sw + sw]);
            });
        return;
    }
    let (fx, fy) = (sw as f64 / dw as f64, sh as f64 / dh as f64);
    let shrink = fx > 1.0 && fy > 1.0;
    // Per-axis source sample positions, precomputed once per frame.
    let xs: Vec<Tap> = (0..dw).map(|x| Tap::new(x, fx, sw, mode)).collect();
    dst.par_chunks_mut(stride)
        .skip(oy)
        .take(dh)
        .enumerate()
        .for_each(|(y, row)| {
            let out = &mut row[ox..ox + dw];
            if shrink {
                let y0 = (y as f64 * fy) as usize;
                let y1 = (((y + 1) as f64 * fy).ceil() as usize).clamp(y0 + 1, sh);
                for (x, o) in out.iter_mut().enumerate() {
                    let x0 = (x as f64 * fx) as usize;
                    let x1 = (((x + 1) as f64 * fx).ceil() as usize).clamp(x0 + 1, sw);
                    let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                    for sy in y0..y1 {
                        for &p in &src[sy * sw + x0..sy * sw + x1] {
                            r += (p >> 16) & 255;
                            g += (p >> 8) & 255;
                            b += p & 255;
                            n += 1;
                        }
                    }
                    *o = ((r / n) << 16) | ((g / n) << 8) | (b / n);
                }
                return;
            }
            let ty = Tap::new(y, fy, sh, mode);
            let (r0, r1) = (&src[ty.i0 * sw..ty.i0 * sw + sw], &src[ty.i1 * sw..ty.i1 * sw + sw]);
            for (o, tx) in out.iter_mut().zip(&xs) {
                *o = if mode == Upscale::Nearest {
                    r0[tx.i0]
                } else {
                    let top = mix(r0[tx.i0], r0[tx.i1], tx.w);
                    let bot = mix(r1[tx.i0], r1[tx.i1], tx.w);
                    mix(top, bot, ty.w)
                };
            }
        });
}

/// Like [`resample`], but the source is column-major (`src[x * sh + y]`, the renderer's
/// native layout), so the frame never has to be transposed. Works on bands of output rows
/// so every source column is read as short contiguous runs.
#[allow(clippy::too_many_arguments)]
pub fn resample_cols(
    src: &[u32],
    sw: usize,
    sh: usize,
    dst: &mut [u32],
    stride: usize,
    ox: usize,
    oy: usize,
    dw: usize,
    dh: usize,
    mode: Upscale,
) {
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return;
    }
    const BAND: usize = 16;
    let (fx, fy) = (sw as f64 / dw as f64, sh as f64 / dh as f64);
    let shrink = fx > 1.0 && fy > 1.0;
    let foot = |d: usize, f: f64, n: usize| {
        let a = ((d as f64 * f) as usize).min(n - 1);
        let b = (((d + 1) as f64 * f).ceil() as usize).clamp(a + 1, n);
        (a, b)
    };
    let mode = if sw == dw && sh == dh { Upscale::Nearest } else { mode };
    let xs: Vec<Tap> = (0..dw).map(|x| Tap::new(x, fx, sw, mode)).collect();
    let ys: Vec<Tap> = (0..dh).map(|y| Tap::new(y, fy, sh, mode)).collect();
    let xf: Vec<(usize, usize)> = (0..dw).map(|x| foot(x, fx, sw)).collect();
    let yf: Vec<(usize, usize)> = (0..dh).map(|y| foot(y, fy, sh)).collect();
    let area = &mut dst[oy * stride..(oy + dh) * stride];
    // Exactly half size (4x supersampling at native resolution): average 2x2 blocks with
    // packed channel sums instead of the general footprint loop.
    if sw == dw * 2 && sh == dh * 2 {
        area.par_chunks_mut(stride * BAND).enumerate().for_each(|(band, rows)| {
            let r0 = band * BAND;
            let n = rows.len() / stride;
            for x in 0..dw {
                let c0 = &src[2 * x * sh..2 * x * sh + sh];
                let c1 = &src[(2 * x + 1) * sh..(2 * x + 1) * sh + sh];
                for k in 0..n {
                    let y = 2 * (r0 + k);
                    let (a, b, c, d) = (c0[y], c0[y + 1], c1[y], c1[y + 1]);
                    let rb = (a & 0xff00ff) + (b & 0xff00ff) + (c & 0xff00ff) + (d & 0xff00ff);
                    let g = (a & 0xff00) + (b & 0xff00) + (c & 0xff00) + (d & 0xff00);
                    rows[k * stride + ox + x] = ((rb >> 2) & 0xff00ff) | ((g >> 2) & 0xff00);
                }
            }
        });
        return;
    }
    area.par_chunks_mut(stride * BAND).enumerate().for_each(|(band, rows)| {
        let r0 = band * BAND;
        let n = rows.len() / stride;
        if shrink {
            let mut acc = [(0u32, 0u32, 0u32, 0u32); BAND];
            for (x, &(x0, x1)) in xf.iter().enumerate() {
                acc[..n].fill((0, 0, 0, 0));
                for c in x0..x1 {
                    let column = &src[c * sh..c * sh + sh];
                    for (k, a) in acc[..n].iter_mut().enumerate() {
                        let (y0, y1) = yf[r0 + k];
                        for &p in &column[y0..y1] {
                            a.0 += (p >> 16) & 255;
                            a.1 += (p >> 8) & 255;
                            a.2 += p & 255;
                            a.3 += 1;
                        }
                    }
                }
                for (k, a) in acc[..n].iter().enumerate() {
                    rows[k * stride + ox + x] = ((a.0 / a.3) << 16) | ((a.1 / a.3) << 8) | (a.2 / a.3);
                }
            }
            return;
        }
        for (x, tx) in xs.iter().enumerate() {
            let c0 = &src[tx.i0 * sh..tx.i0 * sh + sh];
            let c1 = &src[tx.i1 * sh..tx.i1 * sh + sh];
            for k in 0..n {
                let ty = &ys[r0 + k];
                rows[k * stride + ox + x] = if mode == Upscale::Nearest {
                    c0[ty.i0]
                } else {
                    let a = mix(c0[ty.i0], c1[ty.i0], tx.w);
                    let b = mix(c0[ty.i1], c1[ty.i1], tx.w);
                    mix(a, b, ty.w)
                };
            }
        }
    });
}

/// Two neighbouring source samples and the blend weight between them.
struct Tap {
    i0: usize,
    i1: usize,
    w: u32,
}

impl Tap {
    fn new(d: usize, f: f64, n: usize, mode: Upscale) -> Tap {
        if mode == Upscale::Nearest {
            let i = ((d as f64 + 0.5) * f) as usize;
            let i = i.min(n - 1);
            return Tap { i0: i, i1: i, w: 0 };
        }
        let u = ((d as f64 + 0.5) * f - 0.5).clamp(0.0, (n - 1) as f64);
        let i0 = u.floor() as usize;
        let mut t = u - i0 as f64;
        if mode == Upscale::Sharp {
            // Keep each source pixel flat and blend only across a one-output-pixel seam.
            let k = (1.0 / f).max(1.0);
            t = ((t - 0.5) * k + 0.5).clamp(0.0, 1.0);
        }
        Tap {
            i0,
            i1: (i0 + 1).min(n - 1),
            w: (t * 256.0) as u32,
        }
    }
}

fn mix(a: u32, b: u32, w: u32) -> u32 {
    let iw = 256 - w;
    let rb = (((a & 0xff00ff) * iw + (b & 0xff00ff) * w) >> 8) & 0xff00ff;
    let g = (((a & 0x00ff00) * iw + (b & 0x00ff00) * w) >> 8) & 0x00ff00;
    rb | g
}

/// Largest rectangle of aspect `aw:ah` centred in `ww x wh`: (x, y, w, h).
pub fn fit(ww: usize, wh: usize, aw: usize, ah: usize) -> (usize, usize, usize, usize) {
    if aw * wh > ah * ww {
        let h = (ww * ah / aw).max(1);
        (0, (wh - h) / 2, ww, h)
    } else {
        let w = (wh * aw / ah).max(1);
        ((ww - w) / 2, 0, w, wh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_averages_pixels() {
        // 2x2 checkerboard of black/white averages to grey.
        let src = [0xffffff, 0, 0, 0xffffff];
        let mut dst = [0u32; 1];
        resample(&src, 2, 2, &mut dst, 1, 0, 0, 1, 1, Upscale::Sharp);
        assert_eq!(dst[0], 0x7f7f7f);
    }

    #[test]
    fn sharp_keeps_pixel_centres_flat() {
        let src = [0x000000, 0xffffff];
        let mut dst = [0u32; 8];
        resample(&src, 2, 1, &mut dst, 8, 0, 0, 8, 1, Upscale::Sharp);
        assert_eq!(dst[0], 0);
        assert_eq!(dst[1], 0);
        assert_eq!(dst[7], 0xffffff);
        let mut smooth = [0u32; 8];
        resample(&src, 2, 1, &mut smooth, 8, 0, 0, 8, 1, Upscale::Smooth);
        assert_ne!(smooth[2], 0, "bilinear blends further from the seam");
    }

    #[test]
    fn column_major_matches_row_major() {
        let (sw, sh) = (37, 23);
        let rows: Vec<u32> = (0..sw * sh)
            .map(|i| (i as u32).wrapping_mul(2654435761) & 0xffffff)
            .collect();
        let mut cols = vec![0u32; sw * sh];
        for y in 0..sh {
            for x in 0..sw {
                cols[x * sh + y] = rows[y * sw + x];
            }
        }
        for (dw, dh) in [(37, 23), (80, 50), (15, 9)] {
            // (the exact-half fast path is checked separately below)
            for m in Upscale::ALL {
                let mut a = vec![0u32; dw * dh];
                let mut b = vec![0u32; dw * dh];
                resample(&rows, sw, sh, &mut a, dw, 0, 0, dw, dh, m);
                resample_cols(&cols, sw, sh, &mut b, dw, 0, 0, dw, dh, m);
                assert_eq!(a, b, "{dw}x{dh} {m:?}");
            }
        }
    }

    #[test]
    fn half_size_fast_path_matches_box_average() {
        let (sw, sh) = (40, 24);
        let rows: Vec<u32> = (0..sw * sh)
            .map(|i| (i as u32).wrapping_mul(2654435761) & 0xffffff)
            .collect();
        let mut cols = vec![0u32; sw * sh];
        for y in 0..sh {
            for x in 0..sw {
                cols[x * sh + y] = rows[y * sw + x];
            }
        }
        let (dw, dh) = (20, 12);
        let mut fast = vec![0u32; dw * dh];
        resample_cols(&cols, sw, sh, &mut fast, dw, 0, 0, dw, dh, Upscale::Sharp);
        for y in 0..dh {
            for x in 0..dw {
                let px = [
                    rows[2 * y * sw + 2 * x],
                    rows[2 * y * sw + 2 * x + 1],
                    rows[(2 * y + 1) * sw + 2 * x],
                    rows[(2 * y + 1) * sw + 2 * x + 1],
                ];
                let avg = |sh: u32| px.iter().map(|p| (p >> sh) & 255).sum::<u32>() / 4;
                let want = (avg(16) << 16) | (avg(8) << 8) | avg(0);
                assert_eq!(fast[y * dw + x], want, "at {x},{y}");
            }
        }
    }

    #[test]
    fn fit_letterboxes() {
        assert_eq!(fit(1920, 1200, 16, 9), (0, 60, 1920, 1080));
        assert_eq!(fit(1920, 1080, 4, 3), (240, 0, 1440, 1080));
    }
}
