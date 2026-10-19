//! KVX voxel models (Ken Silverman's format) used by Blood for 3D items and decorations.
//!
//! Layout of the first (full-detail) mip level:
//! `i32 numbytes, xsiz, ysiz, zsiz, xpivot, ypivot, zpivot` (pivots in 8.8 fixed point),
//! `i32 xoffset[xsiz+1]`, `u16 xyoffset[xsiz][ysiz+1]`, then per column a list of slabs
//! `{u8 ztop, u8 zleng, u8 cull, u8 colors[zleng]}`. Offsets are relative to the start of
//! the `xoffset` table. The file ends with a 768-byte palette (ignored: colours are indices
//! into the game palette).

pub const EMPTY: u8 = 255;

#[derive(Debug, Clone)]
pub struct Voxel {
    pub xs: usize,
    pub ys: usize,
    pub zs: usize,
    pub pivot: (f64, f64, f64),
    /// Dense grid, index `(x * ys + y) * zs + z`; `EMPTY` where no voxel.
    pub data: Vec<u8>,
}

impl Voxel {
    pub fn parse(b: &[u8]) -> Option<Voxel> {
        let rd = |o: usize| -> Option<i32> { Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
        let numbytes = rd(0)? as usize;
        let (xs, ys, zs) = (rd(4)? as usize, rd(8)? as usize, rd(12)? as usize);
        if xs == 0 || ys == 0 || zs == 0 || xs > 256 || ys > 256 || zs > 256 {
            return None;
        }
        let pivot = (rd(16)? as f64 / 256.0, rd(20)? as f64 / 256.0, rd(24)? as f64 / 256.0);
        let base = 28;
        if numbytes + 4 > b.len() {
            return None;
        }
        let xoff: Vec<usize> = (0..=xs).map(|i| rd(base + i * 4).map(|v| v as usize)).collect::<Option<_>>()?;
        let xyo = base + (xs + 1) * 4;
        let mut data = vec![EMPTY; xs * ys * zs];
        for x in 0..xs {
            let col_base = base + xoff[x];
            let row = |y: usize| -> Option<usize> {
                let o = xyo + (x * (ys + 1) + y) * 2;
                Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize)
            };
            for y in 0..ys {
                let (start, end) = (col_base + row(y)?, col_base + row(y + 1)?);
                let mut p = start;
                while p + 3 <= end && end <= b.len() {
                    let ztop = b[p] as usize;
                    let zlen = b[p + 1] as usize;
                    p += 3;
                    for k in 0..zlen {
                        let z = ztop + k;
                        if z < zs && p + k < b.len() {
                            data[(x * ys + y) * zs + z] = b[p + k];
                        }
                    }
                    p += zlen;
                }
            }
        }
        Some(Voxel { xs, ys, zs, pivot, data })
    }

    #[inline]
    pub fn at(&self, x: i32, y: i32, z: i32) -> u8 {
        if x < 0 || y < 0 || z < 0 || x as usize >= self.xs || y as usize >= self.ys || z as usize >= self.zs {
            return EMPTY;
        }
        self.data[(x as usize * self.ys + y as usize) * self.zs + z as usize]
    }

    /// Casts a ray in model space (units = voxels). Returns `(t, colour)` of the first
    /// solid voxel, where `t` is the ray parameter.
    pub fn cast(&self, o: [f64; 3], d: [f64; 3]) -> Option<(f64, u8)> {
        let size = [self.xs as f64, self.ys as f64, self.zs as f64];
        // Slab intersection with the model box.
        let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
        for a in 0..3 {
            if d[a].abs() < 1e-12 {
                if o[a] < 0.0 || o[a] > size[a] {
                    return None;
                }
            } else {
                let (mut ta, mut tb) = ((0.0 - o[a]) / d[a], (size[a] - o[a]) / d[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
        }
        if t0 > t1 {
            return None;
        }
        // 3D DDA from the entry point.
        let t = t0 + 1e-6;
        let p = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
        let mut cell = [p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32];
        let mut step = [0i32; 3];
        let mut tmax = [f64::INFINITY; 3];
        let mut tdelta = [f64::INFINITY; 3];
        for a in 0..3 {
            if d[a] > 0.0 {
                step[a] = 1;
                tmax[a] = t + ((cell[a] + 1) as f64 - p[a]) / d[a];
                tdelta[a] = 1.0 / d[a];
            } else if d[a] < 0.0 {
                step[a] = -1;
                tmax[a] = t + (cell[a] as f64 - p[a]) / d[a];
                tdelta[a] = -1.0 / d[a];
            }
        }
        let mut tc = t;
        for _ in 0..(self.xs + self.ys + self.zs + 3) {
            let c = self.at(cell[0], cell[1], cell[2]);
            if c != EMPTY {
                return Some((tc, c));
            }
            let a = if tmax[0] < tmax[1] { if tmax[0] < tmax[2] { 0 } else { 2 } } else if tmax[1] < tmax[2] { 1 } else { 2 };
            if tmax[a] > t1 {
                return None;
            }
            tc = tmax[a];
            cell[a] += step[a];
            tmax[a] += tdelta[a];
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x1x3 model: column (0,0) has voxels z=1..2 colour 7; column (1,0) empty.
    fn sample() -> Vec<u8> {
        let (xs, ys, zs) = (2i32, 1i32, 3i32);
        let mut cols: Vec<u8> = Vec::new();
        cols.extend_from_slice(&[1, 2, 0, 7, 7]); // slab for x=0,y=0
        let xoff_tab = (xs as usize + 1) * 4;
        let xy_tab = xs as usize * (ys as usize + 1) * 2;
        let start = (xoff_tab + xy_tab) as i32;
        let mut b = Vec::new();
        let numbytes = 24 + xoff_tab + xy_tab + cols.len();
        for v in [numbytes as i32, xs, ys, zs, 256, 128, 3 * 256] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [start, start + 5, start + 5] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [0u16, 5, 0, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&cols);
        b.extend_from_slice(&[0u8; 768]);
        b
    }

    #[test]
    fn parse_and_cast() {
        let v = Voxel::parse(&sample()).unwrap();
        assert_eq!((v.xs, v.ys, v.zs), (2, 1, 3));
        assert_eq!(v.pivot, (1.0, 0.5, 3.0));
        assert_eq!(v.at(0, 0, 1), 7);
        assert_eq!(v.at(0, 0, 0), EMPTY);
        assert_eq!(v.at(1, 0, 1), EMPTY);
        // Ray along +x at z=1.5 from x=-1 hits voxel (0,0,1) at t=1.
        let (t, c) = v.cast([-1.0, 0.5, 1.5], [1.0, 0.0, 0.0]).unwrap();
        assert!((t - 1.0).abs() < 1e-5 && c == 7, "{t}");
        // Ray along +x at z=0.5 misses (empty row).
        assert!(v.cast([-1.0, 0.5, 0.5], [1.0, 0.0, 0.0]).is_none());
        // Ray from the -x side going -x never enters.
        assert!(v.cast([-1.0, 0.5, 1.5], [-1.0, 0.0, 0.0]).is_none());
    }
}
