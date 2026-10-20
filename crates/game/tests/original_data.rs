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

/// Reference: DOSBox recording of E4M9's start, player idle. Axe zombies land the first
/// hit ~2.5 s in, deal 18 per hit on skill 2 (≈10 on skill 0) and kill the player by ~5.3 s.
#[test]
fn axe_zombie_ambush_matches_original() {
    let Some(r) = res() else { return };
    for (skill, per_hit) in [(2, 18), (0, 10)] {
        let mut g = Game::load_level_skill(&r, "E4M9", skill).unwrap();
        let idle = ob_game::player::Input::default();
        let mut hits = Vec::new();
        let mut hp = g.player.health;
        for t in 0..60 * 12 {
            g.update(&r, &idle, 1.0 / 60.0);
            if g.player.health < hp {
                hits.push((t as f64 / 60.0, hp - g.player.health));
                hp = g.player.health;
            }
            if g.player.dead {
                break;
            }
        }
        eprintln!("skill {skill}: hits {hits:?}");
        let (first, dmg) = hits[0];
        assert!((2.0..3.0).contains(&first), "first hit at {first:.2}s");
        assert!((dmg - per_hit).abs() <= 1, "hit damage {dmg}, want {per_hit}");
        if skill == 2 {
            let death = hits.last().unwrap().0;
            assert!(g.player.dead && (4.3..6.5).contains(&death), "death at {death:.2}s");
        }
    }
}

/// Reference: same E4M9 start on skill 0 swinging the pitchfork non-stop. In the original
/// the zombies flinch and bleed but none dies before the player does (~7 s).
#[test]
fn pitchfork_does_not_outlast_axe_zombies() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level_skill(&r, "E4M9", 0).unwrap();
    let swing = ob_game::player::Input {
        fire: true,
        ..Default::default()
    };
    let zombies: Vec<usize> = (0..g.world.sprites.len())
        .filter(|&i| g.world.sprites[i].lotag == 203)
        .collect();
    for _ in 0..60 * 10 {
        g.update(&r, &swing, 1.0 / 60.0);
        if g.player.dead {
            break;
        }
    }
    let dead = zombies
        .iter()
        .filter(|&&i| matches!(g.ai_state(i), Some(ob_game::game::Ai::Dead | ob_game::game::Ai::Dying)))
        .count();
    eprintln!("player dead {} zombies killed {dead}/{}", g.player.dead, zombies.len());
    assert!(g.player.dead, "player should be overwhelmed");
    assert!(dead <= 1, "pitchfork killed {dead} zombies");
}

/// Reference: in the original a single point-blank shotgun barrel gibs an axe zombie.
#[test]
fn point_blank_shotgun_gibs_axe_zombie() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level_skill(&r, "E4M9", 2).unwrap();
    g.arsenal.give_weapon(3, 50);
    g.arsenal.select(3);
    let idle = ob_game::player::Input::default();
    let fire = ob_game::player::Input {
        fire: true,
        ..Default::default()
    };
    // Wait for a zombie to come close, then turn to face it.
    let close = |g: &Game| {
        (0..g.world.sprites.len()).find(|&i| {
            let s = &g.world.sprites[i];
            s.lotag == 203
                && g.ai_state(i)
                    .is_some_and(|a| !matches!(a, ob_game::game::Ai::Dead | ob_game::game::Ai::Dying))
                && (s.x as f64 - g.player.x).hypot(s.y as f64 - g.player.y) < 600.0
        })
    };
    let mut target = None;
    for _ in 0..60 * 5 {
        g.update(&r, &idle, 1.0 / 60.0);
        if let Some(i) = close(&g) {
            target = Some(i);
            break;
        }
    }
    let i = target.expect("a zombie should come close in front of the player");
    let s = &g.world.sprites[i];
    let a = (s.y as f64 - g.player.y).atan2(s.x as f64 - g.player.x);
    g.player.ang = (a / std::f64::consts::PI * 1024.0).rem_euclid(2048.0);
    g.update(&r, &fire, 1.0 / 60.0);
    for _ in 0..30 {
        g.update(&r, &idle, 1.0 / 60.0);
    }
    assert!(
        matches!(g.ai_state(i), Some(ob_game::game::Ai::Dead | ob_game::game::Ai::Dying)),
        "zombie {i} survived a point-blank blast: {:?}",
        g.ai_state(i)
    );
}

