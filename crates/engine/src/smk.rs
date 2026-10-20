//! Smacker (`SMK2`/`SMK4`) video decoder, written from the public format description:
//! a header with per-frame sizes and types, four Huffman trees, then per frame an optional
//! palette update, audio chunks (skipped; Blood's cutscene audio is in separate WAV files)
//! and a bitstream of 4x4 blocks.

use std::io;

fn invalid(m: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m.to_string())
}

/// Least-significant-bit-first bit reader.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0 }
    }

    #[inline]
    fn bit(&mut self) -> u32 {
        let byte = self.data.get(self.pos >> 3).copied().unwrap_or(0);
        let b = (byte >> (self.pos & 7)) & 1;
        self.pos += 1;
        b as u32
    }

    fn bits(&mut self, n: u32) -> u32 {
        let mut v = 0;
        for i in 0..n {
            v |= self.bit() << i;
        }
        v
    }

    fn overrun(&self) -> bool {
        self.pos > self.data.len() * 8
    }
}

/// Huffman tree stored as nodes: `Branch(zero, one)` or a leaf index into a value table.
#[derive(Clone, Copy)]
enum Node {
    Branch(u32, u32),
    Leaf(u32),
}

/// 8-bit tree used to build the 16-bit trees.
struct SmallTree {
    nodes: Vec<Node>,
    values: Vec<u8>,
}

impl SmallTree {
    fn read(br: &mut Bits) -> io::Result<SmallTree> {
        let mut t = SmallTree {
            nodes: Vec::new(),
            values: Vec::new(),
        };
        if br.bit() == 0 {
            return Ok(t); // empty: always 0
        }
        t.build(br, 0)?;
        br.bit(); // terminator
        Ok(t)
    }

    fn build(&mut self, br: &mut Bits, depth: u32) -> io::Result<u32> {
        if depth > 32 || br.overrun() {
            return Err(invalid("smacker: bad small tree"));
        }
        let me = self.nodes.len() as u32;
        if br.bit() == 1 {
            self.nodes.push(Node::Branch(0, 0));
            let a = self.build(br, depth + 1)?;
            let b = self.build(br, depth + 1)?;
            self.nodes[me as usize] = Node::Branch(a, b);
        } else {
            self.values.push(br.bits(8) as u8);
            self.nodes.push(Node::Leaf(self.values.len() as u32 - 1));
        }
        Ok(me)
    }

    fn decode(&self, br: &mut Bits) -> u32 {
        if self.nodes.is_empty() {
            return 0;
        }
        let mut n = 0usize;
        loop {
            match self.nodes[n] {
                Node::Leaf(v) => return self.values[v as usize] as u32,
                Node::Branch(a, b) => n = if br.bit() == 0 { a } else { b } as usize,
            }
        }
    }
}

/// 16-bit tree with Smacker's three "recent value" escape leaves.
struct BigTree {
    nodes: Vec<Node>,
    values: Vec<u32>,
    last: [usize; 3],
}

