//! Movement and collision against sector geometry (Build `clipmove` / `getzrange`
//! equivalents, written from scratch).
//!
//! The mover is a vertical cylinder: radius `r` (xy units), feet at `z`, extending
//! `height` z units upward. A portal wall blocks when the step into the next sector is
//! higher than `step`, when the head would hit its ceiling, or when the wall's blocking
//! flag is set.

use crate::geom::{inside_sector, zs_of_slope};
use crate::map::{Sector, Wall};

pub const WALL_BLOCK_FLAG: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub struct Mover {
    pub radius: f64,
    pub height: f64,
    pub step: f64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    None,
    Wall(usize),
    Sprite(usize),
}

/// Circle obstacle (blocking sprite) for collision.
#[derive(Debug, Clone, Copy)]
pub struct CircleObstacle {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub zbot: f64,
    pub ztop: f64,
    pub id: usize,
}

#[inline]
fn closest_on_seg(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> (f64, f64) {
    let dx = bx - ax;
    let dy = by - ay;
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-12 {
        return (ax, ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0);
    (ax + dx * t, ay + dy * t)
}

/// Collects sectors reachable through portals whose walls come within `reach` of (x, y).
pub fn nearby_sectors(sectors: &[Sector], walls: &[Wall], start: i32, x: f64, y: f64, reach: f64) -> Vec<usize> {
    let mut out = Vec::new();
    if start < 0 || start as usize >= sectors.len() {
        return out;
    }
    out.push(start as usize);
    let mut i = 0;
    while i < out.len() && out.len() < 64 {
        let s = &sectors[out[i]];
        for wi in s.wallptr as usize..(s.wallptr + s.wallnum) as usize {
            let w = &walls[wi];
            if w.nextsector < 0 {
                continue;
            }
            let w2 = &walls[w.point2 as usize];
            let (cx, cy) = closest_on_seg(x, y, w.x as f64, w.y as f64, w2.x as f64, w2.y as f64);
            if (cx - x).hypot(cy - y) <= reach {
                let ns = w.nextsector as usize;
                if !out.contains(&ns) {
                    out.push(ns);
                }
            }
        }
        i += 1;
    }
    out
}

/// True if wall `wi` (seen from its own sector) blocks a mover with feet at `z`.
pub fn wall_blocks(sectors: &[Sector], walls: &[Wall], wi: usize, px: f64, py: f64, z: f64, m: &Mover) -> bool {
    let w = &walls[wi];
    if w.nextsector < 0 || w.cstat & WALL_BLOCK_FLAG != 0 {
        return true;
    }
    let (c, f) = zs_of_slope(sectors, walls, w.nextsector as usize, px, py);
    f < z - m.step || c > z - m.height || f - c < m.height * 0.5
}

/// Moves (x, y) by (dx, dy) with sliding collision. Returns the new position, sector
/// and the first obstacle touched.
#[allow(clippy::too_many_arguments)]
pub fn clip_move(
    sectors: &[Sector],
    walls: &[Wall],
    obstacles: &[CircleObstacle],
    x: f64,
    y: f64,
    z: f64,
    sect: i32,
    dx: f64,
    dy: f64,
    m: &Mover,
) -> (f64, f64, i32, Hit) {
    let mut hit = Hit::None;
    let dist = dx.hypot(dy);
    if sect < 0 {
        return (x + dx, y + dy, sect, hit);
    }
    let steps = ((dist / (m.radius * 0.5).max(1.0)).ceil() as usize).clamp(1, 64);
    let (sx, sy) = (dx / steps as f64, dy / steps as f64);
    let (mut px, mut py, mut cur) = (x, y, sect);
    for _ in 0..steps {
        let (ox, oy) = (px, py);
        px += sx;
        py += sy;
        // Resolve penetration a few times for corners.
        for _ in 0..4 {
            let mut moved = false;
            let near = nearby_sectors(sectors, walls, cur, px, py, m.radius + dist + 16.0);
            for &s in &near {
                let sec = &sectors[s];
                for wi in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
                    let w = &walls[wi];
                    let w2 = &walls[w.point2 as usize];
                    let (ax, ay, bx, by) = (w.x as f64, w.y as f64, w2.x as f64, w2.y as f64);
                    let (cx, cy) = closest_on_seg(px, py, ax, ay, bx, by);
                    let d = (px - cx).hypot(py - cy);
                    if d >= m.radius {
                        continue;
                    }
                    // Only walls facing the mover: use the side we came from.
                    let side = (bx - ax) * (oy - ay) - (by - ay) * (ox - ax);
                    // Build walls have their sector interior on the right (side > 0 in our
                    // y-down coordinates means right of a->b when looking along it).
                    let from_inside = side >= 0.0;
                    if !from_inside && w.nextsector >= 0 {
                        // Seen from the other side: test the reverse wall instead.
                        continue;
                    }
                    if !wall_blocks(sectors, walls, wi, cx, cy, z, m) {
                        continue;
                    }
                    let (nx, ny) = if d > 1e-6 {
                        ((px - cx) / d, (py - cy) / d)
                    } else {
                        let l = (bx - ax).hypot(by - ay).max(1e-9);
                        (-(by - ay) / l, (bx - ax) / l)
                    };
                    let push = m.radius - d + 0.01;
                    px += nx * push;
                    py += ny * push;
                    moved = true;
                    if hit == Hit::None {
                        hit = Hit::Wall(wi);
                    }
                }
            }
            for o in obstacles {
                if o.zbot < z - m.height || o.ztop > z {
                    continue;
                }
                let d = (px - o.x).hypot(py - o.y);
                let rr = m.radius + o.radius;
                if d < rr && d > 1e-6 {
                    let push = rr - d + 0.01;
                    px += (px - o.x) / d * push;
                    py += (py - o.y) / d * push;
                    moved = true;
                    if hit == Hit::None {
                        hit = Hit::Sprite(o.id);
                    }
                }
            }
            if !moved {
                break;
            }
        }
        cur = find_sector_near(sectors, walls, px, py, cur);
        if cur < 0 {
            // Escaped the map: undo this step.
            px = ox;
            py = oy;
            cur = find_sector_near(sectors, walls, px, py, sect);
            break;
        }
    }
    (px, py, cur, hit)
}

/// Sector lookup preferring neighbours of `hint` (breadth-first over a few levels).
pub fn find_sector_near(sectors: &[Sector], walls: &[Wall], x: f64, y: f64, hint: i32) -> i32 {
    if hint >= 0 && (hint as usize) < sectors.len() {
        let mut list = vec![hint as usize];
        let mut i = 0;
        while i < list.len() && list.len() < 128 {
            let s = list[i];
            if inside_sector(sectors, walls, s, x, y) {
                return s as i32;
            }
            let sec = &sectors[s];
            for wi in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
                let ns = walls[wi].nextsector;
                if ns >= 0 && !list.contains(&(ns as usize)) {
                    list.push(ns as usize);
                }
            }
            i += 1;
            if i > 24 {
                break;
            }
        }
    }
    (0..sectors.len())
        .find(|&s| inside_sector(sectors, walls, s, x, y))
        .map_or(-1, |s| s as i32)
}

