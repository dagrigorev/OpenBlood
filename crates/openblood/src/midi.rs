//! MIDI music: Standard MIDI Files with Apogee's EMIDI extensions, played with the FM voices
//! in [`crate::fm`] and Blood's AdLib timbre bank (`GMTIMBRE.TMB` in SOUNDS.RFF). Used for
//! a level's `Song=` (BLOOD.INI) when no CD audio track was ripped.
//!
//! EMIDI controllers handled (as publicly documented for Apogee's sound system):
//! * 110 / 111 at the start of a track: include / exclude the track for a device
//!   (OpenBlood plays the FM variants, devices [`FM_DEVICES`], or "all" = 127);
//! * 112 / 113: device-specific program change and volume;
//! * 116 / 117: song loop start / end; 118 / 119: track loop start / end.
//!
//! Without loop markers a song restarts from the beginning when every track has ended.

use crate::fm::{Timbre, Voice};

/// EMIDI device numbers treated as "this player": Sound Blaster and AdLib (both OPL FM).
pub const FM_DEVICES: [u8; 2] = [4, 7];
const EMIDI_ALL: u8 = 127;
/// Melodic timbres 0-127, then one percussion timbre per key (128 + key).
pub const BANK_SIZE: usize = 256;
/// Voices sounding at once (the OPL3 has 18 two-operator channels).
const MAX_VOICES: usize = 18;
const PERCUSSION_CHANNEL: u8 = 9;
/// Output gain applied to the sum of all voices.
const MIX_GAIN: f64 = 0.2;
/// Events are applied between blocks of this many samples (about 1.5 ms at 44.1 kHz).
const BLOCK: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Msg {
    NoteOn {
        ch: u8,
        key: u8,
        vel: u8,
    },
    NoteOff {
        ch: u8,
        key: u8,
    },
    Control {
        ch: u8,
        cc: u8,
        val: u8,
    },
    Program {
        ch: u8,
        prog: u8,
    },
    /// Pitch bend, -8192..8191.
    Bend {
        ch: u8,
        val: i16,
    },
    /// Microseconds per quarter note.
    Tempo(u32),
}

#[derive(Debug, Clone, Default)]
pub struct Track {
    /// (absolute tick, message), in file order.
    pub events: Vec<(u64, Msg)>,
    /// Tick of the end-of-track event (the track's length).
    pub end: u64,
}

#[derive(Debug, Clone)]
pub struct Song {
    /// Ticks per quarter note, or for SMPTE timing the negative ticks per second.
    pub division: i32,
    pub tracks: Vec<Track>,
}

