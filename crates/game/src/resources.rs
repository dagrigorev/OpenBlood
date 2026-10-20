//! Locates and loads the original Blood data files supplied by the user.

use ob_engine::art::TileSet;
use ob_engine::palette::{Palette, NUM_SHADES};
use ob_engine::render::ShadeLuts;
use ob_engine::rff::Rff;
use std::io;
use std::path::{Path, PathBuf};

/// Palette lookup tables shipped in BLOOD.RFF, by palookup number.
pub const PLU_NAMES: [&str; 15] = [
    "NORMAL", "SATURATE", "BEAST", "TOMMY", "SPIDER3", "GRAY", "GRAYISH", "SPIDER1", "SPIDER2", "FLAME", "COLD", "P1",
    "P2", "P3", "P4",
];

/// Base palettes: 0 normal, 1 water, 2 beast, 3 sewer, 4 invulnerability.
pub const PAL_NAMES: [&str; 5] = ["BLOOD", "WATER", "BEAST", "SEWER", "INVULN1"];

pub struct Resources {
    pub dir: PathBuf,
    pub blood: Rff,
    pub sounds: Option<Rff>,
    pub tiles: TileSet,
    pub palettes: Vec<Palette>,
    /// Raw 8-bit palookups (`64 * 256`), by palookup number.
    pub plu: Vec<Option<Vec<u8>>>,
    pub trans: Vec<u8>,
    pub surface: Vec<u8>,
    pub voxel_ids: Vec<i16>,
    pub ini: crate::ini::BloodIni,
    /// Voxel models from BLOOD.RFF, indexed by KVX id.
    pub voxels: Vec<Option<ob_engine::voxel::Voxel>>,
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    // Case-insensitive lookup for copies made on case-sensitive file systems.
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}

impl Resources {
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let rff_path = find_file(&dir, "BLOOD.RFF").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "BLOOD.RFF not found in {} - point OpenBlood at your original Blood directory",
                    dir.display()
                ),
            )
        })?;
        let blood = Rff::open(rff_path)?;
        let sounds = find_file(&dir, "SOUNDS.RFF").and_then(|p| Rff::open(p).ok());
        let tiles = TileSet::load_dir(&dir)?;
        let palettes = PAL_NAMES
            .iter()
            .map(|n| blood.read_name(&format!("{n}.PAL")).map(|b| Palette::from_vga(&b)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "palette missing from BLOOD.RFF"))?;
        let mut plu = vec![None; 256];
        for (i, n) in PLU_NAMES.iter().enumerate() {
            plu[i] = blood
                .read_name(&format!("{n}.PLU"))
                .filter(|b| b.len() >= NUM_SHADES * 256);
        }
        let trans = blood.read_name("TRANS.TLU").unwrap_or_default();
        let surface = find_file(&dir, "SURFACE.DAT")
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        let voxel_ids = find_file(&dir, "VOXEL.DAT")
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| {
                b.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect()
            })
            .unwrap_or_default();
        let ini = find_file(&dir, "BLOOD.INI")
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| crate::ini::BloodIni::parse(&String::from_utf8_lossy(&b)))
            .unwrap_or_default();
        let mut voxels: Vec<Option<ob_engine::voxel::Voxel>> = Vec::new();
        for e in blood.entries.iter().filter(|e| e.ext == "KVX") {
            let id = e.id as usize;
            if id >= 4096 {
                continue;
            }
            if voxels.len() <= id {
                voxels.resize(id + 1, None);
            }
            voxels[id] = blood.read(e).ok().and_then(|b| ob_engine::voxel::Voxel::parse(&b));
        }
        Ok(Resources {
            dir,
            blood,
            sounds,
            tiles,
            palettes,
            plu,
            trans,
            surface,
            voxel_ids,
            ini,
            voxels,
        })
    }

    /// Builds true-colour shade tables for the given base palette.
    pub fn build_luts(&self, base_pal: usize) -> ShadeLuts {
        let pal = &self.palettes[base_pal.min(self.palettes.len() - 1)];
        let tables = self
            .plu
            .iter()
            .map(|p| {
                p.as_ref().map(|lk| {
                    let mut out = vec![0u32; NUM_SHADES * 256];
                    for (o, &i) in out.iter_mut().zip(lk.iter()) {
                        *o = pal.colors[i as usize];
                    }
                    out
                })
            })
            .collect();
        ShadeLuts { tables }
    }

    /// Voxel model id for a tile, if the tile is drawn as a voxel.
    pub fn voxel_for_tile(&self, pic: i32) -> Option<u32> {
        let id = *self.voxel_ids.get(pic.max(0) as usize)?;
        (id >= 0 && self.voxels.get(id as usize).is_some_and(|v| v.is_some())).then_some(id as u32)
    }

    pub fn surface_of(&self, pic: i32) -> u8 {
        self.surface.get(pic.max(0) as usize).copied().unwrap_or(0)
    }
}