/// Highest floor and lowest ceiling under a cylinder of `radius` at (x, y).
/// Returns `(ceil_z, ceil_sector, floor_z, floor_sector)`.
pub fn get_z_range(
    sectors: &[Sector],
    walls: &[Wall],
    x: f64,
    y: f64,
    z: f64,
    sect: i32,
    radius: f64,
    step: f64,
) -> (f64, i32, f64, i32) {
    if sect < 0 {
        return (f64::MIN, -1, f64::MAX, -1);
    }
    let (mut cz, mut fz) = zs_of_slope(sectors, walls, sect as usize, x, y);
    let (mut cs, mut fs) = (sect, sect);
    for s in nearby_sectors(sectors, walls, sect, x, y, radius) {
        if s as i32 == sect {
            continue;
        }
        let (c, f) = zs_of_slope(sectors, walls, s, x, y);
        // Floors above the step height act as walls (handled by clip_move), not ground.
        if f < fz && f >= z - step {
            fz = f;
            fs = s as i32;
        }
        if c > cz && c <= z {
            cz = c;
            cs = s as i32;
        }
    }
    (cz, cs, fz, fs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Sector, Wall};

    /// Two 1024x1024 rooms side by side (x 0..1024 and 1024..2048); the right room's
    /// floor is `step` units higher.
    fn rooms(right_floor: i32) -> (Vec<Sector>, Vec<Wall>) {
        let mut walls = Vec::new();
        let mk = |x, y, p2, nw, ns| Wall {
            x,
            y,
            point2: p2,
            nextwall: nw,
            nextsector: ns,
            ..Default::default()
        };
        // room 0 clockwise in y-down coords: (0,0)->(1024,0)->(1024,1024)->(0,1024)
        walls.push(mk(0, 0, 1, -1, -1));
        walls.push(mk(1024, 0, 2, 7, 1));
        walls.push(mk(1024, 1024, 3, -1, -1));
        walls.push(mk(0, 1024, 0, -1, -1));
        // room 1: (1024,0)->(2048,0)->(2048,1024)->(1024,1024)
        walls.push(mk(1024, 0, 5, -1, -1));
        walls.push(mk(2048, 0, 6, -1, -1));
        walls.push(mk(2048, 1024, 7, -1, -1));
        walls.push(mk(1024, 1024, 4, 1, 0));
        let mut s0 = Sector {
            wallptr: 0,
            wallnum: 4,
            ..Default::default()
        };
        s0.ceiling.z = -32768;
        s0.floor.z = 0;
        let mut s1 = Sector {
            wallptr: 4,
            wallnum: 4,
            ..Default::default()
        };
        s1.ceiling.z = -32768;
        s1.floor.z = right_floor;
        (vec![s0, s1], walls)
    }

    const M: Mover = Mover {
        radius: 64.0,
        height: 8192.0,
        step: 4096.0,
    };

    #[test]
    fn solid_wall_stops() {
        let (s, w) = rooms(0);
        let (x, _y, sect, hit) = clip_move(&s, &w, &[], 512.0, 512.0, 0.0, 0, -1000.0, 0.0, &M);
        assert!(x >= 63.9, "x={x}");
        assert_eq!(sect, 0);
        assert!(matches!(hit, Hit::Wall(3)));
    }

    #[test]
    fn walk_through_portal() {
        let (s, w) = rooms(-2048);
        let (x, _, sect, _) = clip_move(&s, &w, &[], 900.0, 512.0, 0.0, 0, 400.0, 0.0, &M);
        assert!((x - 1300.0).abs() < 1.0);
        assert_eq!(sect, 1);
    }

    #[test]
    fn high_step_blocks() {
        let (s, w) = rooms(-8192);
        let (x, _, sect, _) = clip_move(&s, &w, &[], 900.0, 512.0, 0.0, 0, 400.0, 0.0, &M);
        assert!(x <= 1024.0 - 63.9, "x={x}");
        assert_eq!(sect, 0);
    }

    #[test]
    fn zrange_sees_step() {
        let (s, w) = rooms(-2048);
        let (_, _, fz, fs) = get_z_range(&s, &w, 1000.0, 512.0, 0.0, 0, 64.0, 4096.0);
        assert_eq!((fz, fs), (-2048.0, 1));
    }
}

