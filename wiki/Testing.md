# Testing

```
cargo test --workspace                                         # unit tests, no game data
OPENBLOOD_DATA=/path/to/blood cargo test --workspace --release  # + tests on your own files
```

Use `--release` for the data-driven tests: they load every level and simulate many seconds of
game time.

## Unit tests

Live next to the code (`#[cfg(test)] mod tests`) and use synthetic data only, so they run
anywhere, including CI. Examples:

- format readers: obfuscation round trips, bit readers, record sizes, rejecting foreign files
  (`rff`, `art`, `map`, `voxel`, `smk`, `anim`);
- collision: solid walls stop, portals pass, high steps block, z-range and hitscan against
  walls, floors and sprites (`collide`);
- triggers: a door opens, waits and closes; toggling a busy door reverses it; moving sectors
  carry sprites (`triggers`);
- scaling: column-major and row-major resamplers agree; the 2:1 fast path equals a box
  average; letterboxing (`scale`);
- config parsing, menu navigation, inventory, items, weapons.

## Tests against the original data

`crates/engine/tests/original_data.rs` and `crates/game/tests/original_data.rs` run only when
`OPENBLOOD_DATA` points at a folder with `BLOOD.RFF`; otherwise they return early and pass.
The data never enters the repository.

| Test | Checks |
| --- | --- |
| `all_maps_parse` | every MAP in BLOOD.RFF parses, start sector valid |
| `tiles_and_palette_load` | > 3000 non-empty tiles, palette and PLU sizes |
| `all_seq_and_qav_parse` | every animation parses |
| `every_level_loads_and_simulates` | each level loads and runs without panicking |
| `player_dives_through_water_link` | room-over-room transition into water |
| `gunfire_scares_innocents_and_wakes_cultists` | hearing and fleeing AI |
| `shotgun_alt_fire_uses_two_shells` | alternate fire ammo use |
| `save_and_load_round_trip_is_deterministic` | continuing and reloading give identical states |
| `walking_acceleration_matches_original` | speed curve measured from DOSBox video |
| `axe_zombie_ambush_matches_original` | E4M9: first hit and death times by difficulty |
| `pitchfork_does_not_outlast_axe_zombies` | melee balance |
| `point_blank_shotgun_gibs_axe_zombie` | pellet damage adds up |
| `tommy_gun_fire_rate_matches_original` | 14.6 rounds/s |
| `tommy_gun_ambush_matches_original` | E4M9 fight with the tommy gun |
| `e1m1_player_can_climb_out_of_coffin` | jump height and coffin geometry |
| `idle_dudes_wander_near_home` | idle enemies move but stay near their post |
| `camera_moves_smoothly_between_ticks` | interpolation has no jumps |
| `all_cutscenes_decode` | every SMK in `movie/` decodes |
| `original_songs_render` (openblood) | every MIDI song parses and renders audibly without clipping |

## Visual checks

Rendering changes are verified by rendering the same views before and after with `shotg` and
comparing the PNGs (pixel-identical for optimizations). Behaviour that is matched to the
original is compared against DOSBox captures; see [Developer Tools](Developer-Tools).

## In CI

GitHub Actions runs the unit tests on Windows and Linux for every pull request and before
every release. The data-driven tests cannot run there (no game data), so run them locally
before sending changes that touch game behaviour. See [CI and Releases](CI-and-Releases).
