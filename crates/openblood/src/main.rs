//! OpenBlood: an open-source engine that runs the original Blood (1997) data files.
//! You must own the original game; OpenBlood ships no game content.

mod app;
mod audio;
mod cdaudio;
mod config;
mod hud;
mod menu;
mod scale;
mod tools;

use config::Config;
use ob_game::game::Game;
use ob_game::resources::Resources;
use std::path::{Path, PathBuf};

/// Episode map order from BLOOD.INI (falls back to the maps found in BLOOD.RFF).
fn level_list(res: &Resources) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(res.dir.join("BLOOD.INI")) {
        let mut in_episode = false;
        for line in text.lines() {
            let l = line.split(';').next().unwrap_or("").trim();
            if l.starts_with('[') {
                in_episode = l.to_ascii_lowercase().starts_with("[episode");
                continue;
            }
            if !in_episode {
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                let k = k.trim().to_ascii_lowercase();
                if k.starts_with("map") && k[3..].chars().all(|c| c.is_ascii_digit()) {
                    out.push(v.trim().to_ascii_uppercase());
                }
            }
        }
    }
    if out.is_empty() {
        out = res
            .blood
            .entries
            .iter()
            .filter(|e| e.ext == "MAP")
            .map(|e| e.name.clone())
            .collect();
        out.sort();
    }
    out
}

fn find_data_dir(cfg: &Config) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let mut candidates = vec![cfg.data_dir.clone(), PathBuf::from(".")];
    if let Some(d) = &exe_dir {
        candidates.push(d.clone());
        candidates.push(d.join("blood"));
    }
    candidates
        .into_iter()
        .find(|d| d.join("BLOOD.RFF").exists() || d.join("blood.rff").exists())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if tools::run_tool(&args) {
        return;
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let mut cfg = Config::load(&exe_dir.join("openblood.ini"));
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--data" | "-data" if i + 1 < args.len() => {
                cfg.data_dir = PathBuf::from(&args[i + 1]);
                i += 1;
            }
            "--map" | "-map" if i + 1 < args.len() => {
                cfg.start_map = Some(
                    args[i + 1]
                        .trim_end_matches(".map")
                        .trim_end_matches(".MAP")
                        .to_string(),
                );
                i += 1;
            }
            "--scale" if i + 1 < args.len() => {
                cfg.render_scale = args[i + 1].parse().unwrap_or(1.0);
                i += 1;
            }
            "--windowed" => cfg.fullscreen = false,
            "--nosound" => cfg.sound = false,
            "--help" | "-h" => {
                println!("openblood [--data DIR] [--map E1M1] [--windowed] [--scale 0.5] [--nosound]");
                println!("tools: openblood rff-list FILE | rff-extract RFF NAME OUT | map-info RFF | shot DATA MAP OUT.png [W H]");
                return;
            }
            _ => {}
        }
        i += 1;
    }
    let Some(dir) = find_data_dir(&cfg) else {
        eprintln!(
            "OpenBlood needs the files from your original Blood installation (BLOOD.RFF, SOUNDS.RFF, TILES*.ART...).\n\
             Copy them next to openblood.exe, or set data_dir in {} or pass --data DIR.",
            exe_dir.join("openblood.ini").display()
        );
        std::process::exit(1);
    };
    let res = match Resources::load(&dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to load game data from {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    let levels = level_list(&res);
    let direct_map = cfg.start_map.is_some();
    let start = cfg
        .start_map
        .clone()
        .unwrap_or_else(|| levels.first().cloned().unwrap_or_else(|| "E1M1".into()));
    let level_index = levels.iter().position(|l| l.eq_ignore_ascii_case(&start)).unwrap_or(0);
    let game = match Game::load_level(&res, &start) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to load map {start}: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = winit::event_loop::EventLoop::new().expect("event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let show_menu = !direct_map;
    let mut app = app::App::new(cfg, res, game, levels, level_index);
    app.menu_open = show_menu;
    event_loop.run_app(&mut app).expect("run");
}
