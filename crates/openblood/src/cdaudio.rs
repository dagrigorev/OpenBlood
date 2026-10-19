//! Red Book CD audio: extracts music tracks from the user's own disc image (CUE/BIN)
//! into TRACKnn.WAV files, which OpenBlood then plays as level music.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

const SECTOR: u64 = 2352;

#[derive(Debug, Clone)]
pub struct CueTrack {
    pub number: u32,
    pub audio: bool,
    /// INDEX 00 (pregap start) and INDEX 01 (track start), in sectors.
    pub index0: Option<u64>,
    pub index1: u64,
}

fn msf(s: &str) -> Option<u64> {
    let p: Vec<u64> = s.split(':').filter_map(|x| x.trim().parse().ok()).collect();
    (p.len() == 3).then(|| (p[0] * 60 + p[1]) * 75 + p[2])
}

/// Parses a single-file CUE sheet. Returns the BIN path and the tracks.
pub fn parse_cue(path: &Path) -> io::Result<(PathBuf, Vec<CueTrack>)> {
    let text = std::fs::read_to_string(path)?;
    let mut bin = None;
    let mut tracks: Vec<CueTrack> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        let up = l.to_ascii_uppercase();
        if up.starts_with("FILE") {
            let name = l.split('"').nth(1).unwrap_or("").to_string();
            bin = Some(path.parent().unwrap_or(Path::new(".")).join(name));
        } else if up.starts_with("TRACK") {
            let mut it = l.split_whitespace().skip(1);
            let number = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let audio = it.next().is_some_and(|m| m.eq_ignore_ascii_case("AUDIO"));
            tracks.push(CueTrack {
                number,
                audio,
                index0: None,
                index1: 0,
            });
        } else if up.starts_with("INDEX") {
            let mut it = l.split_whitespace().skip(1);
            let idx: u32 = it.next().and_then(|n| n.parse().ok()).unwrap_or(1);
            let pos = it.next().and_then(msf).unwrap_or(0);
            if let Some(t) = tracks.last_mut() {
                if idx == 0 {
                    t.index0 = Some(pos);
                } else if idx == 1 {
                    t.index1 = pos;
                }
            }
        }
    }
    let bin = bin.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CUE has no FILE line"))?;
    Ok((bin, tracks))
}

fn write_wav(path: &Path, pcm: &[u8]) -> io::Result<()> {
    let mut f = io::BufWriter::new(std::fs::File::create(path)?);
    let data_len = pcm.len() as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&2u16.to_le_bytes())?; // stereo
    f.write_all(&44100u32.to_le_bytes())?;
    f.write_all(&(44100u32 * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    f.write_all(pcm)?;
    Ok(())
}

pub fn extract(cue: &Path, out: &Path) -> io::Result<usize> {
    use std::io::{Read, Seek, SeekFrom};
    let (bin, tracks) = parse_cue(cue)?;
    let total = std::fs::metadata(&bin)?.len() / SECTOR;
    std::fs::create_dir_all(out)?;
    let mut f = std::fs::File::open(&bin)?;
    let mut n = 0;
    for (i, t) in tracks.iter().enumerate() {
        if !t.audio {
            continue;
        }
        let end = tracks.get(i + 1).map_or(total, |nt| nt.index0.unwrap_or(nt.index1));
        let len = (end.saturating_sub(t.index1) * SECTOR) as usize;
        let mut pcm = vec![0u8; len];
        f.seek(SeekFrom::Start(t.index1 * SECTOR))?;
        f.read_exact(&mut pcm)?;
        write_wav(&out.join(format!("TRACK{:02}.WAV", t.number)), &pcm)?;
        n += 1;
    }
    Ok(n)
}

/// Loads a 16-bit stereo 44.1 kHz WAV as interleaved f32 samples.
pub fn load_wav(path: &Path) -> Option<Vec<f32>> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    // Find the data chunk.
    let mut p = 12;
    while p + 8 <= b.len() {
        let id = &b[p..p + 4];
        let len = u32::from_le_bytes(b[p + 4..p + 8].try_into().ok()?) as usize;
        if id == b"data" {
            let data = &b[p + 8..(p + 8 + len).min(b.len())];
            return Some(
                data.chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                    .collect(),
            );
        }
        p += 8 + len + (len & 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_parsing() {
        let dir = std::env::temp_dir().join(format!("ob_cue_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cue = dir.join("d.cue");
        std::fs::write(&cue, "FILE \"d.img\" BINARY\n TRACK 1 MODE2/2352\n  INDEX 1 00:00:00\n TRACK 2 AUDIO\n  INDEX 1 00:02:00\n TRACK 3 AUDIO\n  INDEX 0 00:03:00\n  INDEX 1 00:03:10\n").unwrap();
        let (bin, t) = parse_cue(&cue).unwrap();
        assert!(bin.ends_with("d.img"));
        assert_eq!(t.len(), 3);
        assert!(!t[0].audio && t[1].audio);
        assert_eq!(t[1].index1, 150);
        assert_eq!((t[2].index0, t[2].index1), (Some(225), 235));
        // Synthetic image: 300 sectors.
        std::fs::write(dir.join("d.img"), vec![1u8; 300 * 2352]).unwrap();
        let out = dir.join("out");
        assert_eq!(extract(&cue, &out).unwrap(), 2);
        let w = load_wav(&out.join("TRACK02.WAV")).unwrap();
        assert_eq!(w.len(), 75 * 2352 / 2);
    }
}
