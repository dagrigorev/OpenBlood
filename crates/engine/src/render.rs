//! Resolution-independent software renderer for Build-style worlds.
//!
//! Each screen column casts one ray through the sector/portal graph. Walls, sloped
//! floors/ceilings and parallax skies are drawn front to back into a column-major
//! buffer with a per-pixel depth (perpendicular distance in xy units). Sprites and
//! masked walls are drawn afterwards, back to front, with a depth test. Columns are
//! independent, so both passes run in parallel on all CPU cores.

use crate::art::TileSet;
use crate::geom::{ang_to_rad, plane_eq, ray_segment, PlaneEq};
use crate::map::{Sector, Wall};
use crate::palette::NUM_SHADES;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU8, Ordering};

pub const TRANSPARENT: u8 = 255;

// wall cstat
pub const WALL_BLOCK: u16 = 1;
pub const WALL_BOTTOM_SWAP: u16 = 2;
pub const WALL_ALIGN_BOTTOM: u16 = 4;
pub const WALL_XFLIP: u16 = 8;
pub const WALL_MASKED: u16 = 16;
pub const WALL_ONEWAY: u16 = 32;
pub const WALL_TRANS: u16 = 128;
pub const WALL_YFLIP: u16 = 256;
pub const WALL_TRANS_REV: u16 = 512;

// sprite cstat
pub const SPR_BLOCK: u16 = 1;
pub const SPR_TRANS: u16 = 2;
pub const SPR_XFLIP: u16 = 4;
pub const SPR_YFLIP: u16 = 8;
pub const SPR_ALIGN_MASK: u16 = 48;
pub const SPR_WALL: u16 = 16;
pub const SPR_FLOOR: u16 = 32;
pub const SPR_ONESIDED: u16 = 64;
pub const SPR_CENTER: u16 = 128;
pub const SPR_TRANS_REV: u16 = 512;
pub const SPR_INVISIBLE: u16 = 32768;

// plane stat
pub const PLANE_PARALLAX: u16 = 1;
pub const PLANE_SWAPXY: u16 = 4;
pub const PLANE_SMOOSH: u16 = 8;
pub const PLANE_XFLIP: u16 = 16;
pub const PLANE_YFLIP: u16 = 32;
pub const PLANE_RELATIVE: u16 = 64;

#[derive(Debug, Clone, Copy, Default)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Build angle units (2048 per turn).
    pub ang: f64,
    /// Vertical look offset in 200-line units (0 = level, positive = look down).
    pub horiz: f64,
    pub sector: i32,
}

/// A sprite prepared by the game for drawing (picnum already animated/rotated if desired).
#[derive(Debug, Clone, Copy, Default)]
pub struct SpriteDraw {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub picnum: i32,
    pub cstat: u16,
    pub shade: i32,
    pub pal: u8,
    pub xrepeat: u8,
    pub yrepeat: u8,
    pub xoffset: i8,
    pub yoffset: i8,
    pub ang: f64,
    pub sector: i32,
    /// Voxel model index into [`Scene::voxels`] (drawn instead of the tile).
    pub voxel: Option<u32>,
}

/// True-colour shade tables, one per palookup (`pal`), each `64 * 256` entries.
pub struct ShadeLuts {
    pub tables: Vec<Option<Vec<u32>>>,
}

impl ShadeLuts {
    #[inline]
    pub fn get(&self, pal: u8) -> &[u32] {
        match &self.tables[pal as usize] {
            Some(t) => t,
            None => self.tables[0].as_deref().expect("palookup 0 missing"),
        }
    }
}

pub struct Scene<'a> {
    pub sectors: &'a [Sector],
    pub walls: &'a [Wall],
    pub sprites: &'a [SpriteDraw],
    pub tiles: &'a TileSet,
    pub luts: &'a ShadeLuts,
    pub sky_offsets: &'a [i16],
    pub sky_bits: i32,
    /// Global visibility (map header value; larger = darker with distance).
    pub visibility: i32,
    /// Game clock in 120 Hz ticks, used for tile animation.
    pub clock: i32,
    /// Voxel models by id.
    pub voxels: &'a [Option<crate::voxel::Voxel>],
}

#[derive(Clone, Copy)]
struct MaskSeg {
    t: f32,
    wall: u32,
    s: f32,
    ytop: f32,
    ybot: f32,
    sect: u32,
    next: u32,
}

#[derive(Clone, Copy)]
enum SprKind {
    Face {
        sx0: f32,
        sx1: f32,
        depth: f32,
    },
    Wall {
        ax: f64,
        ay: f64,
        bx: f64,
        by: f64,
        sx0: f32,
        sx1: f32,
    },
    Floor {
        sx0: f32,
        sx1: f32,
        cos: f64,
        sin: f64,
        hw: f64,
        hh: f64,
    },
    Voxel {
        id: u32,
        sx0: f32,
        sx1: f32,
        y0: f32,
        y1: f32,
        /// Width axis and facing axis (unit vectors), xy units per voxel, z units per voxel.
        w: (f64, f64),
        f: (f64, f64),
        unit: f64,
        zunit: f64,
    },
}

#[derive(Clone, Copy)]
struct SprProj {
    idx: usize,
    depth: f32,
    kind: SprKind,
    ztop: f64,
    zbot: f64,
    pic: i32,
}

struct View {
    w: usize,
    h: usize,
    fl: f64,
    fly: f64,
    horizon: f64,
    cos: f64,
    sin: f64,
}

impl View {
    #[inline]
    fn dir(&self, x: usize) -> (f64, f64) {
        let u = (x as f64 + 0.5 - self.w as f64 * 0.5) / self.fl;
        (self.cos - self.sin * u, self.sin + self.cos * u)
    }
    #[inline]
    fn proj_y(&self, z: f64, camz: f64, t: f64) -> f64 {
        self.horizon + (z - camz) / 16.0 / t * self.fly
    }
}

