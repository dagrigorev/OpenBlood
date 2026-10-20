//! Window, input and presentation loop.

use crate::audio::Audio;
use crate::config::Config;
use crate::hud;
use ob_engine::render::{Renderer, Scene, ShadeLuts};
use ob_game::game::{Game, GameEvent};
use ob_game::player::Input;
use ob_game::resources::Resources;
use std::collections::HashSet;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

pub struct App {
    pub cfg: Config,
    pub res: Resources,
    pub game: Game,
    pub level_index: usize,
    pub levels: Vec<String>,
    luts: ShadeLuts,
    luts_water: ShadeLuts,
    luts_beast: ShadeLuts,
    renderer: Renderer,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    keys: HashSet<KeyCode>,
    mouse_buttons: HashSet<MouseButton>,
    mouse_dx: f64,
    mouse_dy: f64,
    last: Instant,
    fps_time: Instant,
    fps_frames: u32,
    fps: f64,
    grabbed: bool,
    pub audio: Option<Audio>,
    show_stats: bool,
    audio_map: String,
    pub menu: crate::menu::Menu,
    pub menu_open: bool,
    start_time: Instant,
    /// Window-sized image (letterboxed modes) the HUD is drawn on before presenting.
    out: Vec<u32>,
    /// F12 pressed: save the next presented frame.
    shot_pending: bool,
    /// When the next capped frame may start.
    pace_next: Option<Instant>,
    /// Cutscene playing, and those queued after it.
    movie: Option<crate::movie::Movie>,
    movie_queue: std::collections::VecDeque<crate::movie::MovieFile>,
    prof_log: Vec<String>,
    last_switch: Instant,
    message: Option<(String, Instant)>,
}

impl App {
    pub fn new(cfg: Config, res: Resources, game: Game, levels: Vec<String>, level_index: usize) -> Self {
        let luts = res.build_luts(0);
        let luts_water = res.build_luts(1);
        let luts_beast = res.build_luts(2);
        let mut audio = if cfg.sound { Audio::new(&res).ok() } else { None };
        if let Some(a) = &mut audio {
            a.set_volumes(cfg.volume as f32, if cfg.music { cfg.music_volume as f32 } else { 0.0 });
        }
        App {
            cfg,
            luts,
            luts_water,
            luts_beast,
            res,
            game,
            level_index,
            levels,
            renderer: {
                let mut r = Renderer::new(640, 400);
                r.keep_rows = false;
                r
            },
            window: None,
            surface: None,
            keys: HashSet::new(),
            mouse_buttons: HashSet::new(),
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            last: Instant::now(),
            fps_time: Instant::now(),
            fps_frames: 0,
            fps: 0.0,
            grabbed: false,
            audio,
            show_stats: false,
            audio_map: String::new(),
            menu: crate::menu::Menu::new(),
            menu_open: false,
            start_time: Instant::now(),
            out: Vec::new(),
            shot_pending: false,
            pace_next: None,
            movie: None,
            movie_queue: Default::default(),
            prof_log: Vec::new(),
            last_switch: Instant::now(),
            message: None,
        }
    }

    fn grab(&mut self, on: bool) {
        if let Some(w) = &self.window {
            if on {
                let _ = w
                    .set_cursor_grab(CursorGrabMode::Confined)
                    .or_else(|_| w.set_cursor_grab(CursorGrabMode::Locked));
                w.set_cursor_visible(false);
            } else {
                let _ = w.set_cursor_grab(CursorGrabMode::None);
                w.set_cursor_visible(true);
            }
            self.grabbed = on;
        }
    }

    fn down(&self, k: KeyCode) -> bool {
        self.keys.contains(&k)
    }

