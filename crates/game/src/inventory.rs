//! Usable inventory items and timed power-ups.

/// Inventory slots in HUD order.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvItem {
    DoctorBag = 0,
    DivingSuit = 1,
    CrystalBall = 2,
    BeastVision = 3,
    JumpBoots = 4,
}

pub const INV_SLOTS: usize = 5;
pub const INV_NAMES: [&str; INV_SLOTS] = [
    "Doctor's Bag",
    "Diving Suit",
    "Crystal Ball",
    "Beast Vision",
    "Jumping Boots",
];
/// Blood item type that grants each slot (used for HUD icons).
pub const INV_ITEM_TYPES: [i16; INV_SLOTS] = [107, 118, 121, 125, 115];
/// Maximum charge per slot (health points for the bag, seconds for the rest).
pub const INV_MAX: [f64; INV_SLOTS] = [100.0, 60.0, 1.0, 60.0, 60.0];

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Inventory {
    pub charge: [f64; INV_SLOTS],
    pub active: [bool; INV_SLOTS],
    pub selected: usize,
}

impl Inventory {
    pub fn has(&self, i: InvItem) -> bool {
        self.charge[i as usize] > 0.0
    }

    pub fn is_active(&self, i: InvItem) -> bool {
        self.active[i as usize] && self.has(i)
    }

    /// Adds an item; false if already full.
    pub fn give(&mut self, i: InvItem) -> bool {
        let k = i as usize;
        if self.charge[k] >= INV_MAX[k] {
            return false;
        }
        self.charge[k] = INV_MAX[k];
        if !self.has(self.slot(self.selected)) {
            self.selected = k;
        }
        true
    }

    fn slot(&self, k: usize) -> InvItem {
        [
            InvItem::DoctorBag,
            InvItem::DivingSuit,
            InvItem::CrystalBall,
            InvItem::BeastVision,
            InvItem::JumpBoots,
        ][k % INV_SLOTS]
    }

    /// Cycles the selection through owned items.
    pub fn cycle(&mut self, dir: i32) {
        for step in 1..=INV_SLOTS as i32 {
            let k = (self.selected as i32 + dir * step).rem_euclid(INV_SLOTS as i32) as usize;
            if self.charge[k] > 0.0 {
                self.selected = k;
                return;
            }
        }
    }

    /// Uses the selected item. For the doctor's bag, heals `health` up to 100 and
    /// returns the new health; other items toggle on/off.
    pub fn use_selected(&mut self, health: &mut i32) -> Option<InvItem> {
        let k = self.selected;
        if self.charge[k] <= 0.0 {
            return None;
        }
        let item = self.slot(k);
        match item {
            InvItem::DoctorBag => {
                if *health >= 100 {
                    return None;
                }
                let need = (100 - *health) as f64;
                let heal = need.min(self.charge[k]);
                *health += heal as i32;
                self.charge[k] -= heal;
            }
            InvItem::CrystalBall => {}
            _ => self.active[k] = !self.active[k],
        }
        Some(item)
    }

    /// Drains active timed items.
    pub fn tick(&mut self, dt: f64) {
        for k in 1..INV_SLOTS {
            if self.active[k] && k != InvItem::CrystalBall as usize {
                self.charge[k] = (self.charge[k] - dt).max(0.0);
                if self.charge[k] <= 0.0 {
                    self.active[k] = false;
                }
            }
        }
    }
}

/// Power-ups active for a number of seconds after pickup.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Powerups {
    pub invisible: f64,
    pub invulnerable: f64,
    pub akimbo: f64,
    pub reflect: f64,
    pub feather_fall: f64,
    pub rage: f64,
}

impl Powerups {
    pub fn tick(&mut self, dt: f64) {
        for t in [
            &mut self.invisible,
            &mut self.invulnerable,
            &mut self.akimbo,
            &mut self.reflect,
            &mut self.feather_fall,
            &mut self.rage,
        ] {
            *t = (*t - dt).max(0.0);
        }
    }

    pub fn damage_multiplier(&self) -> i32 {
        if self.rage > 0.0 {
            2
        } else {
            1
        }
    }

    /// Active power-ups with remaining seconds, for the HUD.
    pub fn list(&self) -> Vec<(&'static str, f64)> {
        [
            ("Invisible", self.invisible),
            ("Invulnerable", self.invulnerable),
            ("Guns Akimbo", self.akimbo),
            ("Reflective Shots", self.reflect),
            ("Feather Fall", self.feather_fall),
            ("Rage", self.rage),
        ]
        .into_iter()
        .filter(|(_, t)| *t > 0.0)
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_bag_heals_partially() {
        let mut inv = Inventory::default();
        assert!(inv.give(InvItem::DoctorBag));
        let mut hp = 30;
        assert_eq!(inv.use_selected(&mut hp), Some(InvItem::DoctorBag));
        assert_eq!(hp, 100);
        assert!((inv.charge[0] - 30.0).abs() < 1e-9);
        let mut hp = 100;
        assert_eq!(inv.use_selected(&mut hp), None, "no use at full health");
    }

    #[test]
    fn timed_items_drain_and_cycle() {
        let mut inv = Inventory::default();
        inv.give(InvItem::JumpBoots);
        inv.give(InvItem::DivingSuit);
        inv.selected = 4;
        inv.cycle(1);
        assert_eq!(inv.selected, 1);
        let mut hp = 100;
        inv.use_selected(&mut hp);
        assert!(inv.is_active(InvItem::DivingSuit));
        inv.tick(61.0);
        assert!(!inv.is_active(InvItem::DivingSuit) && !inv.has(InvItem::DivingSuit));
    }

    #[test]
    fn powerups_expire() {
        let mut p = Powerups {
            rage: 1.0,
            ..Default::default()
        };
        assert_eq!(p.damage_multiplier(), 2);
        p.tick(2.0);
        assert_eq!(p.damage_multiplier(), 1);
        assert!(p.list().is_empty());
    }
}
