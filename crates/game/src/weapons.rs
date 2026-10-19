//! Player weapons, ammunition and the weapon animation state machine (driven by QAVs).

use crate::dudes::ProjKind;

pub const AMMO_TYPES: usize = 10;

pub mod ammo {
    pub const NONE: usize = 0;
    pub const FLARES: usize = 1;
    pub const SHELLS: usize = 2;
    pub const BULLETS: usize = 3;
    pub const GASOLINE: usize = 4;
    pub const TNT: usize = 5;
    pub const SPRAY: usize = 6;
    pub const TESLA: usize = 7;
    pub const SOULS: usize = 8;
    pub const VOODOO: usize = 9;
}

pub const AMMO_MAX: [i32; AMMO_TYPES] = [0, 50, 100, 400, 200, 50, 400, 300, 100, 100];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fire {
    Melee {
        damage: i32,
        range: f64,
    },
    Hitscan {
        damage: i32,
        pellets: i32,
        spread: f64,
    },
    Projectile(ProjKind),
    /// Several projectiles fanned out horizontally.
    Burst {
        kind: ProjKind,
        count: i32,
        spread: f64,
    },
    /// Thrown explosive with a fuse (seconds) and throw strength (0..1).
    Throw {
        fuse: f64,
        power: f64,
    },
    /// Short-range flame cone.
    Spray {
        damage: i32,
        range: f64,
    },
    /// Voodoo: damage every visible enemy.
    Curse {
        damage: i32,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct FireMode {
    pub qav: u32,
    pub kind: Fire,
    pub per_shot: i32,
    /// Keep firing while the button is held.
    pub auto: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct WeaponDef {
    pub name: &'static str,
    pub ammo: usize,
    pub up: u32,
    pub idle: u32,
    pub down: u32,
    pub primary: FireMode,
    pub alt: FireMode,
    /// Dual-wield ("guns akimbo") animation set: up, idle, fire, down.
    pub akimbo: Option<[u32; 4]>,
}

const fn mode(qav: u32, kind: Fire, per_shot: i32, auto: bool) -> FireMode {
    FireMode {
        qav,
        kind,
        per_shot,
        auto,
    }
}

const MELEE: Fire = Fire::Melee {
    damage: 12,
    range: 160.0,
};

/// Sawed-off QAVs: idle with 0 / 1 / 2 loaded barrels, first and second barrel, both, reload.
pub const SHOT_IDLE: [u32; 3] = [51, 52, 53];
pub const SHOT_FIRE1: u32 = 54;
pub const SHOT_FIRE2: u32 = 55;
pub const SHOT_RELOAD: u32 = 57;
/// Voodoo stab animations (picked at random).
pub const VOODOO_STABS: [u32; 4] = [103, 104, 105, 106];

/// Weapon slots 1..=10 (index 0 unused).
pub const WEAPONS: [WeaponDef; 11] = [
    WeaponDef {
        name: "",
        ammo: 0,
        up: 0,
        idle: 1,
        down: 3,
        primary: mode(2, MELEE, 0, true),
        alt: mode(2, MELEE, 0, true),
        akimbo: None,
    },
    WeaponDef {
        name: "Pitchfork",
        ammo: ammo::NONE,
        up: 0,
        idle: 1,
        down: 3,
        primary: mode(2, MELEE, 0, true),
        alt: mode(2, MELEE, 0, true),
        akimbo: None,
    },
    WeaponDef {
        name: "Flare Pistol",
        ammo: ammo::FLARES,
        up: 41,
        idle: 42,
        down: 44,
        primary: mode(43, Fire::Projectile(ProjKind::Flare), 1, false),
        alt: mode(
            43,
            Fire::Burst {
                kind: ProjKind::Flare,
                count: 8,
                spread: 0.35,
            },
            8,
            false,
        ),
        akimbo: Some([45, 46, 47, 49]),
    },
    WeaponDef {
        name: "Sawed-off",
        ammo: ammo::SHELLS,
        up: 50,
        idle: 53,
        down: 58,
        primary: mode(
            SHOT_FIRE1,
            Fire::Hitscan {
                damage: 5,
                pellets: 12,
                spread: 0.07,
            },
            1,
            false,
        ),
        alt: mode(
            56,
            Fire::Hitscan {
                damage: 5,
                pellets: 24,
                spread: 0.1,
            },
            2,
            false,
        ),
        akimbo: Some([59, 60, 62, 63]),
    },
    WeaponDef {
        name: "Tommy Gun",
        ammo: ammo::BULLETS,
        up: 64,
        idle: 65,
        down: 68,
        primary: mode(
            66,
            Fire::Hitscan {
                damage: 6,
                pellets: 1,
                spread: 0.025,
            },
            1,
            true,
        ),
        alt: mode(
            67,
            Fire::Hitscan {
                damage: 6,
                pellets: 6,
                spread: 0.12,
            },
            6,
            true,
        ),
        akimbo: Some([69, 70, 71, 72]),
    },
    WeaponDef {
        name: "Napalm Launcher",
        ammo: ammo::GASOLINE,
        up: 89,
        idle: 90,
        down: 92,
        primary: mode(91, Fire::Projectile(ProjKind::Napalm), 1, false),
        alt: mode(
            91,
            Fire::Burst {
                kind: ProjKind::Napalm,
                count: 3,
                spread: 0.15,
            },
            5,
            false,
        ),
        akimbo: Some([120, 121, 122, 124]),
    },
    WeaponDef {
        name: "Dynamite",
        ammo: ammo::TNT,
        up: 16,
        idle: 20,
        down: 17,
        primary: mode(23, Fire::Throw { fuse: 2.5, power: 1.0 }, 1, false),
        alt: mode(22, Fire::Throw { fuse: 4.0, power: 0.15 }, 1, false),
        akimbo: None,
    },
    WeaponDef {
        name: "Spray Can",
        ammo: ammo::SPRAY,
        up: 8,
        idle: 9,
        down: 11,
        primary: mode(
            10,
            Fire::Spray {
                damage: 3,
                range: 900.0,
            },
            1,
            true,
        ),
        alt: mode(14, Fire::Throw { fuse: 1.5, power: 0.8 }, 50, false),
        akimbo: None,
    },
    WeaponDef {
        name: "Tesla Cannon",
        ammo: ammo::TESLA,
        up: 74,
        idle: 75,
        down: 81,
        primary: mode(77, Fire::Projectile(ProjKind::Tesla), 1, true),
        alt: mode(
            78,
            Fire::Burst {
                kind: ProjKind::Tesla,
                count: 7,
                spread: 0.06,
            },
            35,
            false,
        ),
        akimbo: Some([82, 83, 84, 88]),
    },
    WeaponDef {
        name: "Life Leech",
        ammo: ammo::SOULS,
        up: 111,
        idle: 112,
        down: 119,
        primary: mode(114, Fire::Projectile(ProjKind::Leech), 1, true),
        alt: mode(
            115,
            Fire::Burst {
                kind: ProjKind::Leech,
                count: 5,
                spread: 0.2,
            },
            5,
            false,
        ),
        akimbo: None,
    },
    WeaponDef {
        name: "Voodoo Doll",
        ammo: ammo::VOODOO,
        up: 100,
        idle: 101,
        down: 109,
        primary: mode(
            103,
            Fire::Hitscan {
                damage: 20,
                pellets: 1,
                spread: 0.0,
            },
            1,
            false,
        ),
        alt: mode(107, Fire::Curse { damage: 60 }, 0, false),
        akimbo: None,
    },
];

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponState {
    Raising,
    Ready,
    Firing,
    Reloading,
    Lowering,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Arsenal {
    pub have: [bool; 11],
    pub ammo: [i32; AMMO_TYPES],
    pub current: usize,
    pub pending: usize,
    pub state: WeaponState,
    /// Currently playing QAV and ticks since it started.
    pub qav: u32,
    pub qav_time: i32,
    pub last_frame: i32,
    /// The running attack is the alternate fire.
    pub alt: bool,
    /// Loaded sawed-off barrels (0..=2).
    pub barrels: u8,
    /// Dual-wield power-up active.
    pub akimbo: bool,
    rng: u32,
}

impl Default for Arsenal {
    fn default() -> Self {
        let mut have = [false; 11];
        have[1] = true;
        Arsenal {
            have,
            ammo: [0; AMMO_TYPES],
            current: 1,
            pending: 1,
            state: WeaponState::Raising,
            qav: WEAPONS[1].up,
            qav_time: 0,
            last_frame: -1,
            alt: false,
            barrels: 2,
            akimbo: false,
            rng: 7,
        }
    }
}

impl Arsenal {
    pub fn def(&self) -> &'static WeaponDef {
        &WEAPONS[self.current]
    }

    fn dual(&self) -> Option<[u32; 4]> {
        if self.akimbo {
            self.def().akimbo
        } else {
            None
        }
    }

    /// The fire mode of the running (or requested) attack.
    pub fn mode(&self) -> FireMode {
        if self.alt {
            self.def().alt
        } else {
            self.def().primary
        }
    }

    pub fn has_ammo_for(&self, w: usize, alt: bool) -> bool {
        let d = &WEAPONS[w];
        let m = if alt { d.alt } else { d.primary };
        d.ammo == ammo::NONE || self.ammo[d.ammo] >= m.per_shot.max(1)
    }

    pub fn has_ammo(&self, w: usize) -> bool {
        self.has_ammo_for(w, false)
    }

    pub fn add_ammo(&mut self, kind: usize, n: i32) -> bool {
        if kind == 0 || kind >= AMMO_TYPES || self.ammo[kind] >= AMMO_MAX[kind] {
            return false;
        }
        self.ammo[kind] = (self.ammo[kind] + n).min(AMMO_MAX[kind]);
        true
    }

    /// Picks up a weapon; returns true if anything changed.
    pub fn give_weapon(&mut self, w: usize, ammo_amount: i32) -> bool {
        let new = !self.have[w];
        self.have[w] = true;
        let got_ammo = self.add_ammo(WEAPONS[w].ammo, ammo_amount);
        if new && self.state != WeaponState::Firing {
            self.select(w);
        }
        new || got_ammo
    }

    pub fn select(&mut self, w: usize) {
        if w == 0 || w > 10 || !self.have[w] || w == self.current && self.state != WeaponState::Lowering {
            return;
        }
        self.pending = w;
        if self.state != WeaponState::Lowering {
            let down = self.dual().map_or(self.def().down, |d| d[3]);
            self.start(WeaponState::Lowering, down);
        }
    }

    fn start(&mut self, st: WeaponState, qav: u32) {
        self.state = st;
        self.qav = qav;
        self.qav_time = 0;
        self.last_frame = -1;
    }

    fn idle_qav(&self) -> u32 {
        if let Some(d) = self.dual() {
            return d[1];
        }
        if self.current == 3 {
            return SHOT_IDLE[self.barrels.min(2) as usize];
        }
        self.def().idle
    }

    fn up_qav(&self) -> u32 {
        self.dual().map_or(self.def().up, |d| d[0])
    }

    fn fire_qav(&mut self, alt: bool) -> u32 {
        if let Some(d) = self.dual() {
            return d[2];
        }
        if self.current == 3 && !alt {
            return if self.barrels >= 2 { SHOT_FIRE1 } else { SHOT_FIRE2 };
        }
        if self.current == 10 && !alt {
            self.rng = self.rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
            return VOODOO_STABS[(self.rng >> 16) as usize % VOODOO_STABS.len()];
        }
        if alt {
            self.def().alt.qav
        } else {
            self.def().primary.qav
        }
    }

    /// Best weapon with ammo, used when the current one runs dry.
    pub fn best_available(&self) -> usize {
        for w in [4, 3, 8, 5, 2, 7, 9, 6, 1] {
            if self.have[w] && self.has_ammo(w) {
                return w;
            }
        }
        1
    }

    fn try_fire(&mut self, alt: bool) -> bool {
        // The sawed-off's alternate fire needs both barrels; with one loaded it fires that one.
        let alt = alt && !(self.current == 3 && self.barrels < 2);
        if self.has_ammo_for(self.current, alt) {
            self.alt = alt;
            let q = self.fire_qav(alt);
            self.start(WeaponState::Firing, q);
            true
        } else if alt && self.has_ammo_for(self.current, false) {
            self.try_fire(false)
        } else {
            let b = self.best_available();
            self.select(b);
            false
        }
    }

    /// Advances one tick. `qav_len(id)` returns the animation length in ticks.
    /// Returns true when an attack animation just started.
    pub fn tick(&mut self, fire_held: bool, alt_held: bool, qav_len: impl Fn(u32) -> i32) -> bool {
        self.qav_time += 1;
        let done = self.qav_time >= qav_len(self.qav);
        let mut started = false;
        match self.state {
            WeaponState::Raising if done => {
                let q = self.idle_qav();
                self.start(WeaponState::Ready, q)
            }
            WeaponState::Lowering if done => {
                self.current = self.pending;
                let q = self.up_qav();
                self.start(WeaponState::Raising, q);
            }
            WeaponState::Reloading if done => {
                self.barrels = 2;
                let q = self.idle_qav();
                self.start(WeaponState::Ready, q);
            }
            WeaponState::Firing if done => {
                if self.current == 3 && self.barrels == 0 && self.ammo[ammo::SHELLS] > 0 && self.dual().is_none() {
                    self.start(WeaponState::Reloading, SHOT_RELOAD);
                } else if (fire_held || alt_held) && self.mode().auto && self.has_ammo_for(self.current, self.alt) {
                    let alt = self.alt;
                    started = self.try_fire(alt);
                } else {
                    let q = self.idle_qav();
                    self.start(WeaponState::Ready, q);
                }
            }
            WeaponState::Ready => {
                if done {
                    self.qav_time = 0; // loop idle
                    self.last_frame = -1;
                }
                if self.current == 3 && self.barrels == 0 && self.ammo[ammo::SHELLS] > 0 {
                    self.start(WeaponState::Reloading, SHOT_RELOAD);
                } else if alt_held {
                    started = self.try_fire(true);
                } else if fire_held {
                    started = self.try_fire(false);
                }
            }
            _ => {}
        }
        started
    }

    /// Consumes ammo for one shot of the running attack; false if empty.
    pub fn consume(&mut self) -> bool {
        let d = self.def();
        let per = self.mode().per_shot;
        if d.ammo != ammo::NONE {
            if self.ammo[d.ammo] < per.max(1) {
                return false;
            }
            self.ammo[d.ammo] -= per;
        }
        if self.current == 3 {
            self.barrels = self.barrels.saturating_sub(if self.alt { 2 } else { 1 });
            if self.ammo[ammo::SHELLS] < self.barrels as i32 {
                self.barrels = self.ammo[ammo::SHELLS] as u8;
            }
        }
        true
    }

    /// Shots per trigger pull (dual-wielding doubles them).
    pub fn shots(&self) -> i32 {
        if self.dual().is_some() {
            2
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(a: &mut Arsenal) {
        for _ in 0..40 {
            a.tick(false, false, |_| 10);
        }
    }

    #[test]
    fn raise_fire_and_switch() {
        let mut a = Arsenal::default();
        ready(&mut a);
        assert_eq!(a.state, WeaponState::Ready);
        assert!(a.tick(true, false, |_| 10), "fire starts");
        assert_eq!(a.state, WeaponState::Firing);
        assert!(a.give_weapon(3, 8));
        assert_eq!(a.ammo[ammo::SHELLS], 8);
        ready(&mut a);
        a.select(3);
        ready(&mut a);
        assert_eq!(a.current, 3);
    }

    #[test]
    fn shotgun_barrels_and_reload() {
        let mut a = Arsenal::default();
        a.give_weapon(3, 10);
        ready(&mut a);
        assert_eq!(a.current, 3);
        assert!(a.tick(false, true, |_| 10));
        assert_eq!(a.qav, 56, "alternate fire uses both barrels");
        assert!(a.consume());
        assert_eq!((a.barrels, a.ammo[ammo::SHELLS]), (0, 8));
        for _ in 0..11 {
            a.tick(false, false, |_| 10);
        }
        assert_eq!(a.state, WeaponState::Reloading);
        ready(&mut a);
        assert_eq!(a.barrels, 2);
        assert!(a.tick(true, false, |_| 10));
        assert_eq!(a.qav, SHOT_FIRE1);
        a.consume();
        ready(&mut a);
        assert!(a.tick(true, false, |_| 10));
        assert_eq!(a.qav, SHOT_FIRE2, "second barrel");
    }

    #[test]
    fn empty_weapon_switches_away() {
        let mut a = Arsenal::default();
        a.have[4] = true;
        a.current = 4;
        a.state = WeaponState::Ready;
        a.tick(true, false, |_| 10);
        assert_eq!(a.state, WeaponState::Lowering);
        assert_eq!(a.pending, 1);
    }

    #[test]
    fn akimbo_uses_dual_animations() {
        let mut a = Arsenal::default();
        a.give_weapon(4, 100);
        a.akimbo = true;
        ready(&mut a);
        assert_eq!(a.qav, 70);
        a.tick(true, false, |_| 10);
        assert_eq!(a.qav, 71);
        assert_eq!(a.shots(), 2);
    }
}
