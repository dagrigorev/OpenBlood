# Engine crate (`ob-engine`)

Clean-room readers for Build/Blood data formats, geometry and collision, and the software
renderer. No Blood game rules live here. API docs:
<https://dagrigorev.github.io/OpenBlood/ob_engine/>

Dependencies: `rayon` (parallel rendering), `serde` (world types are saved by the game).

## Modules

| Module | Purpose | Key items |
| --- | --- | --- |
| `rff` | RFF archives (`BLOOD.RFF`, `SOUNDS.RFF`, `GUI.RFF`), including v3.01 dictionary and per-file obfuscation | `Rff::open`, `Rff::read_name`, `RffEntry`, `FLAG_ENCRYPTED` |
| `art` | `TILESnnn.ART` tile archives; column-major 8-bit pixels and `picanm` animation data | `TileSet::load_dir`, `Tile`, `PicAnm`, `MAX_TILES` |
| `palette` | `.PAL` palettes, `.PLU` shade tables (64 shades × 256 colours), `.TLU` translucency | `Palette`, `ColorTables`, `NUM_SHADES` |
| `map` | Blood `.MAP` v6/v7: XOR-obfuscated records, Build v7 geometry plus XSECTOR/XWALL/XSPRITE bitfields | `Map::parse`, `Sector`, `Wall`, `Sprite`, `XSector`, `XWall`, `XSprite`, `map_crypt` |
| `geom` | Plane equations for sloped floors/ceilings, point-in-sector, sector lookup, ray/segment intersection | `plane_eq`, `zs_of_slope`, `inside_sector`, `update_sector`, `ray_segment` |
| `collide` | Movement against sectors (Build `clipmove`/`getzrange` equivalents) and hitscan | `Mover`, `clip_move`, `get_z_range`, `hitscan`, `CircleObstacle`, `HitCylinder` |
| `render` | Resolution-independent multithreaded renderer | `Renderer`, `Scene`, `Camera`, `SpriteDraw`, `Ror`, `ShadeLuts` |
| `voxel` | KVX voxel models | `Voxel`, `EMPTY` |
| `smk` | Smacker `SMK2`/`SMK4` video decoder | `Smk` |

## Collision model (`collide`)

The mover is a vertical cylinder: radius `r` in xy units, feet at `z`, extending `height` z
units upward. Moving into another sector through a portal wall is blocked when:

- the floor step up is higher than the mover's `step`,
- the head would hit the next sector's ceiling, or
- the wall's blocking flag (`WALL_BLOCK_FLAG`) is set.

`clip_move` slides along blocking walls; `get_z_range` finds the floor and ceiling under the
cylinder (including sprites standing in as obstacles). `hitscan` traces bullets through
portals and against sprite cylinders.

## Renderer entry points

```rust
let mut r = Renderer::new(width, height);
r.render(&scene, &camera);          // fills r.frame (row-major 0x00RRGGBB) and/or columns
let (pixels, w, h) = r.columns();   // column-major output used by the frontend scaler
```

`Scene` borrows the world geometry, the prepared sprite list, tiles, shade tables, sky
settings, the game clock (tile animation), voxels and room-over-room links. See
[Renderer](Renderer) for the algorithm.

## Writing a new format reader

Follow the existing modules:

1. Module docs (`//!`) describe the byte layout from public documentation or from inspecting
   your own files.
2. Parse from a `&[u8]`, return `io::Result` (or `Option`) instead of panicking on bad data.
3. Add a unit test with a synthetic buffer, and, if useful, a data-driven test in
   `crates/engine/tests/original_data.rs` that skips when `OPENBLOOD_DATA` is not set.

Never commit original game files, or bytes copied from them, as test fixtures. See
[Contributing](Contributing).
