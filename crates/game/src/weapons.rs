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
    /// Short-range flame cone.
    Spray {
        damage: i32,
        range: f64,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct WeaponDef {
    pub name: &'static str,
    pub ammo: usize,
    pub per_shot: i32,
    pub up: u32,
    pub idle: u32,
    pub fire: u32,
    pub down: u32,
    pub fire_kind: Fire,
    /// Keep firing while the button is held.
    pub auto: bool,
}

/// Weapon slots 1..=10 (index 0 unused).
pub const WEAPONS: [WeaponDef; 11] = [
    WeaponDef {
        name: "",
        ammo: 0,
        per_shot: 0,
        up: 0,
        idle: 1,
        fire: 2,
        down: 3,
        fire_kind: Fire::Melee { damage: 0, range: 0.0 },
        auto: false,
    },
    WeaponDef {
        name: "Pitchfork",
        ammo: ammo::NONE,
        per_shot: 0,
        up: 0,
        idle: 1,
        fire: 2,
        down: 3,
        fire_kind: Fire::Melee {
            damage: 12,
            range: 160.0,
        },
        auto: true,
    },
    WeaponDef {
        name: "Flare Pistol",
        ammo: ammo::FLARES,
        per_shot: 1,
        up: 41,
        idle: 42,
        fire: 43,
        down: 44,
        fire_kind: Fire::Projectile(ProjKind::Flare),
        auto: false,
    },
    WeaponDef {
        name: "Sawed-off",
        ammo: ammo::SHELLS,
        per_shot: 1,
        up: 50,
        idle: 53,
        fire: 54,
        down: 58,
        fire_kind: Fire::Hitscan {
            damage: 5,
            pellets: 12,
            spread: 0.07,
        },
        auto: false,
    },
    WeaponDef {
        name: "Tommy Gun",
        ammo: ammo::BULLETS,
        per_shot: 1,
        up: 64,
        idle: 65,
        fire: 66,
        down: 68,
        fire_kind: Fire::Hitscan {
            damage: 6,
            pellets: 1,
            spread: 0.025,
        },
        auto: true,
    },
    WeaponDef {
        name: "Napalm Launcher",
        ammo: ammo::GASOLINE,
        per_shot: 1,
        up: 89,
        idle: 90,
        fire: 91,
        down: 92,
        fire_kind: Fire::Projectile(ProjKind::Napalm),
        auto: false,
    },
    WeaponDef {
        name: "Dynamite",
        ammo: ammo::TNT,
        per_shot: 1,
        up: 16,
        idle: 20,
        fire: 23,
        down: 17,
        fire_kind: Fire::Projectile(ProjKind::Tnt),
        auto: false,
    },
    WeaponDef {
        name: "Spray Can",
        ammo: ammo::SPRAY,
        per_shot: 1,
        up: 8,
        idle: 9,
        fire: 10,
        down: 11,
        fire_kind: Fire::Spray {
            damage: 3,
            range: 900.0,
        },
        auto: true,
    },
    WeaponDef {
        name: "Tesla Cannon",
        ammo: ammo::TESLA,
        per_shot: 1,
        up: 74,
        idle: 75,
        fire: 77,
        down: 81,
        fire_kind: Fire::Projectile(ProjKind::Tesla),
        auto: true,
    },
    WeaponDef {
        name: "Life Leech",
        ammo: ammo::SOULS,
        per_shot: 1,
        up: 111,
        idle: 112,
        fire: 114,
        down: 119,
        fire_kind: Fire::Projectile(ProjKind::Leech),
        auto: true,
    },
    WeaponDef {
        name: "Voodoo Doll",
        ammo: ammo::VOODOO,
        per_shot: 1,
        up: 100,
        idle: 101,
        fire: 103,
        down: 109,
        fire_kind: Fire::Hitscan {
            damage: 20,
            pellets: 1,
            spread: 0.0,
        },
        auto: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponState {
    Raising,
    Ready,
    Firing,
    Lowering,
}

#[derive(Debug, Clone)]
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
        }
    }
}

impl Arsenal {
    pub fn def(&self) -> &'static WeaponDef {
        &WEAPONS[self.current]
    }

    pub fn has_ammo(&self, w: usize) -> bool {
        let d = &WEAPONS[w];
        d.ammo == ammo::NONE || self.ammo[d.ammo] >= d.per_shot
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
            self.start(WeaponState::Lowering, WEAPONS[self.current].down);
        }
    }

    fn start(&mut self, st: WeaponState, qav: u32) {
        self.state = st;
        self.qav = qav;
        self.qav_time = 0;
        self.last_frame = -1;
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

    /// Advances one tick. `qav_len(id)` returns the animation length in ticks.
    /// Returns true when the fire QAV just started (the fire moment is signalled by
    /// QAV callbacks, handled by the caller via [`Arsenal::frame_changed`]).
    pub fn tick(&mut self, fire_held: bool, qav_len: impl Fn(u32) -> i32) -> bool {
        self.qav_time += 1;
        let done = self.qav_time >= qav_len(self.qav);
        let mut started = false;
        match self.state {
            WeaponState::Raising if done => self.start(WeaponState::Ready, self.def().idle),
            WeaponState::Lowering if done => {
                self.current = self.pending;
                self.start(WeaponState::Raising, self.def().up);
            }
            WeaponState::Firing if done => {
                if fire_held && self.def().auto && self.has_ammo(self.current) {
                    self.start(WeaponState::Firing, self.def().fire);
                    started = true;
                } else {
                    self.start(WeaponState::Ready, self.def().idle);
                }
            }
            WeaponState::Ready => {
                if done {
                    self.qav_time = 0; // loop idle
                    self.last_frame = -1;
                }
                if fire_held {
                    if self.has_ammo(self.current) {
                        self.start(WeaponState::Firing, self.def().fire);
                        started = true;
                    } else {
                        let b = self.best_available();
                        self.select(b);
                    }
                }
            }
            _ => {}
        }
        started
    }

    /// Consumes ammo for one shot; false if empty.
    pub fn consume(&mut self) -> bool {
        let d = self.def();
        if d.ammo == ammo::NONE {
            return true;
        }
        if self.ammo[d.ammo] < d.per_shot {
            return false;
        }
        self.ammo[d.ammo] -= d.per_shot;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_fire_and_switch() {
        let mut a = Arsenal::default();
        let len = |_| 10;
        for _ in 0..10 {
            a.tick(false, len);
        }
        assert_eq!(a.state, WeaponState::Ready);
        assert!(a.tick(true, len), "fire starts");
        assert_eq!(a.state, WeaponState::Firing);
        assert!(a.give_weapon(3, 8));
        assert_eq!(a.ammo[ammo::SHELLS], 8);
        for _ in 0..12 {
            a.tick(false, len);
        }
        a.select(3);
        for _ in 0..25 {
            a.tick(false, len);
        }
        assert_eq!(a.current, 3);
        assert!(a.consume());
        assert_eq!(a.ammo[ammo::SHELLS], 7);
    }

    #[test]
    fn empty_weapon_switches_away() {
        let mut a = Arsenal::default();
        a.have[3] = true;
        a.current = 3;
        a.state = WeaponState::Ready;
        a.tick(true, |_| 10);
        assert_eq!(a.state, WeaponState::Lowering);
        assert_eq!(a.pending, 1);
    }
}
