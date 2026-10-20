//! Saved games: the complete mutable game state serialized with bincode.
//! Static data (tiles, animations, sounds) is reloaded from the original files.

use super::{Ai, Game};
use crate::anim::AnimLib;
use crate::inventory::{Inventory, Powerups};
use crate::player::Player;
use crate::resources::Resources;
use crate::triggers::Triggers;
use crate::weapons::Arsenal;
use crate::world::World;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

const MAGIC: &str = "OPENBLOOD-SAVE";
const VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct SaveGame {
    magic: String,
    version: u32,
    /// Shown in the load menu.
    title: String,
    world: World,
    player: Player,
    triggers: Triggers,
    arsenal: Arsenal,
    inventory: Inventory,
    powerups: Powerups,
    air: f64,
    clock: i32,
    kills: i32,
    total_kills: i32,
    skill: i32,
    last_sector: i32,
    rng: u32,
    ai: Vec<Ai>,
    attack_cd: Vec<f64>,
    provoked: Vec<bool>,
    sees: Vec<bool>,
    pic_for_type: HashMap<i16, i16>,
    level_messages: HashMap<u32, String>,
}

/// Borrowed mirror of [`SaveGame`] (same field order, so bincode output matches).
#[derive(Serialize)]
struct SaveRef<'a> {
    magic: &'a str,
    version: u32,
    title: &'a str,
    world: &'a World,
    player: &'a Player,
    triggers: &'a Triggers,
    arsenal: &'a Arsenal,
    inventory: &'a Inventory,
    powerups: &'a Powerups,
    air: f64,
    clock: i32,
    kills: i32,
    total_kills: i32,
    skill: i32,
    last_sector: i32,
    rng: u32,
    ai: &'a Vec<Ai>,
    attack_cd: &'a Vec<f64>,
    provoked: &'a Vec<bool>,
    sees: &'a Vec<bool>,
    pic_for_type: &'a HashMap<i16, i16>,
    level_messages: &'a HashMap<u32, String>,
}

/// Minimal header used to list saves without loading them fully.
#[derive(Deserialize)]
struct SaveHeader {
    magic: String,
    version: u32,
    title: String,
}

fn invalid(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m.into())
}

impl Game {
    /// Serializes the game; `title` describes the save (map, time...).
    pub fn save_bytes(&self, title: &str) -> io::Result<Vec<u8>> {
        let sg = SaveRef {
            magic: MAGIC,
            version: VERSION,
            title,
            world: &self.world,
            player: &self.player,
            triggers: &self.triggers,
            arsenal: &self.arsenal,
            inventory: &self.inventory,
            powerups: &self.powerups,
            air: self.air,
            clock: self.clock,
            kills: self.kills,
            total_kills: self.total_kills,
            skill: self.skill,
            last_sector: self.last_sector,
            rng: self.rng,
            ai: &self.ai,
            attack_cd: &self.attack_cd,
            provoked: &self.provoked,
            sees: &self.sees,
            pic_for_type: &self.pic_for_type,
            level_messages: &self.level_messages,
        };
        bincode::serialize(&sg).map_err(|e| invalid(e.to_string()))
    }

    /// Restores a game saved with [`Game::save_bytes`].
    pub fn from_save(res: &Resources, bytes: &[u8]) -> io::Result<Game> {
        let sg: SaveGame = bincode::deserialize(bytes).map_err(|e| invalid(format!("corrupt save: {e}")))?;
        if sg.magic != MAGIC || sg.version != VERSION {
            return Err(invalid("not an OpenBlood save (or from another version)"));
        }
        let n = sg.world.sprites.len();
        let mut triggers = sg.triggers;
        triggers.rebuild(&sg.world);
        let mut g = Game {
            world: sg.world,
            player: sg.player,
            triggers,
            arsenal: sg.arsenal,
            anims: AnimLib::new(&res.blood),
            clock: sg.clock,
            events: Vec::new(),
            messages: Vec::new(),
            kills: sg.kills,
            total_kills: sg.total_kills,
            damage_flash: 0.0,
            pickup_flash: 0.0,
            skill: sg.skill,
            last_sector: sg.last_sector,
            acc: 0.0,
            use_down: true,
            rng: sg.rng,
            ai: sg.ai,
            attack_cd: sg.attack_cd,
            pic_for_type: sg.pic_for_type,
            weapon_switch_down: false,
            level_messages: sg.level_messages,
            inventory: sg.inventory,
            powerups: sg.powerups,
            air: sg.air,
            inv_keys: (true, true, true),
            provoked: sg.provoked,
            sees: sg.sees,
            burn_kill: false,
            prev_xyz: Vec::new(),
            prev_eye: (0.0, 0.0, 0.0),
            started: true,
        };
        g.ai.resize(n, Ai::Idle);
        g.attack_cd.resize(n, 0.0);
        g.provoked.resize(n, false);
        g.sees.resize(n, false);
        Ok(g)
    }

    pub fn save_to(&self, path: &Path, title: &str) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = self.save_bytes(title)?;
        // Write atomically so a crash can't corrupt an existing save.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }

    pub fn load_from(res: &Resources, path: &Path) -> io::Result<Game> {
        Game::from_save(res, &std::fs::read(path)?)
    }
}

/// Save slot file: `dir/SAVEn.OBS` (slot 0 = quick save).
pub fn slot_path(dir: &Path, slot: usize) -> PathBuf {
    if slot == 0 {
        dir.join("QUICK.OBS")
    } else {
        dir.join(format!("SAVE{slot}.OBS"))
    }
}

/// Title stored in a save file, if it is a valid OpenBlood save.
pub fn slot_title(path: &Path) -> Option<String> {
    let b = std::fs::read(path).ok()?;
    let h: SaveHeader = bincode::deserialize(&b).ok()?;
    (h.magic == MAGIC && h.version == VERSION).then_some(h.title)
}
