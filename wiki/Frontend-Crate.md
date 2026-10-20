# Frontend crate (`openblood`)

The executable: window, input, audio, HUD, menus, cutscenes, configuration and the developer
tools. API docs (private items included): <https://dagrigorev.github.io/OpenBlood/openblood/>

Dependencies: `ob-engine`, `ob-game`, `winit` (window and input), `softbuffer` (present a
CPU framebuffer), `cpal` (audio output), `png` (screenshots), `rayon`.

## Modules

| Module | Purpose | Key items |
| --- | --- | --- |
| `main` | Argument parsing, data directory search, level list, startup | `level_list`, `find_data_dir` |
| `app` | winit `ApplicationHandler`: input → `Game::update` → render → scale → HUD → present; level changes, saves, cutscene queue, frame cap, profiler | `App`, `App::frame`, `App::pace_frame` |
| `config` | `openblood.ini` read/write, option lists | `Config`, `Upscale`, `RESOLUTIONS`, `MAX_FPS`, `ANTIALIAS` |
| `scale` | Frame → window: area averaging (supersampling AA), nearest / bilinear / sharp-bilinear upscaling, letterboxing | `resample`, `resample_cols`, `fit` |
| `hud` | Status bar, weapon QAV overlay, messages, fonts from the original tiles | `draw`, `draw_qav`, `draw_text`, `font_base` |
| `menu` | Front-end menus (episodes, difficulty, options, save/load, help, credits) | `Menu`, `Screen`, `MenuAction`, `SKILLS` |
| `audio` | SFX/RAW mixing with 3D panning, preloading, music streaming, cutscene audio channel | `Audio`, `Sound`, `parse_sfx`, `MOVIE_OWNER` |
| `cdaudio` | CUE/BIN parsing, Red Book track ripping, ISO 9660 reading of the data track, WAV streaming | `parse_cue`, `extract`, `extract_dir`, `stream_wav` |
| `fm` | Clean-room two-operator FM voices (OPL2/OPL3 style): envelopes, waveforms, feedback, key scaling, tremolo, vibrato | `Voice`, `Timbre` |
| `midi` | Standard MIDI files with EMIDI track/loop controllers, AdLib timbre bank, sequencer and streaming thread | `Song`, `Player`, `parse_bank`, `stream` |
| `movie` | Cutscene playback (Smacker video + separate WAV soundtrack) | `Movie`, `intro`, `find` |
| `tools` | Command-line developer tools (`openblood <tool> ...`) | `run_tool`, `shot`, `save_png` |

## Presentation pipeline

1. `Renderer` draws the 3D view column-major at *output × render scale × AA*.
2. `scale::resample_cols` reads the column-major buffer directly (no transpose) and writes the
   window-sized image. An exact 2:1 shrink (4x AA at native size) takes a packed 2×2
   averaging fast path.
3. HUD and menus are drawn at window resolution, then softbuffer presents.
4. With a frame cap, `pace_frame` sleeps until ~1.5 ms before the slot and spins the rest,
   giving even frame spacing (60 fps cap measured at 16.67 ± 0.27 ms).

## Audio

All sound effects are decoded once at startup (`Audio::preload_all`, 8-bit samples, ~10 ms).
CD music is streamed from `music/TRACKnn.WAV` in half-second chunks over a bounded channel, so
level changes never load a whole track. When the level's track was not ripped, `midi::stream`
renders its `Song=` from BLOOD.INI (`NAME.MID` in SOUNDS.RFF) with the FM voices and the
`GMTIMBRE.TMB` instrument bank into the same kind of chunks, on a background thread (about 30x
faster than real time). Cutscenes take over a dedicated owner channel and mute
music while playing.

## Profiling

- `OB_PROFILE=1`: frames slower than 12 ms are written (buffered) to `openblood-profile.log`
  with time per stage.
- `OB_AUTOSWITCH=n`: advance to the next level every n seconds (stress-tests level loading).
- `openblood perf DATA MAP [W H]`: offline simulation and per-phase render timings.

More in [Developer Tools](Developer-Tools).
