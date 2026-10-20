//! BLOOD.INI: episodes, map order, titles, music and in-level messages.

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct MapInfo {
    pub file: String,
    pub title: String,
    pub author: String,
    pub song: String,
    /// Red Book CD audio track number (0 = none).
    pub track: u32,
    /// 1-based map numbers within the episode for the normal / secret exits.
    pub ending_a: usize,
    pub ending_b: usize,
    pub fog: bool,
    pub messages: HashMap<u32, String>,
}

#[derive(Debug, Clone, Default)]
pub struct Episode {
    pub title: String,
    pub maps: Vec<String>,
    pub bloodbath_only: bool,
    /// Cutscene played when entering map number `cutscene_a_level` (1-based) of the episode,
    /// and one played when the episode ends; each with its WAV soundtrack (paths as written
    /// in BLOOD.INI; only the file names are used).
    pub cutscene_a: Option<String>,
    pub wav_a: Option<String>,
    pub cutscene_a_level: usize,
    pub cutscene_b: Option<String>,
    pub wav_b: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BloodIni {
    pub episodes: Vec<Episode>,
    pub maps: HashMap<String, MapInfo>,
}

impl BloodIni {
    pub fn parse(text: &str) -> BloodIni {
        let mut ini = BloodIni::default();
        let mut section = String::new();
        for raw in text.lines() {
            let line = raw.split(';').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_ascii_uppercase();
                if section.starts_with("EPISODE") {
                    ini.episodes.push(Episode {
                        cutscene_a_level: 1,
                        ..Default::default()
                    });
                } else if section != "INSTALL" {
                    ini.maps.entry(section.clone()).or_insert_with(|| MapInfo {
                        file: section.clone(),
                        ..Default::default()
                    });
                }
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().to_string();
            if section.starts_with("EPISODE") {
                let Some(ep) = ini.episodes.last_mut() else { continue };
                match k.as_str() {
                    "title" => ep.title = v,
                    "bloodbathonly" => ep.bloodbath_only = v.trim() != "0",
                    "cutscenea" => ep.cutscene_a = Some(v),
                    "cutwava" => ep.wav_a = Some(v),
                    "cutsceneb" => ep.cutscene_b = Some(v),
                    "cutwavb" => ep.wav_b = Some(v),
                    "cutscenealevel" => ep.cutscene_a_level = v.trim().parse().unwrap_or(1),
                    _ if k.starts_with("map") && k[3..].chars().all(|c| c.is_ascii_digit()) => {
                        let n: usize = k[3..].parse().unwrap_or(0);
                        if n > 0 {
                            if ep.maps.len() < n {
                                ep.maps.resize(n, String::new());
                            }
                            ep.maps[n - 1] = v.to_ascii_uppercase();
                        }
                    }
                    _ => {}
                }
            } else if let Some(m) = ini.maps.get_mut(&section) {
                match k.as_str() {
                    "title" => m.title = v,
                    "author" => m.author = v,
                    "song" => m.song = v.to_ascii_uppercase(),
                    "track" => m.track = v.parse().unwrap_or(0),
                    "endinga" => m.ending_a = v.parse().unwrap_or(0),
                    "endingb" => m.ending_b = v.parse().unwrap_or(0),
                    "fog" => m.fog = v.trim() == "1",
                    _ if k.starts_with("message") => {
                        if let Ok(n) = k[7..].parse::<u32>() {
                            m.messages.insert(n, v);
                        }
                    }
                    _ => {}
                }
            }
        }
        for ep in ini.episodes.iter_mut() {
            ep.maps.retain(|m| !m.is_empty());
        }
        ini
    }

    pub fn map(&self, name: &str) -> Option<&MapInfo> {
        self.maps.get(&name.to_ascii_uppercase())
    }

    /// (episode index, map index) of a map name.
    pub fn locate(&self, name: &str) -> Option<(usize, usize)> {
        let n = name.to_ascii_uppercase();
        for (e, ep) in self.episodes.iter().enumerate() {
            if let Some(m) = ep.maps.iter().position(|m| *m == n) {
                return Some((e, m));
            }
        }
        None
    }

    /// The map that follows `name` for a normal or secret exit (None = episode finished).
    pub fn next_map(&self, name: &str, secret: bool) -> Option<String> {
        let (e, m) = self.locate(name)?;
        let ep = &self.episodes[e];
        let info = self.map(name);
        let target = info.map(|i| if secret { i.ending_b } else { i.ending_a }).unwrap_or(0);
        let idx = if target > 0 { target - 1 } else { m + 1 };
        if idx == m && target > 0 && !secret {
            return None;
        }
        ep.maps.get(idx).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Episode1]\nTitle = The Way of All Flesh\nMap1 = e1m1\nMap2 = e1m2\nMap8 = e1m8\n\n[e1m1]\nTitle = Cradle to Grave\nSong = UNHOLY\nTrack = 3\nEndingA = 2\nEndingB = 8\nMessage2 = Hit the switch to end the level.\n";

    #[test]
    fn parses_episode_and_map() {
        let ini = BloodIni::parse(SAMPLE);
        assert_eq!(ini.episodes[0].title, "The Way of All Flesh");
        assert_eq!(ini.episodes[0].maps, vec!["E1M1", "E1M2", "E1M8"]);
        let m = ini.map("e1m1").unwrap();
        assert_eq!((m.track, m.song.as_str()), (3, "UNHOLY"));
        assert_eq!(m.messages[&2], "Hit the switch to end the level.");
        assert_eq!(ini.next_map("E1M1", false).as_deref(), Some("E1M2"));
    }
}
