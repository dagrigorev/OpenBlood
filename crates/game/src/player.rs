//! Player movement and state.

use crate::world::World;
use ob_engine::collide::{self, CircleObstacle, Hit, Mover};
use ob_engine::geom::ang_to_rad;

/// One frame of player input.
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    /// -1..1 forward/back.
    pub forward: f64,
    /// -1..1 strafe right/left.
    pub strafe: f64,
    /// Turn delta in Build angle units.
    pub turn: f64,
    /// Look delta (200-line units, positive = look up).
    pub look: f64,
    pub run: bool,
    pub jump: bool,
    pub crouch: bool,
    pub use_key: bool,
    pub fire: bool,
    pub alt_fire: bool,
    pub weapon: Option<u8>,
    pub center_view: bool,
    pub inv_prev: bool,
    pub inv_next: bool,
    pub inv_use: bool,
}

pub const EYE_STAND: f64 = 0x2c00 as f64;
pub const EYE_CROUCH: f64 = 0x1400 as f64;
pub const HEIGHT_STAND: f64 = 0x3400 as f64;
pub const HEIGHT_CROUCH: f64 = 0x1c00 as f64;
pub const RADIUS: f64 = 64.0;
pub const STEP: f64 = 0x1a00 as f64;
/// z units / s^2. With [`JUMP_VEL`] a jump lasts 0.8 s and peaks 16000 units up, as measured
/// in the original (DOSBox video, E1M1 coffin).
pub const GRAVITY: f64 = 200_000.0;
pub const JUMP_VEL: f64 = 80_000.0;
/// Fall speed scaled to what it would be under the old 0x10000 gravity, so fall damage
/// keeps depending on the same fall heights.
pub fn legacy_fall_speed(v: f64) -> f64 {
    v * (0x10000 as f64 / GRAVITY).sqrt()
}
/// Top speeds and responsiveness measured from the original (DOSBox video, E1M1 start).
pub const WALK_SPEED: f64 = 4600.0;
pub const RUN_SPEED: f64 = 5600.0;
pub const SWIM_SPEED: f64 = 2400.0;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Player {
    pub x: f64,
    pub y: f64,
    /// Feet z.
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub ang: f64,
    /// Look offset in 200-line units (positive = up).
    pub horiz: f64,
    pub sector: i32,
    pub on_ground: bool,
    pub crouching: bool,
    pub eye: f64,
    pub health: i32,
    pub armor: [i32; 3],
    pub dead: bool,
    pub bob_phase: f64,
    pub bob_amp: f64,
    pub underwater: bool,
    pub keys: [bool; 8],
    pub use_was_down: bool,
    pub jump_was_down: bool,
    pub last_hit: Hit,
    pub sprite: i32,
    pub fall_speed_peak: f64,
    /// Jump strength multiplier (jumping boots).
    pub jump_scale: f64,
}

impl Player {
    pub fn spawn(world: &World) -> Player {
        let (x, y, z, ang, sect) = match world.player_start(0) {
            Some(s) => (s.x as f64, s.y as f64, s.z as f64, s.ang as f64, s.sectnum as i32),
            None => (0.0, 0.0, 0.0, 0.0, 0),
        };
        let mut p = Player {
            x,
            y,
            z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            ang: ang.rem_euclid(2048.0),
            horiz: 0.0,
            sector: sect,
            on_ground: true,
            crouching: false,
            eye: EYE_STAND,
            health: 100,
            armor: [0; 3],
            dead: false,
            bob_phase: 0.0,
            bob_amp: 0.0,
            underwater: false,
            keys: [false; 8],
            use_was_down: false,
            jump_was_down: false,
            last_hit: Hit::None,
            sprite: -1,
            fall_speed_peak: 0.0,
            jump_scale: 1.0,
        };
        if sect >= 0 {
            let (_, f) = world.zs_at(sect as usize, x, y);
            p.z = f;
        }
        p
    }

    pub fn height(&self) -> f64 {
        if self.crouching {
            HEIGHT_CROUCH
        } else {
            HEIGHT_STAND
        }
    }

    pub fn mover(&self) -> Mover {
        Mover {
            radius: RADIUS,
            height: self.height(),
            // Full step height in the air too, so a jump can mantle onto a ledge (climbing
            // out of the E1M1 coffin works this way in the original).
            step: STEP,
        }
    }

    /// Camera z (eye position) including view bob.
    pub fn view_z(&self) -> f64 {
        let bob = (self.bob_phase).sin() * self.bob_amp;
        self.z - self.eye + bob
    }

