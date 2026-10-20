//! Geometry helpers over Build-style sector/wall data.
//!
//! Conventions: x grows east, y grows south, z grows *down*. Angles use 2048 units
//! per full turn, 0 = east, 512 = south. One xy unit equals 16 z units.

use crate::map::{Plane, Sector, Wall};

pub const ANGLE_UNITS: f64 = 2048.0;

#[inline]
pub fn ang_to_rad(a: f64) -> f64 {
    a * std::f64::consts::TAU / ANGLE_UNITS
}

/// Linear plane equation `z = a + bx * x + by * y` for a (possibly sloped) sector plane.
#[derive(Debug, Clone, Copy)]
pub struct PlaneEq {
    pub a: f64,
    pub bx: f64,
    pub by: f64,
}

impl PlaneEq {
    #[inline]
    pub fn z_at(&self, x: f64, y: f64) -> f64 {
        self.a + self.bx * x + self.by * y
    }
}

/// Sloped-plane flag in ceiling/floor stat.
pub const STAT_SLOPED: u16 = 2;

pub fn plane_eq(sector: &Sector, walls: &[Wall], plane: &Plane) -> PlaneEq {
    let z0 = plane.z as f64;
    if plane.stat & STAT_SLOPED == 0 || plane.heinum == 0 || sector.wallnum < 2 {
        return PlaneEq {
            a: z0,
            bx: 0.0,
            by: 0.0,
        };
    }
    let w1 = &walls[sector.wallptr as usize];
    let w2 = &walls[w1.point2 as usize];
    let (x1, y1) = (w1.x as f64, w1.y as f64);
    let dx = w2.x as f64 - x1;
    let dy = w2.y as f64 - y1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return PlaneEq {
            a: z0,
            bx: 0.0,
            by: 0.0,
        };
    }
    // z = z0 + heinum/256 * (dx*(py-y1) - dy*(px-x1)) / len
    let k = plane.heinum as f64 / (256.0 * len);
    let bx = -dy * k;
    let by = dx * k;
    PlaneEq {
        a: z0 - bx * x1 - by * y1,
        bx,
        by,
    }
}

pub fn ceil_eq(sectors: &[Sector], walls: &[Wall], s: usize) -> PlaneEq {
    plane_eq(&sectors[s], walls, &sectors[s].ceiling)
}
pub fn floor_eq(sectors: &[Sector], walls: &[Wall], s: usize) -> PlaneEq {
    plane_eq(&sectors[s], walls, &sectors[s].floor)
}

/// Ceiling and floor z of sector `s` at point (x, y).
pub fn zs_of_slope(sectors: &[Sector], walls: &[Wall], s: usize, x: f64, y: f64) -> (f64, f64) {
    (
        ceil_eq(sectors, walls, s).z_at(x, y),
        floor_eq(sectors, walls, s).z_at(x, y),
    )
}

/// Even-odd point-in-sector test (handles inner loops).
pub fn inside_sector(sectors: &[Sector], walls: &[Wall], s: usize, x: f64, y: f64) -> bool {
    let sec = &sectors[s];
    let start = sec.wallptr as usize;
    let end = start + sec.wallnum as usize;
    let mut inside = false;
    for i in start..end {
        let w1 = &walls[i];
        let w2 = &walls[w1.point2 as usize];
        let (x1, y1, x2, y2) = (w1.x as f64, w1.y as f64, w2.x as f64, w2.y as f64);
        if (y1 > y) != (y2 > y) {
            let xi = x1 + (y - y1) * (x2 - x1) / (y2 - y1);
            if x < xi {
                inside = !inside;
            }
        }
    }
    inside
}