pub struct Renderer {
    pub width: usize,
    pub height: usize,
    /// Row-major output, 0x00RRGGBB.
    pub frame: Vec<u32>,
    col_pix: Vec<u32>,
    col_depth: Vec<f32>,
    masked: Vec<Vec<MaskSeg>>,
    visited: Vec<AtomicU8>,
    /// Horizontal sky texels per full turn multiplier (tuning knob).
    pub sky_scale: f64,
    /// Vertical stretch emulating the 320x200 pixel aspect (1.2 in the original).
    pub y_stretch: f64,
    pub stats_sectors: usize,
}

#[inline]
fn shade_index(shade: i32) -> usize {
    shade.clamp(0, NUM_SHADES as i32 - 1) as usize * 256
}

#[inline]
fn blend(dst: u32, src: u32, rev: bool) -> u32 {
    // Approximation of Build's 1/3 - 2/3 translucency.
    let (a, b) = if rev { (src, dst) } else { (dst, src) };
    let mix = |sh: u32| -> u32 {
        let x = (a >> sh) & 0xff;
        let y = (b >> sh) & 0xff;
        ((x * 2 + y) / 3) << sh
    };
    mix(16) | mix(8) | mix(0)
}

impl Renderer {
    pub fn new(width: usize, height: usize) -> Self {
        let mut r = Renderer {
            width: 0,
            height: 0,
            frame: Vec::new(),
            col_pix: Vec::new(),
            col_depth: Vec::new(),
            masked: Vec::new(),
            visited: Vec::new(),
            sky_scale: 1.0,
            y_stretch: 1.2,
            stats_sectors: 0,
        };
        r.resize(width, height);
        r
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.frame = vec![0; width * height];
        self.col_pix = vec![0; width * height];
        self.col_depth = vec![f32::INFINITY; width * height];
        self.masked = (0..width).map(|_| Vec::new()).collect();
    }

    fn view(&self, cam: &Camera) -> View {
        let w = self.width;
        let h = self.height;
        // Hor+ field of view: a 4:3 region of the screen maps to 90 degrees.
        let fl = h as f64 * 2.0 / 3.0;
        let a = ang_to_rad(cam.ang);
        View {
            w,
            h,
            fl,
            fly: fl * self.y_stretch,
            horizon: h as f64 * 0.5 - cam.horiz * h as f64 / 200.0,
            cos: a.cos(),
            sin: a.sin(),
        }
    }

    /// Distance in xy units for a depth value (used for fog/visibility shading).
    #[inline]
    fn vis_shade(vis: f64, t: f64) -> i32 {
        (t * vis / 1048576.0) as i32
    }

    pub fn render(&mut self, scene: &Scene, cam: &Camera) {
        let view = self.view(cam);
        let h = self.height;
        if self.visited.len() != scene.sectors.len() {
            self.visited = (0..scene.sectors.len()).map(|_| AtomicU8::new(0)).collect();
        } else {
            for v in &self.visited {
                v.store(0, Ordering::Relaxed);
            }
        }
        let sky_scale = self.sky_scale;
        let visited = &self.visited;

        // Pass 1: opaque world.
        self.col_pix
            .par_chunks_mut(h)
            .zip(self.col_depth.par_chunks_mut(h))
            .zip(self.masked.par_iter_mut())
            .enumerate()
            .for_each(|(x, ((pix, depth), masked))| {
                masked.clear();
                cast_column(x, scene, cam, &view, sky_scale, pix, depth, masked, visited);
            });

        self.stats_sectors = self.visited.iter().filter(|v| v.load(Ordering::Relaxed) != 0).count();

        // Project visible sprites.
        let mut sprites: Vec<SprProj> = scene
            .sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.cstat & SPR_INVISIBLE == 0
                    && s.sector >= 0
                    && (s.sector as usize) < self.visited.len()
                    && self.visited[s.sector as usize].load(Ordering::Relaxed) != 0
            })
            .filter_map(|(i, s)| project_sprite(i, s, scene, cam, &view))
            .collect();
        sprites.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));

        // Pass 2: masked walls and sprites, back to front.
        self.col_pix
            .par_chunks_mut(h)
            .zip(self.col_depth.par_chunks(h))
            .zip(self.masked.par_iter_mut())
            .enumerate()
            .for_each(|(x, ((pix, depth), masked))| {
                masked.sort_by(|a, b| b.t.partial_cmp(&a.t).unwrap_or(std::cmp::Ordering::Equal));
                let mut mi = 0;
                for sp in &sprites {
                    while mi < masked.len() && masked[mi].t > sp.depth {
                        draw_masked(x, &masked[mi], scene, cam, &view, pix, depth);
                        mi += 1;
                    }
                    draw_sprite_column(x, sp, scene, cam, &view, pix, depth);
                }
                while mi < masked.len() {
                    draw_masked(x, &masked[mi], scene, cam, &view, pix, depth);
                    mi += 1;
                }
            });

        // Transpose column-major into row-major output.
        let w = self.width;
        let col = &self.col_pix;
        self.frame.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, p) in row.iter_mut().enumerate() {
                *p = col[x * h + y];
            }
        });
    }

    /// Depth (perpendicular distance) at screen pixel, for hit tests and debugging.
    pub fn depth_at(&self, x: usize, y: usize) -> f32 {
        self.col_depth[x * self.height + y]
    }
}

struct PlaneCtx<'a> {
    eq: PlaneEq,
    plane: &'a crate::map::Plane,
    sector: &'a Sector,
    walls: &'a [Wall],
}

