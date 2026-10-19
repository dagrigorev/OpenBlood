//! Sound effects: SFX/RAW from SOUNDS.RFF mixed in software with 3D panning.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ob_game::game::{Game, GameEvent};
use ob_game::resources::Resources;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Sprite types that generate sounds.
const SOUND_SECTOR: i16 = 709;
const SOUND_AMBIENT: i16 = 710;

pub struct Sound {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub loop_start: Option<usize>,
    pub volume: f32,
    pub pitch: f32,
}

/// Parses a Blood `.SFX` descriptor: relVol, pitch, pitchRange, format, loopStart, rawName[9].
pub fn parse_sfx(b: &[u8]) -> Option<(i32, i32, i32, i32, i32, String)> {
    if b.len() < 21 {
        return None;
    }
    let rd = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let name_bytes = &b[20..b.len().min(29)];
    let end = name_bytes.iter().position(|&c| c == 0).unwrap_or(name_bytes.len());
    let name = String::from_utf8_lossy(&name_bytes[..end]).to_string();
    Some((rd(0), rd(4), rd(8), rd(12), rd(16), name))
}

pub fn sfx_rate(format: i32) -> u32 {
    match format {
        f if f >= 9 => 44100,
        5..=8 => 22050,
        _ => 11025,
    }
}

struct Channel {
    sound: Arc<Sound>,
    pos: f64,
    step: f64,
    vol_l: f32,
    vol_r: f32,
    /// World position for positional sounds.
    at: Option<(f64, f64, f64)>,
    radius: (f64, f64),
    base: f32,
    looping: bool,
    #[allow(dead_code)]
    owner: i64,
}

#[derive(Default)]
struct Mixer {
    channels: Vec<Channel>,
    out_rate: f64,
    master: f32,
    /// Interleaved stereo 44.1 kHz music and its play position (in frames).
    music: Option<Arc<Vec<f32>>>,
    music_pos: f64,
    music_vol: f32,
}

impl Mixer {
    fn mix(&mut self, out: &mut [f32], nch: usize) {
        for s in out.iter_mut() {
            *s = 0.0;
        }
        let frames = out.len() / nch;
        for ch in self.channels.iter_mut() {
            let data = &ch.sound.samples;
            for f in 0..frames {
                let i = ch.pos as usize;
                if i >= data.len() {
                    match (ch.looping, ch.sound.loop_start) {
                        (true, Some(ls)) if ls < data.len() => ch.pos = ls as f64,
                        (true, None) => ch.pos = 0.0,
                        _ => break,
                    }
                }
                let i = ch.pos as usize;
                let v = data[i.min(data.len() - 1)];
                if nch >= 2 {
                    out[f * nch] += v * ch.vol_l;
                    out[f * nch + 1] += v * ch.vol_r;
                } else {
                    out[f] += v * (ch.vol_l + ch.vol_r) * 0.5;
                }
                ch.pos += ch.step;
            }
        }
        self.channels
            .retain(|c| (c.pos as usize) < c.sound.samples.len() || c.looping);
        for s in out.iter_mut() {
            *s *= self.master;
        }
        if let Some(m) = &self.music {
            let nframes = m.len() / 2;
            if nframes > 0 {
                let step = 44100.0 / self.out_rate;
                for f in 0..frames {
                    let i = (self.music_pos as usize % nframes) * 2;
                    let (l, r) = (m[i] * self.music_vol, m[i + 1] * self.music_vol);
                    if nch >= 2 {
                        out[f * nch] += l;
                        out[f * nch + 1] += r;
                    } else {
                        out[f] += (l + r) * 0.5;
                    }
                    self.music_pos += step;
                }
                self.music_pos %= nframes as f64;
            }
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

pub struct Audio {
    _stream: cpal::Stream,
    mixer: Arc<Mutex<Mixer>>,
    cache: HashMap<i32, Option<Arc<Sound>>>,
    map_name: String,
    rng: u32,
    track: u32,
}

impl Audio {
    pub fn new(_res: &Resources) -> Result<Audio, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let nch = config.channels as usize;
        let mixer = Arc::new(Mutex::new(Mixer {
            out_rate: config.sample_rate.0 as f64,
            master: 0.8,
            ..Default::default()
        }));
        let m2 = mixer.clone();
        let err = |e| eprintln!("audio error: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |data: &mut [f32], _| m2.lock().unwrap().mix(data, nch),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let mut tmp = Vec::new();
                device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _| {
                        tmp.resize(data.len(), 0.0f32);
                        m2.lock().unwrap().mix(&mut tmp, nch);
                        for (d, s) in data.iter_mut().zip(tmp.iter()) {
                            *d = (s * 32767.0) as i16;
                        }
                    },
                    err,
                    None,
                )
            }
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Audio {
            _stream: stream,
            mixer,
            cache: HashMap::new(),
            map_name: String::new(),
            rng: 12345,
            track: 0,
        })
    }