    /// Advances movement by `dt` seconds.
    pub fn update(&mut self, world: &World, input: &Input, obstacles: &[CircleObstacle], dt: f64) {
        if self.dead {
            self.eye = (self.eye - 0x4000 as f64 * dt).max(0x400 as f64);
            self.apply_gravity(world, dt);
            return;
        }
        self.ang = (self.ang + input.turn).rem_euclid(2048.0);
        self.horiz = (self.horiz + input.look).clamp(-100.0, 100.0);
        if input.center_view {
            self.horiz = 0.0;
        }

        self.crouching = input.crouch;
        let target_eye = if self.crouching { EYE_CROUCH } else { EYE_STAND };
        self.eye += (target_eye - self.eye) * (dt * 12.0).min(1.0);

        // Desired horizontal velocity.
        let a = ang_to_rad(self.ang);
        let (c, s) = (a.cos(), a.sin());
        let speed = if self.underwater {
            SWIM_SPEED
        } else if input.run {
            RUN_SPEED
        } else {
            WALK_SPEED
        } * if self.crouching { 0.5 } else { 1.0 };
        let fwd = input.forward.clamp(-1.0, 1.0);
        let strafe = input.strafe.clamp(-1.0, 1.0);
        let wx = (c * fwd - s * strafe) * speed;
        let wy = (s * fwd + c * strafe) * speed;
        // Ground control vs air control.
        let accel = if self.on_ground || self.underwater { 20.0 } else { 3.0 };
        let k = (accel * dt).min(1.0);
        self.vx += (wx - self.vx) * k;
        self.vy += (wy - self.vy) * k;

        // Jump / swim up.
        if self.underwater {
            if input.jump {
                self.vz = -0x3000 as f64;
            } else if input.crouch {
                self.vz = 0x3000 as f64;
            } else {
                self.vz *= 1.0 - (dt * 3.0).min(1.0);
            }
        } else if input.jump && !self.jump_was_down && self.on_ground {
            self.vz = -JUMP_VEL * self.jump_scale;
            self.on_ground = false;
        }
        self.jump_was_down = input.jump;

        // Horizontal move with collision.
        let m = self.mover();
        let (nx, ny, ns, hit) = collide::clip_move(
            &world.sectors,
            &world.walls,
            obstacles,
            self.x,
            self.y,
            self.z,
            self.sector,
            self.vx * dt,
            self.vy * dt,
            &m,
        );
        if ns >= 0 {
            // Kill velocity into walls to avoid jitter.
            let mdx = nx - self.x;
            let mdy = ny - self.y;
            if dt > 0.0 && hit != Hit::None {
                self.vx = mdx / dt;
                self.vy = mdy / dt;
            }
            self.x = nx;
            self.y = ny;
            self.sector = ns;
        }
        self.last_hit = hit;

        let moving = (self.vx * self.vx + self.vy * self.vy).sqrt();
        if self.on_ground && moving > 50.0 {
            self.bob_phase += dt * moving / 450.0;
            self.bob_amp = (moving / RUN_SPEED * 384.0).min(384.0);
        } else {
            self.bob_amp *= 1.0 - (dt * 8.0).min(1.0);
        }

        self.apply_gravity(world, dt);
    }

    fn apply_gravity(&mut self, world: &World, dt: f64) {
        if self.sector < 0 {
            return;
        }
        let (mut cz, cs, mut fz, fs) = collide::get_z_range(
            &world.sectors,
            &world.walls,
            self.x,
            self.y,
            self.z,
            self.sector,
            RADIUS,
            STEP,
        );
        // Floors/ceilings linked to another room are passable.
        if fs >= 0 && world.links.get(fs as usize).is_some_and(|l| l.floor.is_some()) {
            fz = f64::MAX / 4.0;
        }
        if cs >= 0 && world.links.get(cs as usize).is_some_and(|l| l.ceil.is_some()) {
            cz = f64::MIN / 4.0;
        }
        if !self.underwater {
            self.vz += GRAVITY * dt;
        }
        self.z += self.vz * dt;
        if self.z >= fz {
            if self.vz > self.fall_speed_peak {
                self.fall_speed_peak = self.vz;
            }
            // Smooth step-up instead of snapping.
            if self.on_ground && fz < self.z - 1.0 {
                self.z += (fz - self.z) * (dt * 20.0).min(1.0);
            } else {
                self.z = fz;
            }
            if self.vz > 0.0 {
                self.vz = 0.0;
            }
            self.on_ground = true;
        } else {
            self.on_ground = self.z >= fz - 16.0;
        }
        let head = self.z - self.height();
        if head < cz {
            self.z = (cz + self.height()).min(fz);
            if self.vz < 0.0 {
                self.vz = 0.0;
            }
        }
    }
}