/// Reference: the original tommy gun fires ~14.6 rounds/s with the trigger held.
#[test]
fn tommy_gun_fire_rate_matches_original() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E1M1").unwrap();
    g.arsenal.give_weapon(4, 500);
    g.arsenal.select(4);
    let idle = ob_game::player::Input::default();
    for _ in 0..60 {
        g.update(&r, &idle, 1.0 / 60.0);
    }
    let fire = ob_game::player::Input {
        fire: true,
        ..Default::default()
    };
    let before = g.arsenal.ammo[ob_game::weapons::ammo::BULLETS];
    for _ in 0..60 * 4 {
        g.update(&r, &fire, 1.0 / 60.0);
    }
    let rate = (before - g.arsenal.ammo[ob_game::weapons::ammo::BULLETS]) as f64 / 4.0;
    eprintln!("tommy gun: {rate:.1} rounds/s");
    assert!((12.5..16.5).contains(&rate), "fire rate {rate:.1}/s");
}

/// Reference: E4M9 start on skill 2 holding the tommy gun trigger for ~5 s brings down
/// one axe zombie (not the whole group) while the player drops to ~10 health.
#[test]
fn tommy_gun_ambush_matches_original() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level_skill(&r, "E4M9", 2).unwrap();
    g.arsenal.give_weapon(4, 500);
    g.arsenal.select(4);
    let idle = ob_game::player::Input::default();
    let fire = ob_game::player::Input {
        fire: true,
        ..Default::default()
    };
    for _ in 0..60 {
        g.update(&r, &idle, 1.0 / 60.0);
    }
    for _ in 0..60 * 5 {
        g.update(&r, &fire, 1.0 / 60.0);
    }
    let killed = (0..g.world.sprites.len())
        .filter(|&i| g.world.sprites[i].lotag == 203)
        .filter(|&i| matches!(g.ai_state(i), Some(ob_game::game::Ai::Dead | ob_game::game::Ai::Dying)))
        .count();
    eprintln!("tommy ambush: killed {killed}, health {}", g.player.health);
    assert!((1..=2).contains(&killed), "killed {killed}");
}

/// Reference: E1M1 starts inside a coffin (sector 30). The first input fires level start,
/// the coffin floor rises (busy time 4 s) and the player jumps out over the rim into the crypt.
#[test]
fn e1m1_player_can_climb_out_of_coffin() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E1M1").unwrap();
    let idle = ob_game::player::Input::default();
    let nudge = ob_game::player::Input {
        turn: 1.0,
        ..Default::default()
    };
    g.update(&r, &nudge, 1.0 / 60.0);
    for _ in 0..60 * 5 {
        g.update(&r, &idle, 1.0 / 60.0);
    }
    assert_eq!(g.world.sectors[30].floor.z, -26624, "coffin floor should have risen");
    g.player.ang = 1260.0;
    let out = ob_game::player::Input {
        forward: 1.0,
        jump: true,
        ..Default::default()
    };
    for t in 0..90 {
        g.update(&r, &out, 1.0 / 60.0);
        if t % 10 == 0 {
            eprintln!(
                "t{t} ({:.0},{:.0},{:.0}) sect {}",
                g.player.x, g.player.y, g.player.z, g.player.sector
            );
        }
    }
    for _ in 0..60 {
        g.update(&r, &idle, 1.0 / 60.0);
    }
    let coffin = [28, 29, 30, 67, 68];
    assert!(
        !coffin.contains(&g.player.sector),
        "still in the coffin: sector {} at ({:.0},{:.0},{:.0})",
        g.player.sector,
        g.player.x,
        g.player.y,
        g.player.z
    );
}

