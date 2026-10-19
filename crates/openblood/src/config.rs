//! `openblood.ini` settings (simple `key = value` lines, `;` comments).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    /// Directory holding the original game files (BLOOD.RFF, TILES*.ART, ...).
    pub data_dir: PathBuf,
    pub fullscreen: bool,
    pub window_width: u32,
    pub window_height: u32,
    /// Fraction of the window resolution used for 3D rendering (1.0 = native, e.g. 4K).
    pub render_scale: f64,
    pub mouse_sensitivity: f64,
    pub invert_mouse: bool,
    pub always_run: bool,
    pub sound: bool,
    pub music: bool,
    pub volume: f64,
    pub music_volume: f64,
    pub start_map: Option<String>,
    pub skill: i32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            data_dir: PathBuf::from("."),
            fullscreen: true,
            window_width: 1600,
            window_height: 900,
            render_scale: 1.0,
            mouse_sensitivity: 0.35,
            invert_mouse: false,
            always_run: true,
            sound: true,
            music: true,
            volume: 0.8,
            music_volume: 0.6,
            start_map: None,
            skill: 2,
        }
    }
}

pub const DEFAULT_INI: &str = "; OpenBlood configuration
; Path to your ORIGINAL Blood installation (folder with BLOOD.RFF, SOUNDS.RFF, TILES*.ART)
data_dir = .
fullscreen = 1
window_width = 1600
window_height = 900
; 1.0 renders at native resolution (e.g. 3840x2160); lower values trade sharpness for speed
render_scale = 1.0
mouse_sensitivity = 0.35
invert_mouse = 0
always_run = 1
sound = 1
music = 1
volume = 0.8
music_volume = 0.6
skill = 2
; start_map = E1M1
";

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

impl Config {
    pub fn load(path: &Path) -> Config {
        let mut c = Config::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            let _ = std::fs::write(path, DEFAULT_INI);
            return c;
        };
        let base = path.parent().unwrap_or(Path::new("."));
        for line in text.lines() {
            let line = line.split(';').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            match k.as_str() {
                "data_dir" => {
                    let p = PathBuf::from(v);
                    c.data_dir = if p.is_absolute() { p } else { base.join(p) };
                }
                "fullscreen" => c.fullscreen = truthy(v),
                "window_width" => c.window_width = v.parse().unwrap_or(c.window_width),
                "window_height" => c.window_height = v.parse().unwrap_or(c.window_height),
                "render_scale" => c.render_scale = v.parse().unwrap_or(c.render_scale),
                "mouse_sensitivity" => c.mouse_sensitivity = v.parse().unwrap_or(c.mouse_sensitivity),
                "invert_mouse" => c.invert_mouse = truthy(v),
                "always_run" => c.always_run = truthy(v),
                "sound" => c.sound = truthy(v),
                "music" => c.music = truthy(v),
                "volume" => c.volume = v.parse().unwrap_or(c.volume),
                "music_volume" => c.music_volume = v.parse().unwrap_or(c.music_volume),
                "start_map" => c.start_map = Some(v.to_string()),
                "skill" => c.skill = v.parse().unwrap_or(c.skill),
                _ => {}
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_values() {
        let dir = std::env::temp_dir().join(format!("ob_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("openblood.ini");
        std::fs::write(&p, "fullscreen = 0\nrender_scale = 0.5 ; half\nstart_map=E2M1\n").unwrap();
        let c = Config::load(&p);
        assert!(!c.fullscreen);
        assert_eq!(c.render_scale, 0.5);
        assert_eq!(c.start_map.as_deref(), Some("E2M1"));
    }
}
