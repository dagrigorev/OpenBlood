//! Blood animation formats.
//!
//! * `SEQ` – sprite sequences: 16-byte header (`SEQ\x1a`, u16 version, u16 frames,
//!   u16 ticks per frame, u16 sound id, u32 flags) followed by 8-byte bitfield frames.
//! * `QAV` – screen-space (weapon / HUD) animations: 36-byte header followed by frames of
//!   one callback id, a sound record and 8 tile layers (204 bytes per frame).

use ob_engine::map::BitReader;
use ob_engine::rff::Rff;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct SeqFrame {
    pub tile: i32,
    pub transparent: bool,
    pub transparent2: bool,
    pub blockable: bool,
    pub hittable: bool,
    pub xrepeat: u8,
    pub yrepeat: u8,
    pub shade: i8,
    pub pal: u8,
    /// Fires the owner's callback (attack, footstep...) when this frame starts.
    pub trigger: bool,
    pub smoke: bool,
    pub autoaim: bool,
    pub pushable: bool,
    pub play_sound: bool,
    pub invisible: bool,
    pub flip_x: bool,
    pub flip_y: bool,
    pub sound_range: u8,
    pub surface_sound: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Seq {
    pub id: u32,
    pub name: String,
    pub ticks_per_frame: i32,
    pub sound_id: i32,
    pub flags: u32,
    pub frames: Vec<SeqFrame>,
}

pub const SEQ_LOOP: u32 = 1;
pub const SEQ_REMOVE: u32 = 2;

impl Seq {
    pub fn parse(b: &[u8]) -> Option<Seq> {
        if b.len() < 16 || &b[0..4] != b"SEQ\x1a" {
            return None;
        }
        let rd16 = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let n = rd16(6) as usize;
        let mut s = Seq {
            ticks_per_frame: rd16(8) as i32,
            sound_id: rd16(10) as i32,
            flags: u32::from_le_bytes(b[12..16].try_into().unwrap()),
            ..Default::default()
        };
        for i in 0..n {
            let o = 16 + i * 8;
            if o + 8 > b.len() {
                break;
            }
            let mut r = BitReader::new(&b[o..o + 8]);
            let mut f = SeqFrame {
                tile: r.u(12) as i32,
                ..Default::default()
            };
            f.transparent = r.bit();
            f.transparent2 = r.bit();
            f.blockable = r.bit();
            f.hittable = r.bit();
            f.xrepeat = r.u(8) as u8;
            f.yrepeat = r.u(8) as u8;
            f.shade = r.s(8) as i8;
            f.pal = r.u(5) as u8;
            f.trigger = r.bit();
            f.smoke = r.bit();
            f.autoaim = r.bit();
            f.pushable = r.bit();
            f.play_sound = r.bit();
            f.invisible = r.bit();
            f.flip_x = r.bit();
            f.flip_y = r.bit();
            f.tile |= (r.u(4) as i32) << 12;
            f.sound_range = r.u(4) as u8;
            f.surface_sound = r.bit();
            s.frames.push(f);
        }
        Some(s)
    }

    pub fn looping(&self) -> bool {
        self.flags & SEQ_LOOP != 0
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct QavLayer {
    pub picnum: i32,
    pub x: i32,
    pub y: i32,
    /// Scale, 0x10000 = 100%.
    pub z: i32,
    pub stat: i32,
    pub shade: i8,
    pub pal: u8,
    pub angle: u16,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct QavSound {
    pub id: i32,
    pub priority: u8,
    pub flags: u8,
    pub range: u8,
}

#[derive(Debug, Clone, Default)]
pub struct QavFrame {
    pub callback: i32,
    pub sound: QavSound,
    pub layers: [QavLayer; 8],
}

#[derive(Debug, Clone, Default)]
pub struct Qav {
    pub name: String,
    pub ticks_per_frame: i32,
    pub duration: i32,
    pub x: i32,
    pub y: i32,
    pub frames: Vec<QavFrame>,
}

impl Qav {
    pub const HEADER: usize = 36;
    pub const FRAME: usize = 204;

    pub fn parse(b: &[u8]) -> Option<Qav> {
        if b.len() < Self::HEADER || &b[0..4] != b"QAV\x1a" {
            return None;
        }
        let rd = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let n = rd(8).max(0) as usize;
        let mut q = Qav {
            ticks_per_frame: rd(12).max(1),
            duration: rd(16),
            x: rd(20),
            y: rd(24),
            ..Default::default()
        };
        for i in 0..n {
            let o = Self::HEADER + i * Self::FRAME;
            if o + Self::FRAME > b.len() {
                break;
            }
            let mut f = QavFrame {
                callback: rd(o),
                sound: QavSound {
                    id: rd(o + 4),
                    priority: b[o + 8],
                    flags: b[o + 9],
                    range: b[o + 10],
                },
                ..Default::default()
            };
            for (l, layer) in f.layers.iter_mut().enumerate() {
                let p = o + 12 + l * 24;
                *layer = QavLayer {
                    picnum: rd(p),
                    x: rd(p + 4),
                    y: rd(p + 8),
                    z: rd(p + 12),
                    stat: rd(p + 16),
                    shade: b[p + 20] as i8,
                    pal: b[p + 21],
                    angle: u16::from_le_bytes([b[p + 22], b[p + 23]]),
                };
            }
            q.frames.push(f);
        }
        Some(q)
    }

    /// Frame index shown `t` ticks after the animation started (clamped to the last frame).
    pub fn frame_at(&self, t: i32) -> usize {
        ((t.max(0) / self.ticks_per_frame) as usize).min(self.frames.len().saturating_sub(1))
    }

    pub fn length(&self) -> i32 {
        self.frames.len() as i32 * self.ticks_per_frame
    }
}

/// Lazily loaded animation library.
#[derive(Default)]
pub struct AnimLib {
    seqs: HashMap<u32, Option<Arc<Seq>>>,
    qavs: HashMap<u32, Option<Arc<Qav>>>,
    /// SEQ names by id, for looking up dude animations by suffix.
    seq_names: HashMap<u32, String>,
}

impl AnimLib {
    pub fn new(rff: &Rff) -> Self {
        let mut l = AnimLib::default();
        for e in rff.entries.iter().filter(|e| e.ext == "SEQ") {
            l.seq_names.insert(e.id, e.name.clone());
        }
        l
    }

    pub fn seq(&mut self, rff: &Rff, id: u32) -> Option<Arc<Seq>> {
        self.seqs
            .entry(id)
            .or_insert_with(|| {
                rff.read_id("SEQ", id).and_then(|b| Seq::parse(&b)).map(|mut s| {
                    s.id = id;
                    Arc::new(s)
                })
            })
            .clone()
    }

    pub fn qav(&mut self, rff: &Rff, id: u32) -> Option<Arc<Qav>> {
        self.qavs
            .entry(id)
            .or_insert_with(|| rff.read_id("QAV", id).and_then(|b| Qav::parse(&b)).map(Arc::new))
            .clone()
    }

    /// Finds a SEQ id in `base..base+256` whose name ends with `suffix` (e.g. "I1", "M1").
    pub fn find_seq(&self, base: u32, suffix: &str) -> Option<u32> {
        let mut ids: Vec<u32> = (base..base + 256).filter(|i| self.seq_names.contains_key(i)).collect();
        ids.sort();
        ids.into_iter().find(|i| self.seq_names[i].ends_with(suffix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_frame_bits() {
        let mut b = Vec::from(&b"SEQ\x1a"[..]);
        b.extend_from_slice(&0x300u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&9u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        // tile 0xdcc, blockable; shade -128; tile2 = 1 -> tile 0x1dcc
        let w0: u32 = 0xdcc | (1 << 14) | (40 << 16) | (40 << 24);
        let w1: u32 = 0x80 | (1 << 21);
        b.extend_from_slice(&w0.to_le_bytes());
        b.extend_from_slice(&w1.to_le_bytes());
        let s = Seq::parse(&b).unwrap();
        let f = s.frames[0];
        assert_eq!(f.tile, 0x1dcc);
        assert!(f.blockable && !f.hittable);
        assert_eq!((f.xrepeat, f.yrepeat, f.shade), (40, 40, -128));
        assert!(s.looping());
    }

    #[test]
    fn qav_size_math() {
        let mut b = vec![0u8; Qav::HEADER + 2 * Qav::FRAME];
        b[0..4].copy_from_slice(b"QAV\x1a");
        b[8] = 2;
        b[12] = 5;
        b[Qav::HEADER + Qav::FRAME + 12..Qav::HEADER + Qav::FRAME + 16].copy_from_slice(&3222i32.to_le_bytes());
        let q = Qav::parse(&b).unwrap();
        assert_eq!(q.frames.len(), 2);
        assert_eq!(q.frames[1].layers[0].picnum, 3222);
        assert_eq!(q.frame_at(7), 1);
        assert_eq!(q.frame_at(100), 1);
    }
}