    fn build_input(&mut self, dt: f64) -> Input {
        let mut i = Input::default();
        let fwd = self.down(KeyCode::KeyW) || self.down(KeyCode::ArrowUp);
        let back = self.down(KeyCode::KeyS) || self.down(KeyCode::ArrowDown);
        let left = self.down(KeyCode::KeyA);
        let right = self.down(KeyCode::KeyD);
        i.forward = fwd as i32 as f64 - back as i32 as f64;
        i.strafe = right as i32 as f64 - left as i32 as f64;
        let mut turn = 0.0;
        if self.down(KeyCode::ArrowLeft) {
            turn -= 1.0;
        }
        if self.down(KeyCode::ArrowRight) {
            turn += 1.0;
        }
        i.run = self.down(KeyCode::ShiftLeft) || self.down(KeyCode::ShiftRight) || self.cfg.always_run;
        i.turn = turn * 1024.0 * dt * if i.run { 1.5 } else { 1.0 } + self.mouse_dx * self.cfg.mouse_sensitivity;
        let inv = if self.cfg.invert_mouse { -1.0 } else { 1.0 };
        i.look = -self.mouse_dy * self.cfg.mouse_sensitivity * 0.5 * inv;
        if self.down(KeyCode::PageUp) {
            i.look += 120.0 * dt;
        }
        if self.down(KeyCode::PageDown) {
            i.look -= 120.0 * dt;
        }
        i.center_view = self.down(KeyCode::End);
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
        i.jump = self.down(KeyCode::Space);
        i.crouch = self.down(KeyCode::KeyC) || self.down(KeyCode::ControlLeft);
        i.use_key = self.down(KeyCode::KeyE);
        let ctrl = self.down(KeyCode::ControlLeft) || self.down(KeyCode::ControlRight);
        i.inv_prev = self.down(KeyCode::BracketLeft) && !ctrl;
        i.inv_next = self.down(KeyCode::BracketRight) && !ctrl;
        i.inv_use = self.down(KeyCode::Enter) && !self.down(KeyCode::AltLeft);
        i.fire = self.mouse_buttons.contains(&MouseButton::Left)
            || self.down(KeyCode::KeyF)
            || self.down(KeyCode::ControlRight);
        i.alt_fire = self.mouse_buttons.contains(&MouseButton::Right);
        for (k, n) in [
            (KeyCode::Digit1, 1),
            (KeyCode::Digit2, 2),
            (KeyCode::Digit3, 3),
            (KeyCode::Digit4, 4),
            (KeyCode::Digit5, 5),
            (KeyCode::Digit6, 6),
            (KeyCode::Digit7, 7),
            (KeyCode::Digit8, 8),
            (KeyCode::Digit9, 9),
            (KeyCode::Digit0, 10),
        ] {
            if self.down(k) {
                i.weapon = Some(n);
            }
        }
        i
    }

    fn change_level(&mut self, delta: isize) {
        if self.levels.is_empty() {
            return;
        }
        let n = self.levels.len() as isize;
        self.level_index = ((self.level_index as isize + delta).rem_euclid(n)) as usize;
        let name = self.levels[self.level_index].clone();
        self.load_named(&name, false);
    }

