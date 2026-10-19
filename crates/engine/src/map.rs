//! Reader for Blood `.MAP` files (signature `BLM\x1a`, versions 6.x and 7.x).
//!
//! Version 7 maps obfuscate every record with a rolling byte XOR (`key`, `key+1`, ...).
//! Geometry records use the classic Build v7 layout (sector 40 bytes, wall 32, sprite 44);
//! Blood adds optional "extra" records (XSECTOR / XWALL / XSPRITE) stored as packed,
//! little-endian, LSB-first bitfields directly after their owner record.

use std::io;

pub const SECTOR_SIZE: usize = 40;
pub const WALL_SIZE: usize = 32;
pub const SPRITE_SIZE: usize = 44;

/// Rolling XOR used by v7 maps: byte `i` is XORed with `(key + i) & 0xff`.
pub fn map_crypt(buf: &mut [u8], key: u32) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b ^= key.wrapping_add(i as u32) as u8;
    }
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Little-endian LSB-first bit reader (matches x86 C bitfield packing).
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }
    pub fn u(&mut self, bits: u32) -> u32 {
        let mut v: u64 = 0;
        for i in 0..bits {
            let byte = self.data.get(self.pos / 8).copied().unwrap_or(0);
            let bit = (byte >> (self.pos % 8)) & 1;
            v |= (bit as u64) << i;
            self.pos += 1;
        }
        v as u32
    }
    pub fn s(&mut self, bits: u32) -> i32 {
        let v = self.u(bits);
        if bits < 32 && v & (1 << (bits - 1)) != 0 {
            (v | (!0u32 << bits)) as i32
        } else {
            v as i32
        }
    }
    pub fn bit(&mut self) -> bool {
        self.u(1) != 0
    }
}

struct Cursor<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        if self.p + n > self.d.len() {
            return Err(invalid("unexpected end of map file"));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
}

