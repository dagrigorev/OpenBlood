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
        dst.par_chunks_mut(stride).skip(oy).take(dh).enumerate().for_each(|(y, row)| {
            row[ox..ox + dw].copy_from_slice(&src[y * sw..y * sw + sw]);
        });
        return;
    }
    let (fx, fy) = (sw as f64 / dw as f64, sh as f64 / dh as f64);
    let shrink = fx > 1.0 && fy > 1.0;
    // Per-axis source sample positions, precomputed once per frame.
    let xs: Vec<Tap> = (0..dw).map(|x| Tap::new(x, fx, sw, mode)).collect();
    dst.par_chunks_mut(stride).skip(oy).take(dh).enumerate().for_each(|(y, row)| {
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
    fn fit_letterboxes() {
        assert_eq!(fit(1920, 1200, 16, 9), (0, 60, 1920, 1080));
        assert_eq!(fit(1920, 1080, 4, 3), (240, 0, 1440, 1080));
    }
}
