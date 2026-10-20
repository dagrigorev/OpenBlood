//! Enemy behaviour: perception (sight, hearing, editor flags), movement (chase, flee,
//! strafe, obstacle and ledge avoidance) and special states (rising zombies, stone
//! gargoyles, burning dudes). OpenBlood's own design, tuned to feel like the original.

use super::{Ai, Game, TICK_RATE};
use crate::dudes::{dude_info, is_dude, Attack, DudeInfo};
use crate::resources::Resources;
use crate::world::stat;
use ob_engine::collide;

/// Zombies that start buried or lying down and rise when the player comes close.
pub fn starts_buried(t: i16) -> bool {
    matches!(t, 205 | 244)
}

/// Stone/flesh statues that stay inert until shot or triggered.
pub fn is_statue(t: i16) -> bool {
    matches!(t, 208 | 209)
}

/// Idle dudes only wander while the player is beyond their sight range.
const WANDER_DISTANCE: f64 = 12000.0;

/// Distance at which ranged attackers stop advancing and start strafing.
const KEEP_DISTANCE: f64 = 2600.0;

impl Game {
    /// Initial AI state for a dude placed in the map.
    pub(super) fn initial_ai(&self, i: usize) -> Ai {
        let t = self.world.sprites[i].lotag;
        if starts_buried(t) || is_statue(t) {
            Ai::Dormant
        } else {
            Ai::Idle
        }
    }

    /// True when the dude was placed as an ambusher or guard and has not been provoked.
    fn holds_position(&self, i: usize) -> bool {
        !self.provoked[i] && self.world.xsprite(i).is_some_and(|x| x.dude_ambush || x.dude_guard)
    }

    pub(super) fn tick_dudes(&mut self, res: &Resources, dt: f64) {
        let (px, py) = (self.player.x, self.player.y);
        for i in 0..self.world.sprites.len() {
            let s = self.world.sprites[i].clone();
            if s.statnum == stat::FREE || !is_dude(s.lotag) {
                continue;
            }
            let Some(info) = dude_info(s.lotag) else { continue };
            let ai = self.ai[i];
            if matches!(ai, Ai::Dead | Ai::Dying) {
                self.dude_gravity(i, &info, dt);
                continue;
            }
            self.attack_cd[i] = (self.attack_cd[i] - dt).max(0.0);
            if self.tick_burning(res, i, &info, dt) {
                continue;
            }
            let dist = (px - s.x as f64).hypot(py - s.y as f64);
            match ai {
                Ai::Dormant => {
                    if starts_buried(s.lotag)
                        && !self.player.dead
                        && dist < 1400.0
                        && (self.clock as usize + i).is_multiple_of(10)
                        && self.can_see_player(i)
                    {
                        self.rise(res, i);
                    }
                }
                Ai::Rise | Ai::Attack | Ai::Recoil => self.dude_gravity(i, &info, dt),
                Ai::Idle => {
                    self.idle_wander(res, i, &info, dist, dt);
                    if self.player.dead || dist > 12000.0 || !(self.clock as usize + i).is_multiple_of(15) {
                        continue;
                    }
                    let to = (py - s.y as f64).atan2(px - s.x as f64).to_degrees() / 360.0 * 2048.0;
                    let rel = (to - s.ang as f64 + 1024.0).rem_euclid(2048.0) - 1024.0;
                    if (rel.abs() < 400.0 || dist < 1500.0) && self.can_see_player(i) {
                        self.wake(res, i, &info);
                    }
                }
                Ai::Flee => {
                    self.flee_step(i, &info, dt);
                    self.dude_gravity(i, &info, dt);
                }
                Ai::Chase => {
                    if self.player.dead {
                        self.ai[i] = Ai::Idle;
                        self.play_dude_anim(res, i, "I1");
                        continue;
                    }
                    self.chase_step(res, i, &info, dist, dt);
                }
                Ai::Dying | Ai::Dead => {}
            }
        }
    }

