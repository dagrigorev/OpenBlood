# Installation

OpenBlood is only the engine. You supply the game data from your own Blood disc.

## 1. Download OpenBlood

Get the latest build from the [Releases](Releases) page:

| Platform | File |
| --- | --- |
| Windows 10/11, 64-bit | `openblood-<version>-windows-x64.zip` |
| Linux, 64-bit (X11 or Wayland, ALSA/PulseAudio/PipeWire) | `openblood-<version>-linux-x64.tar.gz` |

Unpack it into a folder of its own, for example `C:\Games\OpenBlood`. Each release also lists
`SHA256SUMS.txt` to verify the download.

## 2. Install and patch Blood

Install Blood from your original disc, for example inside DOSBox, and apply the 1.20 or 1.21
patch. Any installation that has the files below works.

## 3. Copy the data files

Copy these files from the Blood directory next to `openblood.exe`:

| File(s) | Contents |
| --- | --- |
| `BLOOD.RFF` | maps, palettes, animations, fonts |
| `SOUNDS.RFF` | sound effects |
| `GUI.RFF` | menu graphics |
| `TILES000.ART` … `TILES017.ART` | textures and sprites |
| `SURFACE.DAT`, `VOXEL.DAT`, `TABLES.DAT` | surface types, voxel models, lookup tables |
| `BLOOD.INI` | episodes, level order, titles, music |

Instead of copying you can point OpenBlood at the Blood folder: set `data_dir` in
`openblood.ini`, or start it with `--data <dir>`. OpenBlood also looks in the current
directory and in a `blood` subfolder next to the executable.

## 4. Optional: music and cutscenes

Blood's music is CD audio, and the cutscenes are Smacker videos on the disc. OpenBlood reads
both straight from an image of **your** disc (CUE/BIN):

```
openblood extract-cd BLOODCD1.cue music
```

This rips the audio tracks to `music/TRACKnn.WAV` and copies the cutscene videos and their
soundtracks to `movie/`. To copy only the videos:

```
openblood extract-movies BLOODCD1.cue movie
```

## 5. Play

Run `openblood.exe`. It starts fullscreen at your desktop resolution; the intro cutscenes play
first (any key skips), then the main menu opens.

```
openblood --windowed            # in a window
openblood --map E2M4            # jump straight into a level
openblood --scale 0.5 --maxfps 60
```

All options are described in [Configuration](Configuration); the keys are in
[Controls](Controls).

> **E1M1 tip:** you start lying in a coffin. Move once (the coffin floor rises), then jump
> (Space) while walking forward to climb over its edge.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| "OpenBlood needs the files from your original Blood installation" | `BLOOD.RFF` was not found. Copy the data files (step 3) or set `data_dir`. |
| No music | Rip the CD tracks (step 4) and check `music = 1` in `openblood.ini`. |
| Low frame rate at 4K | Lower **Anti-aliasing** or **Render Scale** in Options, or cap FPS. |
| Saved games from an older build will not load | Save-format changes are not backwards compatible; start the level again. |
| Linux: no sound | Install the ALSA runtime (`libasound2`) or the PipeWire/PulseAudio ALSA plugin. |
