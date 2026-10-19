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
    message: Option<(String, Instant)>,
}

impl App {
    pub fn new(cfg: Config, res: Resources, game: Game, levels: Vec<String>, level_index: usize) -> Self {
        let luts = res.build_luts(0);
        let mut audio = if cfg.sound { Audio::new(&res).ok() } else { None };
        if let Some(a) = &mut audio {
            a.set_volumes(cfg.volume as f32, if cfg.music { cfg.music_volume as f32 } else { 0.0 });
        }
        App {
            cfg,
            luts,
            res,
            game,
            level_index,
            levels,
            renderer: Renderer::new(640, 400),
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
        i.use_key = self.down(KeyCode::KeyE) || self.down(KeyCode::Enter);
        i.fire = self.mouse_buttons.contains(&MouseButton::Left) || self.down(KeyCode::KeyF) || self.down(KeyCode::ControlRight);
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
                let title = self.res.ini.map(name).map(|m| m.title.clone()).unwrap_or_default();
                self.flash(if title.is_empty() { name.to_string() } else { format!("{name}: {title}") });
            }
            Err(e) => self.flash(format!("Cannot load {name}: {e}")),
        }
    }

    fn level_exit(&mut self, secret: bool) {
        let cur = self.game.world.map_name.clone();
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

    fn options_view(&self) -> crate::menu::OptionsView {
        crate::menu::OptionsView {
            fullscreen: self.window.as_ref().is_some_and(|w| w.fullscreen().is_some()),
            scale: self.cfg.render_scale,
            mouse: self.cfg.mouse_sensitivity,
            always_run: self.cfg.always_run,
            music: self.cfg.music,
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
        match action {
            MenuAction::None => {}
            MenuAction::Quit => el.exit(),
            MenuAction::Resume => self.close_menu(),
            MenuAction::NewGame { episode, skill } => {
                self.cfg.skill = skill;
                let eps: Vec<_> = self.res.ini.episodes.iter().filter(|e| !e.bloodbath_only).collect();
                let map = eps.get(episode).and_then(|e| e.maps.first().cloned()).unwrap_or_else(|| "E1M1".into());
                self.load_named(&map, false);
                self.menu = crate::menu::Menu::new();
                self.close_menu();
            }
            MenuAction::ToggleFullscreen => self.toggle_fullscreen(),
            MenuAction::ScaleChanged(v) => self.cfg.render_scale = v,
            MenuAction::MouseSensitivity(v) => self.cfg.mouse_sensitivity = v,
            MenuAction::ToggleAlwaysRun => self.cfg.always_run = !self.cfg.always_run,
            MenuAction::ToggleMusic => {
                self.cfg.music = !self.cfg.music;
                if let Some(a) = &mut self.audio {
                    a.set_volumes(self.cfg.volume as f32, if self.cfg.music { self.cfg.music_volume as f32 } else { 0.0 });
                }
            }
        }
    }

    fn flash(&mut self, m: String) {
        self.message = Some((m, Instant::now()));
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;
        let input = self.build_input(dt);
        if !self.menu_open {
            self.game.update(&self.res, &input, dt);
        }

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

        let Some(window) = self.window.clone() else { return };
        let size = window.inner_size();
        let (ww, wh) = (size.width.max(1) as usize, size.height.max(1) as usize);
        let scale = self.cfg.render_scale.clamp(0.1, 1.0);
        let rw = ((ww as f64 * scale) as usize).max(64);
        let rh = ((wh as f64 * scale) as usize).max(48);
        self.renderer.resize(rw, rh);

        let cam = self.game.camera();
        let sprites = self.game.sprite_draws(&self.res, &cam);
        let w = &self.game.world;
        let scene = Scene {
            sectors: &w.sectors,
            walls: &w.walls,
            sprites: &sprites,
            tiles: &self.res.tiles,
            luts: &self.luts,
            sky_offsets: &w.sky_offsets,
            sky_bits: w.sky_bits,
            visibility: w.visibility,
            clock: self.game.clock,
        };
        self.renderer.render(&scene, &cam);

        let (qid, _) = self.game.weapon_view();
        let weapon = self.game.anims.qav(&self.res.blood, qid);
        hud::draw(
            &mut self.renderer.frame,
            rw,
            rh,
            &self.res,
            &self.luts,
            &self.game,
            weapon.as_deref(),
            self.show_stats.then_some(self.fps),
            self.message
                .as_ref()
                .filter(|m| m.1.elapsed().as_secs_f64() < 3.0)
                .map(|m| m.0.as_str()),
        );
        if self.menu_open {
            let o = self.options_view();
            let t = self.start_time.elapsed().as_secs_f64();
            self.menu.draw(&mut self.renderer.frame, rw, rh, &self.res, &self.luts, t, &o);
        }

        let Some(surface) = self.surface.as_mut() else { return };
        if surface
            .resize(NonZeroU32::new(ww as u32).unwrap(), NonZeroU32::new(wh as u32).unwrap())
            .is_err()
        {
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else { return };
        if rw == ww && rh == wh {
            buf.copy_from_slice(&self.renderer.frame);
        } else {
            // Nearest-neighbour upscale from the render resolution.
            let src = &self.renderer.frame;
            let xmap: Vec<usize> = (0..ww).map(|x| (x * rw / ww).min(rw - 1)).collect();
            for y in 0..wh {
                let sy = (y * rh / wh).min(rh - 1);
                let row = &src[sy * rw..sy * rw + rw];
                let out = &mut buf[y * ww..y * ww + ww];
                for (o, &sx) in out.iter_mut().zip(xmap.iter()) {
                    *o = row[sx];
                }
            }
        }
        let _ = buf.present();

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

    fn toggle_fullscreen(&mut self) {
        if let Some(w) = &self.window {
            if w.fullscreen().is_some() {
                w.set_fullscreen(None);
            } else {
                w.set_fullscreen(Some(Fullscreen::Borderless(None)));
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
                            KeyCode::F12 => {
                                let path = crate::tools::screenshot_path();
                                crate::tools::save_png(
                                    &path,
                                    self.renderer.width,
                                    self.renderer.height,
                                    &self.renderer.frame,
                                );
                                self.flash(format!("Saved {path}"));
                            }
                            KeyCode::F9 => {
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
                            KeyCode::BracketRight => self.change_level(1),
                            KeyCode::BracketLeft => self.change_level(-1),
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
