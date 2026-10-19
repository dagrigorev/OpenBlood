# OpenBlood

An open-source engine, written from scratch in Rust, that plays **Blood (1997)** on modern
Windows PCs at any resolution — including 4K — using the data files from **your own copy**
of the game.

OpenBlood contains **no** Blood game content (maps, art, sounds, music, executables).
Without the original files it does nothing. Blood is © 1997 Monolith Productions.

## What works

- Clean-room readers for every original data format: `RFF` archives (incl. v3.01
  dictionary/file obfuscation), `ART` tiles, 8-bit palettes and shade/translucency tables,
  encrypted v7 `MAP` files with Blood's X-sector/X-wall/X-sprite bitfields, `SEQ` sprite
  animations, `QAV` weapon animations, `SFX`/`RAW` sounds, `BLOOD.INI`.
- Resolution-independent multithreaded software renderer (per-column portal ray casting,
  sloped floors/ceilings, parallax skies, masked/translucent walls, face/wall/floor sprites,
  8-direction sprite rotation, distance shading from the original palookups).
  ~60+ FPS at 3840×2160 on a many-core CPU; `render_scale` trades sharpness for speed.
- Player movement and collision (sliding, steps, jumping, crouching, gravity, fall damage).
- Blood's trigger/channel system: doors, lifts, sliding/rotating sectors, switches, wait
  timers, locks and keys, level-start broadcasts, teleporters, secrets, exits, messages.
- Sector lighting effects, ambient/sector/one-shot sounds with 3D panning, CD-audio music
  ripped from your own disc image.
- Weapons with the original QAV animations and sounds (pitchfork, flare pistol, sawed-off,
  tommy gun, napalm, dynamite, spray can, tesla, life leech, voodoo), hitscan and projectiles,
  explosions, enemy AI with original SEQ animations and voices, pickups, the original status
  bar art and menu fonts, episode/difficulty menus, INI-driven level progression.

Not yet: voxels, room-over-room water links, moving path sectors (trains/boats), save/load,
multiplayer, MIDI music fallback, cut-scene playback, the full set of enemy behaviours.

## Setup

1. Install Blood from your original disc (e.g. in DOSBox) and patch it (1.20/1.21 recommended).
2. Copy these files from the Blood directory next to `openblood.exe` (or set `data_dir` in
   `openblood.ini`): `BLOOD.RFF`, `SOUNDS.RFF`, `GUI.RFF`, `TILES000.ART`…`TILES017.ART`,
   `SURFACE.DAT`, `VOXEL.DAT`, `TABLES.DAT`, `BLOOD.INI`.
3. Optional music: rip the CD audio tracks from your disc image:
   `openblood extract-cd BLOODCD1.cue music`
4. Run `openblood.exe` (fullscreen at native resolution by default; `--windowed` for a window).

## Controls

WASD / arrows move, mouse looks, left mouse or F fires, E / Enter uses, Space jumps,
C / Ctrl crouches, Shift runs, 1–0 select weapons, Esc menu, F11 or Alt+Enter fullscreen,
F5 render scale, F12 screenshot, F2 stats, `[` / `]` previous / next level.

## Building

```
cargo build --release
cargo test                     # unit tests
OPENBLOOD_DATA=path/to/blood cargo test --release   # + tests against your original files
```

Crates: `ob-engine` (formats, geometry, collision, renderer), `ob-game` (Blood game logic),
`openblood` (window, input, audio, HUD, menus, developer tools).

Developer tools (`openblood <tool> ...`): `rff-list`, `rff-extract`, `map-info`, `walls`,
`sprites`, `chan`, `tile`, `sheet`, `qav`, `shot` / `shotg` (render a map to PNG), `extract-cd`.

## License

MIT (see `LICENSE-MIT`). Format knowledge comes from public documentation and from
inspecting the user's own files; no original or decompiled source code was used.