impl BigTree {
    fn read(br: &mut Bits) -> io::Result<BigTree> {
        let mut t = BigTree {
            nodes: Vec::new(),
            values: Vec::new(),
            last: [0; 3],
        };
        if br.bit() == 0 {
            // Empty tree: every value is 0 and no bits are read.
            t.values = vec![0; 3];
            t.last = [0, 1, 2];
            return Ok(t);
        }
        let lo = SmallTree::read(br)?;
        let hi = SmallTree::read(br)?;
        let esc = [br.bits(16), br.bits(16), br.bits(16)];
        let mut last: [Option<usize>; 3] = [None; 3];
        t.build(br, &lo, &hi, &esc, &mut last, 0)?;
        br.bit(); // terminator
        for (i, l) in last.iter().enumerate() {
            t.last[i] = match l {
                Some(idx) => *idx,
                None => {
                    // No leaf carried this escape: use a spare slot outside the tree.
                    t.values.push(0);
                    t.values.len() - 1
                }
            };
        }
        Ok(t)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        br: &mut Bits,
        lo: &SmallTree,
        hi: &SmallTree,
        esc: &[u32; 3],
        last: &mut [Option<usize>; 3],
        depth: u32,
    ) -> io::Result<u32> {
        if depth > 32 || br.overrun() {
            return Err(invalid("smacker: bad big tree"));
        }
        let me = self.nodes.len() as u32;
        if br.bit() == 1 {
            self.nodes.push(Node::Branch(0, 0));
            let a = self.build(br, lo, hi, esc, last, depth + 1)?;
            let b = self.build(br, lo, hi, esc, last, depth + 1)?;
            self.nodes[me as usize] = Node::Branch(a, b);
        } else {
            let mut v = lo.decode(br) | (hi.decode(br) << 8);
            let idx = self.values.len();
            if let Some(k) = esc.iter().position(|&e| e == v) {
                last[k] = Some(idx);
                v = 0;
            }
            self.values.push(v);
            self.nodes.push(Node::Leaf(idx as u32));
        }
        Ok(me)
    }

    /// Clears the recent-value cache (done before every frame).
    fn reset(&mut self) {
        for &l in &self.last {
            self.values[l] = 0;
        }
    }

    fn decode(&mut self, br: &mut Bits) -> u32 {
        if self.nodes.is_empty() {
            return 0;
        }
        let mut n = 0usize;
        let leaf = loop {
            match self.nodes[n] {
                Node::Leaf(v) => break v as usize,
                Node::Branch(a, b) => n = if br.bit() == 0 { a } else { b } as usize,
            }
        };
        let v = self.values[leaf];
        let [l0, l1, l2] = self.last;
        if v != self.values[l0] {
            self.values[l2] = self.values[l1];
            self.values[l1] = self.values[l0];
            self.values[l0] = v;
        }
        v
    }
}

const RING_FRAME: u32 = 1;
const Y_INTERLACE: u32 = 2;
const Y_DOUBLE: u32 = 4;

/// An open Smacker video; call [`Smk::next_frame`] to advance and read [`Smk::pixels`].
pub struct Smk {
    data: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub frames: usize,
    /// Milliseconds per frame.
    pub frame_ms: f64,
    flags: u32,
    v4: bool,
    sizes: Vec<u32>,
    types: Vec<u8>,
    offsets: Vec<usize>,
    mmap: BigTree,
    mclr: BigTree,
    full: BigTree,
    ty: BigTree,
    /// Current frame as palette indices, `width * height`.
    index: Vec<u8>,
    /// Current palette, 0x00RRGGBB.
    palette: [u32; 256],
    current: usize,
}

const BLOCK_RUNS: [u32; 64] = {
    let mut t = [0u32; 64];
    let mut i = 0;
    while i < 59 {
        t[i] = i as u32 + 1;
        i += 1;
    }
    t[59] = 128;
    t[60] = 256;
    t[61] = 512;
    t[62] = 1024;
    t[63] = 2048;
    t
};

fn pal6(v: u8) -> u32 {
    ((v as u32 & 63) * 255 + 31) / 63
}

