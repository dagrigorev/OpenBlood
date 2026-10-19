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

#[test]
fn player_dives_through_water_link() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E2M1").unwrap();
    // Find an upper-water marker (type 9) and stand on its sector.
    let (i, m) = g
        .world
        .sprites
        .iter()
        .enumerate()
        .find(|(_, s)| s.lotag == 9)
        .map(|(i, s)| (i, s.clone()))
        .unwrap();
    let link = g.world.links[m.sectnum as usize].floor.expect("water floor link");
    let _ = i;
    g.player.x = m.x as f64;
    g.player.y = m.y as f64;
    g.player.sector = m.sectnum as i32;
    let (_, f) = g.world.zs_at(m.sectnum as usize, m.x as f64, m.y as f64);
    g.player.z = f - 1.0;
    g.player.vz = 0.0;
    let input = ob_game::player::Input::default();
    for _ in 0..240 {
        g.update(&r, &input, 1.0 / 60.0);
        if g.player.sector == link.sector || g.world.xsector(g.player.sector as usize).is_some_and(|x| x.underwater) {
            break;
        }
    }
    let s = g.player.sector as usize;
    assert!(
        g.world.xsector(s).is_some_and(|x| x.underwater) || g.world.links[s].ceil.is_some(),
        "player should end up in the linked underwater sector, got {s}"
    );
}

#[test]
fn gunfire_scares_innocents_and_wakes_cultists() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E3M1").unwrap();
    let fire = ob_game::player::Input {
        fire: true,
        ..Default::default()
    };
    for _ in 0..120 {
        g.update(&r, &fire, 1.0 / 60.0);
    }
    let innocents: Vec<usize> = (0..g.world.sprites.len())
        .filter(|&i| g.world.sprites[i].lotag == 245)
        .collect();
    assert!(!innocents.is_empty());
    assert!(
        innocents
            .iter()
            .any(|&i| g.ai_state(i) == Some(ob_game::game::Ai::Flee)),
        "some innocent should flee: {:?}",
        innocents.iter().map(|&i| g.ai_state(i)).collect::<Vec<_>>()
    );
}

#[test]
fn shotgun_alt_fire_uses_two_shells() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E3M1").unwrap();
    let idle = ob_game::player::Input::default();
    for _ in 0..120 {
        g.update(&r, &idle, 1.0 / 60.0);
    }
    assert_eq!(g.arsenal.current, 3, "sawed-off picked up at the start");
    let before = g.arsenal.ammo[ob_game::weapons::ammo::SHELLS];
    let alt = ob_game::player::Input {
        alt_fire: true,
        ..Default::default()
    };
    for _ in 0..10 {
        g.update(&r, &alt, 1.0 / 60.0);
    }
    assert_eq!(g.arsenal.ammo[ob_game::weapons::ammo::SHELLS], before - 2);
}

#[test]
fn save_and_load_round_trip_is_deterministic() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E1M2").unwrap();
    let walk = ob_game::player::Input {
        forward: 1.0,
        turn: 0.0,
        fire: true,
        ..Default::default()
    };
    let dt = 1.0 / 120.0;
    for _ in 0..240 {
        g.update(&r, &walk, dt);
    }
    let bytes = g.save_bytes("test").unwrap();
    for _ in 0..240 {
        g.update(&r, &walk, dt);
    }
    let mut h = Game::from_save(&r, &bytes).unwrap();
    for _ in 0..240 {
        h.update(&r, &walk, dt);
    }
    assert_eq!(
        (g.player.x, g.player.y, g.player.z),
        (h.player.x, h.player.y, h.player.z)
    );
    assert_eq!(g.clock, h.clock);
    assert_eq!(g.arsenal.ammo, h.arsenal.ammo);
    let pos = |g: &Game| {
        g.world
            .sprites
            .iter()
            .map(|s| (s.x, s.y, s.z, s.picnum))
            .collect::<Vec<_>>()
    };
    assert_eq!(pos(&g), pos(&h), "sprites diverged after load");
    assert!(bytes.len() > 10_000);
}

/// Distances measured frame by frame from a DOSBox video of the original walking forward
/// from the E1M1 start: ~329 units after 0.114 s and ~1027 units after 0.271 s.
#[test]
fn walking_acceleration_matches_original() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E1M1").unwrap();
    let x0 = g.player.x;
    let fwd = ob_game::player::Input {
        forward: 1.0,
        ..Default::default()
    };
    let dt = 1.0 / 120.0;
    let mut samples = Vec::new();
    for tick in 1..=33 {
        g.update(&r, &fwd, dt);
        if tick == 14 || tick == 33 {
            samples.push(g.player.x - x0);
        }
    }
    let ok = |got: f64, want: f64| (got - want).abs() / want < 0.2;
    assert!(ok(samples[0], 329.0) && ok(samples[1], 1027.0), "travel {samples:?}");
}
