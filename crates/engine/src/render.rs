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
    /// Non-zero for sprites seen through a room-over-room portal: only drawn on pixels
    /// that show that portal (see [`portal_id`]).
    pub portal: u32,
}

/// Id of the see-through plane of sector `s` (floor or ceiling) in the portal mask.
pub fn portal_id(s: usize, floor: bool) -> u32 {
    1 + s as u32 * 2 + floor as u32
}

/// Room-over-room link: looking through a floor/ceiling continues in `sector`, offset by
/// (dx, dy, dz) (Blood's stack / link markers).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct RorLink {
    pub sector: i32,
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct Ror {
    pub floor: Option<RorLink>,
    pub ceil: Option<RorLink>,
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
    /// See-through floor/ceiling links per sector (may be empty).
    pub ror: &'a [Ror],
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
        /// Screen rows the sprite can cover (from its projected corners).
        ry0: f32,
        ry1: f32,
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
        /// Nearest depth any part of the model can have (pixels already covered by
        /// something nearer skip the ray cast).
        tmin: f32,
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
    /// Per screen row: vertical ray slope `k` (z units per unit of depth) and `1 / k`.
    /// A flat plane's depth on a row is `height / k`, so it costs a multiply, not a divide.
    row_k: Vec<f64>,
    row_inv_k: Vec<f64>,
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
    /// Per pixel: id of the portal it was drawn through (0 = none).
    col_mask: Vec<u32>,
    masked: Vec<Vec<MaskSeg>>,
    visited: Vec<AtomicU8>,
    /// Horizontal sky texels per full turn multiplier (tuning knob).
    pub sky_scale: f64,
    /// Vertical stretch emulating the 320x200 pixel aspect (1.2 in the original).
    pub y_stretch: f64,
    /// Milliseconds spent in the last frame: walls/planes, portal sprites + projection,
    /// masked walls + sprites, output copy.
    pub phase_ms: [f64; 4],
    /// Also produce the row-major [`Renderer::frame`]. Callers that scale the column-major
    /// image themselves ([`Renderer::columns`]) turn this off to skip a full-frame copy.
    pub keep_rows: bool,
    /// Portal ids seen in each column during the wall pass.
    col_portals: Vec<Vec<u32>>,
    pub stats_sectors: usize,
    /// Projected sprites by kind (face, wall, floor, voxel) and masked wall segments.
    pub stats_kinds: [usize; 5],
}

/// Distance (xy units) times visibility per shade step.
const VIS_DIVISOR: f64 = 5.0 * 65536.0;

/// Visibility of sector `si`: the map's global value scaled by the sector's own, stored as a
/// signed byte offset (0 = use the global value).
fn sector_vis(scene: &Scene, si: i32) -> f64 {
    let vis = scene.visibility as f64;
    match usize::try_from(si).ok().and_then(|i| scene.sectors.get(i)) {
        Some(sec) if sec.visibility != 0 => vis * ((sec.visibility as u32 + 16) & 255) as f64 / 16.0,
        _ => vis,
    }
}

#[inline]
fn shade_index(shade: i32) -> usize {
    shade.clamp(0, NUM_SHADES as i32 - 1) as usize * 256
}

