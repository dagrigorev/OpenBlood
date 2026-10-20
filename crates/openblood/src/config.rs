//! `openblood.ini` settings (simple `key = value` lines, `;` comments).

use std::path::{Path, PathBuf};

/// Selectable output resolutions (besides "native" = the window's own size).
pub const RESOLUTIONS: &[(u32, u32)] = &[
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 720),
    (1280, 1024),
    (1600, 900),
    (1920, 1080),
    (1920, 1200),
    (2560, 1080),
    (2560, 1440),
    (3440, 1440),
    (3840, 2160),
];

/// How the rendered image is scaled to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upscale {
    /// Blocky pixels (classic look).
    Nearest,
    /// Bilinear filtering.
    Smooth,
    /// Crisp pixels with smoothed edges between them (no uneven pixel sizes).
    Sharp,
}

impl Upscale {
    pub const ALL: [Upscale; 3] = [Upscale::Nearest, Upscale::Smooth, Upscale::Sharp];
    pub fn name(self) -> &'static str {
        match self {
            Upscale::Nearest => "Nearest",
            Upscale::Smooth => "Smooth",
            Upscale::Sharp => "Sharp",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Upscale::Nearest => "nearest",
            Upscale::Smooth => "smooth",
            Upscale::Sharp => "sharp",
        }
    }
}

/// Frame-rate limits offered in the menu (0 = unlimited).
pub const MAX_FPS: &[u32] = &[0, 60, 120, 144, 240];

/// Supersampling levels: (menu label, render size multiplier per axis).
pub const ANTIALIAS: &[(&str, f64)] = &[("Off", 1.0), ("2x", std::f64::consts::SQRT_2), ("4x", 2.0)];

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
    /// Output resolution; None = native (window size).
    pub resolution: Option<(u32, u32)>,
    pub upscale: Upscale,
    /// Index into [`ANTIALIAS`].
    pub antialias: usize,
    /// Frame-rate cap (0 = unlimited).
    pub max_fps: u32,
    /// Where the settings were loaded from (saved back there).
    pub ini_path: Option<PathBuf>,
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
            resolution: None,
            upscale: Upscale::Sharp,
            antialias: 0,
            max_fps: 0,
            ini_path: None,
        }
    }
}

pub const DEFAULT_INI: &str = "; OpenBlood configuration
; Path to your ORIGINAL Blood installation (folder with BLOOD.RFF, SOUNDS.RFF, TILES*.ART)
data_dir = .
fullscreen = 1
window_width = 1600
window_height = 900
; native, or a size like 1920x1080 (the image is fitted to the window)
resolution = native
; 1.0 renders at the full resolution; lower values trade sharpness for speed
render_scale = 1.0
; nearest, smooth or sharp: how the image is scaled to the window
upscale = sharp
; anti-aliasing (supersampling): 0 = off, 1 = 2x, 2 = 4x
antialias = 0
; frame-rate cap: 0 = unlimited, or e.g. 60 / 120 / 144
max_fps = 0
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
        let mut c = Config {
            ini_path: Some(path.to_path_buf()),
            ..Config::default()
        };
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
                "resolution" => c.resolution = parse_resolution(v),
                "upscale" => {
                    if let Some(u) = Upscale::ALL.iter().find(|u| u.key() == v.to_ascii_lowercase()) {
                        c.upscale = *u;
                    }
                }
                "antialias" => c.antialias = v.parse::<usize>().unwrap_or(0).min(ANTIALIAS.len() - 1),
                "max_fps" => c.max_fps = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        c
    }
}

fn parse_resolution(v: &str) -> Option<(u32, u32)> {
    let (w, h) = v.to_ascii_lowercase().split_once('x').map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w >= 320 && h >= 200).then_some((w, h))
}

impl Config {
    /// Writes the settings back to the ini file they were loaded from.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.ini_path else { return Ok(()) };
        let b = |v: bool| if v { 1 } else { 0 };
        let res = match self.resolution {
            Some((w, h)) => format!("{w}x{h}"),
            None => "native".into(),
        };
        let data_dir = self.data_dir.to_string_lossy();
        let mut out = format!(
            "; OpenBlood configuration
; Path to your ORIGINAL Blood installation (folder with BLOOD.RFF, SOUNDS.RFF, TILES*.ART)
data_dir = {data_dir}
fullscreen = {}
window_width = {}
window_height = {}
; native, or a size like 1920x1080 (the image is fitted to the window)
resolution = {res}
; 1.0 renders at the full resolution; lower values trade sharpness for speed
render_scale = {}
; nearest, smooth or sharp: how the image is scaled to the window
upscale = {}
; anti-aliasing (supersampling): 0 = off, 1 = 2x, 2 = 4x
antialias = {}
; frame-rate cap: 0 = unlimited, or e.g. 60 / 120 / 144
max_fps = {}
mouse_sensitivity = {}
invert_mouse = {}
always_run = {}
sound = {}
music = {}
volume = {}
music_volume = {}
skill = {}
",
            b(self.fullscreen),
            self.window_width,
            self.window_height,
            self.render_scale,
            self.upscale.key(),
            self.antialias,
            self.max_fps,
            self.mouse_sensitivity,
            b(self.invert_mouse),
            b(self.always_run),
            b(self.sound),
            b(self.music),
            self.volume,
            self.music_volume,
            self.skill,
        );
        match &self.start_map {
            Some(m) => out.push_str(&format!("start_map = {m}
")),
            None => out.push_str("; start_map = E1M1
"),
        }
        std::fs::write(path, out)
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
        assert_eq!(c.resolution, None);
    }

    #[test]
    fn video_settings_round_trip() {
        let dir = std::env::temp_dir().join(format!("ob_cfg_rt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("openblood.ini");
        std::fs::write(&p, "resolution = 1920x1080
upscale = smooth
antialias = 2
").unwrap();
        let mut c = Config::load(&p);
        assert_eq!(c.resolution, Some((1920, 1080)));
        assert_eq!(c.upscale, Upscale::Smooth);
        assert_eq!(c.antialias, 2);
        c.resolution = None;
        c.upscale = Upscale::Nearest;
        c.save().unwrap();
        let c2 = Config::load(&p);
        assert_eq!(c2.resolution, None);
        assert_eq!(c2.upscale, Upscale::Nearest);
        assert_eq!(c2.antialias, 2);
    }
}