    fn chase_step(&mut self, res: &Resources, i: usize, info: &DudeInfo, dist: f64, dt: f64) {
        let (px, py) = (self.player.x, self.player.y);
        let s = &self.world.sprites[i];
        let to = (py - s.y as f64).atan2(px - s.x as f64);
        self.world.sprites[i].ang = ((to.to_degrees() / 360.0 * 2048.0).rem_euclid(2048.0)) as i16;
        let in_range = match info.attack {
            Attack::Melee { range, .. } => dist < range + info.radius + crate::player::RADIUS,
            Attack::Hitscan { range, .. } | Attack::Projectile { range, .. } => dist < range,
            Attack::None => false,
        };
        // Sight is re-checked every few ticks and cached in `sees`.
        if (self.clock as usize + i).is_multiple_of(6) {
            self.sees[i] = self.can_see_player(i);
        }
        let sees = self.sees[i];
        if in_range && sees && self.attack_cd[i] <= 0.0 && info.attack != Attack::None {
            self.ai[i] = Ai::Attack;
            self.attack_cd[i] = info.cooldown * (0.8 + 0.4 * (self.rand() % 100) as f64 / 100.0);
            if !self.play_dude_anim(res, i, "A1") {
                self.dude_attack(res, i);
                self.ai[i] = Ai::Chase;
            }
            return;
        }
        if info.speed > 0.0 && !self.holds_position(i) {
            let ranged = matches!(info.attack, Attack::Hitscan { .. } | Attack::Projectile { .. });
            let (ang, speed) = if ranged && sees && dist < KEEP_DISTANCE {
                // Strafe around the player instead of walking into the line of fire.
                self.world.rt[i].ai_timer -= 1;
                if self.world.rt[i].ai_timer <= 0 {
                    self.world.rt[i].ai_timer = (TICK_RATE * (0.8 + (self.rand() % 70) as f64 / 100.0)) as i32;
                    self.world.rt[i].ai_state = if self.rand().is_multiple_of(2) { 1 } else { -1 };
                }
                let side = self.world.rt[i].ai_state.signum().max(-1) as f64;
                let side = if side == 0.0 { 1.0 } else { side };
                (to + side * std::f64::consts::FRAC_PI_2, info.speed * 0.6)
            } else {
                (to, info.speed)
            };
            let close =
                matches!(info.attack, Attack::Melee { .. }) && dist < info.radius + crate::player::RADIUS + 16.0;
            if !close {
                self.dude_walk(i, info, ang, speed * dt);
            }
        }
        if info.flying {
            let target = self.player.z - 0x2000 as f64;
            let s = &mut self.world.sprites[i];
            let dz = (target - s.z as f64).clamp(-0x6000 as f64 * dt, 0x6000 as f64 * dt);
            s.z += dz as i32;
        }
        self.dude_gravity(i, info, dt);
    }

    /// Moves a dude `dist` units toward `ang`, sliding around obstacles and refusing to
    /// step off high ledges. Returns true if it moved.
    fn dude_walk(&mut self, i: usize, info: &DudeInfo, ang: f64, dist: f64) -> bool {
        let m = Self::dude_mover(info);
        for k in [0.0, 0.7, -0.7, 1.4, -1.4] {
            let a = ang + k;
            let s = &self.world.sprites[i];
            let (x, y, z) = (s.x as f64, s.y as f64, s.z as f64);
            let (nx, ny, ns, _) = collide::clip_move(
                &self.world.sectors,
                &self.world.walls,
                &[],
                x,
                y,
                z,
                s.sectnum as i32,
                a.cos() * dist,
                a.sin() * dist,
                &m,
            );
            if ns < 0 {
                continue;
            }
            let moved = (nx - x).hypot(ny - y);
            if moved < dist * 0.3 {
                continue;
            }
            if !info.flying {
                let (_, f) = self.world.zs_at(ns as usize, nx, ny);
                if f - z > 0x3000 as f64 {
                    continue; // don't walk off cliffs
                }
            }
            if self.crowded(i, info, x, y, nx, ny) {
                continue;
            }
            let s = &mut self.world.sprites[i];
            s.x = nx.round() as i32;
            s.y = ny.round() as i32;
            s.sectnum = ns as i16;
            if k != 0.0 {
                s.ang = ((a.to_degrees() / 360.0 * 2048.0).rem_euclid(2048.0)) as i16;
            }
            return true;
        }
        false
    }

