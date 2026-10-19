//! Enemy ("dude") definitions. Values are OpenBlood's own tuning, chosen to play close
//! to the original; animation sets are resolved from the SEQ names in BLOOD.RFF.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Attack {
    /// Harmless (innocents).
    None,
    Melee {
        damage: i32,
        range: f64,
    },
    /// Instant bullets: damage per pellet, pellets, spread (fraction of unit vector).
    Hitscan {
        damage: i32,
        pellets: i32,
        spread: f64,
        range: f64,
    },
    Projectile {
        kind: ProjKind,
        range: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjKind {
    Flare,
    Fireball,
    Napalm,
    Tesla,
    Bone,
    Puke,
    Tnt,
    Leech,
}

#[derive(Debug, Clone, Copy)]
pub struct DudeInfo {
    pub seq_base: u32,
    pub health: i32,
    /// Movement speed in xy units per second.
    pub speed: f64,
    pub radius: f64,
    pub height: f64,
    pub flying: bool,
    pub attack: Attack,
    /// Seconds between attacks.
    pub cooldown: f64,
    /// Base ids of sound groups (spot, pain, die); 0 = none. Each group has 3 variants.
    pub spot_sound: i32,
    pub pain_sound: i32,
    pub die_sound: i32,
    pub attack_sound: i32,
    /// Chance (0..1) that a hit interrupts the dude.
    pub recoil_chance: f64,
}

const fn d(seq_base: u32, health: i32, speed: f64, radius: f64, attack: Attack) -> DudeInfo {
    DudeInfo {
        seq_base,
        health,
        speed,
        radius,
        height: 0x3000 as f64,
        flying: false,
        attack,
        cooldown: 1.2,
        spot_sound: 0,
        pain_sound: 0,
        die_sound: 0,
        attack_sound: 0,
        recoil_chance: 0.5,
    }
}

pub fn dude_info(t: i16) -> Option<DudeInfo> {
    use Attack::*;
    let melee = |damage, range| Melee { damage, range };
    let mut i = match t {
        201 => DudeInfo {
            cooldown: 0.6,
            spot_sound: 1003,
            pain_sound: 1013,
            die_sound: 1018,
            attack_sound: 1031,
            ..d(
                4096,
                40,
                700.0,
                64.0,
                Hitscan {
                    damage: 4,
                    pellets: 1,
                    spread: 0.04,
                    range: 12000.0,
                },
            )
        },
        202 => DudeInfo {
            cooldown: 1.4,
            spot_sound: 1003,
            pain_sound: 1013,
            die_sound: 1018,
            attack_sound: 411,
            ..d(
                4096,
                40,
                700.0,
                64.0,
                Hitscan {
                    damage: 4,
                    pellets: 6,
                    spread: 0.08,
                    range: 8000.0,
                },
            )
        },
        230 => DudeInfo {
            spot_sound: 1003,
            pain_sound: 1013,
            die_sound: 1018,
            attack_sound: 1031,
            cooldown: 0.6,
            ..d(
                11520,
                40,
                0.0,
                64.0,
                Hitscan {
                    damage: 4,
                    pellets: 1,
                    spread: 0.04,
                    range: 12000.0,
                },
            )
        },
        246 => DudeInfo {
            spot_sound: 1003,
            pain_sound: 1013,
            die_sound: 1018,
            attack_sound: 411,
            ..d(
                11520,
                40,
                0.0,
                64.0,
                Hitscan {
                    damage: 4,
                    pellets: 6,
                    spread: 0.08,
                    range: 8000.0,
                },
            )
        },
        247 => DudeInfo {
            spot_sound: 4003,
            pain_sound: 4013,
            die_sound: 4018,
            attack_sound: 470,
            ..d(
                12800,
                60,
                700.0,
                64.0,
                Projectile {
                    kind: ProjKind::Tesla,
                    range: 10000.0,
                },
            )
        },
        248 => DudeInfo {
            spot_sound: 4003,
            pain_sound: 4013,
            die_sound: 4018,
            attack_sound: 455,
            cooldown: 2.0,
            ..d(
                13056,
                50,
                700.0,
                64.0,
                Projectile {
                    kind: ProjKind::Tnt,
                    range: 6000.0,
                },
            )
        },
        249 => DudeInfo {
            spot_sound: 4003,
            pain_sound: 4013,
            die_sound: 4018,
            ..d(
                13312,
                60,
                700.0,
                64.0,
                Hitscan {
                    damage: 4,
                    pellets: 6,
                    spread: 0.08,
                    range: 8000.0,
                },
            )
        },
        203 | 205 | 244 => DudeInfo {
            spot_sound: 1103,
            pain_sound: 1106,
            die_sound: 1107,
            attack_sound: 1101,
            ..d(4352, 60, 500.0, 64.0, melee(12, 220.0))
        },
        204 => DudeInfo {
            spot_sound: 1200,
            pain_sound: 1202,
            die_sound: 1204,
            attack_sound: 1207,
            recoil_chance: 0.3,
            ..d(4608, 100, 450.0, 80.0, melee(16, 240.0))
        },
        206 | 208 => DudeInfo {
            flying: true,
            spot_sound: 1400,
            pain_sound: 1402,
            die_sound: 1403,
            attack_sound: 1406,
            ..d(4864, 110, 900.0, 64.0, melee(20, 260.0))
        },
        207 | 209 => DudeInfo {
            flying: true,
            spot_sound: 1450,
            pain_sound: 1452,
            die_sound: 1453,
            attack_sound: 1457,
            recoil_chance: 0.2,
            ..d(
                5120,
                200,
                800.0,
                72.0,
                Projectile {
                    kind: ProjKind::Fireball,
                    range: 9000.0,
                },
            )
        },
        210 => DudeInfo {
            flying: true,
            spot_sound: 1600,
            pain_sound: 1602,
            die_sound: 1603,
            ..d(5376, 100, 900.0, 64.0, melee(20, 240.0))
        },
        211 => DudeInfo {
            spot_sound: 1300,
            pain_sound: 1302,
            die_sound: 1303,
            attack_sound: 1306,
            ..d(5632, 70, 1200.0, 64.0, melee(14, 240.0))
        },
        212 => DudeInfo {
            height: 0x1000 as f64,
            spot_sound: 1900,
            pain_sound: 1902,
            die_sound: 1903,
            ..d(5888, 10, 900.0, 32.0, melee(3, 160.0))
        },
        213 => DudeInfo {
            height: 0x1000 as f64,
            spot_sound: 1800,
            pain_sound: 1802,
            die_sound: 1803,
            ..d(6144, 10, 1000.0, 32.0, melee(3, 160.0))
        },
        214 => DudeInfo {
            height: 0x1000 as f64,
            spot_sound: 1800,
            pain_sound: 1802,
            die_sound: 1803,
            ..d(6400, 25, 1000.0, 32.0, melee(5, 160.0))
        },
        215 => DudeInfo {
            height: 0x1400 as f64,
            spot_sound: 1800,
            pain_sound: 1802,
            die_sound: 1803,
            ..d(6656, 75, 1000.0, 48.0, melee(8, 180.0))
        },
        216 => DudeInfo {
            height: 0x2000 as f64,
            spot_sound: 1853,
            pain_sound: 1851,
            die_sound: 1850,
            recoil_chance: 0.1,
            ..d(6912, 300, 700.0, 96.0, melee(14, 260.0))
        },
        217 => DudeInfo {
            spot_sound: 1700,
            pain_sound: 1702,
            die_sound: 1703,
            ..d(7168, 70, 700.0, 64.0, melee(12, 220.0))
        },
        218 => DudeInfo {
            flying: true,
            spot_sound: 1500,
            pain_sound: 1502,
            die_sound: 1503,
            ..d(7424, 30, 800.0, 48.0, melee(8, 200.0))
        },
        219 => DudeInfo {
            flying: true,
            height: 0x1000 as f64,
            spot_sound: 2000,
            pain_sound: 2002,
            die_sound: 2003,
            ..d(7680, 10, 1000.0, 32.0, melee(3, 160.0))
        },
        220 => DudeInfo {
            height: 0x0c00 as f64,
            spot_sound: 2100,
            pain_sound: 2102,
            die_sound: 2103,
            ..d(7936, 5, 900.0, 24.0, melee(2, 140.0))
        },
        221 | 223 | 225 => DudeInfo {
            spot_sound: 2201,
            pain_sound: 2200,
            die_sound: 2203,
            recoil_chance: 0.0,
            ..d(
                8192 + 512 * ((t - 221) / 2) as u32,
                50,
                0.0,
                48.0,
                Projectile {
                    kind: ProjKind::Puke,
                    range: 8000.0,
                },
            )
        },
        222 | 224 | 226 => DudeInfo {
            spot_sound: 2201,
            pain_sound: 2200,
            die_sound: 2203,
            recoil_chance: 0.0,
            ..d(8448 + 512 * ((t - 222) / 2) as u32, 50, 0.0, 48.0, melee(10, 280.0))
        },
        227 => DudeInfo {
            spot_sound: 2300,
            pain_sound: 2302,
            die_sound: 2304,
            attack_sound: 1309,
            recoil_chance: 0.05,
            ..d(
                9728,
                600,
                700.0,
                128.0,
                Projectile {
                    kind: ProjKind::Fireball,
                    range: 10000.0,
                },
            )
        },
        228 => DudeInfo {
            spot_sound: 2300,
            pain_sound: 2302,
            die_sound: 2304,
            attack_sound: 1309,
            recoil_chance: 0.05,
            ..d(
                9984,
                300,
                700.0,
                128.0,
                Projectile {
                    kind: ProjKind::Fireball,
                    range: 10000.0,
                },
            )
        },
        229 => DudeInfo {
            spot_sound: 2360,
            pain_sound: 2370,
            die_sound: 2380,
            recoil_chance: 0.0,
            ..d(
                10240,
                2000,
                400.0,
                160.0,
                Projectile {
                    kind: ProjKind::Fireball,
                    range: 12000.0,
                },
            )
        },
        245 => DudeInfo {
            spot_sound: 7007,
            pain_sound: 7007,
            die_sound: 7008,
            ..d(12544, 10, 600.0, 64.0, None)
        },
        250 => DudeInfo {
            height: 0x1800 as f64,
            ..d(13568, 10, 700.0, 32.0, melee(2, 160.0))
        },
        251 => DudeInfo {
            recoil_chance: 0.1,
            ..d(10496, 500, 800.0, 96.0, melee(30, 300.0))
        },
        _ => return Option::None,
    };
    if i.height == 0.0 {
        i.height = 0x3000 as f64;
    }
    Some(i)
}

pub fn is_dude(t: i16) -> bool {
    (200..=254).contains(&t) && !(231..=238).contains(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_common_dudes() {
        for t in [201, 202, 203, 204, 206, 207, 211, 213, 217, 219, 220, 227, 245] {
            let i = dude_info(t).expect("missing dude");
            assert!(i.health > 0 && i.seq_base >= 4096);
        }
        assert!(dude_info(231).is_none());
        assert!(is_dude(201) && !is_dude(231));
    }
}
