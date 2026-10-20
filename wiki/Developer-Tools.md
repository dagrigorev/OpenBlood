# Developer Tools

`openblood <tool> ...` runs a command-line tool instead of the game
(`crates/openblood/src/tools.rs`). `DATA` is a folder with the original Blood files, `MAP` a
level name such as `E2M4`. Tools never write into `DATA`.

## Data inspection

| Tool | Usage | Output |
| --- | --- | --- |
| `rff-list` | `rff-list FILE.RFF` | version and every entry (name, id, size, flags) |
| `rff-extract` | `rff-extract FILE.RFF NAME OUT` | one entry, de-obfuscated |
| `map-info` | `map-info BLOOD.RFF` | per map: version, counts, start position, song, sky |
| `walls` | `walls DATA MAP SECTOR` | walls of a sector |
| `sprites` | `sprites DATA MAP X Y RADIUS` | sprites near a point |
| `chan` | `chan DATA MAP CHANNEL` | every x-object sending or receiving on a channel |
| `tile` | `tile DATA PIC OUT.png` | one ART tile as PNG |
| `sheet` | `sheet DATA FIRST LAST OUT.png` | contact sheet of a tile range |
| `tiles-scan` | `tiles-scan DATA MINW MINH` | tiles at least that large |
| `qav` | `qav DATA NAME` | frames and layers of a QAV animation |
| `survey` | `survey DATA` | statistics over all maps |
| `find-vox` | `find-vox DATA MAP` | sprites drawn as voxels |
| `vox-test` | `vox-test DATA PIC OUT.png ANG` | voxel next to its flat tile |
| `smk-frames` | `smk-frames FILE.SMK OUTDIR [STEP]` | every STEP-th cutscene frame as PNG |

## Rendering

| Tool | Usage | Output |
| --- | --- | --- |
| `shot` | `shot DATA MAP OUT.png [W H] [x y z ang horiz]` | static render from the map start or a given camera |
| `shotg` | `shotg DATA MAP OUT.png W H SECONDS` | simulates SECONDS of game time, then renders exactly like the game, HUD included |
| `menu-shot` | `menu-shot DATA OUT.png` | options menu at 640×400 |
| `scale-test` | `scale-test DATA MAP OUTDIR` | one view through every upscale filter and AA level |
| `topdown` | `topdown DATA MAP OUT.png CX CY HALF` | top-down wall plot around a point |

`shotg` environment variables:

| Variable | Effect |
| --- | --- |
| `OB_ACT=1` | start the level (as if the player moved) before simulating |
| `OB_CAM=x,y,z,ang` | place the camera (`z = 0`: standing eye height on the floor) |
| `OB_LOOKAT=sprite[,dist[,angle]]` | put the camera in front of a sprite |
| `OB_EYE=n` | eye height above the player's feet |
| `OB_GIVE=1` | give inventory items, akimbo and invulnerability first |
| `OB_NOMSG=1` | hide the sample pickup message (clean screenshots) |
| `YSTRETCH=x` | override the renderer's vertical stretch |
| `NOSPR=1` | (`shot`) render without sprites |

The wiki's [Screenshots](Screenshots) were made with:

```
OB_NOMSG=1 openblood shotg DATA E3M1 screenshots/map_e3m1.png 1600 900 0.5
```

## Performance

| Tool / variable | Use |
| --- | --- |
| `perf DATA MAP [W H]` | level load time, then per-second simulation and per-phase render times (`OB_SECS=n` for longer runs) |
| `OB_PROFILE=1` (game) | frames slower than 12 ms logged to `openblood-profile.log` with a per-stage breakdown |
| `OB_AUTOSWITCH=n` (game) | next level every n seconds |

## Gameplay analysis

| Tool | Usage | Output |
| --- | --- | --- |
| `dude-visible` | `dude-visible DATA` | enemies in clear view from each map start |
| `dude-near` | `dude-near DATA RADIUS TYPE...` | enemies of the given types near each start |
| `vox-visible` | `vox-visible DATA` | voxel sprites in view from each start |
| `noise-test` | `noise-test DATA MAP...` | fires the tommy gun for 1 s at each start, idles 12 s, reports when the player gets hurt |
| `frozen-slides` | `frozen-slides DATA` | slide sectors kept still to avoid folded geometry |

## Disc extraction

| Tool | Usage |
| --- | --- |
| `extract-cd` | `extract-cd DISC.cue OUTDIR`: music tracks to `OUTDIR`, cutscenes to `OUTDIR/../movie` |
| `extract-movies` | `extract-movies DISC.cue OUTDIR`: only the `MOVIE` folder |

## Comparing with the original

The reference workflow used to tune OpenBlood:

1. Run the original in DOSBox and capture frames (Ctrl+F5) or video (Ctrl+Alt+F5).
2. Render the same place with `shotg` (use `OB_CAM` / `OB_LOOKAT`).
3. Compare at 320×240 (the original's 320×200 scaled to 4:3).

For renderer optimizations, render a set of views before and after the change and check that
the PNGs are pixel-identical.