/// Vertical cylinder that can be hit by traces (dudes, things...).
#[derive(Debug, Clone, Copy)]
pub struct HitCylinder {
    pub id: usize,
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub ztop: f64,
    pub zbot: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TraceTarget {
    Nothing,
    Wall(usize),
    Floor(usize),
    Ceiling(usize),
    Sprite(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct TraceHit {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub sector: i32,
    pub dist: f64,
    pub target: TraceTarget,
}

/// 3D hitscan. Direction: unit xy vector `(dx, dy)` and `dz` z units per xy unit.
#[allow(clippy::too_many_arguments)]
pub fn hitscan(
    sectors: &[Sector],
    walls: &[Wall],
    cylinders: &[HitCylinder],
    ox: f64,
    oy: f64,
    oz: f64,
    sect: i32,
    dx: f64,
    dy: f64,
    dz: f64,
    max_dist: f64,
) -> TraceHit {
    use crate::geom::{plane_eq, ray_segment};
    let mut s = sect;
    let mut t_in = 0.0f64;
    let mut result = TraceHit {
        x: ox + dx * max_dist,
        y: oy + dy * max_dist,
        z: oz + dz * max_dist,
        sector: sect,
        dist: max_dist,
        target: TraceTarget::Nothing,
    };
    'walk: for _ in 0..256 {
        if s < 0 || s as usize >= sectors.len() {
            break;
        }
        let sec = &sectors[s as usize];
        let mut best: Option<(f64, usize)> = None;
        for wi in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
            let w1 = &walls[wi];
            let w2 = &walls[w1.point2 as usize];
            if let Some((t, _)) = ray_segment(ox, oy, dx, dy, w1.x as f64, w1.y as f64, w2.x as f64, w2.y as f64) {
                if t > t_in + 1e-7 && best.is_none_or(|b| t < b.0) {
                    best = Some((t, wi));
                }
            }
        }
        let t_out = best.map_or(max_dist, |b| b.0).min(max_dist);
        // Plane hits inside this sector.
        for (is_ceil, plane) in [(true, &sec.ceiling), (false, &sec.floor)] {
            let eq = plane_eq(sec, walls, plane);
            let a = eq.z_at(ox, oy);
            let b = eq.bx * dx + eq.by * dy;
            let den = dz - b;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = (a - oz) / den;
            let toward = if is_ceil { den < 0.0 } else { den > 0.0 };
            if toward && t >= t_in - 1e-6 && t <= t_out {
                let t = t.max(t_in);
                result = TraceHit {
                    x: ox + dx * t,
                    y: oy + dy * t,
                    z: oz + dz * t,
                    sector: s,
                    dist: t,
                    target: if is_ceil {
                        TraceTarget::Ceiling(s as usize)
                    } else {
                        TraceTarget::Floor(s as usize)
                    },
                };
                break 'walk;
            }
        }
        let Some((t, wi)) = best else { break };
        if t > max_dist {
            break;
        }
        let w = &walls[wi];
        let z = oz + dz * t;
        let (hx, hy) = (ox + dx * t, oy + dy * t);
        let blocked = if w.nextsector < 0 || w.cstat & WALL_BLOCK_FLAG != 0 && w.cstat & 64 != 0 {
            true
        } else {
            let (c, f) = zs_of_slope(sectors, walls, w.nextsector as usize, hx, hy);
            z < c || z > f
        };
        if blocked {
            result = TraceHit {
                x: hx,
                y: hy,
                z,
                sector: s,
                dist: t,
                target: TraceTarget::Wall(wi),
            };
            break;
        }
        t_in = t;
        s = w.nextsector as i32;
    }
    // Sprites closer than the geometry hit.
    for c in cylinders {
        let fx = c.x - ox;
        let fy = c.y - oy;
        let along = fx * dx + fy * dy;
        if along <= 0.0 || along > result.dist {
            continue;
        }
        let perp2 = fx * fx + fy * fy - along * along;
        let r2 = c.radius * c.radius;
        if perp2 > r2 {
            continue;
        }
        let t = along - (r2 - perp2).sqrt();
        let t = t.max(0.0);
        let z = oz + dz * t;
        if z >= c.ztop && z <= c.zbot && t < result.dist {
            result = TraceHit {
                x: ox + dx * t,
                y: oy + dy * t,
                z,
                sector: result.sector,
                dist: t,
                target: TraceTarget::Sprite(c.id),
            };
        }
    }
    result
}

#[cfg(test)]
mod trace_tests {
    use super::*;
    use crate::map::{Sector, Wall};

