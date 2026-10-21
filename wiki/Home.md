# OpenBlood

OpenBlood is an open-source engine, written from scratch in Rust, that plays **Blood (1997)**
on modern PCs at any resolution, 4K included, using the data files from **your own copy** of
the game.

![E3M1 rendered by OpenBlood](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e3m1.png)

> OpenBlood contains **no** Blood game content (maps, art, sounds, music or executables).
> Without the original files it does nothing. Blood is © 1997 Monolith Productions.

## Start here

| I want to... | Page |
| --- | --- |
| Download and play | [Releases](Releases), then [Installation](Installation) |
| Learn the keys | [Controls](Controls) |
| Change resolution, anti-aliasing, frame cap | [Configuration](Configuration) |
| See what it looks like | [Screenshots](Screenshots) |
| Build it myself | [Building from Source](Building-from-Source) |
| Understand the code | [Architecture](Architecture), then the crate pages |
| Help out | [Contributing](Contributing) |

## Highlights

- Clean-room readers for every original data format: RFF archives, ART tiles, palettes and
  shade tables, encrypted v7 MAP files, SEQ/QAV animations, SFX/RAW sounds, KVX voxels,
  Smacker cutscenes. See [File Formats](File-Formats).
- Multithreaded, resolution-independent software renderer: sloped planes, parallax skies,
  translucent and masked walls, voxels, see-through room-over-room portals. See
  [Renderer](Renderer).
- Fixed 120 Hz simulation with per-frame interpolation, so motion is smooth at 60, 144 Hz or
  uncapped.
- Blood's trigger and channel system: doors, lifts, sliding and rotating sectors, switches,
  keys, secrets, teleporters. See [Triggers and Channels](Triggers-and-Channels).
- Weapons with alternate fire, inventory, power-ups and enemy AI tuned against recordings of
  the original running in DOSBox.
- Deterministic save games, CD music ripped from your disc image (or the original MIDI score
  through a built-in FM synthesizer), intro and episode cutscenes.

## Documentation map

- **Players:** [Installation](Installation) · [Controls](Controls) ·
  [Configuration](Configuration) · [Screenshots](Screenshots) · [Releases](Releases) ·
  [Roadmap and Known Issues](Roadmap-and-Known-Issues)
- **Developers:** [Building from Source](Building-from-Source) ·
  [Architecture](Architecture) · [Engine crate](Engine-Crate) · [Game crate](Game-Crate) ·
  [Frontend crate](Frontend-Crate) · [File Formats](File-Formats) · [Renderer](Renderer) ·
  [Triggers and Channels](Triggers-and-Channels) · [Developer Tools](Developer-Tools) ·
  [Testing](Testing) · [CI and Releases](CI-and-Releases) · [Contributing](Contributing)
- **API reference (rustdoc):** <https://dagrigorev.github.io/OpenBlood/>
