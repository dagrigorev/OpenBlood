//! Runtime world state built from a Blood map.

use ob_engine::geom;
use ob_engine::map::{Map, Sector, Sprite, Wall, XSector, XSprite, XWall};
use ob_engine::render::{Ror, RorLink};

/// Blood status lists (sprite `statnum`).
pub mod stat {
    pub const DECORATION: i16 = 0;
    pub const FX: i16 = 1;
    pub const EXPLOSION: i16 = 2;
    pub const ITEM: i16 = 3;
    pub const THING: i16 = 4;
    pub const PROJECTILE: i16 = 5;
    pub const DUDE: i16 = 6;
    pub const INACTIVE: i16 = 7;
    pub const RESPAWN: i16 = 8;
    pub const PURGE: i16 = 9;
    pub const MARKER: i16 = 10;
    pub const TRAPS: i16 = 11;
    pub const AMBIENCE: i16 = 12;
    pub const SPARES: i16 = 13;
    pub const FLARE: i16 = 14;
    pub const DEBRIS: i16 = 15;
    pub const PATH_MARKER: i16 = 16;
    pub const FREE: i16 = 1024;
}

/// Per-sprite runtime data not present in the map record.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct SpriteRt {
    /// Velocity in xy units / z units per 120Hz tick.
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    /// Current SEQ animation id and frame (-1 when none).
    pub seq: i32,
    pub seq_frame: i32,
    pub seq_time: i32,
    /// Health for dudes/things.
    pub health: i32,
    pub on_ground: bool,
    pub ai_state: i32,
    pub ai_timer: i32,
    pub target: i32,
    pub burn_time: i32,
    /// Facing the dude was placed with (idle wandering turns back to it).
    pub home_ang: i16,
    /// Eye height above the feet (from the sprite's size), for sight checks.
    pub eye: i32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct World {
    pub map_name: String,
    pub sectors: Vec<Sector>,
    pub walls: Vec<Wall>,
    pub sprites: Vec<Sprite>,
    pub rt: Vec<SpriteRt>,
    pub xsectors: Vec<XSector>,
    pub xwalls: Vec<XWall>,
    pub xsprites: Vec<XSprite>,
    pub sky_offsets: Vec<i16>,
    pub sky_bits: i32,
    pub visibility: i32,
    pub song_id: i32,
    /// Original floor/ceiling heights, used by movers.
    pub base_floor: Vec<i32>,
    pub base_ceil: Vec<i32>,
    /// Original wall vertices, used by sliding/rotating sectors.
    pub base_wall_xy: Vec<(i32, i32)>,
    pub base_sprite_xy: Vec<(i32, i32, i32)>,
    /// Original shades, used by sector lighting effects.
    pub base_floor_shade: Vec<i8>,
    pub base_ceil_shade: Vec<i8>,
    pub base_wall_shade: Vec<i8>,
    /// Floor/ceiling links between stacked rooms (all marker kinds, used for movement).
    pub links: Vec<Ror>,
    /// The subset of links you can see through (stacks and generic links; water is opaque).
    pub ror_view: Vec<Ror>,
}

impl World {
    pub fn from_map(name: &str, m: Map) -> Self {
        let n = m.sprites.len();
        let mut w = World {
            map_name: name.to_string(),
            base_floor: m.sectors.iter().map(|s| s.floor.z).collect(),
            base_ceil: m.sectors.iter().map(|s| s.ceiling.z).collect(),
            base_wall_xy: m.walls.iter().map(|w| (w.x, w.y)).collect(),
            base_sprite_xy: m.sprites.iter().map(|s| (s.x, s.y, s.z)).collect(),
            base_floor_shade: m.sectors.iter().map(|s| s.floor.shade).collect(),
            base_ceil_shade: m.sectors.iter().map(|s| s.ceiling.shade).collect(),
            base_wall_shade: m.walls.iter().map(|w| w.shade).collect(),
            links: Vec::new(),
            ror_view: Vec::new(),
            sectors: m.sectors,
            walls: m.walls,
            sprites: m.sprites,
            rt: vec![
                SpriteRt {
                    seq: -1,
                    target: -1,
                    ..Default::default()
                };
                n
            ],
            xsectors: m.xsectors,
            xwalls: m.xwalls,
            xsprites: m.xsprites,
            sky_offsets: m.sky_offsets,
            sky_bits: m.sky_bits as i32,
            visibility: m.visibility,
            song_id: m.song_id,
        };
        for (i, s) in w.sprites.iter().enumerate() {
            if let Some(x) = (s.extra > 0).then(|| &w.xsprites[s.extra as usize]) {
                w.rt[i].health = x.health as i32;
            }
        }
        w.build_links();
        w
    }