    fn rand(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1103515245).wrapping_add(12345);
        self.rng >> 16
    }

    pub fn load(&mut self, res: &Resources, id: i32) -> Option<Arc<Sound>> {
        if let Some(s) = self.cache.get(&id) {
            return s.clone();
        }
        let snd = (|| {
            let rff = res.sounds.as_ref()?;
            let sfx = rff.read_id("SFX", id as u32)?;
            let (vol, pitch, _range, format, loop_start, raw) = parse_sfx(&sfx)?;
            let pcm = rff.read_name(&format!("{raw}.RAW"))?;
            Some(Arc::new(Sound {
                samples: pcm.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
                rate: sfx_rate(format),
                loop_start: (loop_start >= 0).then_some(loop_start as usize),
                volume: (vol as f32 / 80.0).clamp(0.0, 2.0),
                pitch: if pitch > 0 { pitch as f32 / 65536.0 } else { 1.0 },
            }))
        })();
        self.cache.insert(id, snd.clone());
        snd
    }

    /// Plays sound `id`, optionally at a world position.
    pub fn play(
        &mut self,
        res: &Resources,
        id: i32,
        at: Option<(f64, f64, f64)>,
        looping: bool,
        radius: (f64, f64),
        owner: i64,
    ) {
        let Some(snd) = self.load(res, id) else { return };
        let jitter = 1.0 + ((self.rand() % 100) as f64 - 50.0) / 2000.0;
        let mut m = self.mixer.lock().unwrap();
        let step = snd.rate as f64 / m.out_rate * snd.pitch as f64 * jitter;
        let base = snd.volume;
        m.channels.push(Channel {
            sound: snd,
            pos: 0.0,
            step,
            vol_l: base * 0.7,
            vol_r: base * 0.7,
            at,
            radius,
            base,
            looping,
            owner,
        });
    }

    pub fn stop_all(&mut self) {
        self.mixer.lock().unwrap().channels.clear();
    }

    fn start_level(&mut self, res: &Resources, game: &Game) {
        self.stop_all();
        for (i, s) in game.world.sprites.iter().enumerate() {
            if s.lotag == SOUND_AMBIENT {
                if let Some(x) = game.world.xsprite(i) {
                    if x.data3 > 0 {
                        let r = (x.data1.max(1) as f64, x.data2.max(x.data1 + 1) as f64);
                        self.play(
                            res,
                            x.data3,
                            Some((s.x as f64, s.y as f64, s.z as f64)),
                            true,
                            r,
                            i as i64,
                        );
                    }
                }
            }
        }
    }

    pub fn on_event(&mut self, res: &Resources, game: &Game, e: &GameEvent) {
        if let GameEvent::Sound { id, at } = *e {
            self.play(res, id, at, false, (1500.0, 24000.0), -1);
            return;
        }
        if let GameEvent::SectorSound { sector, opening, start } = *e {
            for (i, s) in game.world.sprites.iter().enumerate() {
                if s.lotag != SOUND_SECTOR || s.sectnum as usize != sector {
                    continue;
                }
                let Some(x) = game.world.xsprite(i) else { continue };
                let id = match (opening, start) {
                    (true, true) => x.data1,
                    (true, false) => x.data2,
                    (false, true) => x.data3,
                    (false, false) => x.data4 as i32,
                };
                if id > 0 {
                    self.play(
                        res,
                        id,
                        Some((s.x as f64, s.y as f64, s.z as f64)),
                        false,
                        (1024.0, 16384.0),
                        -1,
                    );
                }
            }
        }
    }

    /// Updates positional volumes and panning; restarts ambience on level change.
    pub fn update_listener(&mut self, game: &Game) {
        if game.world.map_name != self.map_name {
            self.map_name = game.world.map_name.clone();
            // Needs resources; deferred to `level_changed`.
            self.cache.retain(|_, v| v.is_some());
        }
        let p = &game.player;
        let a = ob_engine::geom::ang_to_rad(p.ang);
        let (fx, fy) = (a.cos(), a.sin());
        let mut m = self.mixer.lock().unwrap();
        for ch in m.channels.iter_mut() {
            let Some((x, y, z)) = ch.at else { continue };
            let dx = x - p.x;
            let dy = y - p.y;
            let dz = (z - p.z) / 16.0;
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            let (r0, r1) = ch.radius;
            let att = if d <= r0 {
                1.0
            } else if d >= r1 {
                0.0
            } else {
                1.0 - (d - r0) / (r1 - r0)
            };
            let side = if d > 1.0 { (-dx * fy + dy * fx) / d } else { 0.0 };
            let pan_r = (0.5 + side * 0.4) as f32;
            let v = ch.base * att as f32;
            ch.vol_l = v * (1.0 - pan_r) * 1.4;
            ch.vol_r = v * pan_r * 1.4;
        }
    }

    pub fn level_changed(&mut self, res: &Resources, game: &Game) {
        self.start_level(res, game);
        let track = res.ini.map(&game.world.map_name).map_or(0, |m| m.track);
        self.play_track(res, track);
    }

    pub fn set_volumes(&mut self, sfx: f32, music: f32) {
        let mut m = self.mixer.lock().unwrap();
        m.master = sfx;
        m.music_vol = music;
    }

    /// Plays CD track `n` from `music/TRACKnn.WAV` (ripped from the user's own disc).
    /// Loading happens on a background thread so level starts don't stall.
    pub fn play_track(&mut self, res: &Resources, n: u32) {
        if n == self.track {
            return;
        }
        self.track = n;
        let mixer = self.mixer.clone();
        mixer.lock().unwrap().music = None;
        if n == 0 {
            return;
        }
        let candidates = [
            res.dir.join("music").join(format!("TRACK{n:02}.WAV")),
            res.dir.join(format!("TRACK{n:02}.WAV")),
        ];
        std::thread::spawn(move || {
            if let Some(samples) = candidates.iter().find_map(|p| crate::cdaudio::load_wav(p)) {
                let mut m = mixer.lock().unwrap();
                m.music = Some(Arc::new(samples));
                m.music_pos = 0.0;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfx_descriptor() {
        let mut b = vec![0u8; 29];
        b[0] = 80;
        b[6] = 1; // pitch 0x10000
        b[12] = 5;
        b[16..20].copy_from_slice(&(-1i32).to_le_bytes());
        b[20..24].copy_from_slice(b"amb3");
        let (v, p, _, f, l, n) = parse_sfx(&b).unwrap();
        assert_eq!((v, p, f, l, n.as_str()), (80, 0x10000, 5, -1, "amb3"));
        assert_eq!(sfx_rate(f), 22050);
    }
}
