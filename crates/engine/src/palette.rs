//! Palettes and lookup tables.
//!
//! * `.PAL`  – 256 RGB triplets, 6 bits per channel.
//! * `.PLU`  – palookup: 64 shade levels x 256 colors (shade 0 = brightest).
//! * `.TLU`  – 256x256 translucency table, `tlu[a * 256 + b]`.

pub const NUM_SHADES: usize = 64;

#[derive(Clone)]
pub struct Palette {
    /// 0x00RRGGBB, expanded to 8 bits per channel.
    pub colors: [u32; 256],
}

impl Palette {
    /// Parses 768 bytes of RGB. Classic VGA palettes use 6 bits per channel; Blood's
    /// palettes are stored with full 8-bit values, so the depth is auto-detected.
    pub fn from_vga(b: &[u8]) -> Self {
        let six_bit = b.iter().take(768).all(|&v| v < 64);
        let mut colors = [0u32; 256];
        for (i, c) in colors.iter_mut().enumerate() {
            let e = |v: u8| -> u32 {
                if six_bit {
                    let v = v as u32;
                    (v << 2) | (v >> 4)
                } else {
                    v as u32
                }
            };
            if i * 3 + 2 < b.len() {
                *c = (e(b[i * 3]) << 16) | (e(b[i * 3 + 1]) << 8) | e(b[i * 3 + 2]);
            }
        }
        Palette { colors }
    }

    pub fn rgb(&self, idx: u8) -> (u8, u8, u8) {
        let c = self.colors[idx as usize];
        ((c >> 16) as u8, (c >> 8) as u8, c as u8)
    }
}

/// Full set of colour tables used by the renderer.
#[derive(Clone)]
pub struct ColorTables {
    pub palettes: Vec<Palette>,
    /// Indexed by sprite/wall `pal` value. `None` falls back to table 0.
    pub lookups: Vec<Option<Vec<u8>>>,
    pub trans: Vec<u8>,
}

impl ColorTables {
    pub fn new(base: Palette) -> Self {
        // Identity shade table: shade simply darkens toward index 0.
        ColorTables {
            palettes: vec![base],
            lookups: vec![None; 256],
            trans: Vec::new(),
        }
    }

    pub fn lookup(&self, pal: u8) -> &[u8] {
        match &self.lookups[pal as usize] {
            Some(t) => t,
            None => self.lookups[0].as_deref().unwrap_or(&[]),
        }
    }

    /// Builds a true-colour shade table for one palookup and base palette:
    /// `out[shade * 256 + index]` = 0x00RRGGBB.
    pub fn build_rgb_lut(&self, pal: u8, base: usize) -> Vec<u32> {
        let p = &self.palettes[base.min(self.palettes.len() - 1)];
        let lk = self.lookup(pal);
        let mut out = vec![0u32; NUM_SHADES * 256];
        for s in 0..NUM_SHADES {
            for c in 0..256 {
                let idx = if lk.len() >= NUM_SHADES * 256 {
                    lk[s * 256 + c]
                } else {
                    c as u8
                };
                out[s * 256 + c] = p.colors[idx as usize];
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_bit_detect() {
        let mut b = vec![0u8; 768];
        b[3] = 200;
        assert_eq!(Palette::from_vga(&b).rgb(1).0, 200);
    }

    #[test]
    fn vga_expand() {
        let mut b = vec![0u8; 768];
        b[3] = 63;
        b[4] = 32;
        b[5] = 1;
        let p = Palette::from_vga(&b);
        assert_eq!(p.rgb(1), (255, 130, 4));
        assert_eq!(p.rgb(0), (0, 0, 0));
    }
}