    pub fn xsector(&self, s: usize) -> Option<&XSector> {
        let e = self.sectors.get(s)?.extra;
        (e > 0).then(|| &self.xsectors[e as usize])
    }
    pub fn xsector_mut(&mut self, s: usize) -> Option<&mut XSector> {
        let e = self.sectors.get(s)?.extra;
        if e > 0 {
            Some(&mut self.xsectors[e as usize])
        } else {
            None
        }
    }
    pub fn xwall(&self, w: usize) -> Option<&XWall> {
        let e = self.walls.get(w)?.extra;
        (e > 0).then(|| &self.xwalls[e as usize])
    }
    pub fn xwall_mut(&mut self, w: usize) -> Option<&mut XWall> {
        let e = self.walls.get(w)?.extra;
        if e > 0 {
            Some(&mut self.xwalls[e as usize])
        } else {
            None
        }
    }
    pub fn xsprite(&self, s: usize) -> Option<&XSprite> {
        let e = self.sprites.get(s)?.extra;
        (e > 0).then(|| &self.xsprites[e as usize])
    }
    pub fn xsprite_mut(&mut self, s: usize) -> Option<&mut XSprite> {
        let e = self.sprites.get(s)?.extra;
        if e > 0 {
            Some(&mut self.xsprites[e as usize])
        } else {
            None
        }
    }

    pub fn zs_at(&self, sect: usize, x: f64, y: f64) -> (f64, f64) {
        geom::zs_of_slope(&self.sectors, &self.walls, sect, x, y)
    }

    pub fn sector_walls(&self, s: usize) -> std::ops::Range<usize> {
        let sec = &self.sectors[s];
        sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize
    }

    /// Spawns a new sprite (reusing freed slots) and returns its index.
    pub fn spawn(&mut self, s: Sprite) -> usize {
        if let Some(i) = self.sprites.iter().position(|sp| sp.statnum == stat::FREE) {
            self.sprites[i] = s;
            self.rt[i] = SpriteRt {
                seq: -1,
                target: -1,
                ..Default::default()
            };
            return i;
        }
        self.sprites.push(s);
        self.rt.push(SpriteRt {
            seq: -1,
            target: -1,
            ..Default::default()
        });
        self.base_sprite_xy.push((0, 0, 0));
        self.sprites.len() - 1
    }

    pub fn delete_sprite(&mut self, i: usize) {
        if let Some(s) = self.sprites.get_mut(i) {
            s.statnum = stat::FREE;
            s.cstat = 0x8000;
            s.extra = -1;
        }
    }

    /// Sprite indices with the given status number.
    pub fn with_stat(&self, st: i16) -> Vec<usize> {
        self.sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| s.statnum == st)
            .map(|(i, _)| i)
            .collect()
    }

    /// Pairs Blood's room-over-room markers. Odd types (7 link, 9 water, 11 stack, 13 goo)
    /// sit in the upper room and make its floor a portal; the even partner (6, 10, 12, 14)
    /// sits in the lower room and makes its ceiling a portal. Partners share `data1`.
    pub fn build_links(&mut self) {
        let n = self.sectors.len();
        self.links = vec![Ror::default(); n];
        self.ror_view = vec![Ror::default(); n];
        let markers: Vec<(usize, i16, i32)> = self
            .sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s.lotag, 6 | 7 | 9..=14) && s.sectnum >= 0)
            .map(|(i, s)| (i, s.lotag, self.xsprite(i).map_or(0, |x| x.data1)))
            .collect();
        for &(ui, ut, id) in markers.iter().filter(|m| m.1 % 2 == 1) {
            let lower_type = if ut == 7 { 6 } else { ut + 1 };
            let Some(&(li, _, _)) = markers.iter().find(|m| m.1 == lower_type && m.2 == id) else {
                continue;
            };
            let (u, l) = (&self.sprites[ui], &self.sprites[li]);
            let (us, ls) = (u.sectnum as usize, l.sectnum as usize);
            if us >= n || ls >= n {
                continue;
            }
            let down = RorLink {
                sector: ls as i32,
                dx: (l.x - u.x) as f64,
                dy: (l.y - u.y) as f64,
                dz: (l.z - u.z) as f64,
            };
            let up = RorLink {
                sector: us as i32,
                dx: -down.dx,
                dy: -down.dy,
                dz: -down.dz,
            };
            self.links[us].floor = Some(down);
            self.links[ls].ceil = Some(up);
            if matches!(ut, 7 | 11) {
                self.ror_view[us].floor = Some(down);
                self.ror_view[ls].ceil = Some(up);
            }
        }
    }

    pub fn player_start(&self, player: i32) -> Option<&Sprite> {
        self.sprites
            .iter()
            .enumerate()
            .find(|(i, s)| s.lotag == 1 && self.xsprite(*i).map_or(player == 0, |x| x.data1 == player))
            .map(|(_, s)| s)
            .or_else(|| self.sprites.iter().find(|s| s.lotag == 1))
    }
}
