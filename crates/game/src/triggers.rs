//! Blood's object messaging system: x-objects listen on receive channels (`rx_id`),
//! change state when commanded (optionally animated over a busy time), and broadcast
//! their own command on `tx_id` when reaching the on/off state.

use crate::world::World;
use std::collections::HashMap;

pub mod cmd {
    pub const OFF: u8 = 0;
    pub const ON: u8 = 1;
    pub const STATE: u8 = 2;
    pub const TOGGLE: u8 = 3;
    pub const NOT_STATE: u8 = 4;
    pub const LINK: u8 = 5;
    pub const LOCK: u8 = 6;
    pub const UNLOCK: u8 = 7;
    pub const TOGGLE_LOCK: u8 = 8;
    pub const STOP_OFF: u8 = 9;
    pub const STOP_ON: u8 = 10;
    pub const STOP_NEXT: u8 = 11;
    pub const COUNTER_SECTOR: u8 = 12;
    pub const CALLBACK: u8 = 20;
    pub const REPEAT: u8 = 21;
    pub const SPRITE_PUSH: u8 = 30;
    pub const SPRITE_IMPACT: u8 = 31;
    pub const SPRITE_PICKUP: u8 = 32;
    pub const SPRITE_TOUCH: u8 = 33;
    pub const SPRITE_SIGHT: u8 = 34;
    pub const SPRITE_PROXIMITY: u8 = 35;
    pub const SPRITE_EXPLODE: u8 = 36;
    pub const SECTOR_PUSH: u8 = 40;
    pub const SECTOR_IMPACT: u8 = 41;
    pub const SECTOR_ENTER: u8 = 42;
    pub const SECTOR_EXIT: u8 = 43;
    pub const WALL_PUSH: u8 = 50;
    pub const WALL_IMPACT: u8 = 51;
    pub const WALL_TOUCH: u8 = 52;
}

/// Reserved channels.
pub mod chan {
    pub const SET_TOTAL_SECRETS: u32 = 1;
    pub const SECRET_FOUND: u32 = 2;
    pub const TEXT_OVER: u32 = 3;
    pub const LEVEL_EXIT_NORMAL: u32 = 4;
    pub const LEVEL_EXIT_SECRET: u32 = 5;
    /// Broadcast once when a single-player level starts.
    pub const LEVEL_START: u32 = 7;
    pub const LEVEL_START_MATCH: u32 = 8;
    pub const LEVEL_START_COOP: u32 = 9;
    pub const FIRST_USER: u32 = 100;
}

pub mod sector_type {
    pub const Z_MOTION: i16 = 600;
    pub const Z_MOTION_SPRITE: i16 = 602;
    pub const WARP: i16 = 603;
    pub const TELEPORT: i16 = 604;
    pub const PATH: i16 = 612;
    pub const ROTATE_STEP: i16 = 613;
    pub const SLIDE_MARKED: i16 = 614;
    pub const ROTATE_MARKED: i16 = 615;
    pub const SLIDE: i16 = 616;
    pub const ROTATE: i16 = 617;
    pub const DAMAGE: i16 = 618;
    pub const COUNTER: i16 = 619;
}

/// Wall flags used by sliding sectors.
pub const WALL_MOVE_FORWARD: u16 = 0x4000;
pub const WALL_MOVE_REVERSE: u16 = 0x8000;

/// Placement of a moving sector relative to its map position: translation plus a
/// rotation by `ang` radians around (cx, cy).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Xform {
    pub dx: f64,
    pub dy: f64,
    pub ang: f64,
    pub cx: f64,
    pub cy: f64,
}

/// Change of a moving sector's transform during one update.
#[derive(Debug, Clone, Copy)]
pub struct Carry {
    pub sector: usize,
    pub old: Xform,
    pub new: Xform,
}