impl Smk {
    pub fn parse(data: Vec<u8>) -> io::Result<Smk> {
        if data.len() < 104 || !(data.starts_with(b"SMK2") || data.starts_with(b"SMK4")) {
            return Err(invalid("not a Smacker file"));
        }
        let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let width = u32_at(4) as usize;
        let height = u32_at(8) as usize;
        let frames = u32_at(12) as usize;
        let rate = u32_at(16) as i32;
        let flags = u32_at(20);
        let trees_size = u32_at(52) as usize;
        if width == 0 || height == 0 || width > 4096 || height > 4096 || frames == 0 || frames > 100_000 {
            return Err(invalid("smacker: bad header"));
        }
        let frame_ms = match rate {
            r if r > 0 => r as f64,
            r if r < 0 => -r as f64 / 100.0,
            _ => 100.0,
        };
        let n = frames + (flags & RING_FRAME) as usize;
        let mut p = 104;
        let need = p + n * 5 + trees_size;
        if data.len() < need {
            return Err(invalid("smacker: truncated"));
        }
        let sizes: Vec<u32> = (0..n).map(|i| u32_at(p + i * 4)).collect();
        p += n * 4;
        let types = data[p..p + n].to_vec();
        p += n;
        let (mmap, mclr, full, ty) = {
            let mut br = Bits::new(&data[p..p + trees_size]);
            (
                BigTree::read(&mut br)?,
                BigTree::read(&mut br)?,
                BigTree::read(&mut br)?,
                BigTree::read(&mut br)?,
            )
        };
        p += trees_size;
        let mut offsets = Vec::with_capacity(n);
        for s in &sizes {
            offsets.push(p);
            p += (s & !3) as usize;
        }
        if p > data.len() {
            return Err(invalid("smacker: frame data truncated"));
        }
        let v4 = data.starts_with(b"SMK4");
        Ok(Smk {
            width,
            height,
            frames,
            frame_ms,
            flags,
            v4,
            sizes,
            types,
            offsets,
            mmap,
            mclr,
            full,
            ty,
            index: vec![0; width * height],
            palette: [0; 256],
            current: 0,
            data,
        })
    }

    /// Height the video is meant to be shown at (doubled/interlaced files are stretched).
    pub fn display_height(&self) -> usize {
        if self.flags & (Y_INTERLACE | Y_DOUBLE) != 0 {
            self.height * 2
        } else {
            self.height
        }
    }

    /// Decodes the next frame. Returns false after the last one.
    pub fn next_frame(&mut self) -> io::Result<bool> {
        if self.current >= self.frames {
            return Ok(false);
        }
        let i = self.current;
        self.current += 1;
        let start = self.offsets[i];
        let end = start + (self.sizes[i] & !3) as usize;
        let mut p = start;
        let ft = self.types[i];
        if ft & 1 != 0 {
            let len = self.data.get(p).copied().unwrap_or(0) as usize * 4;
            if len == 0 || p + len > end {
                return Err(invalid("smacker: bad palette chunk"));
            }
            self.update_palette(p + 1, p + len);
            p += len;
        }
        for track in 0..7 {
            if ft & (2 << track) != 0 {
                let len = u32::from_le_bytes(self.data[p..p + 4].try_into().unwrap()) as usize;
                p += len.max(4);
            }
        }
        if p > end {
            return Err(invalid("smacker: bad audio chunk"));
        }
        self.decode_video(p, end);
        Ok(true)
    }

    fn update_palette(&mut self, mut p: usize, end: usize) {
        let old = self.palette;
        let mut i = 0usize;
        while i < 256 && p < end {
            let t = self.data[p];
            p += 1;
            if t & 0x80 != 0 {
                i += (t & 0x7f) as usize + 1; // keep entries
            } else if t & 0x40 != 0 {
                let off = self.data.get(p).copied().unwrap_or(0) as usize;
                p += 1;
                let n = (t & 0x3f) as usize + 1;
                for k in 0..n {
                    if i + k < 256 && off + k < 256 {
                        self.palette[i + k] = old[off + k];
                    }
                }
                i += n;
            } else {
                let (r, g, b) = (
                    t,
                    self.data.get(p).copied().unwrap_or(0),
                    self.data.get(p + 1).copied().unwrap_or(0),
                );
                p += 2;
                self.palette[i] = (pal6(r) << 16) | (pal6(g) << 8) | pal6(b);
                i += 1;
            }
        }
    }

