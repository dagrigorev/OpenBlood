use ob_engine::{
    geom,
    map::Map,
    render::{Camera, Renderer, Scene, SpriteDraw},
    rff::Rff,
};
use ob_game::resources::Resources;

pub fn save_png(path: &str, w: usize, h: usize, frame: &[u32]) {
    let f = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().unwrap();
    let mut buf = Vec::with_capacity(w * h * 3);
    for &c in frame {
        buf.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
    }
    wr.write_image_data(&buf).unwrap();
}

/// openblood shot DATA_DIR MAP OUT.png [W H] [x y z ang horiz]
pub fn shot(args: &[String]) {
    let res = Resources::load(&args[2]).expect("load resources");
    let map = Map::parse(&res.blood.read_name(&format!("{}.MAP", args[3])).expect("map")).expect("parse map");
    let w: usize = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(1280);
    let h: usize = args.get(6).and_then(|v| v.parse().ok()).unwrap_or(720);
    let mut cam = Camera {
        x: map.start_x as f64,
        y: map.start_y as f64,
        z: 0.0,
        ang: map.start_ang as f64,
        horiz: 0.0,
        sector: map.start_sect as i32,
    };
    if let Some(st) = map.sprites.iter().find(|s| s.lotag == 1) {
        cam.x = st.x as f64;
        cam.y = st.y as f64;
        cam.ang = st.ang as f64;
        cam.sector = st.sectnum as i32;
    }
    if args.len() > 11 {
        cam.x = args[7].parse().unwrap();
        cam.y = args[8].parse().unwrap();
        cam.ang = args[10].parse().unwrap();
        cam.horiz = args[11].parse().unwrap();
        cam.sector = geom::update_sector(&map.sectors, &map.walls, cam.x, cam.y, cam.sector);
    }
    let (_, fz) = geom::zs_of_slope(&map.sectors, &map.walls, cam.sector as usize, cam.x, cam.y);
    cam.z = if args.len() > 11 {
        args[9].parse().unwrap()
    } else {
        fz - 0x2c00 as f64
    };
    let sprites: Vec<SpriteDraw> = map
        .sprites
        .iter()
        .map(|s| SpriteDraw {
            x: s.x as f64,
            y: s.y as f64,
            z: s.z as f64,
            picnum: s.picnum as i32,
            cstat: s.cstat,
            shade: s.shade as i32,
            pal: s.pal,
            xrepeat: s.xrepeat,
            yrepeat: s.yrepeat,
            xoffset: s.xoffset,
            yoffset: s.yoffset,
            ang: s.ang as f64,
            sector: s.sectnum as i32,
            voxel: {
                let v = res.tiles.get(s.picnum as i32).anm.view;
                if v == 6 || v == 7 {
                    res.voxel_for_tile(s.picnum as i32)
                } else {
                    None
                }
            },
            portal: 0,
        })
        .collect();
    let sprites: Vec<SpriteDraw> = if std::env::var("NOSPR").is_ok() {
        Vec::new()
    } else {
        sprites
    };
    let luts = res.build_luts(0);
    let scene = Scene {
        sectors: &map.sectors,
        walls: &map.walls,
        sprites: &sprites,
        tiles: &res.tiles,
        luts: &luts,
        sky_offsets: &map.sky_offsets,
        sky_bits: map.sky_bits as i32,
        visibility: map.visibility,
        clock: 0,
        voxels: &res.voxels,
        ror: &[],
    };
    let mut r = Renderer::new(w, h);
    if let Ok(v) = std::env::var("YSTRETCH") {
        r.y_stretch = v.parse().unwrap();
    }
    let t0 = std::time::Instant::now();
    r.render(&scene, &cam);
    let t1 = t0.elapsed();
    let t0 = std::time::Instant::now();
    for _ in 0..10 {
        r.render(&scene, &cam);
    }
    eprintln!(
        "first {:?}, avg {:?}/frame, {} sectors visible, cam {:?}",
        t1,
        t0.elapsed() / 10,
        r.stats_sectors,
        cam
    );
    save_png(&args[4], w, h, &r.frame);
}

