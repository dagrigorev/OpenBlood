//! Tests against the user's original Blood files (set OPENBLOOD_DATA; skipped otherwise).

use ob_game::anim::{AnimLib, Qav, Seq};
use ob_game::game::Game;
use ob_game::resources::Resources;

fn res() -> Option<Resources> {
    let d = std::env::var("OPENBLOOD_DATA").ok()?;
    Resources::load(d).ok()
}

#[test]
fn all_seq_and_qav_parse() {
    let Some(r) = res() else { return };
    let mut n = 0;
    for e in &r.blood.entries {
        let b = r.blood.read(e).unwrap();
        match e.ext.as_str() {
            "SEQ" => {
                let s = Seq::parse(&b).unwrap_or_else(|| panic!("{}", e.file_name()));
                assert!(
                    b.len() >= 16 + s.frames.len() * 8 && b.len() <= 24 + s.frames.len() * 8,
                    "{}",
                    e.file_name()
                );
                n += 1;
            }
            "QAV" => {
                let q = Qav::parse(&b).unwrap_or_else(|| panic!("{}", e.file_name()));
                assert_eq!(b.len(), Qav::HEADER + q.frames.len() * Qav::FRAME, "{}", e.file_name());
                n += 1;
            }
            _ => {}
        }
    }
    assert!(n > 500);
    let lib = AnimLib::new(&r.blood);
    assert_eq!(lib.find_seq(4096, "M1"), Some(4105));
    assert_eq!(lib.find_seq(4352, "M1"), Some(4360));
}

#[test]
fn every_level_loads_and_simulates() {
    let Some(r) = res() else { return };
    let maps: Vec<String> = r
        .blood
        .entries
        .iter()
        .filter(|e| e.ext == "MAP")
        .map(|e| e.name.clone())
        .collect();
    for m in maps {
        let mut g = Game::load_level(&r, &m).unwrap();
        assert!(g.player.sector >= 0, "{m}: player outside map");
        let input = ob_game::player::Input {
            forward: 1.0,
            use_key: true,
            ..Default::default()
        };
        for _ in 0..120 {
            g.update(&r, &input, 1.0 / 60.0);
        }
        assert!(g.player.sector >= 0, "{m}: player fell out of the map");
    }
}
