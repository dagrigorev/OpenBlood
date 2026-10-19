//! Integration tests against a user-supplied original Blood installation.
//! Set `OPENBLOOD_DATA` to the directory containing BLOOD.RFF; tests are skipped otherwise.

use ob_engine::{art::TileSet, map::Map, palette::Palette, rff::Rff};
use std::path::PathBuf;

fn data_dir() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var("OPENBLOOD_DATA").ok()?);
    d.join("BLOOD.RFF").exists().then_some(d)
}

#[test]
fn all_maps_parse() {
    let Some(dir) = data_dir() else { return };
    let rff = Rff::open(dir.join("BLOOD.RFF")).unwrap();
    let mut n = 0;
    for e in rff.entries.iter().filter(|e| e.ext == "MAP") {
        let data = rff.read(e).unwrap();
        let m = Map::parse(&data).unwrap_or_else(|err| panic!("{}: {err}", e.file_name()));
        assert!(
            m.start_sect >= 0 && (m.start_sect as usize) < m.sectors.len(),
            "{}",
            e.file_name()
        );
        n += 1;
    }
    assert!(n >= 30);
}

#[test]
fn tiles_and_palette_load() {
    let Some(dir) = data_dir() else { return };
    let tiles = TileSet::load_dir(&dir).unwrap();
    let nonempty = tiles.tiles.iter().filter(|t| !t.is_empty()).count();
    assert!(nonempty > 3000, "only {nonempty} tiles");
    let rff = Rff::open(dir.join("BLOOD.RFF")).unwrap();
    let pal = Palette::from_vga(&rff.read_name("BLOOD.PAL").unwrap());
    assert_ne!(pal.colors[255], pal.colors[0]);
    assert_eq!(rff.read_name("NORMAL.PLU").unwrap().len(), 64 * 256);
}