pub fn run_tool(args: &[String]) -> bool {
    match args.get(1).map(String::as_str) {
        Some("rff-list") => {
            let rff = Rff::open(&args[2]).expect("open rff");
            println!("version {:#x}, {} entries", rff.version, rff.entries.len());
            for e in &rff.entries {
                println!(
                    "{:12} id={:5} size={:8} flags={:#04x}",
                    e.file_name(),
                    e.id,
                    e.size,
                    e.flags
                );
            }
        }
        Some("rff-extract") => {
            let rff = Rff::open(&args[2]).expect("open rff");
            let data = rff.read_name(&args[3]).expect("entry not found");
            std::fs::write(&args[4], data).expect("write");
        }
        Some("map-info") => {
            let rff = Rff::open(&args[2]).expect("open rff");
            for e in rff.entries.iter().filter(|e| e.ext == "MAP") {
                let m = Map::parse(&rff.read(e).unwrap()).expect("parse");
                println!(
                    "{:10} v{:#06x} sectors={:4} walls={:5} sprites={:4} x={} {} {} start=({}, {}, {}) ang={} sect={} song={} sky={:?}",
                    e.file_name(), m.version, m.sectors.len(), m.walls.len(), m.sprites.len(),
                    m.xsectors.len() - 1, m.xwalls.len() - 1, m.xsprites.len() - 1,
                    m.start_x, m.start_y, m.start_z, m.start_ang, m.start_sect, m.song_id, &m.sky_offsets[..4.min(m.sky_offsets.len())]
                );
            }
        }
        Some("shot") => shot(args),
        Some("chan") => {
            // List x-objects using a channel (rx or tx).
            let res = Resources::load(&args[2]).expect("load");
            let map = Map::parse(&res.blood.read_name(&format!("{}.MAP", args[3])).unwrap()).unwrap();
            let ch: u32 = args[4].parse().unwrap();
            for s in 0..map.sectors.len() {
                if let Some(x) = map.xsector(s) {
                    if x.rx_id == ch || x.tx_id == ch {
                        println!(
                            "sector {s} type={} rx={} tx={} cmd={} push={} enter={} on={} off={} state={}",
                            map.sectors[s].lotag,
                            x.rx_id,
                            x.tx_id,
                            x.command,
                            x.push,
                            x.enter,
                            x.trigger_on,
                            x.trigger_off,
                            x.state
                        );
                    }
                }
            }
            for w in 0..map.walls.len() {
                if let Some(x) = map.xwall(w) {
                    if x.rx_id == ch || x.tx_id == ch {
                        println!(
                            "wall {w} type={} rx={} tx={} cmd={} push={} state={} sector-of?",
                            map.walls[w].lotag, x.rx_id, x.tx_id, x.command, x.trigger_push, x.state
                        );
                    }
                }
            }
            for i in 0..map.sprites.len() {
                if let Some(x) = map.xsprite(i) {
                    if x.rx_id == ch || x.tx_id == ch {
                        let sp = &map.sprites[i];
                        println!("sprite {i} type={} sect={} pos=({},{},{}) rx={} tx={} cmd={} push={} prox={} pickup={} touch={} state={} busy={} wait={} data={:?}", sp.lotag, sp.sectnum, sp.x, sp.y, sp.z, x.rx_id, x.tx_id, x.command, x.push, x.proximity, x.pickup, x.touch, x.state, x.busy_time, x.wait_time, (x.data1,x.data2,x.data3,x.data4));
                    }
                }
            }
        }
        Some("shotg") => {
            // openblood shotg DATA MAP OUT.png W H SECONDS  - simulate, then render like the game.
            let res = Resources::load(&args[2]).expect("load");
            let mut g = ob_game::game::Game::load_level(&res, &args[3]).expect("level");
            let (w, h): (usize, usize) = (args[5].parse().unwrap(), args[6].parse().unwrap());
            let secs: f64 = args[7].parse().unwrap();
            let input = ob_game::player::Input::default();
            if std::env::var("OB_GIVE").is_ok() {
                use ob_game::inventory::InvItem;
                for it in [InvItem::DoctorBag, InvItem::DivingSuit, InvItem::JumpBoots] {
                    g.inventory.give(it);
                }
                g.powerups.akimbo = 25.0;
                g.powerups.invulnerable = 12.0;
                g.arsenal.give_weapon(4, 200);
            }
            let mut t = 0.0;
            if std::env::var("OB_ACT").is_ok() {
                // A tiny turn counts as the player's first action (starts the level).
                let nudge = ob_game::player::Input {
                    turn: 0.001,
                    ..Default::default()
                };
                g.update(&res, &nudge, 1.0 / 60.0);
            }
            while t < secs {
                g.update(&res, &input, 1.0 / 60.0);
                t += 1.0 / 60.0;
            }
            let luts = res.build_luts(0);
            let mut cam = g.camera();
            if let Ok(v) = std::env::var("OB_CAM") {
                let p: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                cam.x = p[0];
                cam.y = p[1];
                cam.ang = p[3];
                cam.sector = ob_engine::geom::update_sector(&g.world.sectors, &g.world.walls, cam.x, cam.y, -1);
                let (_, f) = g.world.zs_at(cam.sector.max(0) as usize, cam.x, cam.y);
                cam.z = if p[2] != 0.0 { p[2] } else { f - 0x2c00 as f64 };
            }
            if let Some(e) = std::env::var("OB_EYE").ok().and_then(|v| v.parse::<f64>().ok()) {
                cam.z = g.player.z - e;
            }
            // OB_LOOKAT=sprite[,distance[,angle offset]] puts the camera in front of a sprite.
            if let Ok(v) = std::env::var("OB_LOOKAT") {
                let p: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                let sp = &g.world.sprites[p[0] as usize];
                eprintln!(
                    "target sprite: stat={} cstat={:#x} pic={} sect={} z={} rep={}x{}",
                    sp.statnum, sp.cstat, sp.picnum, sp.sectnum, sp.z, sp.xrepeat, sp.yrepeat
                );
                let dist = p.get(1).copied().unwrap_or(600.0);
                // Pick the first of 8 directions with a clear line of sight to the sprite.
                let base = sp.ang as f64 + p.get(2).copied().unwrap_or(0.0);
                let (sx, sy, sz) = (sp.x as f64, sp.y as f64, sp.z as f64 - 0x1000 as f64);
                let off = (0..8)
                    .map(|k| base + k as f64 * 256.0)
                    .find(|&a| {
                        let r = ob_engine::geom::ang_to_rad(a);
                        let h = ob_engine::collide::hitscan(
                            &g.world.sectors,
                            &g.world.walls,
                            &[],
                            sx,
                            sy,
                            sz,
                            sp.sectnum as i32,
                            r.cos(),
                            r.sin(),
                            0.0,
                            dist + 64.0,
                        );
                        h.dist >= dist + 32.0
                    })
                    .unwrap_or(base);
                let a = ob_engine::geom::ang_to_rad(off);
                cam.x = sp.x as f64 + a.cos() * dist;
                cam.y = sp.y as f64 + a.sin() * dist;
                cam.ang = (off + 1024.0).rem_euclid(2048.0);
                cam.sector =
                    ob_engine::geom::update_sector(&g.world.sectors, &g.world.walls, cam.x, cam.y, sp.sectnum as i32);
                cam.z = sp.z as f64 - 0x2000 as f64;
                cam.horiz = 35.0;
            }
            let sprites = g.sprite_draws(&res, &cam);
            let scene = Scene {
                sectors: &g.world.sectors,
                walls: &g.world.walls,
                sprites: &sprites,
                tiles: &res.tiles,
                luts: &luts,
                sky_offsets: &g.world.sky_offsets,
                sky_bits: g.world.sky_bits,
                visibility: g.world.visibility,
                clock: g.clock,
                voxels: &res.voxels,
                ror: &g.world.ror_view,
            };
            let mut r = Renderer::new(w, h);
            if let Ok(v) = std::env::var("YSTRETCH") {
                r.y_stretch = v.parse().unwrap();
            }
            r.render(&scene, &cam);
            let t0 = std::time::Instant::now();
            for _ in 0..10 {
                r.render(&scene, &cam);
            }
            eprintln!("render {:?}/frame at {}x{}", t0.elapsed() / 10, w, h);
            if let Some(q) = args.get(8).and_then(|v| v.parse::<u32>().ok()) {
                let qt: i32 = args.get(9).and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(qav) = g.anims.qav(&res.blood, q) {
                    crate::hud::draw_qav(&mut r.frame, w, h, &res, &luts, &qav, qt, (0.0, 0.0), 0);
                }
            }
            if args.get(8).is_none() {
                let q = g.anims.qav(&res.blood, g.weapon_view().0);
                crate::hud::draw(
                    &mut r.frame,
                    w,
                    h,
                    &res,
                    &luts,
                    &g,
                    q.as_deref(),
                    None,
                    Some("Picked up Sawed-off"),
                );
            }
            save_png(&args[4], w, h, &r.frame);
            println!("cam {:?}", cam);
        }
        Some("extract-cd") => {
            // openblood extract-cd DISC.cue OUTDIR  - rips the Red Book music tracks of your own disc to WAV.
            match crate::cdaudio::extract(std::path::Path::new(&args[2]), std::path::Path::new(&args[3])) {
                Ok(n) => println!("extracted {n} audio tracks"),
                Err(e) => eprintln!("extract failed: {e}"),
            }
        }
        Some("sheet") => {
            // Contact sheet of tiles A..B (each cell 64x64 scaled, labelled order left-to-right).
            let res = Resources::load(&args[2]).expect("load");
            let luts = res.build_luts(0);
            let (a, b): (i32, i32) = (args[3].parse().unwrap(), args[4].parse().unwrap());
            let cols = 16usize;
            let n = (b - a + 1) as usize;
            let rows = n.div_ceil(cols);
            let (cw, ch) = (64usize, 64usize);
            let (w, h) = (cols * cw, rows * ch);
            let mut frame = vec![0x00303030u32; w * h];
            let lut = luts.get(0);
            for k in 0..n {
                let t = res.tiles.get(a + k as i32);
                if t.is_empty() {
                    continue;
                }
                let (ox, oy) = ((k % cols) * cw, (k / cols) * ch);
                let sc = (t.width.max(t.height) as f64 / 62.0).max(1.0);
                for y in 0..ch {
                    for x in 0..cw {
                        let (tx, ty) = ((x as f64 * sc) as usize, (y as f64 * sc) as usize);
                        if tx < t.width as usize && ty < t.height as usize {
                            let c = t.pixels[tx * t.height as usize + ty];
                            if c != 255 {
                                frame[(oy + y) * w + ox + x] = lut[c as usize];
                            }
                        }
                    }
                }
            }
            save_png(&args[5], w, h, &frame);
        }
        Some("survey") => {
            let res = Resources::load(&args[2]).expect("load");
            for e in res.blood.entries.iter().filter(|e| e.ext == "MAP") {
                let m = Map::parse(&res.blood.read(e).unwrap()).unwrap();
                let mut lt = [0usize; 15];
                for s in &m.sprites {
                    if matches!(s.lotag, 6 | 7 | 9..=14) {
                        lt[s.lotag as usize] += 1;
                    }
                }
                let links = format!(
                    "6:{} 7:{} 9:{} 10:{} 11:{} 12:{} 13:{} 14:{}",
                    lt[6], lt[7], lt[9], lt[10], lt[11], lt[12], lt[13], lt[14]
                );
                let paths = m.sectors.iter().filter(|s| s.lotag == 612).count();
                let pm = m.sprites.iter().filter(|s| s.lotag == 15).count();
                let vox = m
                    .sprites
                    .iter()
                    .filter(|s| {
                        let v = res.tiles.get(s.picnum as i32).anm.view;
                        v == 6 || v == 7
                    })
                    .count();
                let types: std::collections::BTreeSet<i16> =
                    m.sectors.iter().map(|s| s.lotag).filter(|&t| t >= 600).collect();
                println!(
                    "{:10} links={:3} path_sectors={:2} path_markers={:3} voxel_sprites={:3} sector_types={:?}",
                    e.file_name(),
                    links,
                    paths,
                    pm,
                    vox,
                    types
                );
            }
        }
        Some("vox-visible") => {
            // Voxel sprites in view from each map's single-player start.
            let res = Resources::load(&args[2]).expect("load");
            for e in res.blood.entries.iter().filter(|e| e.ext == "MAP") {
                let Ok(g) = ob_game::game::Game::load_level(&res, &e.name) else {
                    continue;
                };
                let p = &g.player;
                let (ex, ey, ez) = (p.x, p.y, p.z - 0x2c00 as f64);
                for (i, s) in g.world.sprites.iter().enumerate() {
                    let v = res.tiles.get(s.picnum as i32).anm.view;
                    if !(v == 6 || v == 7) || res.voxel_for_tile(s.picnum as i32).is_none() || s.statnum == 1024 {
                        continue;
                    }
                    let (dx, dy) = (s.x as f64 - ex, s.y as f64 - ey);
                    let d = dx.hypot(dy);
                    if !(400.0..3000.0).contains(&d) {
                        continue;
                    }
                    let ang = dy.atan2(dx).to_degrees() / 360.0 * 2048.0;
                    let rel = (ang - p.ang + 1024.0).rem_euclid(2048.0) - 1024.0;
                    if rel.abs() > 180.0 {
                        continue;
                    }
                    let tz = s.z as f64 - 0x400 as f64;
                    let h = ob_engine::collide::hitscan(
                        &g.world.sectors,
                        &g.world.walls,
                        &[],
                        ex,
                        ey,
                        ez,
                        p.sector,
                        dx / d,
                        dy / d,
                        (tz - ez) / d,
                        d,
                    );
                    if h.dist >= d - 16.0 {
                        println!(
                            "{} sprite {i} type={} pic={} rep={}x{} dist={:.0} rel_ang={:.0} view={}",
                            e.name, s.lotag, s.picnum, s.xrepeat, s.yrepeat, d, rel, v
                        );
                    }
                }
            }
        }
        Some("dude-visible") => {
            // Dudes in clear view from each map's single-player start.
            let res = Resources::load(&args[2]).expect("load");
            for e in res.blood.entries.iter().filter(|e| e.ext == "MAP") {
                let Ok(g) = ob_game::game::Game::load_level(&res, &e.name) else { continue };
                let p = &g.player;
                let (ex, ey, ez) = (p.x, p.y, p.z - 0x2c00 as f64);
                for (i, s) in g.world.sprites.iter().enumerate() {
                    if s.statnum == 1024 || !ob_game::dudes::is_dude(s.lotag) {
                        continue;
                    }
                    let (dx, dy) = (s.x as f64 - ex, s.y as f64 - ey);
                    let d = dx.hypot(dy);
                    if !(300.0..12000.0).contains(&d) {
                        continue;
                    }
                    let ang = dy.atan2(dx).to_degrees() / 360.0 * 2048.0;
                    let rel = (ang - p.ang + 1024.0).rem_euclid(2048.0) - 1024.0;
                    if rel.abs() > 340.0 {
                        continue;
                    }
                    let tz = s.z as f64 - 0x1800 as f64;
                    let h = ob_engine::collide::hitscan(&g.world.sectors, &g.world.walls, &[], ex, ey, ez, p.sector, dx / d, dy / d, (tz - ez) / d, d);
                    if h.dist >= d - 16.0 {
                        println!("{} sprite {i} type={} dist={:.0} rel_ang={:.0} ai={:?}", e.name, s.lotag, d, rel, g.ai_state(i));
                    }
                }
            }
        }
        Some("dude-near") => {
            // Dudes of the given types within RADIUS of each map's start (no sight check).
            // openblood dude-near DATA RADIUS TYPE...
            let res = Resources::load(&args[2]).expect("load");
            let radius: f64 = args[3].parse().expect("radius");
            let types: Vec<i16> = args[4..].iter().map(|t| t.parse().expect("type")).collect();
            for e in res.blood.entries.iter().filter(|e| e.ext == "MAP") {
                let Ok(g) = ob_game::game::Game::load_level(&res, &e.name) else { continue };
                let p = &g.player;
                for (i, s) in g.world.sprites.iter().enumerate() {
                    if s.statnum == 1024 || !types.contains(&s.lotag) {
                        continue;
                    }
                    let (dx, dy) = (s.x as f64 - p.x, s.y as f64 - p.y);
                    let d = dx.hypot(dy);
                    if d < radius {
                        let ang = dy.atan2(dx).to_degrees() / 360.0 * 2048.0;
                        let rel = (ang - p.ang + 1024.0).rem_euclid(2048.0) - 1024.0;
                        let x = g.world.xsprite(i);
                        println!(
                            "{} sprite {i} type={} dist={d:.0} rel_ang={rel:.0} dz={} ambush={} deaf={}",
                            e.name,
                            s.lotag,
                            s.z - p.z as i32,
                            x.is_some_and(|x| x.dude_ambush),
                            x.is_some_and(|x| x.dude_deaf)
                        );
                    }
                }
            }
        }
        Some("noise-test") => {
            // Fires the tommy gun for 1 s at each map's start, then idles 12 s and reports
            // when the player gets hurt. openblood noise-test DATA MAP...
            let res = Resources::load(&args[2]).expect("load");
            for name in &args[3..] {
                let mut g = ob_game::game::Game::load_level(&res, name).expect("map");
                g.arsenal.give_weapon(4, 500);
                g.arsenal.select(4);
                let fire = ob_game::player::Input { fire: true, ..Default::default() };
                let idle = ob_game::player::Input::default();
                let mut hp = g.player.health;
                let mut hits = Vec::new();
                for t in 0..60 * 13 {
                    g.update(&res, if (30..90).contains(&t) { &fire } else { &idle }, 1.0 / 60.0);
                    if g.player.health < hp {
                        hits.push(format!("{:.1}s:{}", t as f64 / 60.0, hp - g.player.health));
                        hp = g.player.health;
                    }
                }
                println!("{name}: {}", hits.join(" "));
            }
        }
        Some("topdown") => {
            // openblood topdown DATA MAP OUT.png CX CY HALF : walls around (CX,CY) within HALF
            // units (red = solid, grey = open), player start (green, with facing), dudes
            // (yellow = cultists, orange = others). Grid lines every 1024 units.
            let res = Resources::load(&args[2]).expect("load");
            let g = ob_game::game::Game::load_level(&res, &args[3]).expect("map");
            let (cx, cy, half): (f64, f64, f64) = (args[5].parse().unwrap(), args[6].parse().unwrap(), args[7].parse().unwrap());
            let n = 1024usize;
            let mut img = vec![0x101010u32; n * n];
            let to = |x: f64, y: f64| (((x - cx) / half * 0.5 + 0.5) * n as f64, ((y - cy) / half * 0.5 + 0.5) * n as f64);
            let mut put = |img: &mut Vec<u32>, x: f64, y: f64, c: u32| {
                if x >= 0.0 && y >= 0.0 && (x as usize) < n && (y as usize) < n {
                    img[y as usize * n + x as usize] = c;
                }
            };
            let line = |img: &mut Vec<u32>, put: &mut dyn FnMut(&mut Vec<u32>, f64, f64, u32), a: (f64, f64), b: (f64, f64), c: u32| {
                let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) as usize).max(1);
                for k in 0..=steps {
                    let t = k as f64 / steps as f64;
                    put(img, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, c);
                }
            };
            let grid = ((cx - half) / 1024.0).floor() as i64;
            for k in 0..((2.0 * half / 1024.0) as i64 + 2) {
                let v = (grid + k) as f64 * 1024.0;
                line(&mut img, &mut put, to(v, cy - half), to(v, cy + half), 0x202838);
                let h = (((cy - half) / 1024.0).floor() as i64 + k) as f64 * 1024.0;
                line(&mut img, &mut put, to(cx - half, h), to(cx + half, h), 0x202838);
            }
            for w in g.world.walls.iter() {
                let p2 = &g.world.walls[w.point2 as usize];
                let c = if w.nextsector >= 0 { 0x505050 } else { 0xd04040 };
                line(&mut img, &mut put, to(w.x as f64, w.y as f64), to(p2.x as f64, p2.y as f64), c);
            }
            for (i, s) in g.world.sprites.iter().enumerate() {
                if s.statnum == 1024 || !ob_game::dudes::is_dude(s.lotag) {
                    continue;
                }
                let c = if matches!(s.lotag, 201 | 202) { 0xffff00 } else { 0xff8000 };
                let (x, y) = to(s.x as f64, s.y as f64);
                for dy in -3..=3 {
                    for dx in -3..=3 {
                        put(&mut img, x + dx as f64, y + dy as f64, c);
                    }
                }
                println!("dude {i} type {} at ({}, {}) z {}", s.lotag, s.x, s.y, s.z);
            }
            let p = &g.player;
            let a = p.ang / 1024.0 * std::f64::consts::PI;
            let (x, y) = to(p.x, p.y);
            line(&mut img, &mut put, (x, y), (x + a.cos() * 20.0, y + a.sin() * 20.0), 0x40ff40);
            for dy in -4..=4 {
                for dx in -4..=4 {
                    put(&mut img, x + dx as f64, y + dy as f64, 0x40ff40);
                }
            }
            println!("player at ({:.0}, {:.0}) ang {:.0}", p.x, p.y, p.ang);
            save_png(&args[4], n, n, &img);
        }
        Some("frozen-slides") => {
            // Slide sectors whose walls are kept still because moving them folds geometry.
            let res = Resources::load(&args[2]).expect("load");
            for e in res.blood.entries.iter().filter(|e| e.ext == "MAP") {
                let Ok(g) = ob_game::game::Game::load_level(&res, &e.name) else { continue };
                let total = g.world.sectors.iter().filter(|s| matches!(s.lotag, 614 | 616)).count();
                let f = g.triggers.frozen_slides();
                if !f.is_empty() {
                    println!("{}: frozen {:?} of {total} slide sectors", e.name, f);
                }
            }
        }
        Some("menu-shot") => {
            // openblood menu-shot DATA OUT.png : options menu rendered at 640x400.
            let res = Resources::load(&args[2]).expect("load");
            let luts = res.build_luts(0);
            let mut m = crate::menu::Menu::new();
            m.screen = crate::menu::Screen::Options;
            let o = crate::menu::OptionsView {
                fullscreen: false,
                scale: 0.5,
                mouse: 0.35,
                always_run: true,
                music: true,
                slots: vec![None; 5],
            };
            let (w, h) = (640, 400);
            let mut frame = vec![0u32; w * h];
            m.draw(&mut frame, w, h, &res, &luts, 0.0, &o);
            save_png(&args[3], w, h, &frame);
        }
        Some("vox-test") => {
            // openblood vox-test DATA PIC OUT.png ANG : voxel (left) next to the flat tile (right).
            let res = Resources::load(&args[2]).expect("load");
            let pic: i32 = args[3].parse().unwrap();
            let ang: f64 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let pts = [(-4096, -4096), (4096, -4096), (4096, 4096), (-4096, 4096)];
            let walls: Vec<ob_engine::map::Wall> = pts
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| ob_engine::map::Wall {
                    x,
                    y,
                    point2: ((i + 1) % 4) as i16,
                    nextwall: -1,
                    nextsector: -1,
                    picnum: 253,
                    xrepeat: 8,
                    yrepeat: 8,
                    shade: 10,
                    ..Default::default()
                })
                .collect();
            let mut sec = ob_engine::map::Sector {
                wallptr: 0,
                wallnum: 4,
                ..Default::default()
            };
            sec.ceiling.z = -40000;
            sec.floor.z = 0;
            sec.floor.picnum = 270;
            sec.ceiling.picnum = 253;
            let sectors = vec![sec];
            let base = SpriteDraw {
                y: 0.0,
                z: 0.0,
                picnum: pic,
                xrepeat: 48,
                yrepeat: 48,
                ang,
                sector: 0,
                ..Default::default()
            };
            let sprites = vec![
                SpriteDraw {
                    x: 0.0,
                    y: -200.0,
                    voxel: res.voxel_for_tile(pic),
                    ..base
                },
                SpriteDraw {
                    x: 0.0,
                    y: 200.0,
                    ..base
                },
            ];
            let luts = res.build_luts(0);
            let scene = Scene {
                sectors: &sectors,
                walls: &walls,
                sprites: &sprites,
                tiles: &res.tiles,
                luts: &luts,
                sky_offsets: &[0],
                sky_bits: 0,
                visibility: 0,
                clock: 0,
                voxels: &res.voxels,
                ror: &[],
            };
            let cam = Camera {
                x: -900.0,
                y: 0.0,
                z: -3000.0,
                ang: 0.0,
                horiz: -25.0,
                sector: 0,
            };
            let mut r = Renderer::new(640, 400);
            r.render(&scene, &cam);
            save_png(&args[4], 640, 400, &r.frame);
            println!(
                "voxel {:?} tile {}x{}",
                res.voxel_for_tile(pic)
                    .and_then(|i| res.voxels[i as usize].as_ref().map(|v| (v.xs, v.ys, v.zs, v.pivot))),
                res.tiles.get(pic).width,
                res.tiles.get(pic).height
            );
        }
        Some("find-vox") => {
            let res = Resources::load(&args[2]).expect("load");
            let map = Map::parse(&res.blood.read_name(&format!("{}.MAP", args[3])).unwrap()).unwrap();
            for (i, s) in map.sprites.iter().enumerate() {
                let v = res.tiles.get(s.picnum as i32).anm.view;
                if (v == 6 || v == 7) && res.voxel_for_tile(s.picnum as i32).is_some() {
                    println!(
                        "{i}: type={} pic={} vox={:?} view={} pos=({},{},{}) rep={}x{} cstat={:#x} ang={}",
                        s.lotag,
                        s.picnum,
                        res.voxel_for_tile(s.picnum as i32),
                        v,
                        s.x,
                        s.y,
                        s.z,
                        s.xrepeat,
                        s.yrepeat,
                        s.cstat,
                        s.ang
                    );
                }
            }
        }
        Some("tiles-scan") => {
            let res = Resources::load(&args[2]).expect("load");
            let (minw, minh): (u16, u16) = (args[3].parse().unwrap(), args[4].parse().unwrap());
            for (i, t) in res.tiles.tiles.iter().enumerate() {
                if t.width >= minw && t.height >= minh && !t.is_empty() {
                    println!("{i}: {}x{}", t.width, t.height);
                }
            }
        }
        Some("tile") => {
            let res = Resources::load(&args[2]).expect("load");
            let luts = res.build_luts(0);
            let t = res.tiles.get(args[3].parse().unwrap());
            let (w, h) = (t.width as usize, t.height as usize);
            let lut = luts.get(0);
            let frame: Vec<u32> = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    lut[t.pixels[x * h + y] as usize]
                })
                .collect();
            save_png(&args[4], w, h, &frame);
            println!("{}x{} anm {:?}", w, h, t.anm);
        }
        Some("qav") => {
            let res = Resources::load(&args[2]).expect("load");
            let q = ob_game::anim::Qav::parse(&res.blood.read_name(&format!("{}.QAV", args[3])).unwrap()).unwrap();
            println!(
                "ticks={} dur={} origin=({},{}) frames={}",
                q.ticks_per_frame,
                q.duration,
                q.x,
                q.y,
                q.frames.len()
            );
            for (i, f) in q.frames.iter().enumerate() {
                let layers: Vec<String> = f
                    .layers
                    .iter()
                    .filter(|l| l.picnum > 0)
                    .map(|l| {
                        let t = res.tiles.get(l.picnum);
                        format!(
                            "pic{}({}x{} off {},{}) @({},{}) z={:#x} stat={:#x} sh={} pal={}",
                            l.picnum,
                            t.width,
                            t.height,
                            t.anm.xoffset,
                            t.anm.yoffset,
                            l.x,
                            l.y,
                            l.z,
                            l.stat,
                            l.shade,
                            l.pal
                        )
                    })
                    .collect();
                println!("{i}: cb={} snd={} | {}", f.callback, f.sound.id, layers.join(" | "));
            }
        }
        Some("sprites") => {
            let res = Resources::load(&args[2]).expect("load");
            let map = Map::parse(&res.blood.read_name(&format!("{}.MAP", args[3])).unwrap()).unwrap();
            let (x, y, r): (i32, i32, i32) = (
                args[4].parse().unwrap(),
                args[5].parse().unwrap(),
                args[6].parse().unwrap(),
            );
            for (i, sp) in map.sprites.iter().enumerate() {
                if (sp.x - x).abs() < r && (sp.y - y).abs() < r {
                    let t = res.tiles.get(sp.picnum as i32);
                    println!("{i}: sect={} [c={} f={}] type={} stat={} pos=({},{},{}) pic={} {}x{} view={} frames={} cstat={:#x} rep={}x{} pal={} shade={} x={:?}",
                        sp.sectnum, map.sectors.get(sp.sectnum.max(0) as usize).map_or(0, |q| q.ceiling.z), map.sectors.get(sp.sectnum.max(0) as usize).map_or(0, |q| q.floor.z),
                        sp.lotag, sp.statnum, sp.x, sp.y, sp.z, sp.picnum, t.width, t.height, t.anm.view, t.anm.frames, sp.cstat, sp.xrepeat, sp.yrepeat, sp.pal, sp.shade,
                        map.xsprite(i).map(|x| (x.rx_id, x.tx_id, x.data1, x.data2, x.data3, x.state)));
                }
            }
        }
        Some("walls") => {
            let res = Resources::load(&args[2]).expect("load");
            let map = Map::parse(&res.blood.read_name(&format!("{}.MAP", args[3])).unwrap()).unwrap();
            let s: usize = args[4].parse().unwrap();
            let sec = &map.sectors[s];
            println!("sector {s}: {:?}", sec);
            if let Some(x) = map.xsector(s) {
                println!("xsector: {:?}", x);
            }
            for i in sec.wallptr as usize..(sec.wallptr + sec.wallnum) as usize {
                let w = &map.walls[i];
                println!(
                    "wall {i}: ({},{}) next={} ns={} cstat={:#06x} pic={} over={} rep={}x{} pan={},{} shade={} pal={} lotag={} x={:?}",
                    w.x, w.y, w.nextwall, w.nextsector, w.cstat, w.picnum, w.overpicnum, w.xrepeat, w.yrepeat, w.xpanning, w.ypanning, w.shade, w.pal, w.lotag, map.xwall(i).map(|x| (x.rx_id, x.tx_id, x.state))
                );
            }
            for (i, sp) in map
                .sprites
                .iter()
                .enumerate()
                .filter(|(_, sp)| sp.lotag == 1 || sp.sectnum as usize == s)
            {
                println!(
                    "sprite {i}: type={} pos=({},{},{}) sect={} pic={} cstat={:#x} rep={}x{} x={:?}",
                    sp.lotag,
                    sp.x,
                    sp.y,
                    sp.z,
                    sp.sectnum,
                    sp.picnum,
                    sp.cstat,
                    sp.xrepeat,
                    sp.yrepeat,
                    map.xsprite(i).map(|x| (x.data1, x.data2))
                );
            }
            println!("visibility {}", map.visibility);
            let masked = map.walls.iter().filter(|w| w.cstat & 16 != 0).count();
            println!("masked walls total {masked}");
        }
        _ => return false,
    }
    true
}

pub fn screenshot_path() -> String {
    for i in 0..10000 {
        let p = format!("openblood_{i:04}.png");
        if !std::path::Path::new(&p).exists() {
            return p;
        }
    }
    "openblood.png".into()
}