/// Finds the sector containing (x, y), searching neighbours of `hint` first.
pub fn update_sector(sectors: &[Sector], walls: &[Wall], x: f64, y: f64, hint: i32) -> i32 {
    if hint >= 0 && (hint as usize) < sectors.len() {
        let h = hint as usize;
        if inside_sector(sectors, walls, h, x, y) {
            return hint;
        }
        let sec = &sectors[h];
        for i in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
            let ns = walls[i].nextsector;
            if ns >= 0 && inside_sector(sectors, walls, ns as usize, x, y) {
                return ns as i32;
            }
        }
    }
    (0..sectors.len())
        .find(|&s| inside_sector(sectors, walls, s, x, y))
        .map_or(-1, |s| s as i32)
}

/// Like [`update_sector`] but also requires z to lie between ceiling and floor.
pub fn update_sector_z(sectors: &[Sector], walls: &[Wall], x: f64, y: f64, z: f64, hint: i32) -> i32 {
    let ok = |s: usize| {
        if !inside_sector(sectors, walls, s, x, y) {
            return false;
        }
        let (c, f) = zs_of_slope(sectors, walls, s, x, y);
        z >= c && z <= f
    };
    if hint >= 0 && (hint as usize) < sectors.len() {
        let h = hint as usize;
        if ok(h) {
            return hint;
        }
        let sec = &sectors[h];
        for wall in &walls[sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize] {
            let ns = wall.nextsector;
            if ns >= 0 && ok(ns as usize) {
                return ns as i32;
            }
        }
    }
    let s = update_sector(sectors, walls, x, y, hint);
    if s >= 0 {
        return s;
    }
    -1
}

/// Ray/segment intersection. Returns `(t, s)` where the hit point is `o + d*t`
/// and `s` in `[0,1]` is the position along the segment `a -> b`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn ray_segment(ox: f64, oy: f64, dx: f64, dy: f64, ax: f64, ay: f64, bx: f64, by: f64) -> Option<(f64, f64)> {
    let ex = bx - ax;
    let ey = by - ay;
    let den = dx * ey - dy * ex;
    if den.abs() < 1e-12 {
        return None;
    }
    let qx = ax - ox;
    let qy = ay - oy;
    let t = (qx * ey - qy * ex) / den;
    let s = (qx * dy - qy * dx) / den;
    if (-1e-9..=1.0 + 1e-9).contains(&s) {
        Some((t, s.clamp(0.0, 1.0)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Sector, Wall};

    pub fn square_sector() -> (Vec<Sector>, Vec<Wall>) {
        let pts = [(0, 0), (1024, 0), (1024, 1024), (0, 1024)];
        let walls: Vec<Wall> = pts
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Wall {
                x,
                y,
                point2: ((i + 1) % 4) as i16,
                nextwall: -1,
                nextsector: -1,
                ..Default::default()
            })
            .collect();
        let mut s = Sector {
            wallptr: 0,
            wallnum: 4,
            ..Default::default()
        };
        s.ceiling.z = -8192;
        s.floor.z = 0;
        (vec![s], walls)
    }

    #[test]
    fn inside_and_update() {
        let (s, w) = square_sector();
        assert!(inside_sector(&s, &w, 0, 512.0, 512.0));
        assert!(!inside_sector(&s, &w, 0, 2000.0, 512.0));
        assert_eq!(update_sector(&s, &w, 10.0, 10.0, -1), 0);
    }

    #[test]
    fn slope_matches_build_formula() {
        let (mut s, w) = square_sector();
        s[0].floor.stat = STAT_SLOPED;
        s[0].floor.heinum = 256; // 1 z unit per xy unit away from first wall
        let (_, f) = zs_of_slope(&s, &w, 0, 100.0, 300.0);
        // first wall runs along +x at y=0, so z grows with y
        assert!((f - 300.0).abs() < 1e-6, "{f}");
    }

    #[test]
    fn ray_hits_segment() {
        let (t, s) = ray_segment(0.0, 0.0, 1.0, 0.0, 10.0, -5.0, 10.0, 5.0).unwrap();
        assert!((t - 10.0).abs() < 1e-9 && (s - 0.5).abs() < 1e-9);
    }
}
