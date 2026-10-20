//! Cutscene playback: Smacker videos (intro logos and episode cutscenes) from the user's own
//! game files, with their separate WAV soundtracks.

use ob_engine::smk::Smk;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A video queued or playing, plus its soundtrack.
#[derive(Debug, Clone)]
pub struct MovieFile {
    pub video: PathBuf,
    pub wav: Option<PathBuf>,
}

pub struct Movie {
    smk: Smk,
    start: Instant,
    pub pixels: Vec<u32>,
}

impl Movie {
    pub fn open(path: &Path) -> Option<Movie> {
        let data = std::fs::read(path).ok()?;
        let mut smk = Smk::parse(data).ok()?;
        smk.next_frame().ok()?;
        let pixels = smk.pixels();
        Some(Movie { smk, start: Instant::now(), pixels })
    }

    pub fn width(&self) -> usize {
        self.smk.width
    }

    pub fn height(&self) -> usize {
        self.smk.height
    }

    /// Decodes up to the frame due at the current time. Returns false when the video ended.
    pub fn update(&mut self) -> bool {
        let due = (self.start.elapsed().as_secs_f64() * 1000.0 / self.smk.frame_ms) as usize;
        let mut changed = false;
        while self.smk.position() <= due {
            match self.smk.next_frame() {
                Ok(true) => changed = true,
                _ => return false,
            }
        }
        if changed {
            self.pixels = self.smk.pixels();
        }
        true
    }
}

/// Finds a file by name (case-insensitive) in the usual places for the user's game data:
/// `DATA/movie`, `DATA`, and next to the executable's data folder.
pub fn find(data_dir: &Path, name: &str) -> Option<PathBuf> {
    let base = Path::new(name.trim()).file_name()?.to_string_lossy().to_string();
    let base = base.rsplit(['\\', '/']).next().unwrap_or(&base).to_string();
    for dir in [data_dir.join("movie"), data_dir.join("MOVIE"), data_dir.to_path_buf()] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().eq_ignore_ascii_case(&base) {
                return Some(e.path());
            }
        }
    }
    None
}

/// The intro shown at startup (Monolith logo, then the GT Interactive logo), if present.
pub fn intro(data_dir: &Path) -> Vec<MovieFile> {
    [("LOGO.SMK", "LOGO811M.WAV"), ("GTI.SMK", "GTI.WAV")]
        .iter()
        .filter_map(|(v, w)| {
            Some(MovieFile {
                video: find(data_dir, v)?,
                wav: find(data_dir, w),
            })
        })
        .collect()
}

/// Loads an 8- or 16-bit mono/stereo PCM WAV as unsigned 8-bit mono and its sample rate.
pub fn load_wav_u8(path: &Path) -> Option<(Vec<u8>, u32)> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut channels, mut rate, mut bits) = (1u16, 22050u32, 8u16);
    let mut p = 12;
    while p + 8 <= b.len() {
        let id = &b[p..p + 4];
        let len = u32::from_le_bytes(b[p + 4..p + 8].try_into().ok()?) as usize;
        let body = &b[p + 8..(p + 8 + len).min(b.len())];
        if id == b"fmt " && body.len() >= 16 {
            channels = u16::from_le_bytes([body[2], body[3]]).max(1);
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            let samples: Vec<u8> = match bits {
                8 => body.chunks(channels as usize).map(|c| c[0]).collect(),
                16 => body
                    .chunks(2 * channels as usize)
                    .filter(|c| c.len() >= 2)
                    .map(|c| ((i16::from_le_bytes([c[0], c[1]]) >> 8) + 128) as u8)
                    .collect(),
                _ => return None,
            };
            return Some((samples, rate));
        }
        p += 8 + len + (len & 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_8bit_mono() {
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&22050u32.to_le_bytes());
        b.extend_from_slice(&22050u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&8u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&[1, 128, 255]);
        let dir = std::env::temp_dir().join(format!("ob_wav_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        std::fs::write(&p, &b).unwrap();
        assert_eq!(load_wav_u8(&p), Some((vec![1, 128, 255], 22050)));
    }
}
