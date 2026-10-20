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
/// Streams the PCM data of a 16-bit WAV file in half-second chunks of interleaved samples,
/// looping forever, from a background thread. Only a few chunks are buffered (the channel is
/// bounded), so memory stays small and nothing large is allocated at level start. The thread
/// exits when the receiver is dropped.
pub fn stream_wav(path: &Path) -> Option<std::sync::mpsc::Receiver<Vec<i16>>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    // Find the data chunk.
    let (start, len) = loop {
        let mut ch = [0u8; 8];
        f.read_exact(&mut ch).ok()?;
        let len = u32::from_le_bytes(ch[4..8].try_into().ok()?) as u64;
        if &ch[0..4] == b"data" {
            break (f.stream_position().ok()?, len);
        }
        f.seek(SeekFrom::Current((len + (len & 1)) as i64)).ok()?;
    };
    if len < 4 {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(6);
    std::thread::spawn(move || {
        const CHUNK: usize = 44100 * 2 * 2 / 2; // half a second of 16-bit stereo, in bytes
        let mut bytes = vec![0u8; CHUNK];
        let mut left = len;
        loop {
            if left == 0 {
                if f.seek(SeekFrom::Start(start)).is_err() {
                    return;
                }
                left = len;
            }
            let n = (CHUNK as u64).min(left) as usize & !3;
            if n == 0 || f.read_exact(&mut bytes[..n]).is_err() {
                return;
            }
            left -= n as u64;
            let samples: Vec<i16> = bytes[..n]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            if tx.send(samples).is_err() {
                return; // music stopped or changed
            }
        }
    });
    Some(rx)
}

/// Reads the ISO 9660 filesystem of the disc's data track (raw 2352-byte sectors) and copies
/// the files of `dir` (e.g. `MOVIE`, Blood's cutscenes) to `out`. Returns the names copied.
pub fn extract_dir(cue: &Path, dir: &str, out: &Path) -> io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let (bin, tracks) = parse_cue(cue)?;
    let data = tracks
        .iter()
        .find(|t| !t.audio)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no data track"))?;
    let base = data.index1;
    let mut f = std::fs::File::open(&bin)?;
    // User data offset inside a raw sector: 16 for mode 1, 24 for mode 2 form 1.
    let mut read_sector = |lba: u64| -> io::Result<Vec<u8>> {
        let mut raw = vec![0u8; SECTOR as usize];
        f.seek(SeekFrom::Start((base + lba) * SECTOR))?;
        f.read_exact(&mut raw)?;
        let off = if raw[15] == 2 { 24 } else { 16 };
        Ok(raw[off..off + 2048].to_vec())
    };
    let pvd = read_sector(16)?;
    if &pvd[1..6] != b"CD001" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "no ISO 9660 filesystem"));
    }
    let le32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as u64;
    // Directory records: (name, extent, size, is_dir).
    let mut list = |lba: u64, size: u64| -> io::Result<Vec<(String, u64, u64, bool)>> {
        let mut out = Vec::new();
        for k in 0..size.div_ceil(2048) {
            let sec = read_sector(lba + k)?;
            let mut p = 0;
            while p < 2048 && sec[p] != 0 {
                let len = sec[p] as usize;
                let nlen = sec[p + 32] as usize;
                let name = String::from_utf8_lossy(&sec[p + 33..p + 33 + nlen]).to_string();
                let name = name.split(';').next().unwrap_or("").to_string();
                out.push((name, le32(&sec, p + 2), le32(&sec, p + 10), sec[p + 25] & 2 != 0));
                p += len;
            }
        }
        Ok(out)
    };
    let root = list(le32(&pvd, 156 + 2), le32(&pvd, 156 + 10))?;
    let (_, lba, size, _) = root
        .into_iter()
        .find(|e| e.3 && e.0.eq_ignore_ascii_case(dir))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no {dir} folder on the disc")))?;
    std::fs::create_dir_all(out)?;
    let mut copied = Vec::new();
    for (name, flba, fsize, is_dir) in list(lba, size)? {
        // Only the videos and their soundtracks (not installer files or AVI duplicates).
        let up = name.to_ascii_uppercase();
        if is_dir || !(up.ends_with(".SMK") || up.ends_with(".WAV")) {
            continue;
        }
        let mut bytes = Vec::with_capacity(fsize as usize);
        for k in 0..fsize.div_ceil(2048) {
            bytes.extend_from_slice(&read_sector(flba + k)?);
        }
        bytes.truncate(fsize as usize);
        std::fs::write(out.join(&name), &bytes)?;
        copied.push(name);
    }
    Ok(copied)
}

#[allow(dead_code)]
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
