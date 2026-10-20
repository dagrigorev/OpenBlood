# File Formats

How OpenBlood reads the original data. Everything here comes from public format
descriptions and from inspecting a legally owned copy; no original or decompiled source code
was used. All values are little endian.

| Format | Files | Reader |
| --- | --- | --- |
| RFF archive | `BLOOD.RFF`, `SOUNDS.RFF`, `GUI.RFF` | `ob_engine::rff` |
| ART tiles | `TILES000.ART` … `TILES017.ART` | `ob_engine::art` |
| Palette | `*.PAL` inside BLOOD.RFF | `ob_engine::palette` |
| Shade / translucency tables | `*.PLU`, `*.TLU` | `ob_engine::palette` |
| Map | `E1M1.MAP` … inside BLOOD.RFF | `ob_engine::map` |
| Voxel model | `*.KVX` (via `VOXEL.DAT`) | `ob_engine::voxel` |
| Sprite sequence | `*.SEQ` | `ob_game::anim` |
| Weapon/HUD animation | `*.QAV` | `ob_game::anim` |
| Sound | `*.SFX` + `*.RAW` in SOUNDS.RFF | `openblood::audio` |
| Music (fallback) | `*.MID` + `GMTIMBRE.TMB` in SOUNDS.RFF | `openblood::midi`, `openblood::fm` |
| Video | `*.SMK` on the CD | `ob_engine::smk` |
| CD audio | CUE/BIN image | `openblood::cdaudio` |
| Episodes and levels | `BLOOD.INI` | `ob_game::ini` |

## RFF archives

```
header (32 bytes)
  char[4] magic "RFF\x1a"
  u16     version          (0x200, 0x300, 0x301)
  u16     pad
  u32     dictionary offset
  u32     entry count
  u8[16]  padding
dictionary: count × 48-byte entries (XOR-obfuscated when version >= 0x300)
```

Entries carry an id, offset, size, flags, 3-character extension and 8-character name. Flag
`0x10` (`FLAG_ENCRYPTED`) means the first 256 bytes of the file are XOR-obfuscated. Version
3.01 derives the dictionary key differently from 3.00; both are handled.

## ART tiles

```
u32 version (1), u32 numtiles (unused), u32 first, u32 last
i16 width[n], i16 height[n], u32 picanm[n]       (n = last - first + 1)
pixels: per tile, column-major (x outer, y inner), one palette index per byte
```

`picanm` packs the animation frame count and type, speed, and the x/y draw offsets. Index
255 is transparent.

## Palettes and lookup tables

- `.PAL`: 256 RGB triplets. Blood's `BLOOD.PAL` uses full **8-bit** channels (classic VGA
  palettes are 6-bit); the reader handles both.
- `.PLU` (palookup): 64 shade levels × 256 colours, shade 0 = brightest. Blood has several
  (normal, water, beast vision, ...) selected per sector or sprite.
- `.TLU`: 256 × 256 translucency table, `tlu[a * 256 + b]`.

OpenBlood combines palette and palookup into true-colour shade tables (`ShadeLuts`) once per
palette, so the renderer outputs `0x00RRGGBB` directly.

## Maps

Signature `BLM\x1a`, versions 6.x and 7.x. Version 7 obfuscates every record with a rolling
byte XOR (`key`, `key+1`, ...). Geometry uses the Build v7 layout:

| Record | Size |
| --- | --- |
| sector | 40 bytes |
| wall | 32 bytes |
| sprite | 44 bytes |

Blood adds optional *extra* records (XSECTOR, XWALL, XSPRITE) stored directly after their
owner as packed, LSB-first bitfields. They hold the trigger data: receive/transmit channels,
commands, state, busy and wait times, keys, flags, and type-specific `data1..data4`. See
[Triggers and Channels](Triggers-and-Channels).

Map geometry is stored in each moving sector's **initial** state, so slide and rotate offsets
are relative to that.

## Voxels (KVX)

```
i32 numbytes, xsiz, ysiz, zsiz, xpivot, ypivot, zpivot   (pivots 8.8 fixed point)
i32 xoffset[xsiz + 1]
u16 xyoffset[xsiz][ysiz + 1]
per column: slabs { u8 ztop, u8 zleng, u8 cull, u8 colors[zleng] }
768-byte palette at the end (ignored: colours index the game palette)
```

Only the first (full-detail) mip level is used. OpenBlood scales voxels by sprite
`xrepeat / 16` per voxel.

## SEQ and QAV animations

- **SEQ** (sprites): 16-byte header (`SEQ\x1a`, u16 version, u16 frame count, u16 ticks per
  frame, u16 sound id, u32 flags: loop / remove when done) followed by 8-byte bitfield frames
  (tile, shade, palette, x/y repeat, translucency, blocking, hittable, "trigger callback",
  play sound, invisible, ...).
- **QAV** (weapon/HUD overlays): 36-byte header followed by frames of one callback id, a sound
  record and 8 tile layers (204 bytes per frame).

## Sounds

`SFX` descriptors (relVol, pitch, pitchRange, format, loopStart, `rawName[9]`) point at `RAW`
sample data in SOUNDS.RFF: unsigned 8-bit mono; `format` selects 11025, 22050 or 44100 Hz.

## MIDI songs and the timbre bank

The 12 songs are Standard MIDI Files (format 1, 480 ticks per quarter note, General MIDI
programs, channel 10 for drums). OpenBlood also understands Apogee's EMIDI controllers
(110/111 track designation per device, 112/113 device program and volume, 116-119 song and
track loops), although Blood's songs do not use them; songs restart when every track has ended.

`GMTIMBRE.TMB` is an AdLib instrument bank: 256 records of 13 bytes. Entries 0-127 are the
General MIDI programs, 128-255 the drum keys (128 + key).

| Bytes | Meaning (modulator, carrier) |
| --- | --- |
| 0-1 | OPL register 0x20: tremolo, vibrato, sustain, key-scale rate, frequency multiplier |
| 2-3 | 0x40: key-scale level, total level |
| 4-5 | 0x60: attack and decay rates |
| 6-7 | 0x80: sustain level and release rate |
| 8-9 | 0xE0: waveform |
| 10 | 0xC0: feedback and connection (FM or additive) |
| 11 | transpose (melodic) or the key to play (drums) |
| 12 | velocity sensitivity |

## Smacker video

`SMK2` / `SMK4`: a header with per-frame sizes and types, four Huffman trees, then per frame
an optional palette update, audio chunks and a bitstream of 4×4 blocks. Blood's cutscenes are
320 pixels wide at 15 or 30 fps with no embedded audio; their soundtracks are separate WAV
files on the CD.

## CD image

`cdaudio` parses the CUE sheet, rips Red Book audio tracks to WAV, and reads files from the
raw MODE2/2352 data track by walking its ISO 9660 directory tree (used to copy `\MOVIE\*.SMK`
and their WAVs).