#[allow(clippy::too_many_arguments)]
fn cast_column(
    x: usize,
    scene: &Scene,
    cam: &Camera,
    view: &View,
    sky_scale: f64,
    pix: &mut [u32],
    depth: &mut [f32],
    masked: &mut Vec<MaskSeg>,
    visited: &[AtomicU8],
) {
    let h = view.h;
    let (dx, dy) = view.dir(x);
    let mut sect = cam.sector;
    let mut ytop = 0.0f64;
    let mut ybot = h as f64;
    let mut t_enter = 0.0f64;
    let mut filled_to_top = 0usize; // rows < this are final
    let _ = &mut filled_to_top;
    let sectors = scene.sectors;
    let walls = scene.walls;
    let vis = scene.visibility as f64;

    for _ in 0..1024 {
        if sect < 0 || sect as usize >= sectors.len() {
            break;
        }
        let si = sect as usize;
        visited[si].store(1, Ordering::Relaxed);
        let sec = &sectors[si];
        let vis_s = if sec.visibility != 0 {
            vis * ((sec.visibility as u32 + 16) & 255) as f64 / 16.0
        } else {
            vis
        };

        // Find exit wall: nearest crossing beyond the entry distance.
        let mut best: Option<(f64, f64, usize)> = None;
        let start = sec.wallptr as usize;
        let end = start + sec.wallnum as usize;
        for wi in start..end.min(walls.len()) {
            let w1 = &walls[wi];
            let w2 = &walls[w1.point2 as usize];
            if let Some((t, s)) = ray_segment(cam.x, cam.y, dx, dy, w1.x as f64, w1.y as f64, w2.x as f64, w2.y as f64)
            {
                if t > t_enter + 1e-7 && best.map_or(true, |b| t < b.0) {
                    best = Some((t, s, wi));
                }
            }
        }

        let ceq = plane_eq(sec, walls, &sec.ceiling);
        let feq = plane_eq(sec, walls, &sec.floor);
        let cctx = PlaneCtx {
            eq: ceq,
            plane: &sec.ceiling,
            sector: sec,
            walls,
        };
        let fctx = PlaneCtx {
            eq: feq,
            plane: &sec.floor,
            sector: sec,
            walls,
        };

        let Some((t, s, wi)) = best else {
            // Open-ended (degenerate) sector: fill the rest with planes.
            let mid = view.horizon.clamp(ytop, ybot);
            draw_plane(
                &cctx, scene, cam, view, sky_scale, dx, dy, x, ytop, mid, vis_s, pix, depth,
            );
            draw_plane(
                &fctx, scene, cam, view, sky_scale, dx, dy, x, mid, ybot, vis_s, pix, depth,
            );
            break;
        };
        let t = t.max(1e-3);
        let hx = cam.x + dx * t;
        let hy = cam.y + dy * t;
        let cz = ceq.z_at(hx, hy);
        let fz = feq.z_at(hx, hy);
        let yc = view.proj_y(cz, cam.z, t);
        let yf = view.proj_y(fz, cam.z, t);

        draw_plane(
            &cctx,
            scene,
            cam,
            view,
            sky_scale,
            dx,
            dy,
            x,
            ytop,
            yc.min(ybot),
            vis_s,
            pix,
            depth,
        );
        draw_plane(
            &fctx,
            scene,
            cam,
            view,
            sky_scale,
            dx,
            dy,
            x,
            yf.max(ytop),
            ybot,
            vis_s,
            pix,
            depth,
        );

        let wall = &walls[wi];
        let ns = wall.nextsector;
        let wlen = {
            let w2 = &walls[wall.point2 as usize];
            ((w2.x - wall.x) as f64).hypot((w2.y - wall.y) as f64)
        };
        let _ = wlen;
        if ns < 0 || ns as usize >= sectors.len() {
            let zref = if wall.cstat & WALL_ALIGN_BOTTOM != 0 {
                sec.floor.z
            } else {
                sec.ceiling.z
            } as f64;
            draw_wall_span(
                wall,
                wall.picnum,
                wall.xpanning,
                wall.ypanning,
                s,
                t,
                zref,
                ytop.max(yc),
                ybot.min(yf),
                scene,
                cam,
                view,
                vis_s,
                pix,
                depth,
            );
            break;
        }
        let nsec = &sectors[ns as usize];
        let nceq = plane_eq(nsec, walls, &nsec.ceiling);
        let nfeq = plane_eq(nsec, walls, &nsec.floor);
        let ncz = nceq.z_at(hx, hy);
        let nfz = nfeq.z_at(hx, hy);
        let ync = view.proj_y(ncz, cam.z, t);
        let ynf = view.proj_y(nfz, cam.z, t);
        let both_sky_c = sec.ceiling.stat & PLANE_PARALLAX != 0 && nsec.ceiling.stat & PLANE_PARALLAX != 0;
        let both_sky_f = sec.floor.stat & PLANE_PARALLAX != 0 && nsec.floor.stat & PLANE_PARALLAX != 0;

        // Upper wall.
        if !both_sky_c && ync > yc {
            let zref = if wall.cstat & WALL_ALIGN_BOTTOM != 0 {
                sec.ceiling.z
            } else {
                nsec.ceiling.z
            } as f64;
            draw_wall_span(
                wall,
                wall.picnum,
                wall.xpanning,
                wall.ypanning,
                s,
                t,
                zref,
                ytop.max(yc),
                ybot.min(ync),
                scene,
                cam,
                view,
                vis_s,
                pix,
                depth,
            );
        }
        // Lower wall.
        if !both_sky_f && ynf < yf {
            let (pic, xp, yp, src) = if wall.cstat & WALL_BOTTOM_SWAP != 0 && wall.nextwall >= 0 {
                let nw = &walls[wall.nextwall as usize];
                (nw.picnum, nw.xpanning, nw.ypanning, nw)
            } else {
                (wall.picnum, wall.xpanning, wall.ypanning, wall)
            };
            let zref = if src.cstat & WALL_ALIGN_BOTTOM != 0 {
                sec.ceiling.z
            } else {
                nsec.floor.z
            } as f64;
            draw_wall_span(
                wall,
                pic,
                xp,
                yp,
                s,
                t,
                zref,
                ytop.max(ynf),
                ybot.min(yf),
                scene,
                cam,
                view,
                vis_s,
                pix,
                depth,
            );
        }

        if !both_sky_c {
            ytop = ytop.max(yc).max(ync);
        }
        if !both_sky_f {
            ybot = ybot.min(yf).min(ynf);
        }
        if wall.cstat & WALL_MASKED != 0 && ytop < ybot {
            masked.push(MaskSeg {
                t: t as f32,
                wall: wi as u32,
                s: s as f32,
                ytop: ytop as f32,
                ybot: ybot as f32,
                sect: si as u32,
                next: ns as u32,
            });
        }
        if ytop >= ybot {
            break;
        }
        t_enter = t;
        sect = ns as i32;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_plane(
    p: &PlaneCtx,
    scene: &Scene,
    cam: &Camera,
    view: &View,
    sky_scale: f64,
    dx: f64,
    dy: f64,
    x: usize,
    y0: f64,
    y1: f64,
    vis: f64,
    pix: &mut [u32],
    depth: &mut [f32],
) {
    let h = view.h as f64;
    let r0 = y0.max(0.0).ceil() as usize;
    let r1 = (y1.min(h).ceil() as isize).max(0) as usize;
    if r0 >= r1 {
        return;
    }
    let pic = scene.tiles.animate(p.plane.picnum as i32, scene.clock);
    let a = p.eq.z_at(cam.x, cam.y);
    let b = p.eq.bx * dx + p.eq.by * dy;
    let lut = scene.luts.get(p.plane.pal);

    if p.plane.stat & PLANE_PARALLAX != 0 {
        draw_sky(
            p, pic, scene, cam, view, sky_scale, dx, dy, x, r0, r1, a, b, lut, pix, depth,
        );
        return;
    }

    let tile = scene.tiles.get(pic);
    let shade = p.plane.shade as i32;
    if tile.is_empty() {
        for y in r0..r1 {
            pix[y] = 0;
            depth[y] = f32::INFINITY;
        }
        return;
    }
    let tw = tile.width as i64;
    let th = tile.height as i64;
    let stat = p.plane.stat;
    let texel = if stat & PLANE_SMOOSH != 0 { 8.0 } else { 16.0 };

    // Relative alignment: rotate into the first wall's frame.
    let (ox, oy, ux, uy) = if stat & PLANE_RELATIVE != 0 && p.sector.wallnum >= 2 {
        let w1 = &p.walls[p.sector.wallptr as usize];
        let w2 = &p.walls[w1.point2 as usize];
        let ex = (w2.x - w1.x) as f64;
        let ey = (w2.y - w1.y) as f64;
        let l = ex.hypot(ey).max(1e-9);
        (w1.x as f64, w1.y as f64, ex / l, ey / l)
    } else {
        (0.0, 0.0, 1.0, 0.0)
    };
    let xpan = p.plane.xpan as f64 * tw as f64 / 256.0;
    let ypan = p.plane.ypan as f64 * th as f64 / 256.0;

    for y in r0..r1 {
        let k = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
        let den = k - b;
        if den.abs() < 1e-12 {
            pix[y] = 0;
            depth[y] = f32::INFINITY;
            continue;
        }
        let mut t = (a - cam.z) / den;
        if !(t > 0.0) {
            t = 1e9;
        }
        let px = cam.x + dx * t - ox;
        let py = cam.y + dy * t - oy;
        let (mut u, mut v) = if stat & PLANE_RELATIVE != 0 {
            (px * ux + py * uy, -px * uy + py * ux)
        } else {
            (px, py)
        };
        if stat & PLANE_SWAPXY != 0 {
            std::mem::swap(&mut u, &mut v);
        }
        if stat & PLANE_XFLIP != 0 {
            u = -u;
        }
        if stat & PLANE_YFLIP != 0 {
            v = -v;
        }
        let tu = ((u / texel + xpan).floor() as i64).rem_euclid(tw);
        let tv = ((v / texel + ypan).floor() as i64).rem_euclid(th);
        let c = tile.pixels[(tu * th + tv) as usize];
        let sh = shade + Renderer::vis_shade(vis, t);
        pix[y] = lut[shade_index(sh) + c as usize];
        depth[y] = t as f32;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_sky(
    p: &PlaneCtx,
    pic: i32,
    scene: &Scene,
    cam: &Camera,
    view: &View,
    sky_scale: f64,
    dx: f64,
    dy: f64,
    _x: usize,
    r0: usize,
    r1: usize,
    a: f64,
    b: f64,
    lut: &[u32],
    pix: &mut [u32],
    depth: &mut [f32],
) {
    let bits = scene.sky_bits.clamp(0, 10);
    let segs = 1i64 << bits;
    // Angle of this ray, 0..1 turn.
    let ang = dy.atan2(dx) / std::f64::consts::TAU;
    let base = scene.tiles.get(pic);
    let tw = base.width.max(1) as f64;
    let total = tw * segs as f64 * sky_scale;
    let texx = (ang.rem_euclid(1.0) * total).floor() as i64;
    let seg = (texx / tw as i64).rem_euclid(segs) as usize;
    let off = scene.sky_offsets.get(seg).copied().unwrap_or(0) as i32;
    let tile = scene.tiles.get(pic + off);
    let shade = p.plane.shade as i32;
    let sh = shade_index(shade);
    let _ = cam;
    if tile.is_empty() {
        for y in r0..r1 {
            pix[y] = lut[sh];
            depth[y] = f32::INFINITY;
        }
        return;
    }
    let th = tile.height as f64;
    let col = tile.column((texx.rem_euclid(tile.width as i64)) as usize);
    // Sky is a cylinder at infinity: one texel per original screen line, horizon near
    // the lower part of the sky tile.
    let px_per_texel = view.h as f64 / 200.0 * view.fly / (view.h as f64 * 0.8);
    for y in r0..r1 {
        let v = (y as f64 + 0.5 - view.horizon) / px_per_texel + th * 0.5;
        let tv = (v.floor() as i64).clamp(0, th as i64 - 1) as usize;
        pix[y] = lut[sh + col[tv] as usize];
        let k = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
        let den = k - b;
        let t = if den.abs() > 1e-12 { (a - cam.z) / den } else { -1.0 };
        depth[y] = if t > 0.0 { t as f32 } else { f32::INFINITY };
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_wall_span(
    wall: &Wall,
    picnum: i16,
    xpan: u8,
    ypan: u8,
    s: f64,
    t: f64,
    zref: f64,
    y0: f64,
    y1: f64,
    scene: &Scene,
    cam: &Camera,
    view: &View,
    vis: f64,
    pix: &mut [u32],
    depth: &mut [f32],
) {
    let h = view.h as f64;
    let r0 = y0.max(0.0).ceil() as usize;
    let r1 = (y1.min(h).ceil() as isize).max(0) as usize;
    if r0 >= r1 {
        return;
    }
    let pic = scene.tiles.animate(picnum as i32, scene.clock);
    let tile = scene.tiles.get(pic);
    let lut = scene.luts.get(wall.pal);
    let sh = shade_index(wall.shade as i32 + Renderer::vis_shade(vis, t));
    if tile.is_empty() {
        for y in r0..r1 {
            pix[y] = lut[sh];
            depth[y] = t as f32;
        }
        return;
    }
    let tw = tile.width as i64;
    let th = tile.height as i64;
    let mut tx = ((s * wall.xrepeat as f64 * 8.0).floor() as i64 + xpan as i64).rem_euclid(tw);
    if wall.cstat & WALL_XFLIP != 0 {
        tx = tw - 1 - tx;
    }
    let col = tile.column(tx as usize);
    let yrep = wall.yrepeat as f64 / 1024.0;
    let pan = ypan as f64 * th as f64 / 256.0;
    let dz_per_row = 16.0 * t / view.fly;
    let yflip = wall.cstat & WALL_YFLIP != 0;
    let tf = t as f32;
    for y in r0..r1 {
        let z = cam.z + (y as f64 + 0.5 - view.horizon) * dz_per_row;
        let mut v = ((z - zref) * yrep + pan).floor() as i64;
        if yflip {
            v = -v - 1;
        }
        let c = col[v.rem_euclid(th) as usize];
        pix[y] = lut[sh + c as usize];
        depth[y] = tf;
    }
}

fn draw_masked(x: usize, m: &MaskSeg, scene: &Scene, cam: &Camera, view: &View, pix: &mut [u32], depth: &[f32]) {
    let _ = x;
    let wall = &scene.walls[m.wall as usize];
    let sec = &scene.sectors[m.sect as usize];
    let nsec = &scene.sectors[m.next as usize];
    let pic = scene.tiles.animate(wall.overpicnum as i32, scene.clock);
    let tile = scene.tiles.get(pic);
    if tile.is_empty() {
        return;
    }
    let h = view.h as f64;
    let r0 = (m.ytop as f64).max(0.0).ceil() as usize;
    let r1 = ((m.ybot as f64).min(h).ceil() as isize).max(0) as usize;
    let t = m.t as f64;
    let vis = scene.visibility as f64;
    let lut = scene.luts.get(wall.pal);
    let sh = shade_index(wall.shade as i32 + Renderer::vis_shade(vis, t));
    let tw = tile.width as i64;
    let th = tile.height as i64;
    let mut tx = ((m.s as f64 * wall.xrepeat as f64 * 8.0).floor() as i64 + wall.xpanning as i64).rem_euclid(tw);
    if wall.cstat & WALL_XFLIP != 0 {
        tx = tw - 1 - tx;
    }
    let col = tile.column(tx as usize);
    let zref = if wall.cstat & WALL_ALIGN_BOTTOM != 0 {
        sec.floor.z.min(nsec.floor.z)
    } else {
        sec.ceiling.z.max(nsec.ceiling.z)
    } as f64;
    let yrep = wall.yrepeat as f64 / 1024.0;
    let pan = wall.ypanning as f64 * th as f64 / 256.0;
    let dz_per_row = 16.0 * t / view.fly;
    let trans = wall.cstat & WALL_TRANS != 0;
    let rev = wall.cstat & WALL_TRANS_REV != 0;
    let yflip = wall.cstat & WALL_YFLIP != 0;
    for y in r0..r1 {
        if m.t >= depth[y] {
            continue;
        }
        let z = cam.z + (y as f64 + 0.5 - view.horizon) * dz_per_row;
        let mut v = ((z - zref) * yrep + pan).floor() as i64;
        if yflip {
            v = -v - 1;
        }
        let c = col[v.rem_euclid(th) as usize];
        if c == TRANSPARENT {
            continue;
        }
        let rgb = lut[sh + c as usize];
        pix[y] = if trans { blend(pix[y], rgb, rev) } else { rgb };
    }
}

/// Sprite world extents: (half width in xy, height in z, left offset texels).
fn sprite_dims(s: &SpriteDraw, tw: f64, th: f64, anm_x: i8, anm_y: i8) -> (f64, f64, f64, f64) {
    let xs = s.xrepeat as f64 / 4.0;
    let zs = s.yrepeat as f64 * 4.0;
    let xoff = anm_x as f64 + s.xoffset as f64;
    let yoff = anm_y as f64 + s.yoffset as f64;
    let height = th * zs;
    let mut zbot = s.z;
    if s.cstat & SPR_CENTER != 0 {
        zbot += height * 0.5;
    }
    zbot -= yoff * zs;
    // left edge offset in xy units relative to the sprite origin
    let left = -(tw * 0.5 + xoff) * xs;
    (left, tw * xs, zbot - height, zbot)
}

fn project_voxel(idx: usize, s: &SpriteDraw, id: u32, v: &crate::voxel::Voxel, cam: &Camera, view: &View) -> Option<SprProj> {
    let a = ang_to_rad(s.ang);
    let w = (a.sin(), -a.cos());
    let f = (a.cos(), a.sin());
    let unit = s.xrepeat as f64 / 4.0;
    let zunit = s.yrepeat as f64 * 4.0;
    if unit <= 0.0 || zunit <= 0.0 {
        return None;
    }
    let height = v.zs as f64 * zunit;
    let mut zbot = s.z;
    if s.cstat & SPR_CENTER != 0 {
        zbot += height * 0.5;
    }
    let ztop = zbot - height;
    // Project the model's bounding box.
    let half_w = view.w as f64 * 0.5;
    let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    let mut behind = false;
    let mut depth = f64::INFINITY;
    for cx in [0.0, v.xs as f64] {
        for cy in [0.0, v.ys as f64] {
            let (mx, my) = ((cx - v.pivot.0) * unit, (cy - v.pivot.1) * unit);
            let wx = s.x + w.0 * mx + f.0 * my;
            let wy = s.y + w.1 * mx + f.1 * my;
            let (rx, ry) = (wx - cam.x, wy - cam.y);
            let fw = rx * view.cos + ry * view.sin;
            let sd = -rx * view.sin + ry * view.cos;
            if fw < 1.0 {
                behind = true;
                continue;
            }
            depth = depth.min(fw);
            let sx = half_w + sd / fw * view.fl;
            x0 = x0.min(sx);
            x1 = x1.max(sx);
            for z in [ztop, zbot] {
                let sy = view.proj_y(z, cam.z, fw);
                y0 = y0.min(sy);
                y1 = y1.max(sy);
            }
        }
    }
    if behind {
        x0 = 0.0;
        x1 = view.w as f64;
        y0 = 0.0;
        y1 = view.h as f64;
        depth = 1.0;
    }
    if !x0.is_finite() {
        return None;
    }
    Some(SprProj {
        idx,
        depth: (s.x - cam.x).hypot(s.y - cam.y).max(depth.min(1e9)) as f32,
        kind: SprKind::Voxel { id, sx0: x0 as f32, sx1: x1 as f32, y0: y0 as f32, y1: y1 as f32, w, f, unit, zunit },
        ztop,
        zbot,
        pic: s.picnum,
    })
}

fn project_sprite(idx: usize, s: &SpriteDraw, scene: &Scene, cam: &Camera, view: &View) -> Option<SprProj> {
    if let Some(id) = s.voxel {
        if let Some(Some(v)) = scene.voxels.get(id as usize) {
            return project_voxel(idx, s, id, v, cam, view);
        }
    }
    let pic = scene.tiles.animate(s.picnum, scene.clock);
    let tile = scene.tiles.get(pic);
    if tile.is_empty() || s.xrepeat == 0 || s.yrepeat == 0 {
        return None;
    }
    let tw = tile.width as f64;
    let th = tile.height as f64;
    let (left, width, ztop, zbot) = sprite_dims(s, tw, th, tile.anm.xoffset, tile.anm.yoffset);
    let rx = s.x - cam.x;
    let ry = s.y - cam.y;
    let fwd = rx * view.cos + ry * view.sin;
    let side = -rx * view.sin + ry * view.cos;
    let half_w = view.w as f64 * 0.5;
    match s.cstat & SPR_ALIGN_MASK {
        SPR_WALL => {
            let a = ang_to_rad(s.ang);
            // Wall sprites extend perpendicular to their facing angle.
            let (px, py) = (a.sin(), -a.cos());
            let ax = s.x + px * left;
            let ay = s.y + py * left;
            let bx = ax + px * width;
            let by = ay + py * width;
            if s.cstat & SPR_ONESIDED != 0 {
                // Visible only from the front side (facing direction).
                let nx = a.cos();
                let ny = a.sin();
                if (cam.x - s.x) * nx + (cam.y - s.y) * ny < 0.0 {
                    return None;
                }
            }
            let pa = |x: f64, y: f64| -> Option<f64> {
                let rx = x - cam.x;
                let ry = y - cam.y;
                let f = rx * view.cos + ry * view.sin;
                let sd = -rx * view.sin + ry * view.cos;
                (f > 0.5).then(|| half_w + sd / f * view.fl)
            };
            let (sx0, sx1) = match (pa(ax, ay), pa(bx, by)) {
                (Some(p), Some(q)) => (p.min(q), p.max(q)),
                (Some(p), None) | (None, Some(p)) => {
                    let _ = p;
                    (0.0, view.w as f64)
                }
                (None, None) => return None,
            };
            Some(SprProj {
                idx,
                depth: fwd.max(1.0) as f32,
                kind: SprKind::Wall {
                    ax,
                    ay,
                    bx,
                    by,
                    sx0: sx0 as f32,
                    sx1: sx1 as f32,
                },
                ztop,
                zbot,
                pic,
            })
        }
        SPR_FLOOR => {
            let a = ang_to_rad(s.ang);
            let hw = width * 0.5;
            let hh = th * s.yrepeat as f64 / 4.0 * 0.5;
            // Bounding columns from the projected corners.
            let (c, sn) = (a.cos(), a.sin());
            let mut minx = f64::INFINITY;
            let mut maxx = f64::NEG_INFINITY;
            let mut any_behind = false;
            for (u, v) in [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)] {
                let wx = s.x + u * c - v * sn;
                let wy = s.y + u * sn + v * c;
                let rx = wx - cam.x;
                let ry = wy - cam.y;
                let f = rx * view.cos + ry * view.sin;
                let sd = -rx * view.sin + ry * view.cos;
                if f <= 1.0 {
                    any_behind = true;
                    continue;
                }
                let sx = half_w + sd / f * view.fl;
                minx = minx.min(sx);
                maxx = maxx.max(sx);
            }
            if any_behind {
                minx = 0.0;
                maxx = view.w as f64;
            }
            if !minx.is_finite() {
                return None;
            }
            Some(SprProj {
                idx,
                depth: fwd.max(1.0) as f32,
                kind: SprKind::Floor {
                    sx0: minx as f32,
                    sx1: maxx as f32,
                    cos: c,
                    sin: sn,
                    hw,
                    hh,
                },
                ztop: s.z,
                zbot: s.z,
                pic,
            })
        }
        _ => {
            if fwd < 4.0 {
                return None;
            }
            let sx_center = half_w + side / fwd * view.fl;
            let scale = view.fl / fwd;
            let sx0 = sx_center + left * scale;
            let sx1 = sx0 + width * scale;
            Some(SprProj {
                idx,
                depth: fwd as f32,
                kind: SprKind::Face {
                    sx0: sx0 as f32,
                    sx1: sx1 as f32,
                    depth: fwd as f32,
                },
                ztop,
                zbot,
                pic,
            })
        }
    }
}

fn draw_sprite_column(
    x: usize,
    sp: &SprProj,
    scene: &Scene,
    cam: &Camera,
    view: &View,
    pix: &mut [u32],
    depth: &[f32],
) {
    let s = &scene.sprites[sp.idx];
    let tile = scene.tiles.get(sp.pic);
    let tw = tile.width as i64;
    let th = tile.height as i64;
    let xc = x as f32 + 0.5;
    let lut = scene.luts.get(s.pal);
    let vis = scene.visibility as f64;
    let trans = s.cstat & SPR_TRANS != 0;
    let rev = s.cstat & SPR_TRANS_REV != 0;
    let xflip = s.cstat & SPR_XFLIP != 0;
    let yflip = s.cstat & SPR_YFLIP != 0;
    let h = view.h as f64;

    let column = |t: f64, u: f64, pix: &mut [u32]| {
        // u in [0,1) across the sprite
        let mut tx = (u * tw as f64).floor() as i64;
        if !(0..tw).contains(&tx) {
            return;
        }
        if xflip {
            tx = tw - 1 - tx;
        }
        let col = tile.column(tx as usize);
        let y0 = view.proj_y(sp.ztop, cam.z, t);
        let y1 = view.proj_y(sp.zbot, cam.z, t);
        let r0 = y0.max(0.0).ceil() as usize;
        let r1 = (y1.min(h).ceil() as isize).max(0) as usize;
        if r0 >= r1 {
            return;
        }
        let sh = shade_index(s.shade + Renderer::vis_shade(vis, t));
        let inv = th as f64 / (y1 - y0);
        let tf = t as f32;
        for y in r0..r1 {
            if tf >= depth[y] {
                continue;
            }
            let mut ty = ((y as f64 + 0.5 - y0) * inv).floor() as i64;
            ty = ty.clamp(0, th - 1);
            if yflip {
                ty = th - 1 - ty;
            }
            let c = col[ty as usize];
            if c == TRANSPARENT {
                continue;
            }
            let rgb = lut[sh + c as usize];
            pix[y] = if trans { blend(pix[y], rgb, rev) } else { rgb };
        }
    };

    match sp.kind {
        SprKind::Face { sx0, sx1, depth: d } => {
            if xc < sx0 || xc >= sx1 {
                return;
            }
            let u = ((xc - sx0) / (sx1 - sx0)) as f64;
            column(d as f64, u, pix);
        }
        SprKind::Wall {
            ax,
            ay,
            bx,
            by,
            sx0,
            sx1,
        } => {
            if xc < sx0 - 1.0 || xc > sx1 + 1.0 {
                return;
            }
            let (dx, dy) = view.dir(x);
            if let Some((t, u)) = ray_segment(cam.x, cam.y, dx, dy, ax, ay, bx, by) {
                if t > 0.5 {
                    column(t, u.min(0.999_999), pix);
                }
            }
        }
        SprKind::Voxel { id, sx0, sx1, y0, y1, w, f, unit, zunit } => {
            if xc < sx0 - 1.0 || xc > sx1 + 1.0 {
                return;
            }
            let Some(Some(v)) = scene.voxels.get(id as usize) else { return };
            let (dx, dy) = view.dir(x);
            // Ray origin and direction in model space.
            let (lx, ly) = (cam.x - s.x, cam.y - s.y);
            let ox = (lx * w.0 + ly * w.1) / unit + v.pivot.0;
            let oy = (lx * f.0 + ly * f.1) / unit + v.pivot.1;
            let oz = (cam.z - sp.ztop) / zunit;
            let ddx = (dx * w.0 + dy * w.1) / unit;
            let ddy = (dx * f.0 + dy * f.1) / unit;
            let r0 = (y0 as f64).max(0.0).floor() as usize;
            let r1 = ((y1 as f64).ceil() as isize).clamp(0, view.h as isize) as usize;
            for y in r0..r1 {
                let dzz = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
                let Some((t, c)) = v.cast([ox, oy, oz], [ddx, ddy, dzz / zunit]) else { continue };
                if t <= 0.0 || t as f32 >= depth[y] {
                    continue;
                }
                let sh = shade_index(s.shade + Renderer::vis_shade(vis, t));
                let rgb = lut[sh + c as usize];
                pix[y] = if trans { blend(pix[y], rgb, rev) } else { rgb };
            }
        }
        SprKind::Floor {
            sx0,
            sx1,
            cos,
            sin,
            hw,
            hh,
        } => {
            if xc < sx0 - 1.0 || xc > sx1 + 1.0 {
                return;
            }
            let (dx, dy) = view.dir(x);
            let dz = sp.zbot - cam.z;
            if dz.abs() < 1.0 {
                return;
            }
            let sh_base = s.shade;
            let (r0, r1) = if dz > 0.0 {
                (view.horizon.max(0.0).ceil() as usize, view.h)
            } else {
                (0, (view.horizon.min(h).ceil() as isize).max(0) as usize)
            };
            for y in r0..r1 {
                let k = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
                if k.abs() < 1e-12 {
                    continue;
                }
                let t = dz / k;
                if t <= 0.0 || t as f32 >= depth[y] {
                    continue;
                }
                let wx = cam.x + dx * t - s.x;
                let wy = cam.y + dy * t - s.y;
                let lu = wx * cos + wy * sin;
                let lv = -wx * sin + wy * cos;
                if lu.abs() >= hw || lv.abs() >= hh {
                    continue;
                }
                let mut tx = (((lu + hw) / (2.0 * hw)) * tw as f64) as i64;
                let mut ty = (((lv + hh) / (2.0 * hh)) * th as f64) as i64;
                tx = tx.clamp(0, tw - 1);
                ty = ty.clamp(0, th - 1);
                if xflip {
                    tx = tw - 1 - tx;
                }
                if yflip {
                    ty = th - 1 - ty;
                }
                let c = tile.pixels[(tx * th + ty) as usize];
                if c == TRANSPARENT {
                    continue;
                }
                let sh = shade_index(sh_base + Renderer::vis_shade(vis, t));
                let rgb = lut[sh + c as usize];
                pix[y] = if trans { blend(pix[y], rgb, rev) } else { rgb };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::art::{Tile, TileSet};
    use crate::map::{Sector, Wall};

    fn room() -> (Vec<Sector>, Vec<Wall>) {
        let pts = [(-1024, -1024), (1024, -1024), (1024, 1024), (-1024, 1024)];
        let walls: Vec<Wall> = pts
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Wall {
                x,
                y,
                point2: ((i + 1) % 4) as i16,
                nextwall: -1,
                nextsector: -1,
                picnum: 1,
                xrepeat: 8,
                yrepeat: 8,
                ..Default::default()
            })
            .collect();
        let mut s = Sector {
            wallptr: 0,
            wallnum: 4,
            ..Default::default()
        };
        s.ceiling.z = -16384;
        s.floor.z = 0;
        s.ceiling.picnum = 2;
        s.floor.picnum = 3;
        (vec![s], walls)
    }

    fn tiles() -> TileSet {
        let mut t = TileSet::default();
        for (i, c) in [(1usize, 10u8), (2, 20), (3, 30)] {
            t.tiles[i] = Tile {
                width: 8,
                height: 8,
                anm: Default::default(),
                pixels: vec![c; 64],
            };
        }
        t
    }

    fn luts() -> ShadeLuts {
        // Encode the palette index directly as the colour so tests can inspect it.
        let mut t = vec![0u32; 64 * 256];
        for s in 0..64 {
            for c in 0..256 {
                t[s * 256 + c] = c as u32;
            }
        }
        let mut tables = vec![None; 256];
        tables[0] = Some(t);
        ShadeLuts { tables }
    }

    #[test]
    fn renders_wall_floor_ceiling() {
        let (sectors, walls) = room();
        let tiles = tiles();
        let luts = luts();
        let scene = Scene {
            sectors: &sectors,
            walls: &walls,
            sprites: &[],
            tiles: &tiles,
            luts: &luts,
            sky_offsets: &[0],
            sky_bits: 0,
            visibility: 0,
            clock: 0,
        };
        let cam = Camera {
            x: 0.0,
            y: 0.0,
            z: -8192.0,
            ang: 0.0,
            horiz: 0.0,
            sector: 0,
        };
        let mut r = Renderer::new(64, 48);
        r.render(&scene, &cam);
        let at = |x: usize, y: usize| r.frame[y * 64 + x];
        assert_eq!(at(32, 0), 20, "ceiling at top");
        assert_eq!(at(32, 47), 30, "floor at bottom");
        assert_eq!(at(32, 24), 10, "wall in the middle");
        // Wall is 1024 units ahead.
        assert!((r.depth_at(32, 24) - 1024.0).abs() < 1.0);
    }

    #[test]
    fn sprite_occludes_wall() {
        let (sectors, walls) = room();
        let mut tiles = tiles();
        tiles.tiles[4] = Tile {
            width: 16,
            height: 16,
            anm: Default::default(),
            pixels: vec![40; 256],
        };
        let luts = luts();
        let spr = [SpriteDraw {
            x: 512.0,
            y: 0.0,
            z: 0.0,
            picnum: 4,
            xrepeat: 64,
            yrepeat: 64,
            sector: 0,
            ..Default::default()
        }];
        let scene = Scene {
            sectors: &sectors,
            walls: &walls,
            sprites: &spr,
            tiles: &tiles,
            luts: &luts,
            sky_offsets: &[0],
            sky_bits: 0,
            visibility: 0,
            clock: 0,
        };
        let cam = Camera {
            x: 0.0,
            y: 0.0,
            z: -2048.0,
            ang: 0.0,
            horiz: 0.0,
            sector: 0,
        };
        let mut r = Renderer::new(64, 48);
        r.render(&scene, &cam);
        assert_eq!(r.frame[30 * 64 + 32], 40);
    }
}
