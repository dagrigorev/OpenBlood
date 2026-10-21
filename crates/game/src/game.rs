//! Top-level game state for one level: world, player, weapons, actors and triggers.

use crate::anim::{AnimLib, SEQ_REMOVE};
use crate::dudes::{dude_info, is_dude, Attack, DudeInfo, ProjKind};
use crate::inventory::{InvItem, Inventory, Powerups};
use crate::items::{pickup_of, Pickup, Timed};
use crate::player::{Input, Player};
use crate::resources::Resources;
use crate::triggers::{cmd, Notice, Obj, Triggers};
use crate::weapons::{Arsenal, Fire, WeaponState};
use crate::world::{stat, World};
use ob_engine::collide::{self, CircleObstacle, HitCylinder, Mover, TraceTarget};
use ob_engine::geom::{ang_to_rad, ray_segment};
use ob_engine::map::{Map, Sprite};
use ob_engine::render::{Camera, SpriteDraw, SPR_BLOCK, SPR_CENTER, SPR_INVISIBLE, SPR_XFLIP, SPR_YFLIP};
use std::collections::HashMap;
use std::io;

mod ai;
pub mod save;

/// Game clock rate.
pub const TICK_RATE: f64 = 120.0;
/// Reach of the "use" action in xy units.
pub const USE_RANGE: f64 = 768.0;
/// Pseudo sprite id used for the player in traces.
pub const PLAYER_ID: usize = usize::MAX;
/// Breath before drowning starts.
pub const AIR_SECONDS: f64 = 20.0;

/// Projectile sprite types (OpenBlood-internal, outside Blood's editor ranges).
const PROJ_BASE: i16 = 3000;

/// Horizontal distance (xy units) within which the player takes an item.
const PICKUP_REACH: f64 = 768.0;

/// Thing types with special handling.
const TNT_BARREL: i16 = 400;
const WALL_CRACK: i16 = 408;
const OBJECT_EXPLODE: i16 = 417;
const EXPLODER: i16 = 459;

/// Events surfaced to the front end (sounds, messages, level changes).
#[derive(Debug, Clone, PartialEq)]
pub enum GameEvent {
    Message(String),
    LevelExit {
        secret: bool,
    },
    UseFailed,
    SectorSound {
        sector: usize,
        opening: bool,
        start: bool,
    },
    Switch,
    /// Play sound `id`; `at` is a world position or `None` for the player's own sounds.
    Sound {
        id: i32,
        at: Option<(f64, f64, f64)>,
    },
    PlayerHurt(i32),
    PlayerDied,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ai {
    /// Inert until provoked (statues) or approached (buried zombies).
    Dormant,
    /// Playing the rise-from-the-ground animation.
    Rise,
    /// Running away (innocents).
    Flee,
    Idle,
    Chase,
    Attack,
    Recoil,
    Dying,
    Dead,
}

pub struct Game {
    pub world: World,
    pub player: Player,
    pub triggers: Triggers,
    pub arsenal: Arsenal,
    pub anims: AnimLib,
    pub clock: i32,
    pub events: Vec<GameEvent>,
    pub messages: Vec<String>,
    pub kills: i32,
    pub total_kills: i32,
    pub damage_flash: f64,
    pub pickup_flash: f64,
    pub skill: i32,
    last_sector: i32,
    acc: f64,
    use_down: bool,
    rng: u32,
    ai: Vec<Ai>,
    attack_cd: Vec<f64>,
    pic_for_type: HashMap<i16, i16>,
    weapon_switch_down: bool,
    level_messages: HashMap<u32, String>,
    pub inventory: Inventory,
    pub powerups: Powerups,
    /// Seconds of breath left underwater.
    pub air: f64,
    inv_keys: (bool, bool, bool),
    /// Dude was hurt (ambushers/guards start moving).
    provoked: Vec<bool>,
    /// Cached line-of-sight to the player.
    sees: Vec<bool>,
    /// Damage currently being applied comes from burning.
    burn_kill: bool,
    /// Sprite positions and player eye before the last tick, for drawing frames between
    /// ticks (smooth motion at any frame rate).
    prev_xyz: Vec<(i32, i32, i32)>,
    prev_eye: (f64, f64, f64),
    /// The level-start channel has been broadcast.
    pub started: bool,
}

/// Sprite types that are editor-only markers.
pub fn is_marker_type(t: i16) -> bool {
    (1..=19).contains(&t)
}

/// Vertical extents of a sprite as drawn: (top z, bottom z).
pub fn sprite_extents(res: &Resources, s: &Sprite) -> (f64, f64) {
    let t = res.tiles.get(s.picnum as i32);
    let h = t.height as f64 * s.yrepeat as f64 * 4.0;
    let mut bot = s.z as f64;
    if s.cstat & SPR_CENTER != 0 {
        bot += h * 0.5;
    }
    bot -= (t.anm.yoffset as f64 + s.yoffset as f64) * s.yrepeat as f64 * 4.0;
    (bot - h, bot)
}

impl Game {
    pub fn load_level(res: &Resources, name: &str) -> io::Result<Game> {
        Self::load_level_skill(res, name, 2)
    }