/// Unalerted enemies wander around their spawn point instead of standing frozen, and
/// stay on their feet (sprite bottom on the floor) while doing it.
#[test]
fn idle_dudes_wander_near_home() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E2M4").unwrap();
    let idle = ob_game::player::Input::default();
    let far: Vec<usize> = (0..g.world.sprites.len())
        .filter(|&i| {
            let s = &g.world.sprites[i];
            matches!(s.lotag, 201 | 202) && (s.x as f64 - g.player.x).hypot(s.y as f64 - g.player.y) > 20000.0
        })
        .collect();
    let start: Vec<(i32, i32)> = far
        .iter()
        .map(|&i| (g.world.sprites[i].x, g.world.sprites[i].y))
        .collect();
    for _ in 0..60 * 6 {
        g.update(&r, &idle, 1.0 / 60.0);
    }
    let mut moved = 0;
    for (k, &i) in far.iter().enumerate() {
        let s = &g.world.sprites[i];
        let d = ((s.x - start[k].0) as f64).hypot((s.y - start[k].1) as f64);
        if d > 32.0 {
            moved += 1;
        }
        assert!(d < 1500.0, "dude {i} strayed {d:.0} units");
        if g.ai_state(i) == Some(ob_game::game::Ai::Idle) && moved > 0 && d > 32.0 {
            // A dude that walked is standing on its floor, not sunk into it.
            let (_, f) = g.world.zs_at(s.sectnum as usize, s.x as f64, s.y as f64);
            assert!(s.z as f64 <= f + 64.0, "dude {i} feet at {} below floor {f}", s.z);
        }
    }
    assert!(moved * 2 > far.len(), "only {moved} of {} idle dudes moved", far.len());
}

/// Frames between the 120 Hz ticks are blended, so at 144 fps (a frame sees 0, 1 or 2
/// ticks) the camera still advances evenly while walking.
#[test]
fn camera_moves_smoothly_between_ticks() {
    let Some(r) = res() else { return };
    let mut g = Game::load_level(&r, "E2M4").unwrap();
    let walk = ob_game::player::Input {
        forward: 1.0,
        ..Default::default()
    };
    let dt = 1.0 / 144.0;
    for _ in 0..144 {
        g.update(&r, &walk, dt);
    }
    let mut steps = Vec::new();
    let mut last = g.camera().x;
    for _ in 0..30 {
        g.update(&r, &walk, dt);
        let x = g.camera().x;
        steps.push(x - last);
        last = x;
    }
    let mean = steps.iter().sum::<f64>() / steps.len() as f64;
    assert!(mean > 10.0, "player should be walking: {mean}");
    for s in &steps {
        assert!(
            (s - mean).abs() < mean * 0.25,
            "uneven camera step {s:.1} vs mean {mean:.1}: {steps:?}"
        );
    }
}

/// Every Smacker cutscene copied from the user's disc decodes from start to end.
#[test]
fn all_cutscenes_decode() {
    let Some(dir) = std::env::var("OPENBLOOD_DATA").ok() else {
        return;
    };
    let Ok(rd) = std::fs::read_dir(std::path::Path::new(&dir).join("movie")) else {
        return;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_ascii_uppercase();
        if !name.ends_with(".SMK") {
            continue;
        }
        let mut v = ob_engine::smk::Smk::parse(std::fs::read(e.path()).unwrap()).unwrap();
        let mut frames = 0;
        while v.next_frame().unwrap_or_else(|err| panic!("{name}: {err}")) {
            frames += 1;
        }
        assert_eq!(frames, v.frames, "{name}");
        n += 1;
    }
    eprintln!("decoded {n} cutscenes");
}