    fn room() -> (Vec<Sector>, Vec<Wall>) {
        let pts = [(0, 0), (4096, 0), (4096, 4096), (0, 4096)];
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
        s.ceiling.z = -32768;
        s.floor.z = 0;
        (vec![s], walls)
    }

    #[test]
    fn hits_wall_floor_and_sprite() {
        let (s, w) = room();
        let h = hitscan(&s, &w, &[], 1000.0, 2048.0, -8192.0, 0, 1.0, 0.0, 0.0, 1e6);
        assert_eq!(h.target, TraceTarget::Wall(1));
        assert!((h.x - 4096.0).abs() < 1e-6);
        let h = hitscan(&s, &w, &[], 1000.0, 2048.0, -8192.0, 0, 1.0, 0.0, 8.0, 1e6);
        assert_eq!(h.target, TraceTarget::Floor(0));
        assert!((h.dist - 1024.0).abs() < 1e-6);
        let cyl = [HitCylinder {
            id: 7,
            x: 2000.0,
            y: 2048.0,
            radius: 64.0,
            ztop: -12000.0,
            zbot: 0.0,
        }];
        let h = hitscan(&s, &w, &cyl, 1000.0, 2048.0, -8192.0, 0, 1.0, 0.0, 0.0, 1e6);
        assert_eq!(h.target, TraceTarget::Sprite(7));
        assert!((h.dist - 936.0).abs() < 1e-6);
    }
}
