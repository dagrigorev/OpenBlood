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
- Voxel models (KVX) for items and decorations, ray-cast per pixel, including spinning pickups.
- Room-over-room links: see-through stacked floors/ceilings (per-pixel portal tracing) and
  moving between linked rooms, including diving into water (with the original water palette),
  with sprites in the linked room visible through the portal.
- Sliding/rotating sectors carry sprites and the player (boats, trains, moving platforms).
- Sector lighting effects, ambient/sector/one-shot sounds with 3D panning, CD-audio music
  ripped from your own disc image.
- Weapons with the original QAV animations and sounds (pitchfork, flare pistol, sawed-off,
  tommy gun, napalm, dynamite, spray can, tesla, life leech, voodoo), hitscan and projectiles,
  explosions, enemy AI with original SEQ animations and voices, pickups, the original status
  bar art and menu fonts, episode/difficulty menus, INI-driven level progression.
- Alternate fire for every weapon (double-barrel, tommy spread, flare starburst, big napalm,
  dynamite drop, spray-can bomb, tesla charge, voodoo curse), sawed-off reload, guns akimbo.
- Inventory (doctor's bag, diving suit, jumping boots, beast vision, crystal ball) and timed
  power-ups (invisibility, invulnerability, akimbo, reflective shots, feather fall, rage),
  breathing and drowning underwater.
- Enemy behaviours: ambush / guard / deaf editor flags, hearing gunfire, ranged enemies strafe
  instead of walking into fire, obstacle and ledge avoidance, fleeing innocents, burning enemies,
  buried zombies rising, stone gargoyles waking when hit or triggered.
- Save / load: quick save and five slots, deterministic restore of the whole game state.

Not yet: multiplayer, MIDI music fallback, cut-scene playback, the full set of enemy behaviours.
Known difference: the E1M1 starting coffin room looks vertically shorter in the original;
the cause is still being investigated.

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

![help](screenshots/help.png)

## Interface

Almost clone one by one

Main menu:

![menu](screenshots/main_menu.png)

Options:

![options](screenshots/options.png)

Credits:

![credits](screenshots/credits.png)

## Levels

Here is some levels (not done yet)

![lvl2](screenshots/episode2.png)

![lvl22](screenshots/episode2_2.png)

also, enemies positioning not completely ready 

![lvl3](screenshots/episode3.png)

## Building

```
cargo build --release
cargo test                     # unit tests
OPENBLOOD_DATA=path/to/blood cargo test --release   # + tests against your original files
```

Crates: `ob-engine` (formats, geometry, collision, renderer), `ob-game` (Blood game logic),
`openblood` (window, input, audio, HUD, menus, developer tools).

Developer tools (`openblood <tool> ...`): `rff-list`, `rff-extract`, `map-info`, `walls`,
`sprites`, `chan`, `tile`, `sheet`, `qav`, `survey`, `find-vox`, `vox-test`, `shot` / `shotg`
(render a map to PNG; `OB_LOOKAT=sprite,dist` aims the camera at a sprite), `extract-cd`.

## License

MIT (see `LICENSE-MIT`). Format knowledge comes from public documentation and from
inspecting the user's own files; no original or decompiled source code was used.