    /// True when moving dude `i` from (x, y) to (nx, ny) pushes it further into another
    /// live dude or the player (dudes don't walk through each other).
    fn crowded(&self, i: usize, info: &DudeInfo, x: f64, y: f64, nx: f64, ny: f64) -> bool {
        let into = |ox: f64, oy: f64, r: f64| {
            let d1 = (nx - ox).hypot(ny - oy);
            d1 < r && d1 < (x - ox).hypot(y - oy)
        };
        if !self.player.dead && into(self.player.x, self.player.y, info.radius + crate::player::RADIUS) {
            return true;
        }
        let z = self.world.sprites[i].z;
        self.world.sprites.iter().enumerate().any(|(j, o)| {
            j != i
                && o.statnum != stat::FREE
                && is_dude(o.lotag)
                && !matches!(self.ai[j], Ai::Dead | Ai::Dying)
                && (o.z - z).abs() < 0x3000
                && dude_info(o.lotag).is_some_and(|oi| into(o.x as f64, o.y as f64, info.radius + oi.radius))
        })
    }

    /// Unalerted dudes don't stand frozen: they take a few slow steps around their spawn
    /// point, pause, and wander again. Ambushers, statues and sleepers stay put.
    fn idle_wander(&mut self, res: &Resources, i: usize, info: &DudeInfo, dist: f64, dt: f64) {
        if info.speed <= 0.0 || info.flying || self.holds_position(i) {
            return;
        }
        // Close to the player: stand and face the way the map placed us (sight checks
        // look along the facing).
        if dist < WANDER_DISTANCE {
            if self.world.rt[i].ai_state == 1 {
                self.world.rt[i].ai_state = 0;
                self.world.rt[i].ai_timer = 0;
                self.world.sprites[i].ang = self.world.rt[i].home_ang;
                self.play_dude_anim(res, i, "I1");
            }
            return;
        }
        let rt = &mut self.world.rt[i];
        rt.ai_timer -= 1;
        if rt.ai_timer <= 0 {
            if rt.ai_state == 1 {
                // Pause.
                self.world.rt[i].ai_state = 0;
                self.world.rt[i].ai_timer = (TICK_RATE * (1.0 + (self.rand() % 200) as f64 / 100.0)) as i32;
                self.play_dude_anim(res, i, "I1");
            } else {
                // Walk a short way: back toward home when strayed, else somewhere nearby.
                let s = &self.world.sprites[i];
                let (hx, hy, _) = self.world.base_sprite_xy.get(i).copied().unwrap_or((s.x, s.y, s.z));
                let (dx, dy) = ((hx - s.x) as f64, (hy - s.y) as f64);
                let a = if dx.hypot(dy) > 384.0 {
                    dy.atan2(dx)
                } else {
                    (self.rand() % 360) as f64 / 180.0 * std::f64::consts::PI
                };
                self.world.sprites[i].ang = ((a.to_degrees() / 360.0 * 2048.0).rem_euclid(2048.0)) as i16;
                self.world.rt[i].ai_state = 1;
                self.world.rt[i].ai_timer = (TICK_RATE * (0.5 + (self.rand() % 70) as f64 / 100.0)) as i32;
                self.play_dude_anim(res, i, "M1");
            }
        }
        if self.world.rt[i].ai_state == 1 {
            let a = ob_engine::geom::ang_to_rad(self.world.sprites[i].ang as f64);
            if !self.dude_walk(i, info, a, info.speed * 0.25 * dt) {
                self.world.rt[i].ai_timer = 0;
            }
        }
        self.dude_gravity(i, info, dt);
    }

