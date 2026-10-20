# Renderer

`ob_engine::render` is a resolution-independent software renderer for Build-style worlds. It
has no fixed 320×200 assumptions: the same code draws 640×360 or 7680×4320.

![E1M4: translucent fire, masked walls](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e1m4.png)

## Overview

1. **Wall pass.** Each screen column casts one 2D ray through the sector/portal graph from the
   camera's sector. Walls, sloped floors and ceilings and parallax skies are drawn front to
   back into a **column-major** buffer, with a per-pixel depth (perpendicular distance in xy
   units).
2. **Sprite and masked-wall pass.** Face sprites, wall- and floor-aligned sprites, voxels and
   masked (see-through) walls are drawn back to front with a depth test.
3. Columns are independent, so both passes run in parallel on all cores (rayon).

The column-major layout keeps every column's pixels contiguous in memory; the frontend scaler
reads it directly, so the frame is never transposed.

## Projection

- Horizontal field of view uses Hor+ scaling: wider screens see more, never less vertically.
- `Renderer::y_stretch` emulates the original's non-square pixels. Fitting renders against
  captures of the original gave `1.0` with the eye height `EYE_STAND = 0x3800`.
- `Camera::horiz` is the vertical look offset (200-line units; positive looks down).

## Walls, floors and skies

- **Walls:** texture u from the position along the wall, v stepped in 32.32 fixed point.
  Flags handle x/y flip, bottom alignment, swapped bottom textures, one-way and translucent
  walls.
- **Flat planes:** a flat floor's depth on a screen row is `height / k`, where `k` is the row's
  ray slope. `View` stores `1 / k` per row, so a pixel costs a multiply, not a divide.
- **Sloped planes:** plane equations from `geom::plane_eq`, intersected per pixel.
- **Parallax skies:** sampled by view angle with the map's sky tile offsets.
- **Texture wrapping:** almost every Build texture is a power of two, so `wrap()` masks
  instead of dividing.
- **Shading:** distance shading from the map's global visibility plus sector/sprite shade,
  looked up in the true-colour `ShadeLuts`.

## Sprites

`Game::sprite_draws` builds a `SpriteDraw` list with animation and 8-direction rotation
frames already resolved and positions interpolated between ticks. The renderer projects them
by kind (face, wall, floor, voxel), then draws columns back to front:

- an opaque, no-portal fast loop with fixed-point texture rows for the common case;
- translucency (`SPR_TRANS`, `SPR_TRANS_REV`) via 50/50 or reversed blends;
- **voxels** are ray-cast per pixel against the KVX model's slabs. Pixels already covered by
  something nearer than the model's closest point skip the ray cast, and voxels completely
  behind the camera are culled.

## Room over room (ROR)

Blood stacks rooms with marker sprites: looking through a floor or ceiling continues in a
linked sector, offset by (dx, dy, dz). Water and goo links are opaque in Blood; stack links
are see-through.

- While casting a column, the renderer marks pixels whose floor/ceiling is a see-through link
  with a **portal id** (`portal_id(sector, floor)`).
- Those pixels are traced into the linked room. For the common case a lazily built per-column
  sector list (`ColTrace` + `trace_column`) is reused by every pixel of the column; nested
  portals inside the linked room fall back to `trace_pixel`.
- Sprites standing in the linked room get the portal id and are only drawn on pixels that show
  that portal.
- The portal mask is cleared only for columns that marked portals in the previous frame.

## Performance history

Measured at 8K (7680×4320), pixel-identical before and after each change:

| Change | Effect |
| --- | --- |
| Lazy per-column ROR traversal instead of per-pixel tracing | E4M1 walls 109 → 22 ms |
| Clear only the portal-mask columns used last frame | E2M4 walls 9.7 → 7.1 ms |
| Fixed-point sprite rows + opaque fast loop | E2M4 close zombies 18.5 → 8.6 ms |
| Cull voxels entirely behind the camera | E1M7 112 → 29 ms |

Use `openblood perf DATA MAP W H` to measure a change, and compare `shotg` renders before and
after to make sure the output did not change.
