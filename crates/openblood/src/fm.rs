//! Two-operator FM voices in the style of the Yamaha OPL2/OPL3 chips, used to play Blood's
//! MIDI music with its AdLib timbre bank when no CD audio was ripped.
//!
//! Written from the chips' publicly documented behaviour, not from any emulator's code and
//! not cycle-exact: each operator has a phase generator (frequency multiplier, vibrato), an
//! envelope (attack / decay / sustain / release rates in the OPL's 4-step-per-octave rate
//! scale, key-scale rate), total level and key-scale level attenuation, tremolo and one of
//! eight OPL3 waveforms. The modulator can feed back into itself and either phase-modulates
//! the carrier (FM) or is mixed with it (additive).

use std::f64::consts::TAU;

/// One instrument from a 13-byte timbre record: the register values an AdLib driver writes
/// to registers 0x20, 0x40, 0x60, 0x80 and 0xE0 for the modulator and carrier, register 0xC0
/// (feedback and connection), a transpose and a velocity sensitivity.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timbre {
    /// [modulator, carrier]: AM, vibrato, sustaining envelope, KSR, frequency multiplier.
    pub flags: [u8; 2],
    /// [modulator, carrier]: key-scale level (bits 6-7) and total level (bits 0-5).
    pub level: [u8; 2],
    /// [modulator, carrier]: attack rate (high nibble) and decay rate (low nibble).
    pub attack_decay: [u8; 2],
    /// [modulator, carrier]: sustain level (high nibble) and release rate (low nibble).
    pub sustain_release: [u8; 2],
    /// [modulator, carrier]: waveform 0-7.
    pub wave: [u8; 2],
    /// Feedback (bits 1-3) and connection (bit 0: 0 = FM, 1 = additive).
    pub feedback: u8,
    /// Semitones added to melodic notes; the note played for a percussion timbre.
    pub transpose: i8,
    pub velocity: i8,
}

impl Timbre {
    pub const SIZE: usize = 13;

    pub fn parse(b: &[u8; Self::SIZE]) -> Timbre {
        Timbre {
            flags: [b[0], b[1]],
            level: [b[2], b[3]],
            attack_decay: [b[4], b[5]],
            sustain_release: [b[6], b[7]],
            wave: [b[8], b[9]],
            feedback: b[10],
            transpose: b[11] as i8,
            velocity: b[12] as i8,
        }
    }

    fn additive(&self) -> bool {
        self.feedback & 1 != 0
    }
}

/// Frequency multipliers for register values 0-15.
const MULT: [f64; 16] = [
    0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 8.0, 10.0, 10.0, 12.0, 12.0, 15.0, 15.0,
];
/// Time for a full attack (silence to full level) at rate 1, in seconds.
const ATTACK_RATE1: f64 = 2.82624;
/// Time for a full decay or release (0 to -96 dB) at rate 1, in seconds.
const DECAY_RATE1: f64 = 39.28064;
/// Envelope floor: below this the operator is silent.
const SILENT_DB: f64 = 96.0;
const TREMOLO_HZ: f64 = 3.7;
const TREMOLO_DB: f64 = 1.0;
const VIBRATO_HZ: f64 = 6.1;
/// Vibrato depth as a frequency ratio (about 7 cents).
const VIBRATO_DEPTH: f64 = 0.004;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

/// Seconds a stage of rate `rate` (0-15) takes, with the key-scale rate offset `rof` (0-15).
/// Rate 0 never advances; every four effective-rate steps halve the time.
fn stage_time(rate: u8, rof: u8, rate1: f64) -> f64 {
    if rate == 0 {
        return f64::INFINITY;
    }
    let eff = (4 * rate as u32 + rof as u32).min(63) as f64;
    rate1 / 2f64.powf((eff - 4.0) / 4.0)
}

