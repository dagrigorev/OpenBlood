//! Front-end menus drawn with the original Blood font and art tiles.

use crate::hud::{tile_text, tile_text_width, vtile, Anchor};
use ob_engine::render::ShadeLuts;
use ob_game::resources::Resources;

pub const SKILLS: [&str; 5] = ["Still Kicking", "Pink On The Inside", "Lightly Broiled", "Well Done", "Extra Crispy"];

/// Title-screen art tile ("BLOOD" logo).
const TITLE_TILE: i32 = 2046;
/// Full-screen menu backdrop.
const BACKDROP_TILE: i32 = 2049;
const MENU_FONT: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Main,
    Episode,
    Skill,
    Pause,
    Options,
    Help,
    Credits,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    None,
    NewGame { episode: usize, skill: i32 },
    Resume,
    Quit,
    ToggleFullscreen,
    ScaleChanged(f64),
    MouseSensitivity(f64),
    ToggleAlwaysRun,
    ToggleMusic,
}

pub struct Menu {
    pub screen: Screen,
    pub cursor: usize,
    pub episode: usize,
    stack: Vec<(Screen, usize)>,
    pub in_game: bool,
}

impl Menu {
    pub fn new() -> Self {
        Menu { screen: Screen::Main, cursor: 0, episode: 0, stack: Vec::new(), in_game: false }
    }

    pub fn open_pause(&mut self) {
        self.screen = Screen::Pause;
        self.cursor = 0;
        self.stack.clear();
        self.in_game = true;
    }

    fn items(&self, res: &Resources, opts: &OptionsView) -> Vec<String> {
        match self.screen {
            Screen::Main => vec!["New Game", "Options", "Help", "Credits", "Quit"].into_iter().map(String::from).collect(),
            Screen::Pause => vec!["Resume", "New Game", "Options", "Help", "Quit"].into_iter().map(String::from).collect(),
            Screen::Episode => {
                let mut v: Vec<String> = res.ini.episodes.iter().filter(|e| !e.bloodbath_only).map(|e| e.title.clone()).collect();
                if v.is_empty() {
                    v.push("Episode 1".into());
                }
                v
            }
            Screen::Skill => SKILLS.iter().map(|s| s.to_string()).collect(),
            Screen::Options => vec![
                format!("Fullscreen: {}", if opts.fullscreen { "On" } else { "Off" }),
                format!("Render Scale: {:.0}%", opts.scale * 100.0),
                format!("Mouse Speed: {:.2}", opts.mouse),
                format!("Always Run: {}", if opts.always_run { "On" } else { "Off" }),
                format!("Music: {}", if opts.music { "On" } else { "Off" }),
            ],
            Screen::Help | Screen::Credits => vec![],
        }
    }

    fn push(&mut self, s: Screen) {
        self.stack.push((self.screen, self.cursor));
        self.screen = s;
        self.cursor = 0;
    }

    /// Returns true if the menu wants to close (back out of the root).
    pub fn back(&mut self) -> bool {
        if let Some((s, c)) = self.stack.pop() {
            self.screen = s;
            self.cursor = c;
            false
        } else {
            true
        }
    }

    pub fn up(&mut self, res: &Resources, opts: &OptionsView) {
        let n = self.items(res, opts).len().max(1);
        self.cursor = (self.cursor + n - 1) % n;
    }

    pub fn down(&mut self, res: &Resources, opts: &OptionsView) {
        let n = self.items(res, opts).len().max(1);
        self.cursor = (self.cursor + 1) % n;
    }

    /// Left/right adjustments on the options screen.
    pub fn adjust(&mut self, dir: i32, opts: &OptionsView) -> MenuAction {
        if self.screen != Screen::Options {
            return MenuAction::None;
        }
        match self.cursor {
            1 => MenuAction::ScaleChanged((opts.scale + 0.25 * dir as f64).clamp(0.25, 1.0)),
            2 => MenuAction::MouseSensitivity((opts.mouse + 0.05 * dir as f64).clamp(0.05, 3.0)),
            _ => self.select(opts),
        }
    }

    pub fn select(&mut self, opts: &OptionsView) -> MenuAction {
        let _ = opts;
        match (self.screen, self.cursor) {
            (Screen::Main, 0) | (Screen::Pause, 1) => {
                self.push(Screen::Episode);
                MenuAction::None
            }
            (Screen::Main, 1) | (Screen::Pause, 2) => {
                self.push(Screen::Options);
                MenuAction::None
            }
            (Screen::Main, 2) | (Screen::Pause, 3) => {
                self.push(Screen::Help);
                MenuAction::None
            }
            (Screen::Main, 3) => {
                self.push(Screen::Credits);
                MenuAction::None
            }
            (Screen::Main, 4) | (Screen::Pause, 4) => MenuAction::Quit,
            (Screen::Pause, 0) => MenuAction::Resume,
            (Screen::Episode, e) => {
                self.episode = e;
                self.push(Screen::Skill);
                self.cursor = 2;
                MenuAction::None
            }
            (Screen::Skill, s) => MenuAction::NewGame { episode: self.episode, skill: s as i32 },
            (Screen::Options, 0) => MenuAction::ToggleFullscreen,
            (Screen::Options, 1) => MenuAction::ScaleChanged(if opts.scale >= 1.0 { 0.5 } else { 1.0 }),
            (Screen::Options, 3) => MenuAction::ToggleAlwaysRun,
            (Screen::Options, 4) => MenuAction::ToggleMusic,
            (Screen::Help, _) | (Screen::Credits, _) => {
                self.back();
                MenuAction::None
            }
            _ => MenuAction::None,
        }
    }

