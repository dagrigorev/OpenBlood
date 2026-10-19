//! Pickup items: what each Blood item type gives the player.

use crate::weapons::ammo;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pickup {
    Key(usize),
    Health { amount: i32, max: i32 },
    Armor { slot: usize, amount: i32 },
    Weapon { slot: usize, ammo: i32 },
    Ammo { kind: usize, amount: i32 },
    Powerup(&'static str),
}

pub fn pickup_of(t: i16) -> Option<(Pickup, &'static str)> {
    use Pickup::*;
    Some(match t {
        100 => (Key(0), "Picked up Skull key"),
        101 => (Key(1), "Picked up Eye key"),
        102 => (Key(2), "Picked up Fire key"),
        103 => (Key(3), "Picked up Dagger key"),
        104 => (Key(4), "Picked up Spider key"),
        105 => (Key(5), "Picked up Moon key"),
        106 => (Key(6), "Picked up key"),
        107 => (Health { amount: 100, max: 100 }, "Picked up Doctor's Bag"),
        108 => (Health { amount: 20, max: 100 }, "Picked up Medicine Pouch"),
        109 => (Health { amount: 10, max: 100 }, "Picked up Life Essence"),
        110 => (Health { amount: 100, max: 200 }, "Picked up Life Seed"),
        111 => (Health { amount: 5, max: 100 }, "Picked up Red Potion"),
        112..=139 => (Powerup("power"), "Picked up power-up"),
        140 => (Armor { slot: 0, amount: 50 }, "Picked up Basic Armor"),
        141 => (Armor { slot: 1, amount: 100 }, "Picked up Body Armor"),
        142 => (Armor { slot: 2, amount: 100 }, "Picked up Fire Armor"),
        143 => (Armor { slot: 0, amount: 100 }, "Picked up Spirit Armor"),
        144 => (Armor { slot: 1, amount: 200 }, "Picked up Super Armor"),
        41 => (Weapon { slot: 3, ammo: 8 }, "Picked up Sawed-off"),
        42 => (Weapon { slot: 4, ammo: 50 }, "Picked up Tommy Gun"),
        43 => (Weapon { slot: 2, ammo: 8 }, "Picked up Flare Pistol"),
        44 => (Weapon { slot: 10, ammo: 1 }, "Picked up Voodoo Doll"),
        45 => (Weapon { slot: 8, ammo: 50 }, "Picked up Tesla Cannon"),
        46 => (Weapon { slot: 5, ammo: 10 }, "Picked up Napalm Launcher"),
        47 => (Weapon { slot: 1, ammo: 0 }, "Picked up Pitchfork"),
        48 => (Weapon { slot: 7, ammo: 100 }, "Picked up Spray Can"),
        49 => (Weapon { slot: 6, ammo: 1 }, "Picked up Dynamite"),
        50 => (Weapon { slot: 9, ammo: 10 }, "Picked up Life Leech"),
        60 => (
            Ammo {
                kind: ammo::SPRAY,
                amount: 100,
            },
            "Picked up Spray Can",
        ),
        62 => (
            Ammo {
                kind: ammo::TNT,
                amount: 1,
            },
            "Picked up Bundle of TNT",
        ),
        63 => (
            Ammo {
                kind: ammo::TNT,
                amount: 5,
            },
            "Picked up Case of TNT",
        ),
        64 | 65 => (
            Ammo {
                kind: ammo::TNT,
                amount: 1,
            },
            "Picked up bomb",
        ),
        66 => (
            Ammo {
                kind: ammo::SOULS,
                amount: 10,
            },
            "Picked up Trapped Soul",
        ),
        67 => (
            Ammo {
                kind: ammo::SHELLS,
                amount: 4,
            },
            "Picked up 4 shotgun shells",
        ),
        68 => (
            Ammo {
                kind: ammo::SHELLS,
                amount: 15,
            },
            "Picked up box of shotgun shells",
        ),
        69 => (
            Ammo {
                kind: ammo::BULLETS,
                amount: 20,
            },
            "Picked up a few bullets",
        ),
        70 => (
            Ammo {
                kind: ammo::VOODOO,
                amount: 1,
            },
            "Picked up Voodoo Doll",
        ),
        72 => (
            Ammo {
                kind: ammo::BULLETS,
                amount: 100,
            },
            "Picked up full drum of bullets",
        ),
        73 => (
            Ammo {
                kind: ammo::TESLA,
                amount: 50,
            },
            "Picked up Tesla charge",
        ),
        76 => (
            Ammo {
                kind: ammo::FLARES,
                amount: 8,
            },
            "Picked up flares",
        ),
        79 => (
            Ammo {
                kind: ammo::GASOLINE,
                amount: 16,
            },
            "Picked up gasoline can",
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_items() {
        assert_eq!(pickup_of(41).unwrap().0, Pickup::Weapon { slot: 3, ammo: 8 });
        assert_eq!(
            pickup_of(68).unwrap().0,
            Pickup::Ammo {
                kind: ammo::SHELLS,
                amount: 15
            }
        );
        assert!(matches!(pickup_of(100).unwrap().0, Pickup::Key(0)));
        assert!(pickup_of(0).is_none());
    }
}