    fn decode_video(&mut self, p: usize, end: usize) {
        let data = std::mem::take(&mut self.data);
        let mut br = Bits::new(&data[p..end.min(data.len())]);
        for t in [&mut self.mmap, &mut self.mclr, &mut self.full, &mut self.ty] {
            t.reset();
        }
        let (w, h) = (self.width, self.height);
        let bw = w / 4;
        let blocks = bw * (h / 4);
        let mut blk = 0usize;
        while blk < blocks && !br.overrun() {
            let v = self.ty.decode(&mut br);
            let kind = v & 3;
            let mut run = BLOCK_RUNS[((v >> 2) & 63) as usize] as usize;
            let arg = v >> 8;
            let mode = if kind == 1 && self.v4 {
                if br.bit() == 1 {
                    1
                } else if br.bit() == 1 {
                    2
                } else {
                    0
                }
            } else {
                0
            };
            while run > 0 && blk < blocks {
                run -= 1;
                let x0 = (blk % bw) * 4;
                let y0 = (blk / bw) * 4;
                let at = |x: usize, y: usize| (y0 + y) * w + x0 + x;
                match kind {
                    0 => {
                        let clr = self.mclr.decode(&mut br);
                        let mut map = self.mmap.decode(&mut br);
                        let (lo, hi) = ((clr & 0xff) as u8, (clr >> 8) as u8);
                        for y in 0..4 {
                            for x in 0..4 {
                                self.index[at(x, y)] = if map & 1 != 0 { hi } else { lo };
                                map >>= 1;
                            }
                        }
                    }
                    1 => match mode {
                        0 => {
                            for y in 0..4 {
                                let a = self.full.decode(&mut br);
                                self.index[at(2, y)] = a as u8;
                                self.index[at(3, y)] = (a >> 8) as u8;
                                let b = self.full.decode(&mut br);
                                self.index[at(0, y)] = b as u8;
                                self.index[at(1, y)] = (b >> 8) as u8;
                            }
                        }
                        1 => {
                            for half in 0..2 {
                                let a = self.full.decode(&mut br);
                                for y in half * 2..half * 2 + 2 {
                                    self.index[at(0, y)] = a as u8;
                                    self.index[at(1, y)] = a as u8;
                                    self.index[at(2, y)] = (a >> 8) as u8;
                                    self.index[at(3, y)] = (a >> 8) as u8;
                                }
                            }
                        }
                        _ => {
                            for half in 0..2 {
                                let a = self.full.decode(&mut br);
                                let b = self.full.decode(&mut br);
                                for y in half * 2..half * 2 + 2 {
                                    self.index[at(0, y)] = b as u8;
                                    self.index[at(1, y)] = (b >> 8) as u8;
                                    self.index[at(2, y)] = a as u8;
                                    self.index[at(3, y)] = (a >> 8) as u8;
                                }
                            }
                        }
                    },
                    2 => {}
                    _ => {
                        let c = arg as u8;
                        for y in 0..4 {
                            for x in 0..4 {
                                self.index[at(x, y)] = c;
                            }
                        }
                    }
                }
                blk += 1;
            }
        }
        self.data = data;
    }

    /// The current frame as 0x00RRGGBB pixels (`width * height`).
    pub fn pixels(&self) -> Vec<u32> {
        self.index.iter().map(|&i| self.palette[i as usize]).collect()
    }

    /// Index of the frame most recently decoded (0-based); `frames` when finished.
    pub fn position(&self) -> usize {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_reader_is_lsb_first() {
        let mut b = Bits::new(&[0b1010_0101, 0xff]);
        assert_eq!(b.bit(), 1);
        assert_eq!(b.bit(), 0);
        assert_eq!(b.bits(6), 0b101001);
        assert_eq!(b.bits(4), 0xf);
    }

    #[test]
    fn six_bit_palette_scale() {
        assert_eq!(pal6(0), 0);
        assert_eq!(pal6(63), 255);
        assert_eq!(pal6(16), 0x41);
    }

    #[test]
    fn rejects_other_files() {
        assert!(Smk::parse(b"RIFF....".to_vec()).is_err());
    }
}
