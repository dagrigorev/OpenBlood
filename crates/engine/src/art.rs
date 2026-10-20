//! Build `.ART` tile archives (TILES000.ART ... TILES0NN.ART).
//!
//! Each file: `u32 version (1)`, `u32 numtiles (unused)`, `u32 first`, `u32 last`,
//! then `i16 width[n]`, `i16 height[n]`, `u32 picanm[n]`, followed by tile pixels
//! stored column-major (x outer, y inner), one palette index per pixel.

use std::io;
use std::path::Path;

pub const MAX_TILES: usize = 6144;

/// Animation / view attributes packed into the 32-bit picanm word.
#[derive(Debug, Clone, Copy, Default)]
pub struct PicAnm {
    pub frames: u8,
    /// 0 none, 1 oscillate, 2 forward, 3 backward.
    pub anim_type: u8,
    pub xoffset: i8,
    pub yoffset: i8,
    pub speed: u8,
    /// Blood sprite view: 0 single, 1 five-angle, 2 eight-angle, 3 bounce,
    /// 6 voxel, 7 spinning voxel.
    pub view: u8,
}

impl PicAnm {
    pub fn from_u32(v: u32) -> Self {
        PicAnm {
            frames: (v & 0x3f) as u8,
            anim_type: ((v >> 6) & 3) as u8,
            xoffset: ((v >> 8) & 0xff) as u8 as i8,
            yoffset: ((v >> 16) & 0xff) as u8 as i8,
            speed: ((v >> 24) & 0x0f) as u8,
            view: ((v >> 28) & 7) as u8,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Tile {
    pub width: u16,
    pub height: u16,
    pub anm: PicAnm,
    /// Column-major pixel data, `width * height` bytes. Empty when the tile is absent.
    pub pixels: Vec<u8>,
}

impl Tile {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }
    #[inline]
    pub fn column(&self, x: usize) -> &[u8] {
        let h = self.height as usize;
        &self.pixels[x * h..x * h + h]
    }
}

pub struct TileSet {
    pub tiles: Vec<Tile>,
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

impl Default for TileSet {
    fn default() -> Self {
        TileSet {
            tiles: vec![Tile::default(); MAX_TILES],
        }
    }
}

impl TileSet {
    /// Loads `TILES000.ART`, `TILES001.ART`, ... from `dir` until a file is missing.
    pub fn load_dir(dir: impl AsRef<Path>) -> io::Result<Self> {
        let mut set = TileSet::default();
        let mut loaded = 0;
        for i in 0..100 {
            let path = dir.as_ref().join(format!("TILES{i:03}.ART"));
            match std::fs::read(&path) {
                Ok(bytes) => {
                    set.add_art(&bytes)?;
                    loaded += 1;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => break,
                Err(e) => return Err(e),
            }
        }
        if loaded == 0 {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no TILES000.ART found"));
        }
        Ok(set)
    }

    pub fn add_art(&mut self, b: &[u8]) -> io::Result<()> {
        if b.len() < 16 {
            return Err(invalid("ART file too short"));
        }
        let rd = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        if rd(0) != 1 {
            return Err(invalid("unsupported ART version"));
        }
        let first = rd(8) as usize;
        let last = rd(12) as usize;
        if last < first || last >= MAX_TILES {
            return Err(invalid("ART tile range invalid"));
        }
        let n = last - first + 1;
        let mut p = 16;
        let need = p + n * 8;
        if b.len() < need {
            return Err(invalid("ART header truncated"));
        }
        let w: Vec<u16> = (0..n)
            .map(|i| u16::from_le_bytes([b[p + i * 2], b[p + i * 2 + 1]]))
            .collect();
        p += n * 2;
        let h: Vec<u16> = (0..n)
            .map(|i| u16::from_le_bytes([b[p + i * 2], b[p + i * 2 + 1]]))
            .collect();
        p += n * 2;
        let anm: Vec<u32> = (0..n).map(|i| rd(p + i * 4)).collect();
        p += n * 4;
        for i in 0..n {
            let size = w[i] as usize * h[i] as usize;
            let t = &mut self.tiles[first + i];
            t.width = w[i];
            t.height = h[i];
            t.anm = PicAnm::from_u32(anm[i]);
            if size > 0 {
                if p + size > b.len() {
                    return Err(invalid(format!("tile {} data truncated", first + i)));
                }
                t.pixels = b[p..p + size].to_vec();
                p += size;
            } else {
                t.pixels.clear();
            }
        }
        Ok(())
    }

    #[inline]
    pub fn get(&self, pic: i32) -> &Tile {
        static EMPTY: Tile = Tile {
            width: 0,
            height: 0,
            anm: PicAnm {
                frames: 0,
                anim_type: 0,
                xoffset: 0,
                yoffset: 0,
                speed: 0,
                view: 0,
            },
            pixels: Vec::new(),
        };
        if pic < 0 || pic as usize >= self.tiles.len() {
            &EMPTY
        } else {
            &self.tiles[pic as usize]
        }
    }

    /// Resolves Build tile animation for the given game clock (120 ticks/sec).
    pub fn animate(&self, pic: i32, clock: i32) -> i32 {
        let t = self.get(pic);
        let a = t.anm;
        if a.frames == 0 || a.anim_type == 0 {
            return pic;
        }
        let frames = a.frames as i32;
        let i = clock >> a.speed;
        let off = match a.anim_type {
            1 => {
                let k = i % (frames * 2);
                if k < frames {
                    k
                } else {
                    frames * 2 - k
                }
            }
            2 => i % (frames + 1),
            3 => -(i % (frames + 1)),
            _ => 0,
        };
        pic + off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picanm_unpack() {
        let v = 5 | (2 << 6) | (0xfe << 8) | (3 << 16) | (4 << 24) | (6 << 28);
        let a = PicAnm::from_u32(v);
        assert_eq!(
            (a.frames, a.anim_type, a.xoffset, a.yoffset, a.speed, a.view),
            (5, 2, -2, 3, 4, 6)
        );
    }

    #[test]
    fn parse_synthetic_art() {
        let mut b = Vec::new();
        for v in [1u32, 0, 10, 11] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let mut set = TileSet::default();
        set.add_art(&b).unwrap();
        assert_eq!(set.get(10).column(1), &[4, 5, 6]);
        assert!(set.get(11).is_empty());
    }
}