fn read_varlen(b: &[u8], p: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        let c = *b.get(*p)?;
        *p += 1;
        v = (v << 7) | (c & 0x7f) as u32;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

impl Song {
    /// Parses a format 0 or 1 Standard MIDI File.
    pub fn parse(b: &[u8]) -> Result<Song, String> {
        let be16 = |p: usize| b.get(p..p + 2).map(|s| u16::from_be_bytes([s[0], s[1]]));
        let be32 = |p: usize| b.get(p..p + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
        if b.get(0..4) != Some(b"MThd") {
            return Err("not a MIDI file".into());
        }
        let hlen = be32(4).ok_or("short header")? as usize;
        let ntracks = be16(10).ok_or("short header")?;
        let div = be16(12).ok_or("short header")?;
        let division = if div & 0x8000 != 0 {
            // SMPTE: frames per second (negative byte) times ticks per frame.
            let fps = -((div >> 8) as i8) as i32;
            -(fps * (div & 0xff) as i32)
        } else {
            div as i32
        };
        if division == 0 {
            return Err("zero time division".into());
        }
        let mut p = 8 + hlen;
        let mut tracks = Vec::new();
        while tracks.len() < ntracks as usize && p + 8 <= b.len() {
            let len = be32(p + 4).ok_or("short chunk")? as usize;
            let body = b.get(p + 8..(p + 8 + len).min(b.len())).ok_or("short chunk")?;
            if &b[p..p + 4] == b"MTrk" {
                tracks.push(Self::parse_track(body)?);
            }
            p += 8 + len;
        }
        Ok(Song { division, tracks })
    }

    fn parse_track(b: &[u8]) -> Result<Track, String> {
        let mut t = Track::default();
        let (mut p, mut tick, mut running) = (0usize, 0u64, 0u8);
        while p < b.len() {
            tick += read_varlen(b, &mut p).ok_or("bad delta time")? as u64;
            let mut status = *b.get(p).ok_or("truncated event")?;
            if status & 0x80 != 0 {
                p += 1;
            } else {
                status = running;
            }
            match status {
                0xff => {
                    let kind = *b.get(p).ok_or("truncated meta")?;
                    p += 1;
                    let len = read_varlen(b, &mut p).ok_or("bad meta length")? as usize;
                    let data = b.get(p..p + len).ok_or("truncated meta")?;
                    p += len;
                    match kind {
                        0x2f => {
                            t.end = tick;
                            return Ok(t);
                        }
                        0x51 if len == 3 => {
                            let us = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | data[2] as u32;
                            t.events.push((tick, Msg::Tempo(us.max(1))));
                        }
                        _ => {}
                    }
                }
                0xf0 | 0xf7 => {
                    let len = read_varlen(b, &mut p).ok_or("bad sysex length")? as usize;
                    p += len;
                }
                0x80..=0xef => {
                    running = status;
                    let ch = status & 15;
                    let d1 = *b.get(p).ok_or("truncated event")? & 0x7f;
                    let two = !matches!(status & 0xf0, 0xc0 | 0xd0);
                    let d2 = if two {
                        *b.get(p + 1).ok_or("truncated event")? & 0x7f
                    } else {
                        0
                    };
                    p += if two { 2 } else { 1 };
                    let msg = match status & 0xf0 {
                        0x80 => Some(Msg::NoteOff { ch, key: d1 }),
                        0x90 if d2 == 0 => Some(Msg::NoteOff { ch, key: d1 }),
                        0x90 => Some(Msg::NoteOn { ch, key: d1, vel: d2 }),
                        0xb0 => Some(Msg::Control { ch, cc: d1, val: d2 }),
                        0xc0 => Some(Msg::Program { ch, prog: d1 }),
                        0xe0 => Some(Msg::Bend {
                            ch,
                            val: (((d2 as i32) << 7) | d1 as i32) as i16 - 8192,
                        }),
                        _ => None, // aftertouch
                    };
                    if let Some(m) = msg {
                        t.events.push((tick, m));
                    }
                }
                _ => return Err(format!("unexpected status byte {status:#04x}")),
            }
        }
        t.end = tick;
        Ok(t)
    }
}

/// EMIDI track designation: whether a track plays on an FM device. Tracks without
/// designation controllers always play.
pub fn track_plays_on_fm(t: &Track) -> bool {
    let start = t.events.first().map_or(0, |e| e.0);
    let mut include: Option<bool> = None;
    for &(tick, m) in &t.events {
        if tick > start {
            break;
        }
        if let Msg::Control { cc, val, .. } = m {
            match cc {
                110 => {
                    let ours = val == EMIDI_ALL || FM_DEVICES.contains(&val);
                    include = Some(include.unwrap_or(false) || ours);
                }
                111 if val == EMIDI_ALL || FM_DEVICES.contains(&val) => return false,
                _ => {}
            }
        }
    }
    include.unwrap_or(true)
}

/// Reads the 256 timbres of an AdLib timbre bank (`.TMB`).
pub fn parse_bank(b: &[u8]) -> Option<Vec<Timbre>> {
    if b.len() < BANK_SIZE * Timbre::SIZE {
        return None;
    }
    Some(
        b.as_chunks::<{ Timbre::SIZE }>().0[..BANK_SIZE]
            .iter()
            .map(Timbre::parse)
            .collect(),
    )
}

#[derive(Debug, Clone, Copy)]
struct ChannelState {
    program: u8,
    volume: u8,
    expression: u8,
    pan: u8,
    /// Pitch bend in semitones.
    bend: f64,
    bend_range: f64,
    sustain: bool,
    rpn: (u8, u8),
}

impl Default for ChannelState {
    fn default() -> Self {
        ChannelState {
            program: 0,
            volume: 100,
            expression: 127,
            pan: 64,
            bend: 0.0,
            bend_range: 2.0,
            sustain: false,
            rpn: (127, 127),
        }
    }
}

struct Playing {
    ch: u8,
    key: u8,
    vel: u8,
    /// Key released while the sustain pedal was down.
    held: bool,
    started: u64,
    voice: Voice,
}

struct TrackPos {
    next: usize,
    /// Added to the track's event ticks (grows with every loop).
    base: u64,
    loop_start: Option<(usize, u64)>,
}

/// Renders a song to 16-bit stereo, looping forever.
pub struct Player {
    song: Song,
    plays: Vec<bool>,
    bank: Vec<Timbre>,
    rate: f64,
    tracks: Vec<TrackPos>,
    /// Current song position in ticks (fractional between events).
    tick: f64,
    us_per_quarter: f64,
    channels: [ChannelState; 16],
    voices: Vec<Playing>,
    song_loop: Option<u64>,
    /// Samples rendered so far.
    clock: u64,
}

impl Player {
    pub fn new(song: Song, bank: Vec<Timbre>, rate: f64) -> Player {
        let plays = song.tracks.iter().map(track_plays_on_fm).collect();
        let tracks = song
            .tracks
            .iter()
            .map(|_| TrackPos {
                next: 0,
                base: 0,
                loop_start: None,
            })
            .collect();
        Player {
            song,
            plays,
            bank,
            rate,
            tracks,
            tick: 0.0,
            us_per_quarter: 500_000.0,
            channels: [ChannelState::default(); 16],
            voices: Vec::new(),
            song_loop: None,
            clock: 0,
        }
    }

    fn ticks_per_sample(&self) -> f64 {
        if self.song.division < 0 {
            -self.song.division as f64 / self.rate
        } else {
            self.song.division as f64 * 1e6 / (self.us_per_quarter * self.rate)
        }
    }

    /// Fills `out` (interleaved stereo) and advances the song.
    pub fn render(&mut self, out: &mut [i16]) {
        for block in out.chunks_mut(BLOCK * 2) {
            self.dispatch();
            let t0 = self.clock as f64 / self.rate;
            for (i, frame) in block.chunks_mut(2).enumerate() {
                let t = t0 + i as f64 / self.rate;
                let (mut l, mut r) = (0.0, 0.0);
                for p in &mut self.voices {
                    let s = p.voice.sample(self.rate, t);
                    l += s * p.voice.gain_l;
                    r += s * p.voice.gain_r;
                }
                frame[0] = ((l * MIX_GAIN).clamp(-1.0, 1.0) * 32767.0) as i16;
                if frame.len() > 1 {
                    frame[1] = ((r * MIX_GAIN).clamp(-1.0, 1.0) * 32767.0) as i16;
                }
            }
            let n = (block.len() / 2) as u64;
            self.clock += n;
            self.tick += self.ticks_per_sample() * n as f64;
            self.voices.retain(|p| !p.voice.is_off());
        }
    }

    /// Applies every event due at the current tick.
    fn dispatch(&mut self) {
        let now = self.tick as u64;
        for ti in 0..self.tracks.len() {
            loop {
                let pos = &self.tracks[ti];
                let Some(&(t, msg)) = self.song.tracks[ti].events.get(pos.next) else {
                    break;
                };
                if t + pos.base > now {
                    break;
                }
                self.tracks[ti].next += 1;
                if let Some(jumped) = self.handle(ti, t, msg) {
                    if jumped {
                        // A loop moved the tracks; re-read from the new positions.
                        return self.dispatch();
                    }
                }
            }
        }
        let finished = self
            .tracks
            .iter()
            .zip(&self.song.tracks)
            .all(|(p, t)| p.next >= t.events.len() && now >= t.end + p.base);
        if finished && !self.song.tracks.is_empty() {
            self.restart(now);
        }
    }

    /// Restarts the song (no loop markers): every track from its first event.
    fn restart(&mut self, now: u64) {
        for v in &mut self.voices {
            v.voice.key_off();
        }
        self.channels = [ChannelState::default(); 16];
        for p in &mut self.tracks {
            *p = TrackPos {
                next: 0,
                base: now + 1,
                loop_start: None,
            };
        }
        self.song_loop = None;
    }

    /// Handles one event of track `ti` at track tick `t`. Returns Some(true) after a loop
    /// jump.
    fn handle(&mut self, ti: usize, t: u64, msg: Msg) -> Option<bool> {
        if let Msg::Tempo(us) = msg {
            self.us_per_quarter = us as f64;
            return None;
        }
        if !self.plays[ti] {
            return None;
        }
        match msg {
            Msg::NoteOn { ch, key, vel } => self.note_on(ch, key, vel),
            Msg::NoteOff { ch, key } => self.note_off(ch, key),
            Msg::Program { ch, prog } => self.channels[ch as usize].program = prog,
            Msg::Bend { ch, val } => {
                let c = &mut self.channels[ch as usize];
                c.bend = val as f64 / 8192.0 * c.bend_range;
                let ratio = 2f64.powf(c.bend / 12.0);
                for p in self.voices.iter_mut().filter(|p| p.ch == ch) {
                    p.voice.bend = ratio;
                }
            }
            Msg::Control { ch, cc, val } => return self.control(ti, t, ch, cc, val),
            Msg::Tempo(_) => {}
        }
        None
    }

    fn control(&mut self, ti: usize, t: u64, ch: u8, cc: u8, val: u8) -> Option<bool> {
        let c = &mut self.channels[ch as usize];
        match cc {
            7 | 113 => {
                c.volume = val;
                self.refresh_volume(ch);
            }
            11 => {
                c.expression = val;
                self.refresh_volume(ch);
            }
            10 => {
                c.pan = val;
                self.refresh_volume(ch);
            }
            64 => {
                c.sustain = val >= 64;
                if !c.sustain {
                    for p in self.voices.iter_mut().filter(|p| p.ch == ch && p.held) {
                        p.held = false;
                        p.voice.key_off();
                    }
                }
            }
            100 => c.rpn.0 = val,
            101 => c.rpn.1 = val,
            6 if c.rpn == (0, 0) => c.bend_range = val as f64,
            112 => c.program = val,
            120 | 123 => {
                for p in self.voices.iter_mut().filter(|p| p.ch == ch) {
                    p.voice.key_off();
                }
            }
            121 => *c = ChannelState::default(),
            116 => self.song_loop = Some(t + self.tracks[ti].base),
            117 => {
                if let Some(start) = self.song_loop {
                    let now = t + self.tracks[ti].base;
                    let len = now.saturating_sub(start).max(1);
                    for (p, tr) in self.tracks.iter_mut().zip(&self.song.tracks) {
                        p.base += len;
                        // Back to the first event at or after the loop start.
                        let local = start.saturating_sub(p.base - len);
                        p.next = tr.events.partition_point(|e| e.0 < local);
                    }
                    self.song_loop = Some(start + len);
                    return Some(true);
                }
            }
            118 => self.tracks[ti].loop_start = Some((self.tracks[ti].next, t)),
            119 => {
                if let Some((next, start_t)) = self.tracks[ti].loop_start {
                    let p = &mut self.tracks[ti];
                    p.base += t.saturating_sub(start_t).max(1);
                    p.next = next;
                    return Some(true);
                }
            }
            _ => {}
        }
        None
    }

    fn volume_of(&self, ch: u8, vel: u8) -> f64 {
        let c = &self.channels[ch as usize];
        (vel as f64 / 127.0) * (c.volume as f64 / 127.0) * (c.expression as f64 / 127.0)
    }

    fn pan_gains(&self, ch: u8) -> (f64, f64) {
        let p = self.channels[ch as usize].pan as f64 / 127.0;
        let a = p * std::f64::consts::FRAC_PI_2;
        (a.cos() * std::f64::consts::SQRT_2, a.sin() * std::f64::consts::SQRT_2)
    }

    fn refresh_volume(&mut self, ch: u8) {
        let (l, r) = self.pan_gains(ch);
        for i in 0..self.voices.len() {
            if self.voices[i].ch != ch {
                continue;
            }
            let vol = self.volume_of(ch, self.voices[i].vel);
            let p = &mut self.voices[i];
            p.voice.set_volume(vol);
            p.voice.gain_l = l;
            p.voice.gain_r = r;
        }
    }

    fn note_on(&mut self, ch: u8, key: u8, vel: u8) {
        let c = self.channels[ch as usize];
        let (timbre, note) = if ch == PERCUSSION_CHANNEL {
            let ti = 128 + key as usize;
            (ti, self.bank[ti].transpose as f64)
        } else {
            let ti = c.program as usize;
            (ti, key as f64 + self.bank[ti].transpose as f64)
        };
        // Same key again on the channel: release the old note first.
        for p in self.voices.iter_mut().filter(|p| p.ch == ch && p.key == key) {
            p.voice.key_off();
            p.held = false;
        }
        if self.voices.len() >= MAX_VOICES {
            // Steal the quietest released voice, else the oldest one.
            let victim = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, p)| p.voice.is_released())
                .max_by(|a, b| a.1.voice.level_db().total_cmp(&b.1.voice.level_db()))
                .or_else(|| self.voices.iter().enumerate().min_by_key(|(_, p)| p.started))
                .map(|(i, _)| i);
            if let Some(i) = victim {
                self.voices.swap_remove(i);
            }
        }
        let freq = 440.0 * 2f64.powf((note - 69.0) / 12.0);
        let t = self.bank[timbre];
        let mut voice = Voice::new(&t, freq, self.volume_of(ch, vel), self.rate);
        if ch != PERCUSSION_CHANNEL {
            voice.bend = 2f64.powf(c.bend / 12.0);
        }
        let (l, r) = self.pan_gains(ch);
        voice.gain_l = l;
        voice.gain_r = r;
        self.voices.push(Playing {
            ch,
            key,
            vel,
            held: false,
            started: self.clock,
            voice,
        });
    }

    fn note_off(&mut self, ch: u8, key: u8) {
        let sustain = self.channels[ch as usize].sustain;
        for p in self
            .voices
            .iter_mut()
            .filter(|p| p.ch == ch && p.key == key && !p.voice.is_released())
        {
            if sustain {
                p.held = true;
            } else {
                p.voice.key_off();
            }
        }
    }
}