    /// Loads a level; `carry` keeps the player's weapons, health and armour.
    fn load_named(&mut self, name: &str, carry: bool) {
        match Game::load_level_skill(&self.res, name, self.cfg.skill) {
            Ok(mut g) => {
                if carry {
                    g.arsenal = self.game.arsenal.clone();
                    g.arsenal.state = ob_game::weapons::WeaponState::Raising;
                    g.arsenal.qav = g.arsenal.def().up;
                    g.arsenal.qav_time = 0;
                    g.player.health = self.game.player.health.max(1);
                    g.player.armor = self.game.player.armor;
                }
                if let Some(i) = self.levels.iter().position(|l| l.eq_ignore_ascii_case(name)) {
                    self.level_index = i;
                }
                self.game = g;
                self.audio_map.clear();
                self.queue_episode_movie(name, true);
                let title = self.res.ini.map(name).map(|m| m.title.clone()).unwrap_or_default();
                self.flash(if title.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}: {title}")
                });
            }
            Err(e) => self.flash(format!("Cannot load {name}: {e}")),
        }
    }

    fn level_exit(&mut self, secret: bool) {
        let cur = self.game.world.map_name.clone();
        if !secret && self.res.ini.next_map(&cur, false).is_none() {
            self.queue_episode_movie(&cur, false);
        }
        let next = self.res.ini.next_map(&cur, secret).or_else(|| {
            // Episode finished: continue with the first map of the next episode.
            let (e, _) = self.res.ini.locate(&cur)?;
            self.res.ini.episodes.get(e + 1).and_then(|ep| ep.maps.first().cloned())
        });
        match next {
            Some(n) => self.load_named(&n, true),
            None => self.flash("The End. Thanks for playing!".into()),
        }
    }

    /// Queues cutscenes to play before the game continues.
    pub fn queue_movies(&mut self, files: Vec<crate::movie::MovieFile>) {
        self.movie_queue.extend(files);
    }

    /// Cutscene A of the episode when `map` is its cutscene level (`entering`), or the
    /// episode's ending cutscene B (when leaving its last map).
    fn queue_episode_movie(&mut self, map: &str, entering: bool) {
        let Some((e, m)) = self.res.ini.locate(map) else { return };
        let ep = &self.res.ini.episodes[e];
        let (video, wav) = if entering {
            if m + 1 != ep.cutscene_a_level.max(1) {
                return;
            }
            (ep.cutscene_a.clone(), ep.wav_a.clone())
        } else {
            (ep.cutscene_b.clone(), ep.wav_b.clone())
        };
        let dir = self.res.dir.clone();
        if let Some(video) = video.and_then(|v| crate::movie::find(&dir, &v)) {
            let wav = wav.and_then(|w| crate::movie::find(&dir, &w));
            self.movie_queue.push_back(crate::movie::MovieFile { video, wav });
        }
    }

    /// Starts the next queued cutscene if none is playing; advances the current one.
    /// Returns true while a cutscene is on screen.
    fn update_movie(&mut self) -> bool {
        loop {
            if let Some(m) = &mut self.movie {
                if m.update() {
                    return true;
                }
                self.movie = None;
                if let Some(a) = &mut self.audio {
                    a.stop_movie();
                }
                self.last = Instant::now();
            }
            let Some(next) = self.movie_queue.pop_front() else { return false };
            if let Some(m) = crate::movie::Movie::open(&next.video) {
                if let (Some(a), Some(w)) = (&mut self.audio, next.wav.as_ref()) {
                    if let Some((samples, rate)) = crate::movie::load_wav_u8(w) {
                        a.play_movie_sound(samples, rate);
                    }
                }
                self.movie = Some(m);
            }
        }
    }

    /// Skips the current cutscene (any key or mouse button).
    fn skip_movie(&mut self) {
        if self.movie.take().is_some() {
            if let Some(a) = &mut self.audio {
                a.stop_movie();
            }
            self.last = Instant::now();
        }
    }

    fn options_view(&self) -> crate::menu::OptionsView {
        crate::menu::OptionsView {
            fullscreen: self.window.as_ref().is_some_and(|w| w.fullscreen().is_some()),
            resolution: self.cfg.resolution,
            upscale: self.cfg.upscale,
            antialias: self.cfg.antialias,
            max_fps: self.cfg.max_fps,
            scale: self.cfg.render_scale,
            mouse: self.cfg.mouse_sensitivity,
            always_run: self.cfg.always_run,
            music: self.cfg.music,
            slots: (1..=crate::menu::SAVE_SLOTS)
                .map(|k| ob_game::game::save::slot_title(&ob_game::game::save::slot_path(&self.save_dir(), k)))
                .collect(),
        }
    }

    fn save_dir(&self) -> std::path::PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("saves")))
            .unwrap_or_else(|| std::path::PathBuf::from("saves"))
    }

    fn save_slot(&mut self, slot: usize) {
        let map = self.game.world.map_name.clone();
        let title = self.res.ini.map(&map).map(|m| m.title.clone()).unwrap_or_default();
        let secs = self.game.clock / 120;
        let label = format!("{map} {title} {}:{:02}", secs / 60, secs % 60);
        let path = ob_game::game::save::slot_path(&self.save_dir(), slot);
        match self.game.save_to(&path, &label) {
            Ok(()) => self.flash(if slot == 0 {
                "Quick saved".into()
            } else {
                format!("Saved to slot {slot}")
            }),
            Err(e) => self.flash(format!("Save failed: {e}")),
        }
    }

    fn load_slot(&mut self, slot: usize) {
        let path = ob_game::game::save::slot_path(&self.save_dir(), slot);
        match ob_game::game::Game::load_from(&self.res, &path) {
            Ok(g) => {
                if let Some(i) = self
                    .levels
                    .iter()
                    .position(|l| l.eq_ignore_ascii_case(&g.world.map_name))
                {
                    self.level_index = i;
                }
                self.game = g;
                self.audio_map.clear();
                self.last = Instant::now();
                self.flash("Game loaded".into());
            }
            Err(e) => self.flash(format!("Load failed: {e}")),
        }
    }

    fn close_menu(&mut self) {
        self.menu_open = false;
        self.keys.clear();
        self.last = Instant::now();
        self.grab(true);
    }

    fn menu_key(&mut self, el: &ActiveEventLoop, code: KeyCode) {
        use crate::menu::MenuAction;
        let o = self.options_view();
        let action = match code {
            KeyCode::ArrowUp | KeyCode::KeyW => {
                self.menu.up(&self.res, &o);
                MenuAction::None
            }
            KeyCode::ArrowDown | KeyCode::KeyS => {
                self.menu.down(&self.res, &o);
                MenuAction::None
            }
            KeyCode::ArrowLeft | KeyCode::KeyA => self.menu.adjust(-1, &o),
            KeyCode::ArrowRight | KeyCode::KeyD => self.menu.adjust(1, &o),
            KeyCode::Enter | KeyCode::Space | KeyCode::KeyE => self.menu.select(&o),
            _ => MenuAction::None,
        };
        if let Some(a) = &mut self.audio {
            if code != KeyCode::Escape {
                a.play(&self.res, 778, None, false, (1.0, 2.0), -1);
            }
        }
        let persist = matches!(
            action,
            MenuAction::ToggleFullscreen
                | MenuAction::ScaleChanged(_)
                | MenuAction::SetResolution(_)
                | MenuAction::SetUpscale(_)
                | MenuAction::SetAntialias(_)
                | MenuAction::SetMaxFps(_)
                | MenuAction::MouseSensitivity(_)
                | MenuAction::ToggleAlwaysRun
                | MenuAction::ToggleMusic
        );
        match action {
            MenuAction::None => {}
            MenuAction::Quit => el.exit(),
            MenuAction::Resume => self.close_menu(),
            MenuAction::NewGame { episode, skill } => {
                self.cfg.skill = skill;
                let eps: Vec<_> = self.res.ini.episodes.iter().filter(|e| !e.bloodbath_only).collect();
                let map = eps
                    .get(episode)
                    .and_then(|e| e.maps.first().cloned())
                    .unwrap_or_else(|| "E1M1".into());
                self.load_named(&map, false);
                self.menu = crate::menu::Menu::new();
                self.close_menu();
            }
            MenuAction::Save(k) => {
                self.save_slot(k);
                self.close_menu();
            }
            MenuAction::Load(k) => {
                self.load_slot(k);
                self.menu = crate::menu::Menu::new();
                self.close_menu();
            }
            MenuAction::ToggleFullscreen => self.toggle_fullscreen(),
            MenuAction::ScaleChanged(v) => self.cfg.render_scale = v,
            MenuAction::SetResolution(r) => self.cfg.resolution = r,
            MenuAction::SetUpscale(u) => self.cfg.upscale = u,
            MenuAction::SetAntialias(a) => self.cfg.antialias = a,
            MenuAction::SetMaxFps(f) => {
                self.cfg.max_fps = f;
                self.pace_next = None;
            }
            MenuAction::MouseSensitivity(v) => self.cfg.mouse_sensitivity = v,
            MenuAction::ToggleAlwaysRun => self.cfg.always_run = !self.cfg.always_run,
            MenuAction::ToggleMusic => {
                self.cfg.music = !self.cfg.music;
                if let Some(a) = &mut self.audio {
                    a.set_volumes(
                        self.cfg.volume as f32,
                        if self.cfg.music {
                            self.cfg.music_volume as f32
                        } else {
                            0.0
                        },
                    );
                }
            }
        }
        if persist {
            if let Err(e) = self.cfg.save() {
                eprintln!("could not save openblood.ini: {e}");
            }
        }
    }

    fn flash(&mut self, m: String) {
        self.message = Some((m, Instant::now()));
    }

    fn frame(&mut self) {
        if self.update_movie() {
            self.draw_movie();
            return;
        }
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;
        let mut prof = Profile::new(dt);
        let input = self.build_input(dt);
        if !self.menu_open {
            self.game.update(&self.res, &input, dt);
        }
        prof.mark("update");

        let events: Vec<GameEvent> = self.game.events.drain(..).collect();
        for e in events {
            match &e {
                GameEvent::LevelExit { secret } => self.level_exit(*secret),
                GameEvent::PlayerDied => self.change_level(0),
                GameEvent::Message(m) => self.flash(m.clone()),
                _ => {}
            }
            if let Some(a) = &mut self.audio {
                a.on_event(&self.res, &self.game, &e);
            }
        }
        if let Some(a) = &mut self.audio {
            if self.audio_map != self.game.world.map_name {
                self.audio_map = self.game.world.map_name.clone();
                a.level_changed(&self.res, &self.game);
            }
            a.update_listener(&self.game);
        }
        prof.mark("audio");

        let Some(window) = self.window.clone() else { return };
        let size = window.inner_size();
        let (ww, wh) = (size.width.max(1) as usize, size.height.max(1) as usize);
        // Image size: chosen resolution (or the window's), times render scale, times the
        // supersampling factor; it is averaged/scaled into a letterboxed window rectangle.
        let (bw, bh) = self.cfg.resolution.map_or((ww, wh), |(w, h)| (w as usize, h as usize));
        let (ox, oy, dw, dh) = crate::scale::fit(ww, wh, bw, bh);
        let ss = crate::config::ANTIALIAS[self.cfg.antialias.min(crate::config::ANTIALIAS.len() - 1)].1;
        let scale = self.cfg.render_scale.clamp(0.1, 1.0) * ss;
        let rw = ((bw as f64 * scale) as usize).max(64);
        let rh = ((bh as f64 * scale) as usize).max(48);
        self.renderer.resize(rw, rh);

        let cam = self.game.camera();
        let sprites = self.game.sprite_draws(&self.res, &cam);
        let w = &self.game.world;
        let scene = Scene {
            sectors: &w.sectors,
            walls: &w.walls,
            sprites: &sprites,
            tiles: &self.res.tiles,
            luts: if self.game.inventory.is_active(ob_game::inventory::InvItem::BeastVision) {
                &self.luts_beast
            } else if self.game.player.underwater {
                &self.luts_water
            } else {
                &self.luts
            },
            sky_offsets: &w.sky_offsets,
            sky_bits: w.sky_bits,
            visibility: w.visibility,
            clock: self.game.clock,
            voxels: &self.res.voxels,
            ror: &self.game.world.ror_view,
        };
        prof.mark("prepare");
        // The main menu (outside a game) covers the whole screen: skip the 3D view.
        if !(self.menu_open && !self.menu.in_game) {
            self.renderer.render(&scene, &cam);
        }
        prof.mark("render");

        let (qid, _) = self.game.weapon_view();
        let weapon = self.game.anims.qav(&self.res.blood, qid);
        let message = self
            .message
            .as_ref()
            .filter(|m| m.1.elapsed().as_secs_f64() < 3.0)
            .map(|m| m.0.clone());
        let menu_view = self.menu_open.then(|| self.options_view());
        let menu_time = self.start_time.elapsed().as_secs_f64();

        let Some(surface) = self.surface.as_mut() else { return };
        if surface
            .resize(NonZeroU32::new(ww as u32).unwrap(), NonZeroU32::new(wh as u32).unwrap())
            .is_err()
        {
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else { return };
        // Scale the renderer's column-major image straight to window size, then draw the
        // HUD and menu at window resolution (sharp, and cheap even with supersampling).
        let direct = (dw, dh) == (ww, wh);
        if !direct {
            self.out.resize(dw * dh, 0);
        }
        let target: &mut [u32] = if direct { &mut buf[..] } else { &mut self.out[..] };
        let (cols, _, _) = self.renderer.columns();
        crate::scale::resample_cols(cols, rw, rh, target, dw, 0, 0, dw, dh, self.cfg.upscale);
        prof.mark("scale");
        hud::draw(
            target,
            dw,
            dh,
            &self.res,
            &self.luts,
            &self.game,
            weapon.as_deref(),
            self.show_stats.then_some(self.fps),
            message.as_deref(),
        );
        if let Some(o) = &menu_view {
            self.menu.draw(target, dw, dh, &self.res, &self.luts, menu_time, o);
        }
        if std::mem::take(&mut self.shot_pending) {
            let path = crate::tools::screenshot_path();
            crate::tools::save_png(&path, dw, dh, target);
            self.message = Some((format!("Saved {path}"), Instant::now()));
        }
        if !direct {
            buf.fill(0);
            for (y, row) in self.out.chunks(dw).enumerate() {
                let o = (oy + y) * ww + ox;
                buf[o..o + dw].copy_from_slice(row);
            }
        }
        prof.mark("hud");

        let _ = buf.present();
        prof.mark("present");
        self.pace_frame();
        prof.finish(&self.game.world.map_name, self.game.clock, self.start_time.elapsed().as_secs_f64(), &mut self.prof_log);
        // Profiling aid: OB_AUTOSWITCH=N switches to the next level every N seconds.
        if let Some(n) = std::env::var("OB_AUTOSWITCH").ok().and_then(|v| v.parse::<f64>().ok()) {
            if self.last_switch.elapsed().as_secs_f64() > n {
                self.last_switch = Instant::now();
                self.change_level(1);
            }
        }

        self.fps_frames += 1;
        let el = self.fps_time.elapsed().as_secs_f64();
        if el >= 0.5 {
            self.fps = self.fps_frames as f64 / el;
            self.fps_frames = 0;
            self.fps_time = Instant::now();
            window.set_title(&format!(
                "OpenBlood - {} - {}x{} - {:.0} fps",
                self.game.world.map_name, rw, rh, self.fps
            ));
        }
    }

    /// Shows the current cutscene frame, fitted to 4:3 (as on the original's screen).
    fn draw_movie(&mut self) {
        let Some(window) = self.window.clone() else { return };
        let size = window.inner_size();
        let (ww, wh) = (size.width.max(1) as usize, size.height.max(1) as usize);
        let Some(m) = &self.movie else { return };
        let (mw, mh) = (m.width(), m.height());
        // Shown as on a 4:3 screen: 320x200 pixels fill 4:3 (each pixel 4/320 x 3/200).
        let (aw, ah) = (mw * 800, mh * 960);
        let (ox, oy, dw, dh) = crate::scale::fit(ww, wh, aw.max(1), ah.max(1));
        let Some(surface) = self.surface.as_mut() else { return };
        if surface
            .resize(NonZeroU32::new(ww as u32).unwrap(), NonZeroU32::new(wh as u32).unwrap())
            .is_err()
        {
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else { return };
        buf.fill(0);
        crate::scale::resample(&m.pixels, mw, mh, &mut buf, ww, ox, oy, dw, dh, crate::config::Upscale::Smooth);
        let _ = buf.present();
        self.pace_frame();
    }

    /// Frame-rate cap: waits until this frame's slot ends. Sleeps most of the way (Rust's
    /// sleep uses a high-resolution timer on Windows) and spins the last moment for
    /// even frame spacing. Motion stays smooth through the tick interpolation.
    fn pace_frame(&mut self) {
        if self.cfg.max_fps == 0 {
            return;
        }
        let period = std::time::Duration::from_secs_f64(1.0 / self.cfg.max_fps as f64);
        let now = Instant::now();
        // Next slot; if we fell behind by more than a frame, start counting from now.
        let target = match self.pace_next {
            Some(t) if t + period > now => t,
            _ => now,
        };
        self.pace_next = Some(target + period);
        if target > now {
            let wait = target - now;
            let margin = std::time::Duration::from_micros(1500);
            if wait > margin {
                std::thread::sleep(wait - margin);
            }
            while Instant::now() < target {
                std::hint::spin_loop();
            }
        }
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(w) = &self.window {
            if w.fullscreen().is_some() {
                w.set_fullscreen(None);
                self.cfg.fullscreen = false;
            } else {
                w.set_fullscreen(Some(Fullscreen::Borderless(None)));
                self.cfg.fullscreen = true;
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs =
            Window::default_attributes()
                .with_title("OpenBlood")
                .with_inner_size(winit::dpi::LogicalSize::new(
                    self.cfg.window_width,
                    self.cfg.window_height,
                ));
        if self.cfg.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Rc::new(el.create_window(attrs).expect("create window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        self.window = Some(window);
        self.surface = Some(surface);
        self.grab(true);
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Focused(f) => {
                if !f {
                    self.keys.clear();
                    self.mouse_buttons.clear();
                    self.grab(false);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && self.movie.is_some() {
                    self.skip_movie();
                    return;
                }
                if state == ElementState::Pressed {
                    if !self.grabbed {
                        self.grab(true);
                        return;
                    }
                    self.mouse_buttons.insert(button);
                } else {
                    self.mouse_buttons.remove(&button);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                if event.state == ElementState::Pressed && self.movie.is_some() {
                    if !event.repeat {
                        self.skip_movie();
                    }
                    return;
                }
                if event.state == ElementState::Pressed {
                    if !event.repeat {
                        match code {
                            KeyCode::Escape => {
                                if self.menu_open {
                                    if self.menu.back() && self.menu.in_game {
                                        self.close_menu();
                                    }
                                } else {
                                    self.menu.open_pause();
                                    self.menu_open = true;
                                    self.keys.clear();
                                    self.grab(false);
                                }
                                return;
                            }
                            _ if self.menu_open => {
                                self.menu_key(el, code);
                                return;
                            }
                            KeyCode::F11 => self.toggle_fullscreen(),
                            KeyCode::Enter if self.keys.contains(&KeyCode::AltLeft) => self.toggle_fullscreen(),
                            KeyCode::F2 => self.show_stats = !self.show_stats,
                            KeyCode::F5 => {
                                self.cfg.render_scale = if self.cfg.render_scale >= 1.0 { 0.5 } else { 1.0 };
                                self.flash(format!("Render scale {:.0}%", self.cfg.render_scale * 100.0));
                            }
                            KeyCode::F12 => self.shot_pending = true,
                            KeyCode::F6 => self.save_slot(0),
                            KeyCode::F9 => self.load_slot(0),
                            KeyCode::F10 => {
                                // Test cheat: all weapons, ammo and keys.
                                let a = &mut self.game.arsenal;
                                for w in 1..=10 {
                                    a.have[w] = true;
                                }
                                for (i, m) in ob_game::weapons::AMMO_MAX.iter().enumerate() {
                                    a.ammo[i] = *m;
                                }
                                self.game.player.keys = [true; 8];
                                self.game.player.health = 200;
                                self.flash("All weapons, ammo and keys".into());
                            }
                            KeyCode::BracketRight
                                if (self.keys.contains(&KeyCode::ControlLeft)
                                    || self.keys.contains(&KeyCode::ControlRight)) =>
                            {
                                self.change_level(1)
                            }
                            KeyCode::BracketLeft
                                if (self.keys.contains(&KeyCode::ControlLeft)
                                    || self.keys.contains(&KeyCode::ControlRight)) =>
                            {
                                self.change_level(-1)
                            }
                            _ => {}
                        }
                    }
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.grabbed {
                self.mouse_dx += delta.0;
                self.mouse_dy += delta.1;
            }
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// Optional frame profiler: with `OB_PROFILE=1`, frames slower than 12 ms are logged to
/// `openblood-profile.log` with the time spent in each stage.
static PROFILE_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

struct Profile {
    on: bool,
    dt: f64,
    start: Instant,
    last: Instant,
    parts: Vec<(&'static str, f64)>,
}

impl Profile {
    fn new(dt: f64) -> Profile {
        let now = Instant::now();
        Profile {
            on: *PROFILE_ON.get_or_init(|| std::env::var_os("OB_PROFILE").is_some()),
            dt,
            start: now,
            last: now,
            parts: Vec::new(),
        }
    }

    fn mark(&mut self, name: &'static str) {
        if self.on {
            let now = Instant::now();
            self.parts.push((name, (now - self.last).as_secs_f64() * 1e3));
            self.last = now;
        }
    }

    fn finish(&self, map: &str, clock: i32, at: f64, log: &mut Vec<String>) {
        let total = self.start.elapsed().as_secs_f64() * 1e3;
        if !self.on || total < 12.0 {
            return;
        }
        let parts: Vec<String> = self.parts.iter().map(|(n, t)| format!("{n} {t:.1}")).collect();
        let line = format!(
            "{at:7.2}s {map} tick {clock}: frame {total:.1} ms (dt {:.1} ms) | {}
",
            self.dt * 1e3,
            parts.join(", ")
        );
        log.push(line);
        // Written in batches: appending to the file every frame was itself slow.
        if log.len() >= 300 {
            flush_profile(log);
        }
    }
}

fn flush_profile(log: &mut Vec<String>) {
    use std::io::Write;
    if log.is_empty() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("openblood-profile.log") {
        let _ = f.write_all(log.concat().as_bytes());
    }
    log.clear();
}

impl Drop for App {
    fn drop(&mut self) {
        flush_profile(&mut self.prof_log);
    }
}