/// Waveform `w` at phase `p` (cycles, 0..1).
fn wave(w: u8, p: f64) -> f64 {
    let s = (TAU * p).sin();
    match w & 7 {
        0 => s,
        1 => s.max(0.0),
        2 => s.abs(),
        3 => {
            if p.fract() % 0.5 < 0.25 {
                s.abs()
            } else {
                0.0
            }
        }
        4 => {
            if p < 0.5 {
                (2.0 * TAU * p).sin()
            } else {
                0.0
            }
        }
        5 => {
            if p < 0.5 {
                (2.0 * TAU * p).sin().abs()
            } else {
                0.0
            }
        }
        6 => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        _ => {
            // Exponentially falling half-cycles, mirrored.
            let h = (p * 2.0).fract();
            let v = 2f64.powf(-8.0 * h);
            if p < 0.5 {
                v
            } else {
                -v
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Operator {
    phase: f64,
    stage: Stage,
    /// Current envelope attenuation in dB (0 = full level, 96 = silent).
    env_db: f64,
    /// Linear 0..1 progress of the attack.
    attack: f64,
    attack_step: f64,
    decay_db_per_s: f64,
    release_db_per_s: f64,
    sustain_db: f64,
    sustaining: bool,
    /// Static attenuation (total level + key-scale level), dB.
    base_db: f64,
    mult: f64,
    am: bool,
    vib: bool,
    wave: u8,
}

impl Operator {
    fn new(t: &Timbre, k: usize, block: u8, octave: f64, rate: f64) -> Operator {
        let flags = t.flags[k];
        let ksr = flags & 0x10 != 0;
        let rof = if ksr { block * 2 } else { block / 2 };
        let ar = t.attack_decay[k] >> 4;
        let dr = t.attack_decay[k] & 15;
        let sl = t.sustain_release[k] >> 4;
        let rr = t.sustain_release[k] & 15;
        let attack_time = if ar == 15 {
            0.0
        } else {
            stage_time(ar, rof, ATTACK_RATE1)
        };
        let per_s = |time: f64| if time.is_finite() { SILENT_DB / time } else { 0.0 };
        // Key-scale level: attenuation grows with pitch at 3, 1.5 or 6 dB per octave.
        let ksl = [0.0, 1.0, 0.5, 2.0][(t.level[k] >> 6) as usize] * 3.0 * (octave - 1.0).max(0.0);
        Operator {
            phase: 0.0,
            stage: Stage::Attack,
            env_db: SILENT_DB,
            attack: 0.0,
            attack_step: if attack_time > 0.0 {
                1.0 / (attack_time * rate)
            } else {
                1.0
            },
            decay_db_per_s: per_s(stage_time(dr, rof, DECAY_RATE1)),
            release_db_per_s: per_s(stage_time(rr, rof, DECAY_RATE1)),
            sustain_db: if sl == 15 { 93.0 } else { sl as f64 * 3.0 },
            sustaining: flags & 0x20 != 0,
            base_db: (t.level[k] & 63) as f64 * 0.75 + ksl,
            mult: MULT[(flags & 15) as usize],
            am: flags & 0x80 != 0,
            vib: flags & 0x40 != 0,
            wave: t.wave[k] & 7,
        }
    }

    /// Advances the envelope by one sample of length `dt`.
    fn step_envelope(&mut self, dt: f64) {
        match self.stage {
            Stage::Attack => {
                self.attack = (self.attack + self.attack_step).min(1.0);
                // Linear in amplitude, expressed as attenuation.
                self.env_db = if self.attack >= 1.0 {
                    0.0
                } else {
                    (-20.0 * self.attack.max(1e-5).log10()).min(SILENT_DB)
                };
                if self.attack >= 1.0 {
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.env_db += self.decay_db_per_s * dt;
                if self.env_db >= self.sustain_db {
                    self.env_db = self.sustain_db;
                    self.stage = if self.sustaining {
                        Stage::Sustain
                    } else {
                        Stage::Release
                    };
                }
            }
            Stage::Sustain => {}
            Stage::Release => {
                self.env_db += self.release_db_per_s * dt;
                if self.env_db >= SILENT_DB {
                    self.env_db = SILENT_DB;
                    self.stage = Stage::Off;
                }
            }
            Stage::Off => {}
        }
    }

    fn key_off(&mut self) {
        if self.stage != Stage::Off {
            self.stage = Stage::Release;
        }
    }

    /// Output in -1..1 for phase offset `pm` (cycles); advances the phase by `inc` cycles.
    fn output(&mut self, inc: f64, pm: f64, extra_db: f64, trem_db: f64) -> f64 {
        let db = self.base_db + self.env_db + extra_db + if self.am { trem_db } else { 0.0 };
        let out = if db >= SILENT_DB {
            0.0
        } else {
            wave(self.wave, (self.phase + pm).rem_euclid(1.0)) * 10f64.powf(-db / 20.0)
        };
        self.phase = (self.phase + inc * self.mult).fract();
        out
    }
}

/// One sounding note: a modulator and a carrier.
#[derive(Debug, Clone, Copy)]
pub struct Voice {
    ops: [Operator; 2],
    /// Fundamental frequency before pitch bend, Hz.
    pub freq: f64,
    /// Frequency ratio from pitch bend.
    pub bend: f64,
    feedback: u8,
    additive: bool,
    /// Last two modulator outputs, for feedback.
    fb_hist: [f64; 2],
    /// Extra carrier attenuation from note velocity and channel volume, dB.
    pub volume_db: f64,
    pub gain_l: f64,
    pub gain_r: f64,
}

impl Voice {
    /// Starts a note at `freq` Hz; `volume` (0..1) attenuates the carrier (and the modulator
    /// for additive timbres).
    pub fn new(t: &Timbre, freq: f64, volume: f64, rate: f64) -> Voice {
        let octave = (freq / 32.70).log2().clamp(0.0, 8.0);
        let block = octave.floor().clamp(0.0, 7.0) as u8;
        let mut v = Voice {
            ops: [
                Operator::new(t, 0, block, octave, rate),
                Operator::new(t, 1, block, octave, rate),
            ],
            freq,
            bend: 1.0,
            feedback: (t.feedback >> 1) & 7,
            additive: t.additive(),
            fb_hist: [0.0; 2],
            volume_db: 0.0,
            gain_l: 1.0,
            gain_r: 1.0,
        };
        v.set_volume(volume);
        v
    }

    /// Volume 0..1 (velocity times channel volume and expression) on the General MIDI curve,
    /// 40 log10(volume) dB, on top of the timbre's own total level.
    pub fn set_volume(&mut self, volume: f64) {
        self.volume_db = (-40.0 * volume.max(1e-6).log10()).min(SILENT_DB);
    }

    pub fn key_off(&mut self) {
        for op in &mut self.ops {
            op.key_off();
        }
    }

    pub fn is_off(&self) -> bool {
        self.ops[1].stage == Stage::Off && (!self.additive || self.ops[0].stage == Stage::Off)
    }

    pub fn is_released(&self) -> bool {
        matches!(self.ops[1].stage, Stage::Release | Stage::Off)
    }

    /// Current carrier attenuation (for choosing which voice to steal).
    pub fn level_db(&self) -> f64 {
        self.ops[1].env_db
    }

    /// Renders one sample. `t` is the song time in seconds (for tremolo and vibrato).
    pub fn sample(&mut self, rate: f64, t: f64) -> f64 {
        let dt = 1.0 / rate;
        let trem_db = TREMOLO_DB * 0.5 * (1.0 - (TAU * TREMOLO_HZ * t).cos());
        let vib = 1.0 + VIBRATO_DEPTH * (TAU * VIBRATO_HZ * t).sin();
        let base_inc = self.freq * self.bend / rate;
        let fb = if self.feedback == 0 {
            0.0
        } else {
            (self.fb_hist[0] + self.fb_hist[1]) * 4.0 / 2f64.powi(9 - self.feedback as i32)
        };
        let [m, c] = &mut self.ops;
        m.step_envelope(dt);
        c.step_envelope(dt);
        let inc_m = if m.vib { base_inc * vib } else { base_inc };
        let inc_c = if c.vib { base_inc * vib } else { base_inc };
        let mod_db = if self.additive { self.volume_db } else { 0.0 };
        let mo = m.output(inc_m, fb, mod_db, trem_db);
        self.fb_hist = [self.fb_hist[1], mo];
        if self.additive {
            mo + c.output(inc_c, 0.0, self.volume_db, trem_db)
        } else {
            // A full-level modulator shifts the carrier's phase by up to four cycles.
            c.output(inc_c, mo * 4.0, self.volume_db, trem_db)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn organ() -> Timbre {
        Timbre::parse(&[0x21, 0x21, 0x3f, 0x00, 0xf0, 0xf0, 0x00, 0x0f, 0, 0, 0x01, 0, 0])
    }

    #[test]
    fn rate_halves_every_four_steps() {
        let a = stage_time(4, 0, DECAY_RATE1);
        let b = stage_time(5, 0, DECAY_RATE1);
        assert!((a / b - 2.0).abs() < 1e-9);
        assert!(stage_time(0, 0, DECAY_RATE1).is_infinite());
    }

    #[test]
    fn sustained_carrier_plays_the_note_frequency() {
        // Additive timbre with a silent modulator: a plain sine at the note's frequency.
        let rate = 44100.0;
        let mut v = Voice::new(&organ(), 441.0, 1.0, rate);
        let s: Vec<f64> = (0..44100).map(|i| v.sample(rate, i as f64 / rate)).collect();
        let tail = &s[22050..];
        let crossings = tail.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!(
            (crossings as i32 - 220).abs() <= 2,
            "{crossings} rising crossings in 0.5 s"
        );
        let peak = tail.iter().fold(0f64, |a, &x| a.max(x.abs()));
        // The modulator at total level 63 (-47 dB) adds a little on top.
        assert!(peak > 0.9 && peak <= 1.01, "peak {peak}");
    }

    #[test]
    fn release_reaches_silence() {
        let rate = 8000.0;
        let mut t = organ();
        t.sustain_release = [0x0f, 0x0f];
        let mut v = Voice::new(&t, 220.0, 1.0, rate);
        for i in 0..800 {
            v.sample(rate, i as f64 / rate);
        }
        v.key_off();
        for i in 0..8000 {
            v.sample(rate, i as f64 / rate);
        }
        assert!(v.is_off());
    }

    #[test]
    fn volume_follows_the_general_midi_curve() {
        let mut v = Voice::new(&organ(), 220.0, 0.5, 8000.0);
        assert!((v.volume_db - 12.04).abs() < 0.01, "{}", v.volume_db);
        v.set_volume(1.0);
        assert_eq!(v.volume_db, 0.0);
    }

    #[test]
    fn waveforms_stay_in_range() {
        for w in 0..8 {
            for i in 0..1000 {
                let x = wave(w, i as f64 / 1000.0);
                assert!((-1.0..=1.0).contains(&x), "wave {w}");
            }
        }
    }
}