fn i16_at(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, Default)]
pub struct Plane {
    pub z: i32,
    pub stat: u16,
    pub picnum: i16,
    pub heinum: i16,
    pub shade: i8,
    pub pal: u8,
    pub xpan: u8,
    pub ypan: u8,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Sector {
    pub wallptr: i16,
    pub wallnum: i16,
    pub ceiling: Plane,
    pub floor: Plane,
    pub visibility: u8,
    pub filler: u8,
    /// Blood: sector type.
    pub lotag: i16,
    pub hitag: i16,
    /// Index into `xsectors` or -1 / 0 when none.
    pub extra: i16,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Wall {
    pub x: i32,
    pub y: i32,
    pub point2: i16,
    pub nextwall: i16,
    pub nextsector: i16,
    pub cstat: u16,
    pub picnum: i16,
    pub overpicnum: i16,
    pub shade: i8,
    pub pal: u8,
    pub xrepeat: u8,
    pub yrepeat: u8,
    pub xpanning: u8,
    pub ypanning: u8,
    pub lotag: i16,
    pub hitag: i16,
    pub extra: i16,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Sprite {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub cstat: u16,
    pub picnum: i16,
    pub shade: i8,
    pub pal: u8,
    pub clipdist: u8,
    pub flags: u8,
    pub xrepeat: u8,
    pub yrepeat: u8,
    pub xoffset: i8,
    pub yoffset: i8,
    pub sectnum: i16,
    pub statnum: i16,
    pub ang: i16,
    pub owner: i16,
    pub xvel: i16,
    pub yvel: i16,
    pub zvel: i16,
    /// Blood: sprite type.
    pub lotag: i16,
    pub hitag: i16,
    pub extra: i16,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct XSector {
    pub reference: i32,
    pub state: u8,
    pub busy: u32,
    pub data: u32,
    pub tx_id: u32,
    pub busy_wave_a: u8,
    pub busy_wave_b: u8,
    pub rx_id: u32,
    pub command: u8,
    pub trigger_on: bool,
    pub trigger_off: bool,
    pub busy_time_a: u32,
    pub wait_time_a: u32,
    pub rest_state: bool,
    pub interruptable: bool,
    pub amplitude: i32,
    pub freq: u32,
    pub retrigger_a: bool,
    pub retrigger_b: bool,
    pub phase: u32,
    pub wave: u8,
    pub shade_always: bool,
    pub shade_floor: bool,
    pub shade_ceiling: bool,
    pub shade_walls: bool,
    pub shade: i32,
    pub pan_always: bool,
    pub pan_floor: bool,
    pub pan_ceiling: bool,
    pub drag: bool,
    pub underwater: bool,
    pub depth: u8,
    pub pan_vel: u32,
    pub pan_angle: u32,
    pub decoupled: bool,
    pub trigger_once: bool,
    pub is_triggered: bool,
    pub key: u8,
    pub push: bool,
    pub vector: bool,
    pub reserved: bool,
    pub enter: bool,
    pub exit: bool,
    pub wallpush: bool,
    pub color: bool,
    pub busy_time_b: u32,
    pub wait_time_b: u32,
    pub stop_on: bool,
    pub stop_off: bool,
    pub ceil_pal: u8,
    pub off_ceil_z: i32,
    pub on_ceil_z: i32,
    pub off_floor_z: i32,
    pub on_floor_z: i32,
    pub marker0: u32,
    pub marker1: u32,
    pub crush: bool,
    pub ceil_xpan_frac: u8,
    pub ceil_ypan_frac: u8,
    pub floor_xpan_frac: u8,
    pub damage_type: u8,
    pub floor_pal: u8,
    pub floor_ypan_frac: u8,
    pub locked: bool,
    pub wind_vel: u32,
    pub wind_ang: u32,
    pub wind_always: bool,
    pub dude_lockout: bool,
    pub bob_theta: u32,
    pub bob_z_range: u32,
    pub bob_speed: i32,
    pub bob_always: bool,
    pub bob_floor: bool,
    pub bob_ceiling: bool,
    pub bob_rotate: bool,
}

impl XSector {
    pub fn parse(b: &[u8]) -> Self {
        let mut r = BitReader::new(b);
        let mut x = XSector::default();
        x.reference = r.s(14);
        x.state = r.u(1) as u8;
        x.busy = r.u(17);
        x.data = r.u(16);
        x.tx_id = r.u(10);
        x.busy_wave_a = r.u(3) as u8;
        x.busy_wave_b = r.u(3) as u8;
        x.rx_id = r.u(10);
        x.command = r.u(8) as u8;
        x.trigger_on = r.bit();
        x.trigger_off = r.bit();
        x.busy_time_a = r.u(12);
        x.wait_time_a = r.u(12);
        x.rest_state = r.bit();
        x.interruptable = r.bit();
        x.amplitude = r.s(8);
        x.freq = r.u(8);
        x.retrigger_a = r.bit();
        x.retrigger_b = r.bit();
        x.phase = r.u(8);
        x.wave = r.u(4) as u8;
        x.shade_always = r.bit();
        x.shade_floor = r.bit();
        x.shade_ceiling = r.bit();
        x.shade_walls = r.bit();
        x.shade = r.s(8);
        x.pan_always = r.bit();
        x.pan_floor = r.bit();
        x.pan_ceiling = r.bit();
        x.drag = r.bit();
        x.underwater = r.bit();
        x.depth = r.u(3) as u8;
        x.pan_vel = r.u(8);
        x.pan_angle = r.u(11);
        r.u(1);
        x.decoupled = r.bit();
        x.trigger_once = r.bit();
        x.is_triggered = r.bit();
        x.key = r.u(3) as u8;
        x.push = r.bit();
        x.vector = r.bit();
        x.reserved = r.bit();
        x.enter = r.bit();
        x.exit = r.bit();
        x.wallpush = r.bit();
        x.color = r.bit();
        r.u(1);
        x.busy_time_b = r.u(12);
        x.wait_time_b = r.u(12);
        x.stop_on = r.bit();
        x.stop_off = r.bit();
        x.ceil_pal = r.u(4) as u8;
        x.off_ceil_z = r.s(32);
        x.on_ceil_z = r.s(32);
        x.off_floor_z = r.s(32);
        x.on_floor_z = r.s(32);
        x.marker0 = r.u(16);
        x.marker1 = r.u(16);
        x.crush = r.bit();
        x.ceil_xpan_frac = r.u(8) as u8;
        x.ceil_ypan_frac = r.u(8) as u8;
        x.floor_xpan_frac = r.u(8) as u8;
        x.damage_type = r.u(3) as u8;
        x.floor_pal = r.u(4) as u8;
        x.floor_ypan_frac = r.u(8) as u8;
        x.locked = r.bit();
        x.wind_vel = r.u(10);
        x.wind_ang = r.u(11);
        x.wind_always = r.bit();
        x.dude_lockout = r.bit();
        x.bob_theta = r.u(11);
        x.bob_z_range = r.u(5);
        x.bob_speed = r.s(12);
        x.bob_always = r.bit();
        x.bob_floor = r.bit();
        x.bob_ceiling = r.bit();
        x.bob_rotate = r.bit();
        x
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct XWall {
    pub reference: i32,
    pub state: u8,
    pub busy: u32,
    pub data: i32,
    pub tx_id: u32,
    pub rx_id: u32,
    pub command: u8,
    pub trigger_on: bool,
    pub trigger_off: bool,
    pub busy_time: u32,
    pub wait_time: u32,
    pub rest_state: bool,
    pub interruptable: bool,
    pub pan_always: bool,
    pub pan_xvel: i32,
    pub pan_yvel: i32,
    pub decoupled: bool,
    pub trigger_once: bool,
    pub is_triggered: bool,
    pub key: u8,
    pub trigger_push: bool,
    pub trigger_vector: bool,
    pub trigger_touch: bool,
    pub xpan_frac: u8,
    pub ypan_frac: u8,
    pub locked: bool,
    pub dude_lockout: bool,
}

impl XWall {
    pub fn parse(b: &[u8]) -> Self {
        let mut r = BitReader::new(b);
        let mut x = XWall::default();
        x.reference = r.s(14);
        x.state = r.u(1) as u8;
        x.busy = r.u(17);
        x.data = r.s(16);
        x.tx_id = r.u(10);
        r.u(6);
        x.rx_id = r.u(10);
        x.command = r.u(8) as u8;
        x.trigger_on = r.bit();
        x.trigger_off = r.bit();
        x.busy_time = r.u(12);
        x.wait_time = r.u(12);
        x.rest_state = r.bit();
        x.interruptable = r.bit();
        x.pan_always = r.bit();
        x.pan_xvel = r.s(8);
        x.pan_yvel = r.s(8);
        x.decoupled = r.bit();
        x.trigger_once = r.bit();
        x.is_triggered = r.bit();
        x.key = r.u(3) as u8;
        x.trigger_push = r.bit();
        x.trigger_vector = r.bit();
        x.trigger_touch = r.bit();
        r.u(2);
        x.xpan_frac = r.u(8) as u8;
        x.ypan_frac = r.u(8) as u8;
        x.locked = r.bit();
        x.dude_lockout = r.bit();
        x
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct XSprite {
    pub reference: i32,
    pub state: u8,
    pub busy: u32,
    pub tx_id: u32,
    pub rx_id: u32,
    pub command: u8,
    pub trigger_on: bool,
    pub trigger_off: bool,
    pub wave: u8,
    pub busy_time: u32,
    pub wait_time: u32,
    pub rest_state: bool,
    pub interruptable: bool,
    pub respawn_pending: u8,
    pub launch_team: bool,
    pub drop_msg: u8,
    pub decoupled: bool,
    pub trigger_once: bool,
    pub is_triggered: bool,
    pub key: u8,
    pub push: bool,
    pub vector: bool,
    pub impact: bool,
    pub pickup: bool,
    pub touch: bool,
    pub sight: bool,
    pub proximity: bool,
    /// Bit per difficulty level (1..=5): sprite is *excluded* on that skill.
    pub launch_skill: u8,
    pub launch_single: bool,
    pub launch_bloodbath: bool,
    pub launch_coop: bool,
    pub dude_lockout: bool,
    pub data1: i32,
    pub data2: i32,
    pub data3: i32,
    pub goal_ang: u32,
    pub dodge_dir: i32,
    pub locked: bool,
    pub medium: u8,
    pub respawn: u8,
    pub data4: u32,
    pub lock_msg: u8,
    pub health: u32,
    pub dude_deaf: bool,
    pub dude_ambush: bool,
    pub dude_guard: bool,
    pub dude_flag4: bool,
    pub target: i32,
    pub target_x: i32,
    pub target_y: i32,
    pub target_z: i32,
    pub burn_time: u32,
    pub burn_source: i32,
    pub height: u32,
    pub state_timer: u32,
}

impl XSprite {
    pub fn parse(b: &[u8]) -> Self {
        let mut r = BitReader::new(b);
        let mut x = XSprite::default();
        x.reference = r.s(14);
        x.state = r.u(1) as u8;
        x.busy = r.u(17);
        x.tx_id = r.u(10);
        x.rx_id = r.u(10);
        x.command = r.u(8) as u8;
        x.trigger_on = r.bit();
        x.trigger_off = r.bit();
        x.wave = r.u(2) as u8;
        x.busy_time = r.u(12);
        x.wait_time = r.u(12);
        x.rest_state = r.bit();
        x.interruptable = r.bit();
        r.u(2);
        x.respawn_pending = r.u(2) as u8;
        r.u(1);
        x.launch_team = r.bit();
        x.drop_msg = r.u(8) as u8;
        x.decoupled = r.bit();
        x.trigger_once = r.bit();
        x.is_triggered = r.bit();
        x.key = r.u(3) as u8;
        x.push = r.bit();
        x.vector = r.bit();
        x.impact = r.bit();
        x.pickup = r.bit();
        x.touch = r.bit();
        x.sight = r.bit();
        x.proximity = r.bit();
        r.u(2);
        x.launch_skill = r.u(5) as u8;
        x.launch_single = r.bit();
        x.launch_bloodbath = r.bit();
        x.launch_coop = r.bit();
        x.dude_lockout = r.bit();
        x.data1 = r.s(16);
        x.data2 = r.s(16);
        x.data3 = r.s(16);
        x.goal_ang = r.u(11);
        x.dodge_dir = r.s(2);
        x.locked = r.bit();
        x.medium = r.u(2) as u8;
        x.respawn = r.u(2) as u8;
        x.data4 = r.u(16);
        r.u(6);
        x.lock_msg = r.u(8) as u8;
        x.health = r.u(12);
        x.dude_deaf = r.bit();
        x.dude_ambush = r.bit();
        x.dude_guard = r.bit();
        x.dude_flag4 = r.bit();
        x.target = r.s(16);
        x.target_x = r.s(32);
        x.target_y = r.s(32);
        x.target_z = r.s(32);
        x.burn_time = r.u(16);
        x.burn_source = r.s(16);
        x.height = r.u(16);
        x.state_timer = r.u(16);
        x
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Map {
    pub version: u16,
    pub start_x: i32,
    pub start_y: i32,
    pub start_z: i32,
    pub start_ang: i16,
    pub start_sect: i16,
    pub sky_bits: i16,
    pub visibility: i32,
    pub song_id: i32,
    pub parallax: u8,
    pub revision: i32,
    pub sky_offsets: Vec<i16>,
    pub sectors: Vec<Sector>,
    pub walls: Vec<Wall>,
    pub sprites: Vec<Sprite>,
    /// Indexed by `Sector::extra` (entry 0 is a dummy when absent).
    pub xsectors: Vec<XSector>,
    pub xwalls: Vec<XWall>,
    pub xsprites: Vec<XSprite>,
    pub crc: u32,
}

fn parse_plane(b: &[u8], z_off: usize, stat_off: usize, pic_off: usize) -> Plane {
    Plane {
        z: i32_at(b, z_off),
        stat: u16_at(b, stat_off),
        picnum: i16_at(b, pic_off),
        heinum: i16_at(b, pic_off + 2),
        shade: b[pic_off + 4] as i8,
        pal: b[pic_off + 5],
        xpan: b[pic_off + 6],
        ypan: b[pic_off + 7],
    }
}

impl Sector {
    pub fn parse(b: &[u8]) -> Self {
        Sector {
            wallptr: i16_at(b, 0),
            wallnum: i16_at(b, 2),
            ceiling: parse_plane(b, 4, 12, 16),
            floor: parse_plane(b, 8, 14, 24),
            visibility: b[32],
            filler: b[33],
            lotag: i16_at(b, 34),
            hitag: i16_at(b, 36),
            extra: i16_at(b, 38),
        }
    }
}

impl Wall {
    pub fn parse(b: &[u8]) -> Self {
        Wall {
            x: i32_at(b, 0),
            y: i32_at(b, 4),
            point2: i16_at(b, 8),
            nextwall: i16_at(b, 10),
            nextsector: i16_at(b, 12),
            cstat: u16_at(b, 14),
            picnum: i16_at(b, 16),
            overpicnum: i16_at(b, 18),
            shade: b[20] as i8,
            pal: b[21],
            xrepeat: b[22],
            yrepeat: b[23],
            xpanning: b[24],
            ypanning: b[25],
            lotag: i16_at(b, 26),
            hitag: i16_at(b, 28),
            extra: i16_at(b, 30),
        }
    }
}

impl Sprite {
    pub fn parse(b: &[u8]) -> Self {
        Sprite {
            x: i32_at(b, 0),
            y: i32_at(b, 4),
            z: i32_at(b, 8),
            cstat: u16_at(b, 12),
            picnum: i16_at(b, 14),
            shade: b[16] as i8,
            pal: b[17],
            clipdist: b[18],
            flags: b[19],
            xrepeat: b[20],
            yrepeat: b[21],
            xoffset: b[22] as i8,
            yoffset: b[23] as i8,
            sectnum: i16_at(b, 24),
            statnum: i16_at(b, 26),
            ang: i16_at(b, 28),
            owner: i16_at(b, 30),
            xvel: i16_at(b, 32),
            yvel: i16_at(b, 34),
            zvel: i16_at(b, 36),
            lotag: i16_at(b, 38),
            hitag: i16_at(b, 40),
            extra: i16_at(b, 42),
        }
    }
}

impl Map {
    pub fn parse(data: &[u8]) -> io::Result<Map> {
        if data.len() < 6 || &data[0..4] != b"BLM\x1a" {
            return Err(invalid("not a Blood map (missing BLM signature)"));
        }
        let version = u16_at(data, 4);
        let encrypted = match version & 0xff00 {
            0x0700 => true,
            0x0600 => false,
            _ => return Err(invalid(format!("unsupported map version {version:#x}"))),
        };
        let mut c = Cursor { d: data, p: 6 };
        let mut h = c.take(37)?.to_vec();
        let song_raw = i32_at(&h, 22);
        if song_raw != 0 && song_raw != 0x504d_6172 && song_raw != 0x7261_504d {
            map_crypt(&mut h, 0x4d);
        }
        let mut m = Map {
            version,
            start_x: i32_at(&h, 0),
            start_y: i32_at(&h, 4),
            start_z: i32_at(&h, 8),
            start_ang: i16_at(&h, 12),
            start_sect: i16_at(&h, 14),
            sky_bits: i16_at(&h, 16),
            visibility: i32_at(&h, 18),
            song_id: i32_at(&h, 22),
            parallax: h[26],
            revision: i32_at(&h, 27),
            ..Default::default()
        };
        let nsectors = u16_at(&h, 31) as usize;
        let nwalls = u16_at(&h, 33) as usize;
        let nsprites = u16_at(&h, 35) as usize;

        let (mut xsprite_size, mut xwall_size, mut xsector_size) = (56usize, 24usize, 60usize);
        if encrypted {
            let mut h2 = c.take(128)?.to_vec();
            map_crypt(&mut h2, nwalls as u32);
            let rd = |o: usize| i32_at(&h2, o) as usize;
            if rd(64) != 0 {
                xsprite_size = rd(64);
                xwall_size = rd(68);
                xsector_size = rd(72);
            }
        }

        let nsky = 1usize << m.sky_bits.clamp(0, 12);
        let mut sky = c.take(nsky * 2)?.to_vec();
        if encrypted {
            map_crypt(&mut sky, (nsky * 2) as u32);
        }
        m.sky_offsets = sky.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();

        let rev = m.revision as u32;
        m.xsectors.push(XSector::default());
        for _ in 0..nsectors {
            let mut b = c.take(SECTOR_SIZE)?.to_vec();
            if encrypted {
                map_crypt(&mut b, rev.wrapping_mul(SECTOR_SIZE as u32));
            }
            let mut s = Sector::parse(&b);
            if s.extra > 0 {
                let xb = c.take(xsector_size)?;
                s.extra = m.xsectors.len() as i16;
                m.xsectors.push(XSector::parse(xb));
            } else {
                s.extra = -1;
            }
            m.sectors.push(s);
        }
        m.xwalls.push(XWall::default());
        for _ in 0..nwalls {
            let mut b = c.take(WALL_SIZE)?.to_vec();
            if encrypted {
                map_crypt(&mut b, rev.wrapping_mul(SECTOR_SIZE as u32) | 0x4d);
            }
            let mut w = Wall::parse(&b);
            if w.extra > 0 {
                let xb = c.take(xwall_size)?;
                w.extra = m.xwalls.len() as i16;
                m.xwalls.push(XWall::parse(xb));
            } else {
                w.extra = -1;
            }
            m.walls.push(w);
        }
        m.xsprites.push(XSprite::default());
        for _ in 0..nsprites {
            let mut b = c.take(SPRITE_SIZE)?.to_vec();
            if encrypted {
                map_crypt(&mut b, rev.wrapping_mul(SPRITE_SIZE as u32) | 0x4d);
            }
            let mut s = Sprite::parse(&b);
            if s.extra > 0 {
                let xb = c.take(xsprite_size)?;
                s.extra = m.xsprites.len() as i16;
                m.xsprites.push(XSprite::parse(xb));
            } else {
                s.extra = -1;
            }
            m.sprites.push(s);
        }
        if let Ok(crc) = c.take(4) {
            m.crc = u32::from_le_bytes(crc.try_into().unwrap());
        }
        if c.p != data.len() {
            return Err(invalid(format!(
                "map parse ended at {} of {} bytes (layout mismatch)",
                c.p,
                data.len()
            )));
        }
        m.validate()?;
        Ok(m)
    }

    /// Sanity-checks geometry references.
    pub fn validate(&self) -> io::Result<()> {
        let nw = self.walls.len() as i32;
        let ns = self.sectors.len() as i32;
        for (i, s) in self.sectors.iter().enumerate() {
            let a = s.wallptr as i32;
            if a < 0 || a + s.wallnum as i32 > nw {
                return Err(invalid(format!("sector {i} walls out of range")));
            }
        }
        for (i, w) in self.walls.iter().enumerate() {
            if w.point2 < 0 || w.point2 as i32 >= nw {
                return Err(invalid(format!("wall {i} point2 out of range")));
            }
            if w.nextsector >= ns as i16 || w.nextwall >= nw as i16 {
                return Err(invalid(format!("wall {i} portal out of range")));
            }
        }
        Ok(())
    }

    pub fn xsector(&self, sect: usize) -> Option<&XSector> {
        let e = self.sectors.get(sect)?.extra;
        (e > 0).then(|| &self.xsectors[e as usize])
    }
    pub fn xwall(&self, wall: usize) -> Option<&XWall> {
        let e = self.walls.get(wall)?.extra;
        (e > 0).then(|| &self.xwalls[e as usize])
    }
    pub fn xsprite(&self, spr: usize) -> Option<&XSprite> {
        let e = self.sprites.get(spr)?.extra;
        (e > 0).then(|| &self.xsprites[e as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitreader_signed_and_unsigned() {
        let data = [0b1010_1101u8, 0b0000_0011];
        let mut r = BitReader::new(&data);
        assert_eq!(r.u(3), 0b101);
        assert_eq!(r.s(3), -3); // 0b101 sign-extended
        assert_eq!(r.u(4), 0b1110);
    }

    #[test]
    fn xstruct_bit_totals() {
        // Parsing an all-ones buffer must not panic and must consume exactly the record.
        let ones = [0xffu8; 60];
        let xs = XSector::parse(&ones);
        assert!(xs.bob_rotate);
        let xw = XWall::parse(&ones[..24]);
        assert!(xw.dude_lockout);
        let xp = XSprite::parse(&ones[..56]);
        assert_eq!(xp.state_timer, 0xffff);
    }
}