/// Euclidean `v mod n`, using a mask when `n` is a power of two (almost every Build
/// texture): an integer division per pixel was a large part of the wall/floor cost.
#[inline(always)]
fn wrap(v: i64, n: i64) -> i64 {
    if n & (n - 1) == 0 {
        v & (n - 1)
    } else {
        v.rem_euclid(n)
    }
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
            col_mask: Vec::new(),
            masked: Vec::new(),
            visited: Vec::new(),
            sky_scale: 1.0,
            // Vertical/horizontal focal ratio; 1.0 matched the original best (see EYE_STAND).
            y_stretch: 1.0,
            phase_ms: [0.0; 4],
            keep_rows: true,
            col_portals: Vec::new(),
            stats_sectors: 0,
            stats_kinds: [0; 5],
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
        self.col_mask = vec![0; width * height];
        self.masked = (0..width).map(|_| Vec::new()).collect();
        self.col_portals = (0..width).map(|_| Vec::new()).collect();
    }

    /// The last frame in column-major order: (pixels, width, height), `pixels[x * h + y]`.
    pub fn columns(&self) -> (&[u32], usize, usize) {
        (&self.col_pix, self.width, self.height)
    }

    fn view(&self, cam: &Camera) -> View {
        let w = self.width;
        let h = self.height;
        // Hor+ field of view: a 4:3 region of the screen maps to 90 degrees.
        let fl = h as f64 * 2.0 / 3.0;
        let a = ang_to_rad(cam.ang);
        let fly = fl * self.y_stretch;
        let horizon = h as f64 * 0.5 - cam.horiz * h as f64 / 200.0;
        let row_k: Vec<f64> = (0..h).map(|y| (y as f64 + 0.5 - horizon) / fly * 16.0).collect();
        let row_inv_k = row_k
            .iter()
            .map(|&k| if k.abs() < 1e-12 { 0.0 } else { 1.0 / k })
            .collect();
        View {
            w,
            h,
            fl,
            fly,
            horizon,
            cos: a.cos(),
            sin: a.sin(),
            row_k,
            row_inv_k,
        }
    }

    /// Distance in xy units for a depth value (used for fog/visibility shading).
    #[inline]
    fn vis_shade(vis: f64, t: f64) -> i32 {
        // Fitted to DOSBox captures of nine map starts (mean block-brightness error 7.4 ->
        // 4.3 on a 0-255 scale): distance darkens about 3.2x faster than the old 1/2^20.
        (t * vis / VIS_DIVISOR) as i32
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
        let t0 = std::time::Instant::now();
        self.col_pix
            .par_chunks_mut(h)
            .zip(self.col_depth.par_chunks_mut(h))
            .zip(self.col_mask.par_chunks_mut(h))
            .zip(self.masked.par_iter_mut())
            .zip(self.col_portals.par_iter_mut())
            .enumerate()
            .for_each(|(x, ((((pix, depth), mask), masked), portals))| {
                masked.clear();
                // The mask only holds marks where this column showed a portal last frame;
                // everywhere else it is already clear (clearing the whole screen every frame
                // was a large share of the memory traffic at high resolutions).
                if !portals.is_empty() {
                    mask.fill(0);
                    portals.clear();
                }
                cast_column(
                    x, scene, cam, &view, sky_scale, pix, depth, mask, portals, masked, visited,
                );
            });

        let t1 = std::time::Instant::now();
        // Sprites in rooms seen through floor/ceiling portals, moved into the viewer's frame.
        let mut portal_sprites: Vec<SpriteDraw> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        for ids in &self.col_portals {
            for &m in ids {
                if !seen.contains(&m) {
                    seen.push(m);
                }
            }
        }
        seen.sort_unstable();
        for id in seen {
            let si = ((id - 1) / 2) as usize;
            let floor = (id - 1) % 2 == 1;
            let Some(r) = scene.ror.get(si) else { continue };
            let Some(l) = (if floor { r.floor } else { r.ceil }) else {
                continue;
            };
            let room = linked_room(scene, l.sector, 96);
            for sp in scene
                .sprites
                .iter()
                .filter(|sp| sp.portal == 0 && sp.sector >= 0 && room.contains(&(sp.sector as usize)))
            {
                portal_sprites.push(SpriteDraw {
                    x: sp.x - l.dx,
                    y: sp.y - l.dy,
                    z: sp.z - l.dz,
                    portal: id,
                    ..*sp
                });
            }
        }
        let combined: Vec<SpriteDraw>;
        let scene_with_portals;
        let scene = if portal_sprites.is_empty() {
            scene
        } else {
            combined = scene.sprites.iter().copied().chain(portal_sprites).collect();
            scene_with_portals = Scene {
                sprites: &combined,
                ..*scene
            };
            &scene_with_portals
        };

        self.stats_sectors = self.visited.iter().filter(|v| v.load(Ordering::Relaxed) != 0).count();

        // Project visible sprites.
        let mut sprites: Vec<SprProj> = scene
            .sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.cstat & SPR_INVISIBLE == 0
                    && s.sector >= 0
                    && (s.portal != 0
                        || (s.sector as usize) < self.visited.len()
                            && self.visited[s.sector as usize].load(Ordering::Relaxed) != 0)
            })
            .filter_map(|(i, s)| project_sprite(i, s, scene, cam, &view))
            .collect();
        sprites.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));
        let mut kinds = [0usize; 5];
        for sp in &sprites {
            kinds[match sp.kind {
                SprKind::Face { .. } => 0,
                SprKind::Wall { .. } => 1,
                SprKind::Floor { .. } => 2,
                SprKind::Voxel { .. } => 3,
            }] += 1;
        }
        kinds[4] = self.masked.iter().map(|m| m.len()).sum();
        self.stats_kinds = kinds;

        // Pass 2: masked walls and sprites, back to front.
        let t2 = std::time::Instant::now();
        self.col_pix
            .par_chunks_mut(h)
            .zip(self.col_depth.par_chunks(h))
            .zip(self.col_mask.par_chunks(h))
            .zip(self.masked.par_iter_mut())
            .enumerate()
            .for_each(|(x, (((pix, depth), mask), masked))| {
                masked.sort_by(|a, b| b.t.partial_cmp(&a.t).unwrap_or(std::cmp::Ordering::Equal));
                let mut mi = 0;
                for sp in &sprites {
                    while mi < masked.len() && masked[mi].t > sp.depth {
                        draw_masked(x, &masked[mi], scene, cam, &view, pix, depth);
                        mi += 1;
                    }
                    draw_sprite_column(x, sp, scene, cam, &view, pix, depth, mask);
                }
                while mi < masked.len() {
                    draw_masked(x, &masked[mi], scene, cam, &view, pix, depth);
                    mi += 1;
                }
            });

        // Transpose column-major into row-major output.
        let t3 = std::time::Instant::now();
        if !self.keep_rows {
            self.phase_ms = [
                (t1 - t0).as_secs_f64() * 1e3,
                (t2 - t1).as_secs_f64() * 1e3,
                (t3 - t2).as_secs_f64() * 1e3,
                0.0,
            ];
            return;
        }
        let w = self.width;
        let col = &self.col_pix;
        // In bands of rows so each column is read as a short contiguous run (a plain
        // per-pixel strided read was a quarter of the frame time at 8K).
        const BAND: usize = 32;
        self.frame
            .par_chunks_mut(w * BAND)
            .enumerate()
            .for_each(|(band, rows)| {
                let y0 = band * BAND;
                let n = rows.len() / w;
                for x in 0..w {
                    let src = &col[x * h + y0..x * h + y0 + n];
                    for (k, &p) in src.iter().enumerate() {
                        rows[k * w + x] = p;
                    }
                }
            });
        let t4 = std::time::Instant::now();
        let ms = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1e3;
        self.phase_ms = [ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t3, t4)];
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
    link: Option<RorLink>,
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
    mask: &mut [u32],
    portals: &mut Vec<u32>,
    masked: &mut Vec<MaskSeg>,
    visited: &[AtomicU8],
) {
    let h = view.h;
    // Marks rows drawn by a see-through plane with that portal's id.
    // Marks rows drawn by a see-through plane with that portal's id, and records the id.
    let mark = |mask: &mut [u32], portals: &mut Vec<u32>, y0: f64, y1: f64, id: u32| {
        let r0 = y0.max(0.0).ceil() as usize;
        let r1 = (y1.min(h as f64).ceil() as isize).max(0) as usize;
        if r0 >= r1 {
            return;
        }
        for m in mask.iter_mut().take(r1).skip(r0) {
            *m = id;
        }
        if !portals.contains(&id) {
            portals.push(id);
        }
    };
    let (dx, dy) = view.dir(x);
    let mut sect = cam.sector;
    let mut ytop = 0.0f64;
    let mut ybot = h as f64;
    let mut t_enter = 0.0f64;
    let mut filled_to_top = 0usize; // rows < this are final
    let _ = &mut filled_to_top;
    let sectors = scene.sectors;
    let walls = scene.walls;
    for _ in 0..1024 {
        if sect < 0 || sect as usize >= sectors.len() {
            break;
        }
        let si = sect as usize;
        visited[si].store(1, Ordering::Relaxed);
        let sec = &sectors[si];
        let vis_s = sector_vis(scene, si as i32);

        // Find exit wall: nearest crossing beyond the entry distance.
        let mut best: Option<(f64, f64, usize)> = None;
        let start = sec.wallptr as usize;
        let end = start + sec.wallnum as usize;
        for wi in start..end.min(walls.len()) {
            let w1 = &walls[wi];
            let w2 = &walls[w1.point2 as usize];
            if let Some((t, s)) = ray_segment(cam.x, cam.y, dx, dy, w1.x as f64, w1.y as f64, w2.x as f64, w2.y as f64)
            {
                if t > t_enter + 1e-7 && best.is_none_or(|b| t < b.0) {
                    best = Some((t, s, wi));
                }
            }
        }

        let ceq = plane_eq(sec, walls, &sec.ceiling);
        let feq = plane_eq(sec, walls, &sec.floor);
        let ror = scene.ror.get(si).copied().unwrap_or_default();
        let cctx = PlaneCtx {
            eq: ceq,
            plane: &sec.ceiling,
            sector: sec,
            walls,
            link: ror.ceil,
        };
        let fctx = PlaneCtx {
            eq: feq,
            plane: &sec.floor,
            sector: sec,
            walls,
            link: ror.floor,
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
            if cctx.link.is_some() {
                mark(mask, portals, ytop, mid, portal_id(si, false));
            }
            if fctx.link.is_some() {
                mark(mask, portals, mid, ybot, portal_id(si, true));
            }
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
        if cctx.link.is_some() {
            mark(mask, portals, ytop, yc.min(ybot), portal_id(si, false));
        }
        if fctx.link.is_some() {
            mark(mask, portals, yf.max(ytop), ybot, portal_id(si, true));
        }

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

    if let Some(l) = p.link {
        // Look through the plane into the linked room. Every pixel of this column travels
        // along the same 2D line, so the sectors it crosses are found once per column.
        let mut tmin = f64::INFINITY;
        for y in r0..r1 {
            let k = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
            let den = k - b;
            let t = if den.abs() > 1e-12 { (a - cam.z) / den } else { -1.0 };
            if t > 0.0 {
                tmin = tmin.min(t);
            }
        }
        let (ox, oy, oz) = (cam.x + l.dx, cam.y + l.dy, cam.z + l.dz);
        let mut segs = ColTrace::new(scene, l.sector, ox, oy, dx, dy, tmin);
        for y in r0..r1 {
            let k = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
            let den = k - b;
            let t = if den.abs() > 1e-12 { (a - cam.z) / den } else { -1.0 };
            if t.is_nan() || t <= 0.0 {
                pix[y] = 0;
                depth[y] = f32::INFINITY;
                continue;
            }
            let (c, tt) = trace_column(scene, &mut segs, ox, oy, oz, dx, dy, k, t);
            pix[y] = c;
            depth[y] = tt as f32;
        }
        return;
    }

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
    // Orientation flags folded into one linear map (u, v) = M (px, py), in texels.
    let (mut m00, mut m01, mut m10, mut m11) = if stat & PLANE_RELATIVE != 0 {
        (ux, uy, -uy, ux)
    } else {
        (1.0, 0.0, 0.0, 1.0)
    };
    if stat & PLANE_SWAPXY != 0 {
        std::mem::swap(&mut m00, &mut m10);
        std::mem::swap(&mut m01, &mut m11);
    }
    if stat & PLANE_XFLIP != 0 {
        m00 = -m00;
        m01 = -m01;
    }
    if stat & PLANE_YFLIP != 0 {
        m10 = -m10;
        m11 = -m11;
    }
    let inv_texel = 1.0 / texel;
    let (m00, m01, m10, m11) = (m00 * inv_texel, m01 * inv_texel, m10 * inv_texel, m11 * inv_texel);
    let k_scale = 16.0 / view.fly;
    let height = a - cam.z;

    let flat = b == 0.0;
    for y in r0..r1 {
        let k = if flat {
            view.row_k[y]
        } else {
            (y as f64 + 0.5 - view.horizon) * k_scale
        };
        let den = k - b;
        if den.abs() < 1e-12 {
            pix[y] = 0;
            depth[y] = f32::INFINITY;
            continue;
        }
        let mut t = if flat { height * view.row_inv_k[y] } else { height / den };
        if t.is_nan() || t <= 0.0 {
            t = 1e9;
        }
        let px = cam.x + dx * t - ox;
        let py = cam.y + dy * t - oy;
        let tu = wrap((px * m00 + py * m01 + xpan).floor() as i64, tw);
        let tv = wrap((px * m10 + py * m11 + ypan).floor() as i64, th);
        let c = tile.pixels[(tu * th + tv) as usize];
        let sh = shade + Renderer::vis_shade(vis, t);
        pix[y] = lut[shade_index(sh) + c as usize];
        depth[y] = t as f32;
    }
}

/// Sectors reachable from `start` through open walls (bounded), i.e. one "room".
fn linked_room(scene: &Scene, start: i32, limit: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if start < 0 || start as usize >= scene.sectors.len() {
        return out;
    }
    out.push(start as usize);
    let mut i = 0;
    while i < out.len() && out.len() < limit {
        let sec = &scene.sectors[out[i]];
        for wi in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
            let ns = scene.walls[wi].nextsector;
            if ns >= 0 && !out.contains(&(ns as usize)) {
                out.push(ns as usize);
            }
        }
        i += 1;
    }
    out
}

/// Texture colour of a floor or ceiling at world point (px, py), `t` units away.
fn sample_plane(scene: &Scene, si: usize, floor: bool, px: f64, py: f64, t: f64) -> u32 {
    let sec = &scene.sectors[si];
    let plane = if floor { &sec.floor } else { &sec.ceiling };
    let lut = scene.luts.get(plane.pal);
    let pic = scene.tiles.animate(plane.picnum as i32, scene.clock);
    let tile = scene.tiles.get(pic);
    let sh = shade_index(plane.shade as i32 + Renderer::vis_shade(sector_vis(scene, si as i32), t));
    if tile.is_empty() {
        return lut[sh];
    }
    let (tw, th) = (tile.width as i64, tile.height as i64);
    let stat = plane.stat;
    let texel = if stat & PLANE_SMOOSH != 0 { 8.0 } else { 16.0 };
    let (mut u, mut v) = if stat & PLANE_RELATIVE != 0 && sec.wallnum >= 2 {
        let w1 = &scene.walls[sec.wallptr as usize];
        let w2 = &scene.walls[w1.point2 as usize];
        let (ex, ey) = ((w2.x - w1.x) as f64, (w2.y - w1.y) as f64);
        let l = ex.hypot(ey).max(1e-9);
        let (ux, uy) = (ex / l, ey / l);
        let (rx, ry) = (px - w1.x as f64, py - w1.y as f64);
        (rx * ux + ry * uy, -rx * uy + ry * ux)
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
    let tu = ((u / texel + plane.xpan as f64 * tw as f64 / 256.0).floor() as i64).rem_euclid(tw);
    let tv = ((v / texel + plane.ypan as f64 * th as f64 / 256.0).floor() as i64).rem_euclid(th);
    lut[sh + tile.pixels[(tu * th + tv) as usize] as usize]
}

/// Texture colour of a wall at position `s` (0..1 along it) and height `z`.
#[allow(clippy::too_many_arguments)]
fn sample_wall(
    scene: &Scene,
    si: usize,
    wall: &Wall,
    picnum: i16,
    xpan: u8,
    ypan: u8,
    s: f64,
    z: f64,
    zref: f64,
    t: f64,
) -> u32 {
    let lut = scene.luts.get(wall.pal);
    let tile = scene.tiles.get(scene.tiles.animate(picnum as i32, scene.clock));
    let sh = shade_index(wall.shade as i32 + Renderer::vis_shade(sector_vis(scene, si as i32), t));
    if tile.is_empty() {
        return lut[sh];
    }
    let (tw, th) = (tile.width as i64, tile.height as i64);
    let mut tx = ((s * wall.xrepeat as f64 * 8.0).floor() as i64 + xpan as i64).rem_euclid(tw);
    if wall.cstat & WALL_XFLIP != 0 {
        tx = tw - 1 - tx;
    }
    let mut v = ((z - zref) * wall.yrepeat as f64 / 1024.0 + ypan as f64 * th as f64 / 256.0).floor() as i64;
    if wall.cstat & WALL_YFLIP != 0 {
        v = -v - 1;
    }
    lut[sh + tile.pixels[(tx * th + v.rem_euclid(th)) as usize] as usize]
}

/// Traces one pixel's ray through sectors starting at (ox, oy, oz) in sector `s`.
/// `dz` is z units per xy unit; `t_base` is the distance already travelled.
#[allow(clippy::too_many_arguments)]
/// One stretch of a column's 2D ray inside a sector (global ray parameter `t`).
struct ColSeg {
    sector: usize,
    t1: f64,
    ceil: PlaneEq,
    floor: PlaneEq,
    /// Wall hit at `t1` (index, position along it) and, for a portal, the next sector's
    /// ceiling/floor heights at the hit point.
    wall: Option<WallHit>,
}

/// (wall index, position along the wall, next sector's (ceiling, floor) z for a portal).
type WallHit = (usize, f64, Option<(f64, f64)>);

/// Sectors crossed by a column's ray `(ox, oy) + t (dx, dy)`, found lazily: only as far
/// along as some pixel of the column actually reaches.
struct ColTrace {
    segs: Vec<ColSeg>,
    /// Next sector to enter and the ray parameter where it starts (-1 = finished).
    next: i32,
    t_in: f64,
    ox: f64,
    oy: f64,
    dx: f64,
    dy: f64,
}

impl ColTrace {
    fn new(scene: &Scene, start: i32, ox: f64, oy: f64, dx: f64, dy: f64, t_start: f64) -> ColTrace {
        let (sectors, walls) = (scene.sectors, scene.walls);
        let mut s = -1;
        if t_start.is_finite() {
            let (sx, sy) = (ox + dx * t_start, oy + dy * t_start);
            s = start;
            if s < 0 || s as usize >= sectors.len() || !crate::geom::inside_sector(sectors, walls, s as usize, sx, sy) {
                s = crate::geom::update_sector(sectors, walls, sx, sy, s);
            }
        }
        ColTrace {
            segs: Vec::with_capacity(8),
            next: s,
            t_in: t_start,
            ox,
            oy,
            dx,
            dy,
        }
    }

    /// Segment `i`, computing it (and any before it) if needed.
    fn get(&mut self, scene: &Scene, i: usize) -> Option<&ColSeg> {
        let (sectors, walls) = (scene.sectors, scene.walls);
        while self.segs.len() <= i {
            if self.next < 0 || self.segs.len() >= 64 {
                return None;
            }
            let si = self.next as usize;
            let sec = &sectors[si];
            let (ox, oy, dx, dy) = (self.ox, self.oy, self.dx, self.dy);
            let mut best: Option<(f64, f64, usize)> = None;
            for wi in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
                let w1 = &walls[wi];
                let w2 = &walls[w1.point2 as usize];
                if let Some((t, u)) = ray_segment(ox, oy, dx, dy, w1.x as f64, w1.y as f64, w2.x as f64, w2.y as f64) {
                    if t > self.t_in + 1e-7 && best.is_none_or(|b| t < b.0) {
                        best = Some((t, u, wi));
                    }
                }
            }
            let ceil = plane_eq(sec, walls, &sec.ceiling);
            let floor = plane_eq(sec, walls, &sec.floor);
            let Some((t, u, wi)) = best else {
                self.segs.push(ColSeg {
                    sector: si,
                    t1: 1e9,
                    ceil,
                    floor,
                    wall: None,
                });
                self.next = -1;
                continue;
            };
            let w = &walls[wi];
            let next = (w.nextsector >= 0)
                .then(|| crate::geom::zs_of_slope(sectors, walls, w.nextsector as usize, ox + dx * t, oy + dy * t));
            self.segs.push(ColSeg {
                sector: si,
                t1: t,
                ceil,
                floor,
                wall: Some((wi, u, next)),
            });
            self.next = w.nextsector as i32;
            self.t_in = t;
        }
        self.segs.get(i)
    }
}

/// Colour and depth for one pixel's ray `(ox, oy, oz) + t (dx, dy, dz)` entering the linked
/// room at `t_entry`, using the column's precomputed sector list. Same result as
/// [`trace_pixel`], which still handles portals nested inside the linked room.
#[allow(clippy::too_many_arguments)]
fn trace_column(
    scene: &Scene,
    segs: &mut ColTrace,
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    t_entry: f64,
) -> (u32, f64) {
    let (sectors, walls) = (scene.sectors, scene.walls);
    let mut t_in = t_entry;
    let mut idx = 0;
    while let Some(g) = segs.get(scene, idx) {
        idx += 1;
        if g.t1 <= t_entry + 1e-7 {
            continue;
        }
        let si = g.sector;
        let sec = &sectors[si];
        let ror = scene.ror.get(si).copied().unwrap_or_default();
        for (floor, eq) in [(false, &g.ceil), (true, &g.floor)] {
            // Plane height along the ray: a + b t.
            let a = eq.z_at(ox, oy);
            let b = eq.bx * dx + eq.by * dy;
            let den = dz - b;
            if den.abs() < 1e-12 || (floor && den < 0.0) || (!floor && den > 0.0) {
                continue;
            }
            let t = (a - oz) / den;
            if t < t_in - 1e-6 || t > g.t1 {
                continue;
            }
            let (hx, hy, hz) = (ox + dx * t, oy + dy * t, oz + dz * t);
            let link = if floor { ror.floor } else { ror.ceil };
            if let Some(l2) = link {
                return trace_pixel(scene, l2.sector, hx + l2.dx, hy + l2.dy, hz + l2.dz, dx, dy, dz, t, 2);
            }
            return (sample_plane(scene, si, floor, hx, hy, t), t);
        }
        let Some((wi, u, next)) = g.wall else {
            return (0, f64::INFINITY);
        };
        let w = &walls[wi];
        let t = g.t1;
        let hz = oz + dz * t;
        match next {
            None => {
                let zref = if w.cstat & WALL_ALIGN_BOTTOM != 0 {
                    sec.floor.z
                } else {
                    sec.ceiling.z
                } as f64;
                return (
                    sample_wall(scene, si, w, w.picnum, w.xpanning, w.ypanning, u, hz, zref, t),
                    t,
                );
            }
            Some((c, f)) => {
                let ns = w.nextsector as usize;
                if hz < c {
                    let zref = if w.cstat & WALL_ALIGN_BOTTOM != 0 {
                        sec.ceiling.z
                    } else {
                        sectors[ns].ceiling.z
                    } as f64;
                    return (
                        sample_wall(scene, si, w, w.picnum, w.xpanning, w.ypanning, u, hz, zref, t),
                        t,
                    );
                }
                if hz > f {
                    let src = if w.cstat & WALL_BOTTOM_SWAP != 0 && w.nextwall >= 0 {
                        &walls[w.nextwall as usize]
                    } else {
                        w
                    };
                    let zref = if src.cstat & WALL_ALIGN_BOTTOM != 0 {
                        sec.ceiling.z
                    } else {
                        sectors[ns].floor.z
                    } as f64;
                    return (
                        sample_wall(scene, si, w, src.picnum, src.xpanning, src.ypanning, u, hz, zref, t),
                        t,
                    );
                }
            }
        }
        t_in = t;
    }
    (0, f64::INFINITY)
}

#[allow(clippy::too_many_arguments)]
fn trace_pixel(
    scene: &Scene,
    mut s: i32,
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    t_base: f64,
    depth: u32,
) -> (u32, f64) {
    let (sectors, walls) = (scene.sectors, scene.walls);
    if s < 0 || s as usize >= sectors.len() || !crate::geom::inside_sector(sectors, walls, s as usize, ox, oy) {
        s = crate::geom::update_sector(sectors, walls, ox, oy, s);
    }
    let mut t_in = 0.0f64;
    for _ in 0..64 {
        if s < 0 {
            return (0, f64::INFINITY);
        }
        let si = s as usize;
        let sec = &sectors[si];
        let mut best: Option<(f64, f64, usize)> = None;
        for wi in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
            let w1 = &walls[wi];
            let w2 = &walls[w1.point2 as usize];
            if let Some((t, u)) = ray_segment(ox, oy, dx, dy, w1.x as f64, w1.y as f64, w2.x as f64, w2.y as f64) {
                if t > t_in + 1e-7 && best.is_none_or(|b| t < b.0) {
                    best = Some((t, u, wi));
                }
            }
        }
        let t_out = best.map_or(1e9, |b| b.0);
        let ror = scene.ror.get(si).copied().unwrap_or_default();
        for (floor, plane) in [(false, &sec.ceiling), (true, &sec.floor)] {
            let eq = plane_eq(sec, walls, plane);
            let a = eq.z_at(ox, oy);
            let b = eq.bx * dx + eq.by * dy;
            let den = dz - b;
            if den.abs() < 1e-12 || (floor && den < 0.0) || (!floor && den > 0.0) {
                continue;
            }
            let t = (a - oz) / den;
            if t < t_in - 1e-6 || t > t_out {
                continue;
            }
            let (hx, hy, hz) = (ox + dx * t, oy + dy * t, oz + dz * t);
            let link = if floor { ror.floor } else { ror.ceil };
            if let (Some(l), true) = (link, depth > 0) {
                return trace_pixel(
                    scene,
                    l.sector,
                    hx + l.dx,
                    hy + l.dy,
                    hz + l.dz,
                    dx,
                    dy,
                    dz,
                    t_base + t,
                    depth - 1,
                );
            }
            return (sample_plane(scene, si, floor, hx, hy, t_base + t), t_base + t);
        }
        let Some((t, u, wi)) = best else {
            return (0, f64::INFINITY);
        };
        let w = &walls[wi];
        let hz = oz + dz * t;
        let tt = t_base + t;
        if w.nextsector < 0 {
            let zref = if w.cstat & WALL_ALIGN_BOTTOM != 0 {
                sec.floor.z
            } else {
                sec.ceiling.z
            } as f64;
            return (
                sample_wall(scene, si, w, w.picnum, w.xpanning, w.ypanning, u, hz, zref, tt),
                tt,
            );
        }
        let ns = w.nextsector as usize;
        let (c, f) = crate::geom::zs_of_slope(sectors, walls, ns, ox + dx * t, oy + dy * t);
        if hz < c {
            let zref = if w.cstat & WALL_ALIGN_BOTTOM != 0 {
                sec.ceiling.z
            } else {
                sectors[ns].ceiling.z
            } as f64;
            return (
                sample_wall(scene, si, w, w.picnum, w.xpanning, w.ypanning, u, hz, zref, tt),
                tt,
            );
        }
        if hz > f {
            let src = if w.cstat & WALL_BOTTOM_SWAP != 0 && w.nextwall >= 0 {
                &walls[w.nextwall as usize]
            } else {
                w
            };
            let zref = if src.cstat & WALL_ALIGN_BOTTOM != 0 {
                sec.ceiling.z
            } else {
                sectors[ns].floor.z
            } as f64;
            return (
                sample_wall(scene, si, w, src.picnum, src.xpanning, src.ypanning, u, hz, zref, tt),
                tt,
            );
        }
        s = ns as i32;
        t_in = t;
    }
    (0, f64::INFINITY)
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
    let texel_per_px = 1.0 / px_per_texel;
    let k_scale = 16.0 / view.fly;
    let lut = &lut[sh..sh + 256];
    for y in r0..r1 {
        let dy = y as f64 + 0.5 - view.horizon;
        let v = dy * texel_per_px + th * 0.5;
        let tv = (v.floor() as i64).clamp(0, th as i64 - 1) as usize;
        pix[y] = lut[col[tv] as usize];
        let t = if b == 0.0 {
            if view.row_k[y].abs() > 1e-12 {
                (a - cam.z) * view.row_inv_k[y]
            } else {
                -1.0
            }
        } else {
            let den = dy * k_scale - b;
            if den.abs() > 1e-12 {
                (a - cam.z) / den
            } else {
                -1.0
            }
        };
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
    // Texture row stepped in 32.32 fixed point: one add per pixel.
    const ONE: f64 = 4294967296.0;
    let z0 = cam.z + (r0 as f64 + 0.5 - view.horizon) * dz_per_row;
    let mut vf = (((z0 - zref) * yrep + pan) * ONE) as i64;
    let step = (dz_per_row * yrep * ONE) as i64;
    let lut = &lut[sh..sh + 256];
    for y in r0..r1 {
        let mut v = vf >> 32;
        if yflip {
            v = -v - 1;
        }
        let c = col[wrap(v, th) as usize];
        pix[y] = lut[c as usize];
        depth[y] = tf;
        vf += step;
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
    let vis = sector_vis(scene, m.sect as i32);
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
    let lut = &lut[sh..sh + 256];
    // Texture row stepped in 32.32 fixed point, as for solid walls.
    const ONE: f64 = 4294967296.0;
    let z0 = cam.z + (r0 as f64 + 0.5 - view.horizon) * dz_per_row;
    let mut vf = (((z0 - zref) * yrep + pan) * ONE) as i64;
    let step = (dz_per_row * yrep * ONE) as i64;
    for y in r0..r1 {
        if m.t < depth[y] {
            let mut v = vf >> 32;
            if yflip {
                v = -v - 1;
            }
            let c = col[wrap(v, th) as usize];
            if c != TRANSPARENT {
                let rgb = lut[c as usize];
                pix[y] = if trans { blend(pix[y], rgb, rev) } else { rgb };
            }
        }
        vf += step;
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

fn project_voxel(
    idx: usize,
    s: &SpriteDraw,
    id: u32,
    v: &crate::voxel::Voxel,
    cam: &Camera,
    view: &View,
) -> Option<SprProj> {
    let a = ang_to_rad(s.ang);
    // KVX x runs along the sprite's facing, y across it (the flat tile's horizontal axis).
    let w = (a.cos(), a.sin());
    let f = (-a.sin(), a.cos());
    // One voxel is one sprite texel at the same repeat, and the model stands on the sprite's
    // z even when the sprite is flagged as centred. Both fitted to DOSBox captures of voxel
    // crates at the E4M9 and E2M4 starts (22x14 px and 13x8 px in the original; a quarter
    // texel per voxel drew them 6 px wide, half sunk into the floor).
    let unit = s.xrepeat as f64 / 4.0;
    let zunit = s.yrepeat as f64 * 4.0;
    if unit <= 0.0 || zunit <= 0.0 {
        return None;
    }
    let height = v.zs as f64 * zunit;
    let zbot = s.z;
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
    // Entirely behind the camera: nothing to draw (this used to cover the whole screen and
    // cast a ray for every pixel).
    if !x0.is_finite() {
        return None;
    }
    if behind {
        // Straddles the camera plane: can reach any part of the screen.
        x0 = 0.0;
        x1 = view.w as f64;
        y0 = 0.0;
        y1 = view.h as f64;
        depth = 1.0;
    }
    Some(SprProj {
        idx,
        depth: (s.x - cam.x).hypot(s.y - cam.y).max(depth.min(1e9)) as f32,
        kind: SprKind::Voxel {
            id,
            sx0: x0 as f32,
            sx1: x1 as f32,
            y0: y0 as f32,
            y1: y1 as f32,
            tmin: depth.min(1e9) as f32,
            w,
            f,
            unit,
            zunit,
        },
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
            let (mut miny, mut maxy) = (f64::INFINITY, f64::NEG_INFINITY);
            let dz = s.z - cam.z;
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
                let sy = view.horizon + dz / 16.0 / f * view.fly;
                miny = miny.min(sy);
                maxy = maxy.max(sy);
            }
            if any_behind {
                minx = 0.0;
                maxx = view.w as f64;
                // Part of it is behind the camera: it reaches the screen edge on its side.
                if dz > 0.0 {
                    maxy = view.h as f64;
                    miny = miny.min(view.h as f64);
                } else {
                    miny = 0.0;
                    maxy = maxy.max(0.0);
                }
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
                    ry0: (miny - 1.0) as f32,
                    ry1: (maxy + 1.0) as f32,
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

#[allow(clippy::too_many_arguments)]
fn draw_sprite_column(
    x: usize,
    sp: &SprProj,
    scene: &Scene,
    cam: &Camera,
    view: &View,
    pix: &mut [u32],
    depth: &[f32],
    mask: &[u32],
) {
    let s = &scene.sprites[sp.idx];
    // Portal sprites only appear where their portal is visible.
    let portal = s.portal;
    let shown = |y: usize| portal == 0 || mask[y] == portal;
    let tile = scene.tiles.get(sp.pic);
    let tw = tile.width as i64;
    let th = tile.height as i64;
    let xc = x as f32 + 0.5;
    let lut = scene.luts.get(s.pal);
    let vis = sector_vis(scene, s.sector);
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
        let lut = &lut[sh..sh + 256];
        // Texture row stepped in 32.32 fixed point, clamped to the tile.
        const ONE: f64 = 4294967296.0;
        let mut vf = (((r0 as f64 + 0.5 - y0) * inv) * ONE) as i64;
        let step = (inv * ONE) as i64;
        let last = th - 1;
        let row = |vf: i64| {
            let ty = (vf >> 32).clamp(0, last);
            (if yflip { last - ty } else { ty }) as usize
        };
        if portal == 0 && !trans {
            // Common case: opaque sprite, no portal mask.
            for y in r0..r1 {
                if tf < depth[y] {
                    let c = col[row(vf)];
                    if c != TRANSPARENT {
                        pix[y] = lut[c as usize];
                    }
                }
                vf += step;
            }
            return;
        }
        for y in r0..r1 {
            if tf < depth[y] && shown(y) {
                let c = col[row(vf)];
                if c != TRANSPARENT {
                    let rgb = lut[c as usize];
                    pix[y] = if trans { blend(pix[y], rgb, rev) } else { rgb };
                }
            }
            vf += step;
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
        SprKind::Voxel {
            id,
            sx0,
            sx1,
            y0,
            y1,
            tmin,
            w,
            f,
            unit,
            zunit,
        } => {
            if xc < sx0 - 1.0 || xc > sx1 + 1.0 {
                return;
            }
            let Some(Some(v)) = scene.voxels.get(id as usize) else {
                return;
            };
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
                if depth[y] <= tmin || !shown(y) {
                    continue; // hidden behind something nearer than the whole model
                }
                let dzz = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
                let Some((t, c)) = v.cast([ox, oy, oz], [ddx, ddy, dzz / zunit]) else {
                    continue;
                };
                if t <= 0.0 || t as f32 >= depth[y] || !shown(y) {
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
            ry0,
            ry1,
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
            // Only the rows its corners project to (a decal far away is a thin strip).
            let r0 = r0.max(ry0.max(0.0) as usize);
            let r1 = r1.min((ry1.max(0.0) as usize).min(view.h));
            for y in r0..r1 {
                let k = (y as f64 + 0.5 - view.horizon) / view.fly * 16.0;
                if k.abs() < 1e-12 {
                    continue;
                }
                let t = dz / k;
                if t <= 0.0 || t as f32 >= depth[y] || !shown(y) {
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
            voxels: &[],
            ror: &[],
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
            voxels: &[],
            ror: &[],
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