impl Carry {
    pub fn dang(&self) -> f64 {
        self.new.ang - self.old.ang
    }
    pub fn is_identity(&self) -> bool {
        (self.new.dx - self.old.dx).abs() < 1e-9 && (self.new.dy - self.old.dy).abs() < 1e-9 && self.dang().abs() < 1e-12
    }
    /// Maps a point that moved with the sector from the old placement to the new one.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = (x - self.old.dx, y - self.old.dy);
        let a = self.dang();
        let (c, s) = (a.cos(), a.sin());
        let (rx, ry) = (x - self.new.cx, y - self.new.cy);
        let (x, y) = (self.new.cx + rx * c - ry * s, self.new.cy + rx * s + ry * c);
        (x + self.new.dx, y + self.new.dy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Obj {
    Sector(usize),
    Wall(usize),
    Sprite(usize),
}

#[derive(Debug, Clone, Copy)]
struct Event {
    time: u64,
    obj: Obj,
    cmd: u8,
}

#[derive(Debug, Clone, Copy)]
struct Busy {
    obj: Obj,
    /// Target busy value 0 or 65536.
    target: u32,
    /// Busy units per tick.
    rate: f64,
    pos: f64,
}

/// Notifications for the game layer (sounds, messages, level flow).
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    LevelExit { secret: bool },
    SecretFound,
    TotalSecrets(i32),
    Text(u32),
    SectorStart(usize, bool),
    SectorStop(usize, bool),
    SwitchToggled(Obj, u8),
    Teleport { sector: usize },
}

pub struct Triggers {
    pub tick: u64,
    events: Vec<Event>,
    busy: Vec<Busy>,
    rx_index: HashMap<u32, Vec<Obj>>,
    /// Vertex groups: walls sharing a position (for moving shared vertices together).
    vertex_group: Vec<Vec<usize>>,
    pub notices: Vec<Notice>,
    /// Sector movements since the game last consumed them (to carry the player).
    pub carries: Vec<Carry>,
    xforms: HashMap<usize, Xform>,
    init_state: HashMap<usize, f64>,
    pub secrets_total: i32,
    pub secrets_found: i32,
}

/// Interpolation curves for busy motion (Blood "waves").
pub fn wave(busy: f64, kind: u8) -> f64 {
    let b = busy.clamp(0.0, 1.0);
    match kind {
        0 => b,
        1 => {
            if b >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        2 => (1.0 - (b * std::f64::consts::PI).cos()) * 0.5,
        3 => b * b,
        4 => 1.0 - (1.0 - b) * (1.0 - b),
        _ => b,
    }
}

/// Busy/wait times are stored in tenths of a second; the game clock runs at 120 Hz.
pub fn tenths_to_ticks(t: u32) -> u64 {
    (t as u64 * 12).max(1)
}

impl Triggers {
    pub fn new(world: &World) -> Self {
        let mut t = Triggers {
            tick: 0,
            events: Vec::new(),
            busy: Vec::new(),
            rx_index: HashMap::new(),
            vertex_group: Vec::new(),
            notices: Vec::new(),
            carries: Vec::new(),
            xforms: HashMap::new(),
            init_state: HashMap::new(),
            secrets_total: 0,
            secrets_found: 0,
        };
        t.reindex(world);
        // Vertex groups by original position.
        let mut by_pos: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, &(x, y)) in world.base_wall_xy.iter().enumerate() {
            by_pos.entry((x, y)).or_default().push(i);
        }
        t.vertex_group = world.base_wall_xy.iter().map(|p| by_pos[p].clone()).collect();
        t
    }

    pub fn reindex(&mut self, world: &World) {
        self.rx_index.clear();
        for s in 0..world.sectors.len() {
            if let Some(x) = world.xsector(s) {
                if x.rx_id > 0 {
                    self.rx_index.entry(x.rx_id).or_default().push(Obj::Sector(s));
                }
            }
        }
        for w in 0..world.walls.len() {
            if let Some(x) = world.xwall(w) {
                if x.rx_id > 0 {
                    self.rx_index.entry(x.rx_id).or_default().push(Obj::Wall(w));
                }
            }
        }
        for s in 0..world.sprites.len() {
            if world.sprites[s].statnum == crate::world::stat::FREE {
                continue;
            }
            if let Some(x) = world.xsprite(s) {
                if x.rx_id > 0 {
                    self.rx_index.entry(x.rx_id).or_default().push(Obj::Sprite(s));
                }
            }
        }
    }

    /// Initial pass after loading: apply start states and count secrets.
    pub fn init_level(&mut self, world: &mut World) {
        for s in 0..world.sectors.len() {
            if let Some(x) = world.xsector(s).cloned() {
                let busy = if x.state != 0 { 65536 } else { 0 };
                if let Some(xm) = world.xsector_mut(s) {
                    xm.busy = busy;
                }
                self.apply_sector(world, s, busy as f64 / 65536.0);
            }
        }
    }

    pub fn post(&mut self, obj: Obj, cmd: u8, delay_ticks: u64) {
        self.events.push(Event {
            time: self.tick + delay_ticks,
            obj,
            cmd,
        });
    }

    /// Broadcast `cmd` on channel `ch`.
    pub fn send(&mut self, world: &mut World, ch: u32, cmd: u8) {
        if ch == 0 {
            return;
        }
        match ch {
            chan::SET_TOTAL_SECRETS => {
                self.secrets_total += 1;
                self.notices.push(Notice::TotalSecrets(self.secrets_total));
                return;
            }
            chan::SECRET_FOUND => {
                self.secrets_found += 1;
                self.notices.push(Notice::SecretFound);
                return;
            }
            chan::TEXT_OVER => {
                self.notices.push(Notice::Text(cmd as u32));
                return;
            }
            chan::LEVEL_EXIT_NORMAL => {
                self.notices.push(Notice::LevelExit { secret: false });
                return;
            }
            chan::LEVEL_EXIT_SECRET => {
                self.notices.push(Notice::LevelExit { secret: true });
                return;
            }
            _ => {}
        }
        if let Some(list) = self.rx_index.get(&ch).cloned() {
            for o in list {
                self.trigger(world, o, cmd);
            }
        }
    }

    fn state_of(world: &World, o: Obj) -> Option<(u8, bool)> {
        match o {
            Obj::Sector(s) => world.xsector(s).map(|x| (x.state, x.locked)),
            Obj::Wall(w) => world.xwall(w).map(|x| (x.state, x.locked)),
            Obj::Sprite(s) => world.xsprite(s).map(|x| (x.state, x.locked)),
        }
    }

    fn set_locked(world: &mut World, o: Obj, v: Option<bool>) {
        let f = |l: &mut bool| *l = v.unwrap_or(!*l);
        match o {
            Obj::Sector(s) => {
                if let Some(x) = world.xsector_mut(s) {
                    f(&mut x.locked)
                }
            }
            Obj::Wall(w) => {
                if let Some(x) = world.xwall_mut(w) {
                    f(&mut x.locked)
                }
            }
            Obj::Sprite(s) => {
                if let Some(x) = world.xsprite_mut(s) {
                    f(&mut x.locked)
                }
            }
        }
    }

    /// Delivers a command to one object.
    pub fn trigger(&mut self, world: &mut World, o: Obj, c: u8) {
        let Some((state, locked)) = Self::state_of(world, o) else {
            return;
        };
        match c {
            cmd::LOCK => return Self::set_locked(world, o, Some(true)),
            cmd::UNLOCK => return Self::set_locked(world, o, Some(false)),
            cmd::TOGGLE_LOCK => return Self::set_locked(world, o, None),
            _ => {}
        }
        if locked {
            return;
        }
        if let Obj::Sector(s) = o {
            let t = world.sectors[s].lotag;
            if t == sector_type::TELEPORT || t == sector_type::WARP {
                self.notices.push(Notice::Teleport { sector: s });
                return;
            }
            if t == sector_type::COUNTER {
                return;
            }
        }
        // Busy objects interrupted mid-motion reverse direction.
        let busy_now = self.busy.iter().position(|b| b.obj == o);
        let target = match c {
            cmd::OFF => 0,
            cmd::ON => 1,
            cmd::STOP_OFF | cmd::STOP_ON | cmd::STOP_NEXT => {
                if let Some(i) = busy_now {
                    self.busy.remove(i);
                }
                return;
            }
            cmd::NOT_STATE => {
                if state == 0 {
                    1
                } else {
                    0
                }
            }
            cmd::CALLBACK | cmd::REPEAT | cmd::LINK | cmd::COUNTER_SECTOR => return,
            _ => {
                // Toggle and all "event" commands toggle the object; a busy object reverses.
                if let Some(i) = busy_now {
                    if self.busy[i].target == 65536 {
                        0
                    } else {
                        1
                    }
                } else if state == 0 {
                    1
                } else {
                    0
                }
            }
        };
        self.set_state(world, o, target);
    }

    fn busy_ticks(world: &World, o: Obj, target: u8) -> u32 {
        match o {
            Obj::Sector(s) => world
                .xsector(s)
                .map(|x| {
                    if target == 1 {
                        x.busy_time_a
                    } else {
                        x.busy_time_b.max(if x.busy_time_b == 0 { x.busy_time_a } else { 0 })
                    }
                })
                .unwrap_or(0),
            Obj::Wall(w) => world.xwall(w).map(|x| x.busy_time).unwrap_or(0),
            Obj::Sprite(s) => world.xsprite(s).map(|x| x.busy_time).unwrap_or(0),
        }
    }

    fn current_busy(world: &World, o: Obj) -> u32 {
        match o {
            Obj::Sector(s) => world.xsector(s).map(|x| x.busy).unwrap_or(0),
            Obj::Wall(w) => world.xwall(w).map(|x| x.busy).unwrap_or(0),
            Obj::Sprite(s) => world.xsprite(s).map(|x| x.busy).unwrap_or(0),
        }
    }

    fn set_busy(world: &mut World, o: Obj, b: u32) {
        match o {
            Obj::Sector(s) => {
                if let Some(x) = world.xsector_mut(s) {
                    x.busy = b
                }
            }
            Obj::Wall(w) => {
                if let Some(x) = world.xwall_mut(w) {
                    x.busy = b
                }
            }
            Obj::Sprite(s) => {
                if let Some(x) = world.xsprite_mut(s) {
                    x.busy = b
                }
            }
        }
    }

    fn set_state(&mut self, world: &mut World, o: Obj, target: u8) {
        let (state, _) = Self::state_of(world, o).unwrap_or((0, false));
        let busy_target = target as u32 * 65536;
        let cur = Self::current_busy(world, o);
        let already = self.busy.iter().position(|b| b.obj == o);
        if already.is_none() && state == target && cur == busy_target {
            return;
        }
        let bt = Self::busy_ticks(world, o, target);
        let animated = matches!(o, Obj::Sector(_)) && bt > 0;
        if let Some(i) = already {
            self.busy.remove(i);
        }
        if animated {
            if let Obj::Sector(s) = o {
                self.notices.push(Notice::SectorStart(s, target == 1));
            }
            let ticks = tenths_to_ticks(bt) as f64;
            self.busy.push(Busy {
                obj: o,
                target: busy_target,
                rate: 65536.0 / ticks,
                pos: cur as f64,
            });
        } else {
            Self::set_busy(world, o, busy_target);
            if let Obj::Sector(s) = o {
                self.apply_sector(world, s, target as f64);
            }
            self.reach_state(world, o, target);
        }
    }

    fn reach_state(&mut self, world: &mut World, o: Obj, st: u8) {
        let (tx, command, on, off, wait, rest, once) = match o {
            Obj::Sector(s) => {
                let Some(x) = world.xsector_mut(s) else { return };
                x.state = st;
                let wait = if st == 1 { x.wait_time_a } else { x.wait_time_b };
                // Sectors return to the opposite state after their wait time.
                (
                    x.tx_id,
                    x.command,
                    x.trigger_on,
                    x.trigger_off,
                    wait,
                    1 - st,
                    x.trigger_once,
                )
            }
            Obj::Wall(w) => {
                let Some(x) = world.xwall_mut(w) else { return };
                x.state = st;
                (
                    x.tx_id,
                    x.command,
                    x.trigger_on,
                    x.trigger_off,
                    x.wait_time,
                    x.rest_state as u8,
                    x.trigger_once,
                )
            }
            Obj::Sprite(s) => {
                let Some(x) = world.xsprite_mut(s) else { return };
                x.state = st;
                (
                    x.tx_id,
                    x.command,
                    x.trigger_on,
                    x.trigger_off,
                    x.wait_time,
                    x.rest_state as u8,
                    x.trigger_once,
                )
            }
        };
        if let Obj::Sector(s) = o {
            self.notices.push(Notice::SectorStop(s, st == 1));
        } else {
            self.notices.push(Notice::SwitchToggled(o, st));
        }
        if (st == 1 && on) || (st == 0 && off) {
            self.send(world, tx, command);
            if once {
                Self::set_locked(world, o, Some(true));
            }
        }
        if wait > 0 && st != rest {
            let c = if rest == 1 { cmd::ON } else { cmd::OFF };
            self.post(o, c, tenths_to_ticks(wait));
        }
    }

    /// Advances one 120 Hz tick.
    pub fn update(&mut self, world: &mut World) {
        self.tick += 1;
        // Due events.
        let now = self.tick;
        let mut due: Vec<Event> = Vec::new();
        self.events.retain(|e| {
            if e.time <= now {
                due.push(*e);
                false
            } else {
                true
            }
        });
        due.sort_by_key(|e| e.time);
        for e in due {
            self.trigger(world, e.obj, e.cmd);
        }
        // Busy motion.
        let mut finished = Vec::new();
        for b in self.busy.iter_mut() {
            if (b.target as f64) > b.pos {
                b.pos = (b.pos + b.rate).min(b.target as f64);
            } else {
                b.pos = (b.pos - b.rate).max(b.target as f64);
            }
            if (b.pos - b.target as f64).abs() < 0.5 {
                finished.push(*b);
            }
        }
        let snapshot: Vec<Busy> = self.busy.clone();
        for b in &snapshot {
            Self::set_busy(world, b.obj, b.pos as u32);
            if let Obj::Sector(s) = b.obj {
                let w = world
                    .xsector(s)
                    .map(|x| if b.target > 0 { x.busy_wave_a } else { x.busy_wave_b })
                    .unwrap_or(0);
                self.apply_sector(world, s, wave(b.pos / 65536.0, w));
            }
        }
        for b in finished {
            self.busy.retain(|x| x.obj != b.obj);
            self.reach_state(world, b.obj, (b.target / 65536) as u8);
        }
    }

    pub fn is_busy(&self, o: Obj) -> bool {
        self.busy.iter().any(|b| b.obj == o)
    }

    /// Positions a moving sector for interpolation factor `f` (0 = off, 1 = on).
    pub fn apply_sector(&mut self, world: &mut World, s: usize, f: f64) {
        let t = world.sectors[s].lotag;
        let Some(x) = world.xsector(s).cloned() else { return };
        match t {
            sector_type::Z_MOTION | sector_type::Z_MOTION_SPRITE => Self::apply_z(world, s, &x, f),
            sector_type::SLIDE | sector_type::SLIDE_MARKED => {
                let m0 = x.marker0 as usize;
                let m1 = x.marker1 as usize;
                if m0 >= world.sprites.len() || m1 >= world.sprites.len() {
                    return;
                }
                let (ax, ay, _) = world.base_sprite_xy[m0];
                let (bx, by, _) = world.base_sprite_xy[m1];
                // Map geometry is stored at the sector's initial state.
                let f = f - self.initial_state(world, s);
                let dx = ((bx - ax) as f64 * f).round() as i32;
                let dy = ((by - ay) as f64 * f).round() as i32;
                self.translate_sector(world, s, dx, dy, t == sector_type::SLIDE_MARKED);
                if t == sector_type::SLIDE {
                    self.carry(world, s, Xform { dx: dx as f64, dy: dy as f64, ang: 0.0, cx: 0.0, cy: 0.0 });
                }
            }
            sector_type::ROTATE | sector_type::ROTATE_MARKED | sector_type::ROTATE_STEP => {
                let m0 = x.marker0 as usize;
                if m0 >= world.sprites.len() {
                    return;
                }
                let (cx, cy, _) = world.base_sprite_xy[m0];
                let ang = world.sprites[m0].ang as f64;
                let a = ob_engine::geom::ang_to_rad(ang * (f - self.initial_state(world, s)));
                self.rotate_sector(world, s, cx, cy, a, t == sector_type::ROTATE_MARKED);
                if t != sector_type::ROTATE_MARKED {
                    self.carry(world, s, Xform { dx: 0.0, dy: 0.0, ang: a, cx: cx as f64, cy: cy as f64 });
                }
            }
            _ => {}
        }
    }

    /// Interpolates floor/ceiling between the x-sector's off and on heights, carrying
    /// sprites that rest on the floor.
    fn apply_z(world: &mut World, s: usize, x: &ob_engine::map::XSector, f: f64) {
        if x.on_floor_z == x.off_floor_z && x.on_ceil_z == x.off_ceil_z {
            return;
        }
        let old_floor = world.sectors[s].floor.z;
        let nf = x.off_floor_z as f64 + (x.on_floor_z - x.off_floor_z) as f64 * f;
        let nc = x.off_ceil_z as f64 + (x.on_ceil_z - x.off_ceil_z) as f64 * f;
        world.sectors[s].floor.z = nf.round() as i32;
        world.sectors[s].ceiling.z = nc.round() as i32;
        // Carry sprites resting on the floor.
        let dz = world.sectors[s].floor.z - old_floor;
        if dz != 0 {
            for sp in world.sprites.iter_mut() {
                if sp.sectnum as usize == s && sp.statnum != crate::world::stat::FREE && (sp.z - old_floor).abs() <= 256
                {
                    sp.z += dz;
                }
            }
        }
    }

    fn moved_walls(world: &World, s: usize, marked: bool) -> Vec<(usize, i32)> {
        let mut out = Vec::new();
        let range = world.sector_walls(s);
        let any_flag = range
            .clone()
            .any(|w| world.walls[w].cstat & (WALL_MOVE_FORWARD | WALL_MOVE_REVERSE) != 0);
        for w in range {
            let cs = world.walls[w].cstat;
            if cs & WALL_MOVE_FORWARD != 0 {
                out.push((w, 1));
            } else if cs & WALL_MOVE_REVERSE != 0 {
                out.push((w, -1));
            } else if !marked && !any_flag {
                out.push((w, 1));
            }
        }
        out
    }

    fn initial_state(&mut self, world: &World, s: usize) -> f64 {
        *self.init_state.entry(s).or_insert_with(|| world.xsector(s).map_or(0.0, |x| x.state as f64))
    }

    /// Moves sprites standing in a moving sector by the change from its previous transform,
    /// and records the change so the game can carry the player too.
    fn carry(&mut self, world: &mut World, s: usize, new: Xform) {
        let old = self.xforms.insert(s, new).unwrap_or(Xform { cx: new.cx, cy: new.cy, ..Default::default() });
        let d = Carry { sector: s, old, new };
        if d.is_identity() {
            return;
        }
        for sp in world.sprites.iter_mut() {
            if sp.sectnum as usize != s || sp.statnum == crate::world::stat::FREE || sp.statnum == crate::world::stat::MARKER {
                continue;
            }
            let (nx, ny) = d.apply(sp.x as f64, sp.y as f64);
            sp.x = nx.round() as i32;
            sp.y = ny.round() as i32;
            sp.ang = ((sp.ang as f64 + d.dang() * 2048.0 / std::f64::consts::TAU).round() as i32).rem_euclid(2048) as i16;
        }
        self.carries.push(d);
    }

    fn translate_sector(&mut self, world: &mut World, s: usize, dx: i32, dy: i32, marked: bool) {
        for (w, sign) in Self::moved_walls(world, s, marked) {
            let (bx, by) = world.base_wall_xy[w];
            for &g in &self.vertex_group[w] {
                world.walls[g].x = bx + dx * sign;
                world.walls[g].y = by + dy * sign;
            }
        }
    }

    fn rotate_sector(&mut self, world: &mut World, s: usize, cx: i32, cy: i32, a: f64, marked: bool) {
        let (c, sn) = (a.cos(), a.sin());
        for (w, sign) in Self::moved_walls(world, s, marked) {
            let (bx, by) = world.base_wall_xy[w];
            let (rx, ry) = ((bx - cx) as f64, (by - cy) as f64);
            let sn = sn * sign as f64;
            let nx = cx as f64 + rx * c - ry * sn;
            let ny = cy as f64 + rx * sn + ry * c;
            for &g in &self.vertex_group[w] {
                world.walls[g].x = nx.round() as i32;
                world.walls[g].y = ny.round() as i32;
            }
        }
    }

    /// Player pressed "use" on a wall (and possibly the sector behind it).
    pub fn push_wall(&mut self, world: &mut World, w: usize) -> bool {
        let mut done = false;
        if let Some(x) = world.xwall(w).cloned() {
            if x.trigger_push || x.tx_id > 0 || world.walls[w].lotag >= 20 {
                self.trigger(world, Obj::Wall(w), cmd::WALL_PUSH);
                done = true;
            }
        }
        let ns = world.walls[w].nextsector;
        if ns >= 0 {
            if let Some(x) = world.xsector(ns as usize).cloned() {
                if x.push || x.wallpush {
                    self.trigger(world, Obj::Sector(ns as usize), cmd::SECTOR_PUSH);
                    done = true;
                }
            }
        }
        done
    }

    pub fn push_sprite(&mut self, world: &mut World, s: usize) -> bool {
        if let Some(x) = world.xsprite(s).cloned() {
            if x.push || x.tx_id > 0 {
                self.trigger(world, Obj::Sprite(s), cmd::SPRITE_PUSH);
                return true;
            }
        }
        false
    }

    pub fn push_sector(&mut self, world: &mut World, s: usize) -> bool {
        if let Some(x) = world.xsector(s).cloned() {
            if x.push {
                self.trigger(world, Obj::Sector(s), cmd::SECTOR_PUSH);
                return true;
            }
        }
        false
    }

    pub fn enter_sector(&mut self, world: &mut World, s: usize) {
        if let Some(x) = world.xsector(s).cloned() {
            if x.enter {
                self.trigger(world, Obj::Sector(s), cmd::SECTOR_ENTER);
            }
            let t = world.sectors[s].lotag;
            if t == sector_type::TELEPORT || t == sector_type::WARP {
                self.notices.push(Notice::Teleport { sector: s });
            }
        }
    }

    pub fn exit_sector(&mut self, world: &mut World, s: usize) {
        if let Some(x) = world.xsector(s).cloned() {
            if x.exit {
                self.trigger(world, Obj::Sector(s), cmd::SECTOR_EXIT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ob_engine::map::{Map, Sector, Wall, XSector};

    fn door_world() -> World {
        let mut m = Map::default();
        let pts = [(0, 0), (512, 0), (512, 512), (0, 512)];
        for (i, &(x, y)) in pts.iter().enumerate() {
            m.walls.push(Wall {
                x,
                y,
                point2: ((i + 1) % 4) as i16,
                nextwall: -1,
                nextsector: -1,
                ..Default::default()
            });
        }
        let mut s = Sector {
            wallptr: 0,
            wallnum: 4,
            lotag: sector_type::Z_MOTION,
            extra: 1,
            ..Default::default()
        };
        s.ceiling.z = 0;
        s.floor.z = 0;
        m.sectors.push(s);
        m.xsectors.push(XSector::default());
        m.xsectors.push(XSector {
            rx_id: 100,
            busy_time_a: 10,
            wait_time_a: 20,
            off_ceil_z: 0,
            on_ceil_z: -8192,
            push: true,
            ..Default::default()
        });
        m.xwalls.push(Default::default());
        m.xsprites.push(Default::default());
        World::from_map("TEST", m)
    }

    #[test]
    fn door_opens_waits_and_closes() {
        let mut w = door_world();
        let mut t = Triggers::new(&w);
        t.init_level(&mut w);
        t.send(&mut w, 100, cmd::TOGGLE);
        for _ in 0..60 {
            t.update(&mut w);
        }
        assert!(
            w.sectors[0].ceiling.z < -1000 && w.sectors[0].ceiling.z > -8192,
            "half open"
        );
        for _ in 0..70 {
            t.update(&mut w);
        }
        assert_eq!(w.sectors[0].ceiling.z, -8192, "fully open");
        assert_eq!(w.xsector(0).unwrap().state, 1);
        // waits 2 s (240 ticks) then closes over 1 s
        for _ in 0..400 {
            t.update(&mut w);
        }
        assert_eq!(w.sectors[0].ceiling.z, 0, "closed again");
        assert_eq!(w.xsector(0).unwrap().state, 0);
    }

    #[test]
    fn toggling_busy_door_reverses() {
        let mut w = door_world();
        let mut t = Triggers::new(&w);
        t.push_sector(&mut w, 0);
        for _ in 0..60 {
            t.update(&mut w);
        }
        let mid = w.sectors[0].ceiling.z;
        t.push_sector(&mut w, 0);
        t.update(&mut w);
        assert!(w.sectors[0].ceiling.z > mid, "moving back down");
    }

    #[test]
    fn carry_translates_and_rotates() {
        let c = Carry { sector: 0, old: Xform::default(), new: Xform { dx: 10.0, dy: 0.0, ..Default::default() } };
        assert_eq!(c.apply(5.0, 5.0), (15.0, 5.0));
        let r = Carry {
            sector: 0,
            old: Xform::default(),
            new: Xform { ang: std::f64::consts::FRAC_PI_2, ..Default::default() },
        };
        let (x, y) = r.apply(10.0, 0.0);
        assert!((x - 0.0).abs() < 1e-9 && (y - 10.0).abs() < 1e-9);
    }

    #[test]
    fn waves_are_bounded() {
        for k in 0..5 {
            assert_eq!(wave(0.0, k), 0.0);
            assert!((wave(1.0, k) - 1.0).abs() < 1e-9);
        }
    }
}