    pub fn draw(&self, frame: &mut [u32], w: usize, h: usize, res: &Resources, luts: &ShadeLuts, clock: f64, opts: &OptionsView) {
        if self.in_game {
            // Darken the game view behind the menu.
            for p in frame.iter_mut() {
                *p = (*p >> 2) & 0x003f3f3f;
            }
        } else {
            for p in frame.iter_mut() {
                *p = 0;
            }
            if !res.tiles.get(TITLE_TILE).is_empty() && self.screen == Screen::Main {
                vtile(frame, w, h, res, luts, TITLE_TILE, 160.0 - 160.0 * 0.5, 2.0, Anchor::Center, 0.5, 0, 0);
            } else {
                vtile(frame, w, h, res, luts, BACKDROP_TILE, 0.0, 0.0, Anchor::Center, 1.0, 8, 0);
            }
        }
        match self.screen {
            Screen::Help => {
                // The original "default keys" art is replaced by OpenBlood's own key list.
                let lines = [
                    "W A S D / ARROWS - MOVE",
                    "MOUSE - LOOK AND TURN",
                    "LEFT MOUSE / F - FIRE",
                    "E / ENTER - USE, OPEN DOORS",
                    "SPACE - JUMP   C / CTRL - CROUCH",
                    "SHIFT - RUN   1-0 - WEAPONS",
                    "F11 / ALT+ENTER - FULLSCREEN",
                    "F5 - RENDER SCALE   F12 - SCREENSHOT",
                    "[ ] - PREVIOUS / NEXT LEVEL",
                    "ESC - MENU",
                ];
                for (i, l) in lines.iter().enumerate() {
                    let tw = tile_text_width(res, 0, l, 1.0);
                    tile_text(frame, w, h, res, luts, 0, l, 160.0 - tw / 2.0, 50.0 + i as f64 * 10.0, Anchor::Center, 1.0, 0, 0);
                }
                return;
            }
            Screen::Credits => {
                let lines = [
                    "OPENBLOOD",
                    "AN OPEN-SOURCE ENGINE FOR BLOOD (1997)",
                    "WRITTEN FROM SCRATCH IN RUST",
                    "",
                    "BLOOD IS (C) 1997 MONOLITH PRODUCTIONS",
                    "GAME DATA IS LOADED FROM YOUR OWN COPY",
                    "NO ORIGINAL GAME CONTENT IS INCLUDED",
                ];
                for (i, l) in lines.iter().enumerate() {
                    let tw = tile_text_width(res, 0, l, 1.0);
                    tile_text(frame, w, h, res, luts, 0, l, 160.0 - tw / 2.0, 60.0 + i as f64 * 11.0, Anchor::Center, 1.0, 0, 0);
                }
                return;
            }
            _ => {}
        }
        let title = match self.screen {
            Screen::Episode => "Episode",
            Screen::Skill => "Difficulty",
            Screen::Options => "Options",
            Screen::Pause => "Paused",
            _ => "",
        };
        let items = self.items(res, opts);
        let top = if self.screen == Screen::Main { 110.0 } else { 55.0 };
        if !title.is_empty() {
            let tw = tile_text_width(res, MENU_FONT, title, 1.0);
            tile_text(frame, w, h, res, luts, MENU_FONT, title, 160.0 - tw / 2.0, top - 30.0, Anchor::Center, 1.0, 0, 2);
        }
        let spacing = if items.len() > 5 { 16.0 } else { 18.0 };
        for (i, it) in items.iter().enumerate() {
            let tw = tile_text_width(res, MENU_FONT, it, 1.0);
            let sel = i == self.cursor;
            // Selected entry pulses, like the original menu highlight.
            let shade = if sel { ((clock * 8.0).sin() * 8.0 + 8.0) as i32 } else { 24 };
            tile_text(frame, w, h, res, luts, MENU_FONT, it, 160.0 - tw / 2.0, top + i as f64 * spacing, Anchor::Center, 1.0, shade, 0);
        }
    }
}

/// Current option values shown in the options menu.
pub struct OptionsView {
    pub fullscreen: bool,
    pub scale: f64,
    pub mouse: f64,
    pub always_run: bool,
    pub music: bool,
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> OptionsView {
        OptionsView { fullscreen: true, scale: 1.0, mouse: 0.35, always_run: true, music: true }
    }

    #[test]
    fn navigation_to_new_game() {
        let mut m = Menu::new();
        let o = opts();
        assert_eq!(m.select(&o), MenuAction::None);
        assert_eq!(m.screen, Screen::Episode);
        m.select(&o);
        assert_eq!(m.screen, Screen::Skill);
        assert_eq!(m.cursor, 2);
        assert_eq!(m.select(&o), MenuAction::NewGame { episode: 0, skill: 2 });
        assert!(!m.back());
        assert!(!m.back());
        assert!(m.back(), "root reached");
    }
}