    pub fn load_level_skill(res: &Resources, name: &str, skill: i32) -> io::Result<Game> {
        let bytes = res
            .blood
            .read_name(&format!("{}.MAP", name.to_ascii_uppercase()))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("map {name} not found")))?;
        let map = Map::parse(&bytes)?;
        let mut world = World::from_map(&name.to_ascii_uppercase(), map);
        let mut pic_for_type = HashMap::new();
        for i in 0..world.sprites.len() {
            let s = world.sprites[i].clone();
            pic_for_type.entry(s.lotag).or_insert(s.picnum);
            if is_marker_type(s.lotag) || matches!(s.statnum, stat::MARKER | stat::PATH_MARKER | stat::AMBIENCE) {
                world.sprites[i].cstat |= SPR_INVISIBLE;
            }
            // Difficulty / game-mode filtering: launch_skill bit set = absent on that skill.
            if let Some(x) = world.xsprite(i) {
                let skill_bit = 1u8 << skill.clamp(0, 4);
                if x.launch_skill & skill_bit != 0 || x.launch_single {
                    world.delete_sprite(i);
                    continue;
                }
            }
            // Exploder markers (type 459) are never drawn; they explode when switched on.
            if s.lotag == EXPLODER {
                world.sprites[i].cstat |= SPR_INVISIBLE;
            }
        }
        let player = Player::spawn(&world);
        let mut triggers = Triggers::new(&world);
        triggers.init_level(&mut world);
        let last_sector = player.sector;
        let n = world.sprites.len();
        let mut g = Game {
            world,
            player,
            triggers,
            arsenal: Arsenal::default(),
            anims: AnimLib::new(&res.blood),
            clock: 0,
            events: Vec::new(),
            messages: Vec::new(),
            kills: 0,
            total_kills: 0,
            damage_flash: 0.0,
            pickup_flash: 0.0,
            skill,
            last_sector,
            acc: 0.0,
            use_down: false,
            rng: 0x1234_5678,
            ai: vec![Ai::Idle; n],
            attack_cd: vec![0.0; n],
            pic_for_type,
            weapon_switch_down: false,
            level_messages: res.ini.map(name).map(|m| m.messages.clone()).unwrap_or_default(),
            inventory: Inventory::default(),
            powerups: Powerups::default(),
            air: AIR_SECONDS,
            inv_keys: (false, false, false),
            provoked: vec![false; n],
            sees: vec![false; n],
            burn_kill: false,
            prev_xyz: Vec::new(),
            prev_eye: (0.0, 0.0, 0.0),
            started: false,
        };
        // The level-start broadcast is sent on the player's first input (see `tick`).
        // Dudes start in their idle animation.
        for i in 0..n {
            let s = &g.world.sprites[i];
            if s.statnum != stat::FREE && is_dude(s.lotag) {
                if let Some(info) = dude_info(s.lotag) {
                    g.world.rt[i].health = if g.world.rt[i].health > 0 {
                        g.world.rt[i].health
                    } else {
                        info.health
                    };
                    g.total_kills += if s.lotag == 245 { 0 } else { 1 };
                    // Maps store the sprite as drawn; the game keeps dude z at the feet.
                    if s.cstat & 48 == 0 {
                        let (top, bot) = sprite_extents(res, s);
                        g.world.sprites[i].z = bot.round() as i32;
                        g.world.rt[i].eye = ((bot - top) * 0.85) as i32;
                    }
                    g.world.rt[i].home_ang = g.world.sprites[i].ang;
                    g.play_dude_anim(res, i, "I1");
                    g.ai[i] = g.initial_ai(i);
                }
            }
        }
        g.fix_sprite_sectors();
        Ok(g)
    }

    /// Some maps store pickups, dudes and blocking sprites with the sector number of a
    /// neighbour, or (in stacked rooms) of a sector at another height. Such a sprite would
    /// collide, light and draw with the wrong floor, or never be drawn. Reassign it to the
    /// sector that contains its position and height (or at least its position).
    fn fix_sprite_sectors(&mut self) {
        const SLACK: f64 = 256.0;
        let w = &self.world;
        let mut moves = Vec::new();
        for (i, s) in w.sprites.iter().enumerate() {
            if s.statnum == stat::FREE || s.sectnum < 0 || s.cstat & 48 != 0 {
                continue;
            }
            if !(pickup_of(s.lotag).is_some() || is_dude(s.lotag) || s.cstat & SPR_BLOCK != 0) {
                continue;
            }
            let (x, y, z) = (s.x as f64, s.y as f64, s.z as f64);
            let fits = |sec: usize| {
                let (c, f) = w.zs_at(sec, x, y);
                z >= c - SLACK && z <= f + SLACK
            };
            let cur = s.sectnum as usize;
            if ob_engine::geom::inside_sector(&w.sectors, &w.walls, cur, x, y) && fits(cur) {
                continue;
            }
            let containing: Vec<usize> = (0..w.sectors.len())
                .filter(|&k| ob_engine::geom::inside_sector(&w.sectors, &w.walls, k, x, y))
                .collect();
            let best = containing.iter().copied().find(|&k| fits(k)).or_else(|| {
                if ob_engine::geom::inside_sector(&w.sectors, &w.walls, cur, x, y) {
                    None
                } else {
                    containing.first().copied()
                }
            });
            if let Some(k) = best.filter(|&k| k != cur) {
                moves.push((i, k));
            }
        }
        for (i, k) in moves {
            self.world.sprites[i].sectnum = k as i16;
        }
    }

    fn rand(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.rng >> 8) & 0xffff
    }

    /// Uniform float in [-1, 1].
    fn randf(&mut self) -> f64 {
        self.rand() as f64 / 32767.5 - 1.0
    }

    fn sound(&mut self, id: i32, at: Option<(f64, f64, f64)>) {
        if id > 0 {
            self.events.push(GameEvent::Sound { id, at });
        }
    }

    fn sound_group(&mut self, base: i32, at: (f64, f64, f64)) {
        if base > 0 {
            let v = (self.rand() % 3) as i32;
            self.sound(base + v, Some(at));
        }
    }

    /// Advances the simulation by real time `dt` (seconds) using fixed 120 Hz ticks.
    pub fn update(&mut self, res: &Resources, input: &Input, dt: f64) {
        self.acc += dt.min(0.25);
        let step = 1.0 / TICK_RATE;
        let mut first = true;
        while self.acc >= step {
            self.acc -= step;
            let mut inp = *input;
            if !first {
                inp.turn = 0.0;
                inp.look = 0.0;
            }
            first = false;
            self.snapshot();
            self.tick(res, &inp, step);
        }
        self.damage_flash = (self.damage_flash - dt * 2.0).max(0.0);
        self.pickup_flash = (self.pickup_flash - dt * 3.0).max(0.0);
    }

    fn snapshot(&mut self) {
        self.prev_xyz.clear();
        self.prev_xyz.extend(self.world.sprites.iter().map(|s| (s.x, s.y, s.z)));
        self.prev_eye = (self.player.x, self.player.y, self.player.view_z());
    }

    /// Fraction of a tick elapsed since the last simulated tick.
    fn alpha(&self) -> f64 {
        (self.acc * TICK_RATE).clamp(0.0, 1.0)
    }

    /// Position of sprite `i` blended between the last two ticks.
    fn draw_pos(&self, i: usize) -> (f64, f64, f64) {
        let s = &self.world.sprites[i];
        let cur = (s.x as f64, s.y as f64, s.z as f64);
        match self.prev_xyz.get(i) {
            Some(&(x, y, z)) => lerp3((x as f64, y as f64, z as f64), cur, self.alpha()),
            None => cur,
        }
    }

    fn obstacles(&self) -> Vec<CircleObstacle> {
        let (px, py) = (self.player.x, self.player.y);
        self.world
            .sprites
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                s.statnum != stat::FREE
                    && s.cstat & SPR_BLOCK != 0
                    && s.cstat & SPR_INVISIBLE == 0
                    && s.cstat & 48 == 0
                    && self.ai.get(*i).is_none_or(|a| !matches!(a, Ai::Dead | Ai::Dying))
                    && ((s.x as f64 - px).abs() < 2048.0 && (s.y as f64 - py).abs() < 2048.0)
            })
            .map(|(i, s)| {
                let r = (s.clipdist as f64 * 4.0).max(16.0);
                CircleObstacle {
                    x: s.x as f64,
                    y: s.y as f64,
                    radius: r,
                    zbot: s.z as f64 + 0x800 as f64,
                    ztop: s.z as f64 - 0x3000 as f64,
                    id: i,
                }
            })
            .collect()
    }

    fn tick(&mut self, res: &Resources, input: &Input, dt: f64) {
        self.clock += 1;
        // Like the original, level-start events fire once the player first acts.
        if !self.started {
            let acted = input.forward != 0.0
                || input.strafe != 0.0
                || input.turn != 0.0
                || input.fire
                || input.alt_fire
                || input.use_key
                || input.jump;
            if acted {
                self.started = true;
                self.triggers
                    .send(&mut self.world, crate::triggers::chan::LEVEL_START, cmd::ON);
            }
        }
        if self.ai.len() < self.world.sprites.len() {
            self.ai.resize(self.world.sprites.len(), Ai::Idle);
            self.attack_cd.resize(self.world.sprites.len(), 0.0);
        }
        if self.provoked.len() < self.world.sprites.len() {
            self.provoked.resize(self.world.sprites.len(), false);
            self.sees.resize(self.world.sprites.len(), false);
        }
        let obstacles = self.obstacles();
        self.player.underwater = self
            .world
            .xsector(self.player.sector.max(0) as usize)
            .is_some_and(|x| x.underwater);
        let was_on_ground = self.player.on_ground;
        self.player.update(&self.world, input, &obstacles, dt);
        self.room_transition();
        if !was_on_ground && self.player.on_ground {
            let v = crate::player::legacy_fall_speed(self.player.fall_speed_peak);
            self.player.fall_speed_peak = 0.0;
            if v > 0x18000 as f64 && self.powerups.feather_fall <= 0.0 {
                let dmg = ((v - 0x18000 as f64) / 1024.0) as i32;
                self.damage_player(dmg.max(1), None);
            }
            if v > 0x6000 as f64 {
                self.sound(701, None);
            }
        }

        if input.use_key && !self.use_down {
            if self.player.dead {
                self.events.push(GameEvent::PlayerDied);
            } else {
                self.do_use(res);
            }
        }
        self.use_down = input.use_key;

        if let Some(w) = input.weapon {
            if !self.weapon_switch_down {
                self.arsenal.select(w as usize);
            }
            self.weapon_switch_down = true;
        } else {
            self.weapon_switch_down = false;
        }
        if !self.player.dead {
            self.tick_weapon(res, input.fire, input.alt_fire);
            self.tick_inventory(input, dt);
        }

        if self.player.sector != self.last_sector {
            if self.last_sector >= 0 {
                self.triggers.exit_sector(&mut self.world, self.last_sector as usize);
            }
            if self.player.sector >= 0 {
                self.triggers.enter_sector(&mut self.world, self.player.sector as usize);
            }
            self.last_sector = self.player.sector;
        }

        self.tick_seqs(res);
        self.tick_dudes(res, dt);
        self.tick_projectiles(res, dt);
        self.check_pickups_and_proximity();
        self.sector_damage(dt);
        if self.clock % 2 == 0 {
            self.sector_lighting();
        }
        self.triggers.update(&mut self.world);
        self.carry_player();
        self.handle_notices(res);
    }

    /// Moves the player along with sliding / rotating sectors they stand in.
    fn carry_player(&mut self) {
        let carries: Vec<_> = std::mem::take(&mut self.triggers.carries);
        for c in carries {
            if self.player.sector as usize == c.sector && self.player.on_ground {
                let (x, y) = c.apply(self.player.x, self.player.y);
                self.player.x = x;
                self.player.y = y;
                self.player.ang = (self.player.ang + c.dang() * 2048.0 / std::f64::consts::TAU).rem_euclid(2048.0);
                let ns = collide::find_sector_near(&self.world.sectors, &self.world.walls, x, y, self.player.sector);
                if ns >= 0 {
                    self.player.sector = ns;
                }
            }
        }
    }

    /// Light waveform in -1..=1 (or 0..=1) for Blood's lighting wave types.
    fn light_wave(&mut self, kind: u8, t: f64) -> f64 {
        let f = t.rem_euclid(1.0);
        match kind {
            0 => 1.0,
            1 => {
                if f < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            2 => f,
            3 => f * f,
            4 => 1.0 - f,
            5 => 0.5 + 0.5 * (f * std::f64::consts::TAU).sin(),
            6..=9 => {
                // Flicker: random, biased toward lit.
                let r = (self.rand() % 100) as f64 / 100.0;
                if r < 0.15 * (kind - 5) as f64 {
                    0.0
                } else {
                    1.0
                }
            }
            10 => {
                if f < 0.1 {
                    1.0
                } else {
                    0.0
                }
            }
            11 => (1.0 - (f * 2.0 - 1.0).abs()).powi(2),
            _ => 1.0,
        }
    }

    fn sector_lighting(&mut self) {
        for s in 0..self.world.sectors.len() {
            let Some(x) = self.world.xsector(s).cloned() else {
                continue;
            };
            if !(x.shade_floor || x.shade_ceiling || x.shade_walls) || x.amplitude == 0 {
                continue;
            }
            let v = if x.shade_always {
                let t = self.clock as f64 * x.freq as f64 / (120.0 * 16.0) + x.phase as f64 / 256.0;
                self.light_wave(x.wave, t)
            } else if x.busy > 0 {
                x.busy as f64 / 65536.0
            } else {
                0.0
            };
            let d = (x.amplitude as f64 * v).round() as i32;
            let clamp = |b: i8| (b as i32 + d).clamp(-128, 127) as i8;
            if x.shade_floor {
                self.world.sectors[s].floor.shade = clamp(self.world.base_floor_shade[s]);
            }
            if x.shade_ceiling {
                self.world.sectors[s].ceiling.shade = clamp(self.world.base_ceil_shade[s]);
            }
            if x.shade_walls {
                for w in self.world.sector_walls(s) {
                    self.world.walls[w].shade = clamp(self.world.base_wall_shade[w]);
                }
            }
        }
    }

    /// Moves the player between linked rooms when their eye crosses a portal plane.
    fn room_transition(&mut self) {
        let p = &self.player;
        if p.sector < 0 {
            return;
        }
        let s = p.sector as usize;
        let eye = p.z - p.eye;
        let (c, f) = self.world.zs_at(s, p.x, p.y);
        let link = self.world.links.get(s).copied().unwrap_or_default();
        let target = match (link.floor, link.ceil) {
            (Some(l), _) if eye > f => Some(l),
            (_, Some(l)) if eye < c => Some(l),
            _ => None,
        };
        let Some(l) = target else { return };
        let (nx, ny) = (p.x + l.dx, p.y + l.dy);
        let ns = collide::find_sector_near(&self.world.sectors, &self.world.walls, nx, ny, l.sector);
        if ns < 0 {
            return;
        }
        let p = &mut self.player;
        p.x = nx;
        p.y = ny;
        p.z += l.dz;
        p.sector = ns;
        self.last_sector = ns;
        let water = self.world.xsector(ns as usize).is_some_and(|x| x.underwater);
        if water != self.player.underwater {
            self.sound(if water { 720 } else { 721 }, None);
        }
    }

    fn sector_damage(&mut self, _dt: f64) {
        if self.player.sector < 0 || self.player.dead || !self.player.on_ground {
            return;
        }
        let s = self.player.sector as usize;
        let damage_floor = self.world.sectors[s].lotag == crate::triggers::sector_type::DAMAGE
            && self.world.xsector(s).is_some_and(|x| x.state == 1);
        if damage_floor && self.clock % 60 == 0 {
            let d = self.world.xsector(s).map_or(5, |x| (x.data as i32).clamp(1, 50));
            self.damage_player(d, None);
        }
    }

    fn handle_notices(&mut self, res: &Resources) {
        let notices: Vec<Notice> = std::mem::take(&mut self.triggers.notices);
        for n in notices {
            match n {
                Notice::LevelExit { secret } => self.events.push(GameEvent::LevelExit { secret }),
                Notice::SecretFound => self.push_message("You found a secret area!".into()),
                Notice::Teleport { sector } => self.teleport_from(sector),
                Notice::SectorStart(s, opening) => self.events.push(GameEvent::SectorSound {
                    sector: s,
                    opening,
                    start: true,
                }),
                Notice::SectorStop(s, opening) => self.events.push(GameEvent::SectorSound {
                    sector: s,
                    opening,
                    start: false,
                }),
                Notice::SwitchToggled(o, st) => {
                    self.events.push(GameEvent::Switch);
                    match o {
                        Obj::Wall(w) => {
                            if (20..=22).contains(&self.world.walls[w].lotag) {
                                let d = if st == 1 { 1 } else { -1 };
                                self.world.walls[w].picnum += d;
                                self.sound(if st == 1 { 211 } else { 212 }, Some(self.wall_center(w)));
                            }
                        }
                        Obj::Sprite(i) => {
                            let t = self.world.sprites[i].lotag;
                            if (20..=22).contains(&t) {
                                let d = if st == 1 { 1 } else { -1 };
                                self.world.sprites[i].picnum += d;
                                let p = self.sprite_pos(i);
                                self.sound(if st == 1 { 211 } else { 212 }, Some(p));
                            }
                            if is_dude(t) && st == 1 {
                                // Triggered dudes (e.g. statues) come alive.
                                self.provoked[i] = true;
                                if self.ai[i] == Ai::Dormant && ai::is_statue(t) {
                                    self.ai[i] = Ai::Idle;
                                }
                            }
                            if t == 708 {
                                if let Some(x) = self.world.xsprite(i) {
                                    let id = x.data2;
                                    let p = self.sprite_pos(i);
                                    self.sound(id, Some(p));
                                }
                            }
                            if t == 711 {
                                if let Some(id) = self.world.xsprite(i).map(|x| x.data1) {
                                    self.sound(id, None);
                                }
                            }
                            if t == EXPLODER && st == 1 {
                                // Scripted blasts, e.g. a wall giving way after its crack is
                                // blown (E1M2's wagon). Size and damage are estimates.
                                let p = self.sprite_pos(i);
                                let sect = self.world.sprites[i].sectnum as i32;
                                self.explode(res, p.0, p.1, p.2, sect, 1000.0, 40, false);
                            }
                        }
                        Obj::Sector(_) => {}
                    }
                }
                Notice::Text(n) => {
                    if let Some(m) = self.level_messages.get(&n).cloned() {
                        if !m.trim().is_empty() {
                            self.push_message(m);
                        }
                    }
                }
                Notice::TotalSecrets(_) => {}
            }
        }
    }

    fn wall_center(&self, w: usize) -> (f64, f64, f64) {
        let a = &self.world.walls[w];
        let b = &self.world.walls[a.point2 as usize];
        ((a.x + b.x) as f64 * 0.5, (a.y + b.y) as f64 * 0.5, self.player.z)
    }

    fn sprite_pos(&self, i: usize) -> (f64, f64, f64) {
        let s = &self.world.sprites[i];
        (s.x as f64, s.y as f64, s.z as f64)
    }

    pub fn push_message(&mut self, m: String) {
        self.messages.push(m.clone());
        self.events.push(GameEvent::Message(m));
    }

    fn teleport_from(&mut self, sector: usize) {
        let Some(x) = self.world.xsector(sector) else { return };
        let target = x.marker0 as usize;
        if target == 0 || target >= self.world.sprites.len() || self.player.sector as usize != sector {
            return;
        }
        let t = &self.world.sprites[target];
        self.player.x = t.x as f64;
        self.player.y = t.y as f64;
        self.player.ang = t.ang as f64;
        self.player.sector = t.sectnum as i32;
        let (_, f) = self.world.zs_at(t.sectnum as usize, t.x as f64, t.y as f64);
        self.player.z = f;
        self.player.vx = 0.0;
        self.player.vy = 0.0;
        self.last_sector = self.player.sector;
        self.sound(350, None);
    }

    // ------------------------------------------------------------------ use / keys

    /// Traces a horizontal ray through open portals. Returns the first blocking wall and distance.
    pub fn trace_wall(&self, x: f64, y: f64, z: f64, sect: i32, ang: f64, max: f64) -> Option<(usize, f64)> {
        let a = ang_to_rad(ang);
        let (dx, dy) = (a.cos(), a.sin());
        let w = &self.world;
        let mut s = sect;
        let mut t_in = 0.0;
        for _ in 0..64 {
            if s < 0 {
                return None;
            }
            let mut best: Option<(f64, usize)> = None;
            for wi in w.sector_walls(s as usize) {
                let w1 = &w.walls[wi];
                let w2 = &w.walls[w1.point2 as usize];
                if let Some((t, _)) = ray_segment(x, y, dx, dy, w1.x as f64, w1.y as f64, w2.x as f64, w2.y as f64) {
                    if t > t_in + 1e-6 && best.is_none_or(|b| t < b.0) {
                        best = Some((t, wi));
                    }
                }
            }
            let (t, wi) = best?;
            if t > max {
                return None;
            }
            let wall = &w.walls[wi];
            if wall.nextsector < 0 {
                return Some((wi, t));
            }
            let (c, f) = w.zs_at(wall.nextsector as usize, x + dx * t, y + dy * t);
            if z < c || z > f || f - c < 1024.0 || wall.extra > 0 {
                return Some((wi, t));
            }
            if let Some(xs) = w.xsector(wall.nextsector as usize) {
                if (xs.push || xs.wallpush) && f - c < 0x2000 as f64 {
                    return Some((wi, t));
                }
            }
            s = wall.nextsector as i32;
            t_in = t;
        }
        None
    }

    fn key_name(k: u8) -> &'static str {
        ["", "Skull", "Eye", "Fire", "Dagger", "Spider", "Moon", "seventh"][k.min(7) as usize]
    }

    /// Returns false (and tells the player) when a key is required but missing.
    fn check_key(&mut self, key: u8) -> bool {
        if key == 0 || key > 7 || self.player.keys[key as usize - 1] {
            return true;
        }
        self.push_message(format!("You need the {} key", Self::key_name(key)));
        self.sound(206, None);
        false
    }

    fn do_use(&mut self, res: &Resources) {
        let p = &self.player;
        let (x, y, z, ang, sect) = (p.x, p.y, p.view_z(), p.ang, p.sector);
        let a = ang_to_rad(ang);
        let (fx, fy) = (a.cos(), a.sin());
        let wall_hit = self.trace_wall(x, y, z, sect, ang, USE_RANGE);
        let wall_dist = wall_hit.map_or(f64::MAX, |w| w.1);
        let mut best_spr: Option<(f64, usize)> = None;
        for (i, s) in self.world.sprites.iter().enumerate() {
            if s.statnum == stat::FREE || s.extra <= 0 || is_dude(s.lotag) {
                continue;
            }
            let rx = s.x as f64 - x;
            let ry = s.y as f64 - y;
            let along = rx * fx + ry * fy;
            let side = (-rx * fy + ry * fx).abs();
            if along > 0.0
                && along < USE_RANGE.min(wall_dist + 32.0)
                && side < 160.0
                && best_spr.is_none_or(|b| along < b.0)
            {
                best_spr = Some((along, i));
            }
        }
        let mut done = false;
        if let Some((_, i)) = best_spr {
            let key = self.world.xsprite(i).map_or(0, |x| x.key);
            if self.world.xsprite(i).is_some_and(|x| x.push || x.tx_id > 0) {
                if !self.check_key(key) {
                    return;
                }
                done = self.triggers.push_sprite(&mut self.world, i);
            }
        }
        if !done {
            if let Some((wi, _)) = wall_hit {
                let wkey = self.world.xwall(wi).map_or(0, |x| x.key);
                let ns = self.world.walls[wi].nextsector;
                let skey = if ns >= 0 {
                    self.world.xsector(ns as usize).map_or(0, |x| x.key)
                } else {
                    0
                };
                let wall_usable = self.world.xwall(wi).is_some();
                let sector_usable = ns >= 0 && self.world.xsector(ns as usize).is_some_and(|x| x.push || x.wallpush);
                if (wall_usable || sector_usable) && !self.check_key(wkey.max(skey)) {
                    return;
                }
                done = self.triggers.push_wall(&mut self.world, wi);
            }
        }
        if !done && sect >= 0 {
            if let Some(k) = self.world.xsector(sect as usize).filter(|x| x.push).map(|x| x.key) {
                if !self.check_key(k) {
                    return;
                }
            }
            done = self.triggers.push_sector(&mut self.world, sect as usize);
        }
        if !done {
            self.events.push(GameEvent::UseFailed);
        }
        self.handle_notices(res);
    }

    // ------------------------------------------------------------------ animation

    pub fn start_seq(&mut self, res: &Resources, i: usize, id: u32) {
        if self.world.rt[i].seq == id as i32 {
            return;
        }
        if self.anims.seq(&res.blood, id).is_none() {
            return;
        }
        let rt = &mut self.world.rt[i];
        rt.seq = id as i32;
        rt.seq_frame = -1;
        rt.seq_time = 0;
        self.apply_seq_frame(res, i, 0);
    }

    fn play_dude_anim(&mut self, res: &Resources, i: usize, suffix: &str) -> bool {
        let Some(info) = dude_info(self.world.sprites[i].lotag) else {
            return false;
        };
        match self.anims.find_seq(info.seq_base, suffix) {
            Some(id) => {
                self.start_seq(res, i, id);
                true
            }
            None => false,
        }
    }

    fn apply_seq_frame(&mut self, res: &Resources, i: usize, frame: i32) -> bool {
        let id = self.world.rt[i].seq;
        let Some(seq) = self.anims.seq(&res.blood, id as u32) else {
            return false;
        };
        let Some(f) = seq.frames.get(frame as usize).copied() else {
            return false;
        };
        self.world.rt[i].seq_frame = frame;
        let s = &mut self.world.sprites[i];
        s.picnum = f.tile as i16;
        s.shade = f.shade;
        if f.xrepeat > 0 {
            s.xrepeat = f.xrepeat;
        }
        if f.yrepeat > 0 {
            s.yrepeat = f.yrepeat;
        }
        let keep_invisible_marker = is_marker_type(s.lotag);
        if f.invisible || keep_invisible_marker {
            s.cstat |= SPR_INVISIBLE;
        } else {
            s.cstat &= !SPR_INVISIBLE;
        }
        s.cstat = (s.cstat & !(SPR_XFLIP | SPR_YFLIP))
            | if f.flip_x { SPR_XFLIP } else { 0 }
            | if f.flip_y { SPR_YFLIP } else { 0 };
        if f.transparent {
            s.cstat |= 2;
        } else {
            s.cstat &= !2;
        }
        if f.play_sound && seq.sound_id > 0 {
            let p = (s.x as f64, s.y as f64, s.z as f64);
            self.sound(seq.sound_id, Some(p));
        }
        f.trigger
    }

    /// Advances all sprite sequences; fires callbacks on trigger frames.
    fn tick_seqs(&mut self, res: &Resources) {
        for i in 0..self.world.sprites.len() {
            if self.world.sprites[i].statnum == stat::FREE || self.world.rt[i].seq < 0 {
                continue;
            }
            let id = self.world.rt[i].seq as u32;
            let Some(seq) = self.anims.seq(&res.blood, id) else {
                continue;
            };
            if seq.frames.is_empty() {
                continue;
            }
            let tpf = seq.ticks_per_frame.max(1);
            self.world.rt[i].seq_time += 1;
            if self.world.rt[i].seq_time < tpf {
                continue;
            }
            self.world.rt[i].seq_time = 0;
            let mut next = self.world.rt[i].seq_frame + 1;
            if next as usize >= seq.frames.len() {
                if seq.looping() {
                    next = 0;
                } else if seq.flags & SEQ_REMOVE != 0 && !is_dude(self.world.sprites[i].lotag) {
                    self.world.delete_sprite(i);
                    continue;
                } else {
                    // Hold on the last frame and let AI know the animation ended.
                    self.world.rt[i].seq = -(id as i32) - 2;
                    self.seq_finished(res, i);
                    continue;
                }
            }
            if self.apply_seq_frame(res, i, next) {
                self.seq_callback(res, i);
            }
        }
    }

    fn seq_finished(&mut self, res: &Resources, i: usize) {
        match self.ai.get(i).copied() {
            Some(Ai::Attack) | Some(Ai::Recoil) => {
                self.ai[i] = Ai::Chase;
                self.play_dude_anim(res, i, "M1");
            }
            Some(Ai::Dying) => self.ai[i] = Ai::Dead,
            Some(Ai::Rise) => {
                self.ai[i] = Ai::Chase;
                self.play_dude_anim(res, i, "M1");
            }
            _ => {}
        }
    }

    fn seq_callback(&mut self, res: &Resources, i: usize) {
        if self.ai.get(i) == Some(&Ai::Attack) {
            self.dude_attack(res, i);
        }
    }

    // ------------------------------------------------------------------ dudes

    fn can_see_player(&self, i: usize) -> bool {
        let s = &self.world.sprites[i];
        // Dude z is at the feet; eye height comes from the sprite's size.
        let eye = match self.world.rt[i].eye {
            e if e > 0 => e as f64,
            _ => crate::player::EYE_STAND,
        };
        let (sx, sy, sz) = (s.x as f64, s.y as f64, s.z as f64 - eye);
        let (px, py, pz) = (self.player.x, self.player.y, self.player.view_z());
        let d = (px - sx).hypot(py - sy);
        if d < 1.0 {
            return true;
        }
        if self.powerups.invisible > 0.0 && d > 1200.0 {
            return false;
        }
        let h = collide::hitscan(
            &self.world.sectors,
            &self.world.walls,
            &[],
            sx,
            sy,
            sz,
            s.sectnum as i32,
            (px - sx) / d,
            (py - sy) / d,
            (pz - sz) / d,
            d,
        );
        h.dist >= d - 8.0
    }

    fn dude_mover(info: &DudeInfo) -> Mover {
        Mover {
            radius: info.radius,
            height: info.height,
            step: 0x2400 as f64,
        }
    }

    fn dude_gravity(&mut self, i: usize, info: &DudeInfo, dt: f64) {
        let s = &self.world.sprites[i];
        if s.sectnum < 0 {
            return;
        }
        let (c, f) = self.world.zs_at(s.sectnum as usize, s.x as f64, s.y as f64);
        let alive = !matches!(self.ai[i], Ai::Dead | Ai::Dying);
        if info.flying && alive {
            let s = &mut self.world.sprites[i];
            s.z = (s.z as f64).clamp(c + info.height, f) as i32;
            return;
        }
        let rt = &mut self.world.rt[i];
        rt.vz += crate::player::GRAVITY * dt;
        let s = &mut self.world.sprites[i];
        let mut z = s.z as f64 + rt.vz * dt;
        if z >= f {
            z = f;
            rt.vz = 0.0;
        }
        s.z = z as i32;
    }

    fn dude_attack(&mut self, res: &Resources, i: usize) {
        let s = self.world.sprites[i].clone();
        let Some(info) = dude_info(s.lotag) else { return };
        let (sx, sy, sz) = (s.x as f64, s.y as f64, s.z as f64 - info.height * 0.6);
        let (px, py, pz) = (self.player.x, self.player.y, self.player.view_z() + 0x800 as f64);
        let d = (px - sx).hypot(py - sy).max(1.0);
        let at = (sx, sy, sz);
        if info.attack_sound > 0 {
            self.sound(info.attack_sound, Some(at));
        }
        match info.attack {
            Attack::None => {}
            Attack::Melee { damage, range } => {
                if d < range + info.radius + crate::player::RADIUS + 32.0 && (pz - sz).abs() < 0x4000 as f64 {
                    self.damage_player(damage, Some(i));
                }
            }
            Attack::Hitscan {
                damage,
                pellets,
                spread,
                range,
            } => {
                let cyl = self.player_cylinder();
                for _ in 0..pellets {
                    let ex = self.randf() * spread * 1.5;
                    let ey = self.randf() * spread * 1.5;
                    let ez = self.randf() * spread * 16.0;
                    let (mut dx, mut dy) = ((px - sx) / d + ex, (py - sy) / d + ey);
                    let l = dx.hypot(dy);
                    dx /= l;
                    dy /= l;
                    let dz = (pz - sz) / d + ez;
                    let h = collide::hitscan(
                        &self.world.sectors,
                        &self.world.walls,
                        &[cyl],
                        sx,
                        sy,
                        sz,
                        s.sectnum as i32,
                        dx,
                        dy,
                        dz,
                        range,
                    );
                    match h.target {
                        TraceTarget::Sprite(PLAYER_ID) if self.powerups.reflect > 0.0 => {
                            self.damage_sprite(res, i, damage, false);
                        }
                        TraceTarget::Sprite(PLAYER_ID) => self.damage_player(damage, Some(i)),
                        TraceTarget::Wall(_) | TraceTarget::Floor(_) | TraceTarget::Ceiling(_)
                            if self.rand().is_multiple_of(3) =>
                        {
                            self.spawn_fx(44, h.x, h.y, h.z, h.sector);
                        }
                        _ => {}
                    }
                }
            }
            Attack::Projectile { kind, .. } => {
                let ang = (py - sy).atan2(px - sx);
                let dz = (pz - sz) / d;
                self.spawn_projectile(kind, sx, sy, sz, s.sectnum as i32, ang, dz, i as i64);
            }
        }
    }

    fn player_cylinder(&self) -> HitCylinder {
        HitCylinder {
            id: PLAYER_ID,
            x: self.player.x,
            y: self.player.y,
            radius: crate::player::RADIUS + 16.0,
            ztop: self.player.z - self.player.height(),
            zbot: self.player.z,
        }
    }

    /// Inventory keys, power-up timers and breathing.
    fn tick_inventory(&mut self, input: &Input, dt: f64) {
        let keys = (input.inv_prev, input.inv_next, input.inv_use);
        if keys.0 && !self.inv_keys.0 {
            self.inventory.cycle(-1);
        }
        if keys.1 && !self.inv_keys.1 {
            self.inventory.cycle(1);
        }
        if keys.2 && !self.inv_keys.2 {
            let mut hp = self.player.health;
            if let Some(item) = self.inventory.use_selected(&mut hp) {
                self.player.health = hp;
                let name = crate::inventory::INV_NAMES[item as usize];
                let msg = match item {
                    InvItem::DoctorBag => format!("Used {name}"),
                    _ if self.inventory.active[item as usize] => format!("{name} on"),
                    _ => format!("{name} off"),
                };
                self.push_message(msg);
                self.sound(if item == InvItem::DoctorBag { 780 } else { 776 }, None);
            }
        }
        self.inv_keys = keys;
        self.inventory.tick(dt);
        self.powerups.tick(dt);
        self.player.jump_scale = if self.inventory.is_active(InvItem::JumpBoots) {
            1.6
        } else {
            1.0
        };
        // Breathing.
        let suit = self.inventory.is_active(InvItem::DivingSuit);
        if self.player.underwater && !suit && !self.player.dead {
            self.air -= dt;
            if self.air < 0.0 && self.clock % 120 == 0 {
                self.damage_player(4, None);
                self.sound(724, None);
            }
        } else {
            if self.air < AIR_SECONDS * 0.5 && !self.player.underwater {
                self.sound(722, None);
            }
            self.air = AIR_SECONDS;
        }
    }

    pub fn damage_player(&mut self, amount: i32, _from: Option<usize>) {
        if self.player.dead || amount <= 0 || self.powerups.invulnerable > 0.0 {
            return;
        }
        // Difficulty scales damage taken. Measured in DOSBox (E4M9 axe-zombie hits, 18 on
        // skill 2): 10-11, 14-15, 18, 21-22 and 26 per hit, i.e. 9/16, 13/16, 1, 19/16, 23/16.
        const SKILL_SCALE: [f64; 5] = [0.5625, 0.8125, 1.0, 1.1875, 1.4375];
        let mut dmg = ((amount as f64 * SKILL_SCALE[self.skill.clamp(0, 4) as usize]).round() as i32).max(1);
        for a in self.player.armor.iter_mut() {
            if *a > 0 && dmg > 0 {
                let absorbed = (dmg / 2).min(*a);
                *a -= absorbed;
                dmg -= absorbed;
            }
        }
        self.player.health -= dmg;
        self.damage_flash = (self.damage_flash + amount as f64 / 25.0).min(1.0);
        self.events.push(GameEvent::PlayerHurt(dmg));
        if self.player.health <= 0 {
            self.player.health = 0;
            self.player.dead = true;
            let v = (self.rand() % 3) as i32;
            self.sound(716 + v, None);
            self.push_message("You are dead. Press Use to restart.".into());
        } else if self.rand().is_multiple_of(2) {
            let v = (self.rand() % 6) as i32;
            self.sound(710 + v, None);
        }
    }

    /// Damage that may set the target on fire.
    pub fn damage_sprite_kind(&mut self, res: &Resources, i: usize, amount: i32, explosive: bool, fire: bool) {
        if fire && is_dude(self.world.sprites[i].lotag) {
            self.world.rt[i].burn_time = self.world.rt[i].burn_time.max((4.0 * TICK_RATE) as i32);
        }
        self.damage_sprite(res, i, amount, explosive);
    }

    /// Applies damage to a sprite (dudes and destructible things).
    pub fn damage_sprite(&mut self, res: &Resources, i: usize, amount: i32, explosive: bool) {
        let s = &self.world.sprites[i];
        if s.statnum == stat::FREE {
            return;
        }
        let t = s.lotag;
        let p = self.sprite_pos(i);
        if is_dude(t) {
            let Some(info) = dude_info(t) else { return };
            if matches!(self.ai[i], Ai::Dead | Ai::Dying) {
                return;
            }
            self.world.rt[i].health -= amount;
            self.spawn_fx(59, p.0, p.1, p.2 - info.height * 0.5, s.sectnum as i32);
            if self.world.rt[i].health <= 0 {
                self.kill_dude(res, i, &info, explosive || amount >= info.health);
            } else {
                self.provoke(res, i, &info);
                if (self.rand() % 100) as f64 / 100.0 < info.recoil_chance && self.ai[i] != Ai::Attack {
                    if self.play_dude_anim(res, i, "R1") {
                        self.ai[i] = Ai::Recoil;
                    }
                    self.sound_group(info.pain_sound, p);
                }
            }
            return;
        }
        // Things with health (barrels, glass, ...): destroy and fire their trigger. Wall cracks
        // and exploding scenery (type 417, e.g. the E4M9 tree, which survives ~90 tommy-gun
        // rounds in a recording of the original) only give way to explosions.
        let breakable = if t == WALL_CRACK || t == OBJECT_EXPLODE {
            explosive
        } else {
            self.world.sprites[i].cstat & 256 != 0 || self.world.rt[i].health > 0
        };
        if self.world.xsprite(i).is_some() && breakable {
            self.world.rt[i].health -= amount;
            if self.world.rt[i].health <= 0 {
                self.triggers
                    .trigger(&mut self.world, Obj::Sprite(i), cmd::SPRITE_IMPACT);
                let is_explosive = matches!(t, TNT_BARREL | 416..=418 | 425..=427);
                self.world.delete_sprite(i);
                if is_explosive {
                    self.explode(
                        res,
                        p.0,
                        p.1,
                        p.2,
                        self.world.sprites[i].sectnum as i32,
                        1200.0,
                        80,
                        false,
                    );
                }
            }
        }
    }

    fn kill_dude(&mut self, res: &Resources, i: usize, info: &DudeInfo, gib: bool) {
        let p = self.sprite_pos(i);
        self.ai[i] = Ai::Dying;
        let s = &mut self.world.sprites[i];
        s.cstat &= !(SPR_BLOCK | 256);
        if s.lotag != 245 {
            self.kills += 1;
        }
        self.sound_group(info.die_sound, p);
        let burning = self.burn_kill || self.world.rt[i].burn_time > 0;
        let played = if burning {
            self.play_dude_anim(res, i, "D3") || self.play_dude_anim(res, i, "D1")
        } else if gib {
            self.play_dude_anim(res, i, "D2") || self.play_dude_anim(res, i, "D1")
        } else {
            self.play_dude_anim(res, i, "D1")
        };
        if !played {
            self.ai[i] = Ai::Dead;
        }
        // Drop item and fire the dude's own trigger.
        if let Some(x) = self.world.xsprite(i).cloned() {
            if x.drop_msg > 0 {
                self.spawn_item(x.drop_msg as i16, p.0, p.1, self.world.sprites[i].sectnum as i32);
            }
            if x.tx_id > 0 {
                self.triggers.send(&mut self.world, x.tx_id, x.command);
            }
        }
    }

    fn spawn_item(&mut self, t: i16, x: f64, y: f64, sect: i32) {
        let Some(&pic) = self.pic_for_type.get(&t) else { return };
        if sect < 0 {
            return;
        }
        let (_, f) = self.world.zs_at(sect as usize, x, y);
        let s = Sprite {
            x: x as i32,
            y: y as i32,
            z: f as i32,
            picnum: pic,
            xrepeat: 32,
            yrepeat: 32,
            sectnum: sect as i16,
            statnum: stat::ITEM,
            lotag: t,
            extra: -1,
            ..Default::default()
        };
        let i = self.world.spawn(s);
        self.ensure_slots(i);
    }

    fn ensure_slots(&mut self, i: usize) {
        if self.ai.len() <= i {
            self.ai.resize(i + 1, Ai::Idle);
            self.attack_cd.resize(i + 1, 0.0);
        }
        if self.provoked.len() <= i {
            self.provoked.resize(i + 1, false);
            self.sees.resize(i + 1, false);
        }
        self.provoked[i] = false;
        self.ai[i] = Ai::Idle;
    }

    // ------------------------------------------------------------------ effects

    pub fn spawn_fx(&mut self, seq: u32, x: f64, y: f64, z: f64, sect: i32) -> Option<usize> {
        if sect < 0 {
            return None;
        }
        let s = Sprite {
            x: x as i32,
            y: y as i32,
            z: z as i32,
            xrepeat: 48,
            yrepeat: 48,
            cstat: SPR_CENTER,
            sectnum: sect as i16,
            statnum: stat::FX,
            extra: -1,
            picnum: 0,
            ..Default::default()
        };
        let i = self.world.spawn(s);
        self.ensure_slots(i);
        self.world.rt[i].seq = -1;
        self.world.rt[i].ai_state = seq as i32;
        // Seq is started lazily in tick_projectiles (needs resources); mark pending.
        self.world.rt[i].ai_timer = -1;
        Some(i)
    }

    #[allow(clippy::too_many_arguments)]
    fn explode(&mut self, res: &Resources, x: f64, y: f64, z: f64, sect: i32, radius: f64, damage: i32, fire: bool) {
        if let Some(i) = self.spawn_fx(if fire { 5 } else { 3 }, x, y, z, sect) {
            self.world.sprites[i].xrepeat = 64;
            self.world.sprites[i].yrepeat = 64;
        }
        self.sound(if fire { 307 } else { 304 }, Some((x, y, z)));
        // Player.
        let d = (self.player.x - x)
            .hypot(self.player.y - y)
            .hypot((self.player.z - 0x1800 as f64 - z) / 16.0);
        if d < radius {
            let dmg = (damage as f64 * (1.0 - d / radius)) as i32;
            self.damage_player(dmg, None);
        }
        // Sprites.
        let targets: Vec<(usize, f64)> = self
            .world
            .sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| s.statnum != stat::FREE && (is_dude(s.lotag) || s.extra > 0))
            .map(|(i, s)| (i, (s.x as f64 - x).hypot(s.y as f64 - y).hypot((s.z as f64 - z) / 16.0)))
            .filter(|(_, d)| *d < radius)
            .collect();
        for (i, d) in targets {
            let dmg = (damage as f64 * (1.0 - d / radius)) as i32;
            if dmg > 0 {
                self.damage_sprite(res, i, dmg, true);
            }
        }
    }

    // ------------------------------------------------------------------ projectiles

    #[allow(clippy::too_many_arguments)]
    fn spawn_projectile(
        &mut self,
        kind: ProjKind,
        x: f64,
        y: f64,
        z: f64,
        sect: i32,
        ang: f64,
        dz: f64,
        owner: i64,
    ) -> Option<usize> {
        if sect < 0 {
            return None;
        }
        let (speed, seq) = match kind {
            ProjKind::Flare => (7000.0, 41),
            ProjKind::Fireball => (4500.0, 22),
            ProjKind::Napalm => (4500.0, 22),
            ProjKind::Tesla => (11250.0, 23),
            ProjKind::Bone => (3750.0, 24),
            ProjKind::Puke => (3250.0, 68),
            ProjKind::Tnt => (2600.0, 0),
            ProjKind::Leech => (7500.0, 23),
        };
        let s = Sprite {
            x: x as i32,
            y: y as i32,
            z: z as i32,
            xrepeat: 32,
            yrepeat: 32,
            cstat: SPR_CENTER,
            sectnum: sect as i16,
            statnum: stat::PROJECTILE,
            lotag: PROJ_BASE + kind as i16,
            ang: ((ang.to_degrees() / 360.0 * 2048.0).rem_euclid(2048.0)) as i16,
            picnum: if kind == ProjKind::Tnt {
                self.pic_for_type.get(&62).copied().unwrap_or(0)
            } else {
                0
            },
            extra: -1,
            ..Default::default()
        };
        let i = self.world.spawn(s);
        self.ensure_slots(i);
        let rt = &mut self.world.rt[i];
        rt.vx = ang.cos() * speed;
        rt.vy = ang.sin() * speed;
        rt.vz = dz * speed;
        if kind == ProjKind::Tnt {
            rt.vz -= 0x6000 as f64;
        }
        rt.target = owner as i32;
        rt.ai_timer = if kind == ProjKind::Tnt { 300 } else { 600 };
        rt.ai_state = seq;
        rt.seq = -1;
        Some(i)
    }

    fn proj_kind(t: i16) -> Option<ProjKind> {
        use ProjKind::*;
        Some(match t - PROJ_BASE {
            0 => Flare,
            1 => Fireball,
            2 => Napalm,
            3 => Tesla,
            4 => Bone,
            5 => Puke,
            6 => Tnt,
            7 => Leech,
            _ => return None,
        })
    }

    fn tick_projectiles(&mut self, res: &Resources, dt: f64) {
        for i in 0..self.world.sprites.len() {
            let st = self.world.sprites[i].statnum;
            if st == stat::FX {
                if self.world.rt[i].ai_timer == -1 {
                    self.world.rt[i].ai_timer = 0;
                    let seq = self.world.rt[i].ai_state as u32;
                    self.start_seq(res, i, seq);
                    if self.world.rt[i].seq < 0 {
                        self.world.delete_sprite(i);
                    }
                } else if self.world.rt[i].seq < -1 {
                    self.world.delete_sprite(i);
                }
                continue;
            }
            if st != stat::PROJECTILE {
                continue;
            }
            let Some(kind) = Self::proj_kind(self.world.sprites[i].lotag) else {
                continue;
            };
            if self.world.rt[i].seq == -1 && self.world.rt[i].ai_state > 0 {
                let seq = self.world.rt[i].ai_state as u32;
                self.start_seq(res, i, seq);
                self.world.rt[i].ai_state = 0;
            }
            self.world.rt[i].ai_timer -= 1;
            let s = self.world.sprites[i].clone();
            let (x, y, z) = (s.x as f64, s.y as f64, s.z as f64);
            if kind == ProjKind::Tnt {
                self.world.rt[i].vz += crate::player::GRAVITY * dt;
                if self.world.rt[i].ai_timer <= 0 {
                    self.world.delete_sprite(i);
                    self.explode(res, x, y, z, s.sectnum as i32, 1500.0, 120, false);
                    continue;
                }
            }
            if self.world.rt[i].ai_timer <= 0 {
                self.world.delete_sprite(i);
                continue;
            }
            let rt = &self.world.rt[i];
            let (vx, vy, vz) = (rt.vx * dt, rt.vy * dt, rt.vz * dt);
            let owner = rt.target;
            let len = vx.hypot(vy);
            if len < 1e-6 {
                continue;
            }
            let mut cyls: Vec<HitCylinder> = Vec::new();
            if owner != -2 {
                cyls.push(self.player_cylinder());
            }
            for (j, o) in self.world.sprites.iter().enumerate() {
                if j as i32 == owner || j == i || o.statnum == stat::FREE || !is_dude(o.lotag) {
                    continue;
                }
                if matches!(self.ai[j], Ai::Dead | Ai::Dying) {
                    continue;
                }
                if (o.x as f64 - x).abs() > len + 256.0 || (o.y as f64 - y).abs() > len + 256.0 {
                    continue;
                }
                let r = dude_info(o.lotag).map_or(64.0, |d| d.radius);
                let h = dude_info(o.lotag).map_or(0x3000 as f64, |d| d.height);
                cyls.push(HitCylinder {
                    id: j,
                    x: o.x as f64,
                    y: o.y as f64,
                    radius: r,
                    ztop: o.z as f64 - h,
                    zbot: o.z as f64,
                });
            }
            let h = collide::hitscan(
                &self.world.sectors,
                &self.world.walls,
                &cyls,
                x,
                y,
                z,
                s.sectnum as i32,
                vx / len,
                vy / len,
                vz / len,
                len,
            );
            if h.target == TraceTarget::Nothing {
                let ns = collide::find_sector_near(&self.world.sectors, &self.world.walls, h.x, h.y, s.sectnum as i32);
                let sp = &mut self.world.sprites[i];
                sp.x = h.x as i32;
                sp.y = h.y as i32;
                sp.z = h.z as i32;
                if ns >= 0 {
                    sp.sectnum = ns as i16;
                }
                continue;
            }
            if kind == ProjKind::Tnt {
                // Bounce and keep the fuse burning.
                let rt = &mut self.world.rt[i];
                match h.target {
                    TraceTarget::Floor(_) | TraceTarget::Ceiling(_) => rt.vz = -rt.vz * 0.4,
                    _ => {
                        rt.vx = -rt.vx * 0.4;
                        rt.vy = -rt.vy * 0.4;
                    }
                }
                rt.vx *= 0.8;
                rt.vy *= 0.8;
                continue;
            }
            self.world.delete_sprite(i);
            let (dmg, radius, fire) = match kind {
                ProjKind::Flare => (20, 0.0, true),
                ProjKind::Fireball => (25, 600.0, true),
                ProjKind::Napalm => (45, 900.0, true),
                ProjKind::Tesla => (18, 0.0, false),
                ProjKind::Bone => (15, 0.0, false),
                ProjKind::Puke => (10, 0.0, false),
                ProjKind::Leech => (16, 0.0, false),
                ProjKind::Tnt => (0, 0.0, false),
            };
            match h.target {
                TraceTarget::Sprite(PLAYER_ID) => self.damage_player(dmg, None),
                TraceTarget::Sprite(j) => self.damage_sprite(res, j, dmg, radius > 0.0),
                _ => {}
            }
            if radius > 0.0 {
                self.explode(res, h.x, h.y, h.z, h.sector, radius, dmg, fire);
            } else {
                self.spawn_fx(if fire { 54 } else { 44 }, h.x, h.y, h.z, h.sector);
            }
        }
    }

    // ------------------------------------------------------------------ player weapons

    fn tick_weapon(&mut self, res: &Resources, fire: bool, alt: bool) {
        self.arsenal.akimbo = self.powerups.akimbo > 0.0;
        let lens: HashMap<u32, i32> = [self.arsenal.qav]
            .iter()
            .filter_map(|&q| self.anims.qav(&res.blood, q).map(|a| (q, a.length())))
            .collect();
        let lib_len = |q: u32| lens.get(&q).copied().unwrap_or(30);
        let before = (self.arsenal.state, self.arsenal.qav);
        self.arsenal.tick(fire, alt, lib_len);
        if (self.arsenal.state, self.arsenal.qav) != before {
            // New animation: make sure its length is known next tick.
            let _ = self.anims.qav(&res.blood, self.arsenal.qav);
        }
        let Some(q) = self.anims.qav(&res.blood, self.arsenal.qav) else {
            return;
        };
        let frame = q.frame_at(self.arsenal.qav_time) as i32;
        if frame != self.arsenal.last_frame {
            self.arsenal.last_frame = frame;
            let f = &q.frames[frame as usize];
            if f.sound.id > 0 {
                self.sound(f.sound.id, None);
            }
            if f.callback > 0 && self.arsenal.state == WeaponState::Firing {
                self.player_shoot(res);
            }
        }
        // QAVs without callbacks fire on their first frame.
        if self.arsenal.state == WeaponState::Firing
            && self.arsenal.qav_time == 1
            && !q.frames.iter().any(|f| f.callback > 0)
        {
            self.player_shoot(res);
        }
    }

    /// Aim vector (dx, dy, dz) for the player, with light vertical auto-aim.
    fn player_aim(&self) -> (f64, f64, f64) {
        let a = ang_to_rad(self.player.ang);
        let (dx, dy) = (a.cos(), a.sin());
        let mut dz = -self.player.horiz / 160.0 * 16.0;
        let (ex, ey, ez) = (self.player.x, self.player.y, self.player.view_z());
        let mut best: Option<f64> = None;
        for (i, s) in self.world.sprites.iter().enumerate() {
            if s.statnum == stat::FREE || !is_dude(s.lotag) || matches!(self.ai.get(i), Some(Ai::Dead | Ai::Dying)) {
                continue;
            }
            let rx = s.x as f64 - ex;
            let ry = s.y as f64 - ey;
            let along = rx * dx + ry * dy;
            if !(32.0..12000.0).contains(&along) {
                continue;
            }
            let side = (-rx * dy + ry * dx).abs();
            if side > along * 0.08 + 48.0 {
                continue;
            }
            if best.is_none_or(|b| along < b) {
                best = Some(along);
                let h = dude_info(s.lotag).map_or(0x2000 as f64, |d| d.height);
                dz = (s.z as f64 - h * 0.5 - ez) / along;
            }
        }
        (dx, dy, dz)
    }

    fn player_shoot(&mut self, res: &Resources) {
        if !self.arsenal.consume() {
            return;
        }
        let kind = self.arsenal.mode().kind;
        let (dx, dy, dz) = self.player_aim();
        for shot in 0..self.arsenal.shots() {
            // Dual guns fire from the left and right hand.
            let side = if self.arsenal.shots() > 1 {
                if shot == 0 {
                    -24.0
                } else {
                    24.0
                }
            } else {
                0.0
            };
            self.fire_once(res, kind, dx, dy, dz, side);
        }
    }

    fn fire_once(&mut self, res: &Resources, kind: Fire, dx: f64, dy: f64, dz: f64, side: f64) {
        let (ex, ey, ez) = (
            self.player.x - dy * side,
            self.player.y + dx * side,
            self.player.view_z(),
        );
        let sect = self.player.sector;
        let cyls = self.dude_cylinders(ex, ey);
        let mult = self.powerups.damage_multiplier();
        match kind {
            Fire::Melee { damage, range } | Fire::Spray { damage, range } => {
                let h = collide::hitscan(
                    &self.world.sectors,
                    &self.world.walls,
                    &cyls,
                    ex,
                    ey,
                    ez,
                    sect,
                    dx,
                    dy,
                    dz,
                    range + crate::player::RADIUS,
                );
                if let TraceTarget::Sprite(j) = h.target {
                    let fire = matches!(kind, Fire::Spray { .. });
                    self.damage_sprite_kind(res, j, damage * mult, false, fire);
                } else if h.target != TraceTarget::Nothing {
                    self.sound(500, Some((h.x, h.y, h.z)));
                }
                if matches!(kind, Fire::Spray { .. }) {
                    self.spawn_fx(55, ex + dx * 300.0, ey + dy * 300.0, ez + dz * 300.0, sect);
                }
                self.alert_nearby(res, 1200.0);
            }
            Fire::Hitscan {
                damage,
                pellets,
                spread,
            } => {
                // Pellets hitting the same target add up into one hit, so a point-blank
                // shotgun blast can gib like in the original.
                let mut hits: Vec<(usize, i32)> = Vec::new();
                for _ in 0..pellets {
                    let (mut ax, mut ay) = (dx + self.randf() * spread, dy + self.randf() * spread);
                    let l = ax.hypot(ay);
                    ax /= l;
                    ay /= l;
                    let az = dz + self.randf() * spread * 12.0;
                    let h = collide::hitscan(
                        &self.world.sectors,
                        &self.world.walls,
                        &cyls,
                        ex,
                        ey,
                        ez,
                        sect,
                        ax,
                        ay,
                        az,
                        30000.0,
                    );
                    match h.target {
                        TraceTarget::Sprite(j) => match hits.iter_mut().find(|h| h.0 == j) {
                            Some(h) => h.1 += damage * mult,
                            None => hits.push((j, damage * mult)),
                        },
                        TraceTarget::Wall(w) => {
                            let fx = 44 + self.rand() % 3;
                            self.spawn_fx(fx, h.x - ax * 4.0, h.y - ay * 4.0, h.z, h.sector);
                            // Shooting a wall can trigger it (impact-activated switches).
                            if self.world.xwall(w).is_some_and(|x| x.trigger_vector) {
                                self.triggers.trigger(&mut self.world, Obj::Wall(w), cmd::WALL_IMPACT);
                            }
                        }
                        TraceTarget::Floor(_) | TraceTarget::Ceiling(_) => {
                            let fx = 44 + self.rand() % 3;
                            self.spawn_fx(fx, h.x, h.y, h.z, h.sector);
                        }
                        TraceTarget::Nothing => {}
                    }
                }
                let bullets = self.arsenal.def().ammo == crate::weapons::ammo::BULLETS;
                for (j, amount) in hits {
                    // Tommy-gun rounds (not shotgun shells) are scaled by the dude's bullet resistance.
                    let scale = if bullets {
                        dude_info(self.world.sprites[j].lotag).map_or(1.0, |d| d.bullet_scale)
                    } else {
                        1.0
                    };
                    self.damage_sprite(res, j, ((amount as f64 * scale).round() as i32).max(1), false);
                }
                self.alert_nearby(res, 10000.0); // gunshots carry far
            }
            Fire::Projectile(pk) => {
                let ang = dy.atan2(dx);
                self.spawn_projectile(pk, ex + dx * 64.0, ey + dy * 64.0, ez + 0x400 as f64, sect, ang, dz, -2);
                self.alert_nearby(res, 4000.0);
            }
            Fire::Burst {
                kind: pk,
                count,
                spread,
            } => {
                let base = dy.atan2(dx);
                for k in 0..count {
                    let f = if count > 1 {
                        k as f64 / (count - 1) as f64 - 0.5
                    } else {
                        0.0
                    };
                    let ang = base + f * spread * 2.0;
                    let jitter = self.randf() * 0.02;
                    self.spawn_projectile(
                        pk,
                        ex + dx * 64.0,
                        ey + dy * 64.0,
                        ez + 0x400 as f64,
                        sect,
                        ang,
                        dz + jitter,
                        -2,
                    );
                }
                self.alert_nearby(res, 5000.0);
            }
            Fire::Throw { fuse, power } => {
                let ang = dy.atan2(dx);
                if let Some(i) = self.spawn_projectile(
                    ProjKind::Tnt,
                    ex + dx * 64.0,
                    ey + dy * 64.0,
                    ez + 0x800 as f64,
                    sect,
                    ang,
                    dz,
                    -2,
                ) {
                    let rt = &mut self.world.rt[i];
                    let scale = 0.25 + 0.75 * power;
                    rt.vx *= scale;
                    rt.vy *= scale;
                    rt.vz = rt.vz * scale - 0x6000 as f64 * power;
                    rt.ai_timer = (fuse * TICK_RATE) as i32;
                }
            }
            Fire::Curse { damage } => {
                // Hurts every dude that can see the player.
                let targets: Vec<usize> = (0..self.world.sprites.len())
                    .filter(|&i| {
                        let s = &self.world.sprites[i];
                        s.statnum != stat::FREE
                            && is_dude(s.lotag)
                            && !matches!(self.ai[i], Ai::Dead | Ai::Dying)
                            && (s.x as f64 - ex).hypot(s.y as f64 - ey) < 10000.0
                            && self.can_see_player(i)
                    })
                    .collect();
                for i in targets {
                    self.damage_sprite(res, i, damage * mult, false);
                }
                let left = self.arsenal.ammo[crate::weapons::ammo::VOODOO];
                self.arsenal.ammo[crate::weapons::ammo::VOODOO] = (left - 5).max(0);
                self.sound(463, None);
            }
        }
    }

    fn dude_cylinders(&self, x: f64, y: f64) -> Vec<HitCylinder> {
        self.world
            .sprites
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                s.statnum != stat::FREE
                    && s.cstat & SPR_INVISIBLE == 0
                    && (is_dude(s.lotag) && !matches!(self.ai.get(*i), Some(Ai::Dead | Ai::Dying))
                        || s.cstat & 256 != 0 && s.extra > 0 && !is_dude(s.lotag))
                    && (s.x as f64 - x).abs() < 30000.0
                    && (s.y as f64 - y).abs() < 30000.0
            })
            .map(|(i, s)| {
                let (r, h) = dude_info(s.lotag).map_or(((s.clipdist as f64 * 4.0).max(32.0), 0x2000 as f64), |d| {
                    (d.radius, d.height)
                });
                HitCylinder {
                    id: i,
                    x: s.x as f64,
                    y: s.y as f64,
                    radius: r,
                    ztop: s.z as f64 - h,
                    zbot: s.z as f64 + 256.0,
                }
            })
            .collect()
    }

    // ------------------------------------------------------------------ pickups

    fn check_pickups_and_proximity(&mut self) {
        if self.player.dead {
            return;
        }
        let (px, py, pz) = (self.player.x, self.player.y, self.player.z);
        let top = pz - self.player.height() - 0x800 as f64;
        for i in 0..self.world.sprites.len() {
            let s = &self.world.sprites[i];
            if s.statnum == stat::FREE {
                continue;
            }
            let d = (s.x as f64 - px).hypot(s.y as f64 - py);
            if d > 768.0 {
                continue;
            }
            let t = s.lotag;
            let sz = s.z as f64;
            if let Some((pick, msg)) = pickup_of(t) {
                // Reach measured from the original: items 689 units away are taken at the
                // E2M1 start, items 1024+ away are not (E1M4, E1M7, E4M2, E3M5).
                if s.cstat & SPR_INVISIBLE != 0 || d > PICKUP_REACH || sz < top || sz > pz + 0x1000 as f64 {
                    continue;
                }
                if self.apply_pickup(pick) {
                    self.push_message(msg.to_string());
                    let snd = match pick {
                        Pickup::Key(_) => 781,
                        Pickup::Health { .. } => 780,
                        Pickup::Armor { .. } => 779,
                        Pickup::Weapon { .. } => 777,
                        Pickup::Ammo { .. } => 782,
                        Pickup::Powerup(_) | Pickup::Timed(..) | Pickup::Inventory(_) => 776,
                    };
                    self.sound(snd, None);
                    self.pickup_flash = 1.0;
                    if self.world.xsprite(i).is_some() {
                        self.triggers
                            .trigger(&mut self.world, Obj::Sprite(i), cmd::SPRITE_PICKUP);
                    }
                    self.world.delete_sprite(i);
                }
                continue;
            }
            if let Some(x) = self.world.xsprite(i) {
                if x.proximity && !x.is_triggered && d < 256.0 && (sz - pz).abs() < 0x4000 as f64 {
                    if let Some(xm) = self.world.xsprite_mut(i) {
                        xm.is_triggered = true;
                    }
                    self.triggers
                        .trigger(&mut self.world, Obj::Sprite(i), cmd::SPRITE_PROXIMITY);
                }
            }
        }
    }

    fn apply_pickup(&mut self, p: Pickup) -> bool {
        let pl = &mut self.player;
        match p {
            Pickup::Key(k) => {
                if pl.keys[k] {
                    return false;
                }
                pl.keys[k] = true;
                true
            }
            Pickup::Health { amount, max } => {
                if pl.health >= max {
                    return false;
                }
                pl.health = (pl.health + amount).min(max);
                true
            }
            Pickup::Armor { slot, amount } => {
                if pl.armor[slot] >= amount {
                    return false;
                }
                pl.armor[slot] = amount;
                true
            }
            Pickup::Weapon { slot, ammo } => self.arsenal.give_weapon(slot, ammo),
            Pickup::Ammo { kind, amount } => self.arsenal.add_ammo(kind, amount),
            Pickup::Inventory(i) => self.inventory.give(i),
            Pickup::Timed(k, secs) => {
                let p = &mut self.powerups;
                let t = match k {
                    Timed::Invisible => &mut p.invisible,
                    Timed::Invulnerable => &mut p.invulnerable,
                    Timed::Akimbo => &mut p.akimbo,
                    Timed::Reflect => &mut p.reflect,
                    Timed::FeatherFall => &mut p.feather_fall,
                    Timed::Rage => &mut p.rage,
                };
                *t = t.max(secs);
                true
            }
            Pickup::Powerup(_) => true,
        }
    }

    // ------------------------------------------------------------------ view

    pub fn camera(&self) -> Camera {
        let (x, y, z) = lerp3(
            self.prev_eye,
            (self.player.x, self.player.y, self.player.view_z()),
            self.alpha(),
        );
        Camera {
            x,
            y,
            z,
            ang: self.player.ang,
            // Blood's view centre sits 10 lines above the screen centre. The renderer's
            // horiz grows downward while the player's grows upward (look up = positive).
            horiz: 10.0 - self.player.horiz,
            sector: ob_engine::geom::update_sector(&self.world.sectors, &self.world.walls, x, y, self.player.sector),
        }
    }

    /// AI state of sprite `i` (for tests and debugging).
    pub fn ai_state(&self, i: usize) -> Option<Ai> {
        self.ai.get(i).copied()
    }

    /// Tile used by an item type in this level (for HUD icons).
    pub fn item_pic(&self, t: i16) -> Option<i32> {
        self.pic_for_type.get(&t).map(|&p| p as i32)
    }

    /// Current weapon QAV and its elapsed ticks, for the HUD.
    pub fn weapon_view(&self) -> (u32, i32) {
        (self.arsenal.qav, self.arsenal.qav_time)
    }

    /// Builds the renderer sprite list, resolving rotation frames for the camera.
    pub fn sprite_draws(&self, res: &Resources, cam: &Camera) -> Vec<SpriteDraw> {
        let mut out = Vec::with_capacity(self.world.sprites.len());
        for (i, s) in self.world.sprites.iter().enumerate() {
            if s.statnum == stat::FREE || s.cstat & SPR_INVISIBLE != 0 || s.picnum <= 0 && s.statnum != stat::DECORATION
            {
                continue;
            }
            let mut pic = res.tiles.animate(s.picnum as i32, self.clock);
            let mut cstat = s.cstat;
            let view = res.tiles.get(pic).anm.view;
            let voxel = if view == 6 || view == 7 {
                res.voxel_for_tile(pic)
            } else {
                None
            };
            // Spinning voxels rotate continuously.
            let ang = if view == 7 {
                (s.ang as f64 + self.clock as f64 * 4.0).rem_euclid(2048.0)
            } else {
                s.ang as f64
            };
            if (view == 1 || view == 2) && s.cstat & 48 == 0 {
                let to_cam = (cam.y - s.y as f64).atan2(cam.x - s.x as f64);
                let rel = (to_cam.to_degrees() / 360.0 * 2048.0 - s.ang as f64).rem_euclid(2048.0);
                let k = (((rel + 128.0) / 256.0).floor() as i32).rem_euclid(8);
                if view == 1 {
                    if k <= 4 {
                        pic += k;
                    } else {
                        pic += 8 - k;
                        cstat ^= SPR_XFLIP;
                    }
                } else {
                    pic += k;
                }
            }
            let (x, y, mut z) = self.draw_pos(i);
            // Dudes keep z at their feet; each animation frame's own offsets would otherwise
            // sink walking frames into the floor, so the frame's bottom edge goes to z.
            if s.statnum == stat::DUDE && s.cstat & 48 == 0 {
                let frame = Sprite {
                    picnum: pic as i16,
                    ..s.clone()
                };
                let (_, bot) = sprite_extents(res, &frame);
                z -= bot - s.z as f64;
            }
            out.push(SpriteDraw {
                x,
                y,
                z,
                picnum: pic,
                cstat,
                // Sprites take on their sector's (current, light-effected) shade, as in the
                // original: a zombie in a dark corridor is as dark as the corridor (E2M4), a
                // tree under the open sky is lit like the sky (E4M9).
                shade: s.shade as i32
                    + self.world.sectors.get(s.sectnum.max(0) as usize).map_or(0, |sec| {
                        if sec.ceiling.stat & ob_engine::render::PLANE_PARALLAX != 0 {
                            sec.ceiling.shade as i32
                        } else {
                            sec.floor.shade as i32
                        }
                    }),
                pal: s.pal,
                xrepeat: s.xrepeat,
                yrepeat: s.yrepeat,
                xoffset: s.xoffset,
                yoffset: s.yoffset,
                ang,
                sector: s.sectnum as i32,
                voxel,
                portal: 0,
            });
        }
        out
    }
}

/// Blends `a` toward `b`; big jumps (teleports, respawns) are not blended.
fn lerp3(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    if (b.0 - a.0).abs() > 1024.0 || (b.1 - a.1).abs() > 1024.0 || (b.2 - a.2).abs() > 0x4000 as f64 {
        return b;
    }
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}