    /// Running away from the player (innocents, burning dudes).
    fn flee_step(&mut self, i: usize, info: &DudeInfo, dt: f64) {
        let s = &self.world.sprites[i];
        let away = (s.y as f64 - self.player.y).atan2(s.x as f64 - self.player.x);
        self.world.rt[i].ai_timer -= 1;
        if self.world.rt[i].ai_timer <= 0 {
            self.world.rt[i].ai_timer = (TICK_RATE * (0.6 + (self.rand() % 100) as f64 / 100.0)) as i32;
            self.world.rt[i].ai_state = (self.rand() % 120) as i32 - 60;
        }
        let jitter = self.world.rt[i].ai_state as f64 / 60.0 * 0.9;
        let a = away + jitter;
        self.world.sprites[i].ang = ((a.to_degrees() / 360.0 * 2048.0).rem_euclid(2048.0)) as i16;
        let speed = info.speed.max(1500.0) * 1.2;
        self.dude_walk(i, info, a, speed * dt);
    }

    /// Burning dudes panic and take damage. Returns true when burning handled the tick.
    fn tick_burning(&mut self, res: &Resources, i: usize, info: &DudeInfo, dt: f64) -> bool {
        if self.world.rt[i].burn_time <= 0 {
            return false;
        }
        self.world.rt[i].burn_time -= 1;
        let p = self.sprite_pos(i);
        if self.world.rt[i].burn_time % 30 == 0 {
            let sect = self.world.sprites[i].sectnum as i32;
            self.spawn_fx(55, p.0, p.1, p.2 - info.height * 0.4, sect);
            self.burn_kill = true;
            self.damage_sprite(res, i, 3, false);
            self.burn_kill = false;
        }
        if matches!(self.ai[i], Ai::Dead | Ai::Dying) {
            self.world.rt[i].burn_time = 0;
            return true;
        }
        if self.world.rt[i].burn_time % 90 == 0 {
            self.sound_group(info.pain_sound, p);
        }
        self.flee_step(i, info, dt);
        self.dude_gravity(i, info, dt);
        true
    }

    fn rise(&mut self, res: &Resources, i: usize) {
        if self.play_dude_anim(res, i, "UP") {
            self.ai[i] = Ai::Rise;
        } else {
            self.ai[i] = Ai::Chase;
            self.play_dude_anim(res, i, "M1");
        }
        let p = self.sprite_pos(i);
        self.sound(1100, Some(p));
    }

    pub(super) fn wake(&mut self, res: &Resources, i: usize, info: &DudeInfo) {
        match self.ai[i] {
            Ai::Idle => {}
            Ai::Dormant => {
                if starts_buried(self.world.sprites[i].lotag) {
                    self.rise(res, i);
                    return;
                }
            }
            _ => return,
        }
        let p = self.sprite_pos(i);
        if info.attack == Attack::None {
            self.ai[i] = Ai::Flee;
            self.play_dude_anim(res, i, "M1");
            self.sound_group(info.spot_sound, p);
            return;
        }
        self.ai[i] = Ai::Chase;
        self.play_dude_anim(res, i, "M1");
        self.sound_group(info.spot_sound, p);
    }

    /// Wakes dudes that can hear the player's weapon (deaf dudes ignore noise).
    pub(super) fn alert_nearby(&mut self, res: &Resources, radius: f64) {
        for i in 0..self.world.sprites.len() {
            let s = &self.world.sprites[i];
            if s.statnum == stat::FREE || !is_dude(s.lotag) || self.ai[i] != Ai::Idle {
                continue;
            }
            if self.world.xsprite(i).is_some_and(|x| x.dude_deaf) {
                continue;
            }
            let d = (s.x as f64 - self.player.x).hypot(s.y as f64 - self.player.y);
            if d < radius && (d < radius * 0.4 || self.can_see_player(i)) {
                if let Some(info) = dude_info(s.lotag) {
                    self.wake(res, i, &info);
                }
            }
        }
    }

    /// Called when a dude is hurt: provokes ambushers, wakes statues and sleepers.
    pub(super) fn provoke(&mut self, res: &Resources, i: usize, info: &DudeInfo) {
        self.provoked[i] = true;
        if self.ai[i] == Ai::Dormant && is_statue(self.world.sprites[i].lotag) {
            self.ai[i] = Ai::Idle;
        }
        if matches!(self.ai[i], Ai::Idle | Ai::Dormant) {
            self.wake(res, i, info);
        }
    }
}