/// Streams a song as interleaved 16-bit stereo 44.1 kHz chunks (half a second each) from a
/// background thread, in the same form as [`crate::cdaudio::stream_wav`]. The thread exits
/// when the receiver is dropped.
pub fn stream(song: Song, bank: Vec<Timbre>) -> std::sync::mpsc::Receiver<Vec<i16>> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(6);
    std::thread::spawn(move || {
        let mut player = Player::new(song, bank, 44100.0);
        loop {
            let mut chunk = vec![0i16; 44100];
            player.render(&mut chunk);
            if tx.send(chunk).is_err() {
                return;
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a format-0 MIDI file from raw track bytes.
    fn smf(track: &[u8]) -> Vec<u8> {
        let mut b = b"MThd\0\0\0\x06\0\0\0\x01\0\x60".to_vec();
        b.extend_from_slice(b"MTrk");
        b.extend_from_slice(&(track.len() as u32).to_be_bytes());
        b.extend_from_slice(track);
        b
    }

    fn bank() -> Vec<Timbre> {
        let t = [0x21, 0x21, 0x3f, 0x00, 0xf0, 0xf0, 0x00, 0x0f, 0, 0, 0x01, 0, 0];
        parse_bank(&t.repeat(BANK_SIZE)).unwrap()
    }

    #[test]
    fn parses_running_status_tempo_and_note_off_as_zero_velocity() {
        let track = [
            0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, // tempo 500000
            0x00, 0x90, 60, 100, // note on
            0x60, 60, 0, // running status, velocity 0 = note off
            0x00, 0xff, 0x2f, 0x00,
        ];
        let s = Song::parse(&smf(&track)).unwrap();
        assert_eq!(s.division, 96);
        assert_eq!(
            s.tracks[0].events,
            vec![
                (0, Msg::Tempo(500_000)),
                (
                    0,
                    Msg::NoteOn {
                        ch: 0,
                        key: 60,
                        vel: 100
                    }
                ),
                (96, Msg::NoteOff { ch: 0, key: 60 }),
            ]
        );
    }

    #[test]
    fn emidi_track_designation() {
        let ctl = |cc, val| (0, Msg::Control { ch: 0, cc, val });
        let track = |events| Track { events, end: 0 };
        let only_gm = track(vec![ctl(110, 0)]);
        let fm_too = track(vec![ctl(110, 0), ctl(110, 4)]);
        let not_fm = track(vec![ctl(111, 7)]);
        let plain = track(vec![(0, Msg::Program { ch: 0, prog: 1 })]);
        assert!(!track_plays_on_fm(&only_gm));
        assert!(track_plays_on_fm(&fm_too));
        assert!(!track_plays_on_fm(&not_fm));
        assert!(track_plays_on_fm(&plain));
    }

    #[test]
    fn plays_a_note_then_loops_the_song() {
        // One quarter note at 120 bpm (0.5 s), then silence to the end of the track.
        let track = [0x00, 0x90, 69, 127, 0x60, 0x80, 69, 0, 0x60, 0xff, 0x2f, 0x00];
        let s = Song::parse(&smf(&track)).unwrap();
        let mut p = Player::new(s, bank(), 8000.0);
        let mut out = vec![0i16; 8000 * 2]; // one second, stereo
        p.render(&mut out);
        let loud = |a: &[i16]| a.iter().map(|&x| (x as i32).abs()).max().unwrap();
        assert!(loud(&out[..4000 * 2]) > 1000, "note audible");
        assert!(loud(&out[5000 * 2..]) < 100, "silent after the note");
        // Second pass of the loop starts after 1 s.
        let mut more = vec![0i16; 8000 * 2];
        p.render(&mut more);
        assert!(loud(&more[200..4000 * 2]) > 1000, "song restarted");
    }

    #[test]
    fn emidi_song_loop_jumps_back() {
        // Loop start at 0, note, loop end at tick 96: the note repeats every 0.5 s.
        let track = [
            0x00, 0xb0, 116, 0, // song loop start
            0x00, 0x90, 69, 127, 0x30, 0x80, 69, 0, // 1/8 note
            0x30, 0xb0, 117, 0, // song loop end at tick 96
            0x83, 0x00, 0xff, 0x2f, 0x00, // track would end much later
        ];
        let s = Song::parse(&smf(&track)).unwrap();
        let mut p = Player::new(s, bank(), 8000.0);
        let mut out = vec![0i16; 8000 * 2];
        p.render(&mut out);
        let loud = |a: &[i16]| a.iter().map(|&x| (x as i32).abs()).max().unwrap();
        // Second repeat starts at 0.5 s.
        assert!(loud(&out[4100 * 2..5500 * 2]) > 1000);
    }

    /// With `OPENBLOOD_DATA` set: every song in the user's SOUNDS.RFF parses, has FM tracks
    /// and renders audibly without clipping.
    #[test]
    fn original_songs_render() {
        let Some(dir) = std::env::var("OPENBLOOD_DATA").ok() else {
            return;
        };
        let Ok(rff) = ob_engine::rff::Rff::open(std::path::Path::new(&dir).join("SOUNDS.RFF")) else {
            return;
        };
        let bank = parse_bank(&rff.read_name("GMTIMBRE.TMB").unwrap()).unwrap();
        let mut n = 0;
        for e in rff.entries.iter().filter(|e| e.ext == "MID") {
            let song = Song::parse(&rff.read(e).unwrap()).unwrap_or_else(|err| panic!("{}: {err}", e.file_name()));
            assert!(song.tracks.iter().any(track_plays_on_fm), "{}", e.file_name());
            let mut p = Player::new(song, bank.clone(), 22050.0);
            let mut out = vec![0i16; 22050 * 2 * 20];
            p.render(&mut out);
            let peak = out.iter().map(|&s| (s as i32).abs()).max().unwrap();
            assert!(peak > 500 && peak < 32767, "{}: peak {peak}", e.file_name());
            n += 1;
        }
        assert!(n >= 12);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Song::parse(b"RIFF....").is_err());
        assert!(parse_bank(&[0; 100]).is_none());
    }
}
