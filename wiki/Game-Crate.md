# Game crate (`ob-game`)

Blood's game rules on top of `ob-engine`. Pure logic: no window, input device or audio
output, so it runs headless in tests and tools. API docs:
<https://dagrigorev.github.io/OpenBlood/ob_game/>

Dependencies: `ob-engine`, `serde` + `bincode` (saves).

## Modules

| Module | Purpose | Key items |
| --- | --- | --- |
| `resources` | Finds and loads the user's original files; builds true-colour shade tables | `Resources::load`, `Resources::build_luts`, `PAL_NAMES`, `PLU_NAMES` |
| `ini` | `BLOOD.INI`: episodes, map order, titles, music, in-level messages, cutscene placement | `BloodIni`, `Episode`, `MapInfo` |
| `world` | Runtime copy of a map: sectors, walls, sprites, x-records, sprite status lists, room-over-room links | `World`, `SpriteRt`, `stat` |
| `game` | Top-level per-level state and the 120 Hz tick | `Game`, `Game::update`, `Game::camera`, `Game::sprite_draws`, `GameEvent`, `Ai`, `TICK_RATE` |
| `game::ai` | Enemy perception (sight, hearing, editor flags) and movement (chase, flee, strafe, obstacle and ledge avoidance), rising zombies, stone gargoyles, burning | `starts_buried`, `is_statue` |
| `game::save` | Save slots: the whole mutable `Game` via bincode | `slot_path`, `slot_title` |
| `player` | Player movement, jumping, crouching, swimming, view height | `Player`, `Input`, `EYE_STAND`, `WALK_SPEED`, `GRAVITY`, `JUMP_VEL` |
| `weapons` | Weapon table, ammo, fire modes and the QAV-driven weapon state machine | `WEAPONS`, `WeaponDef`, `FireMode`, `Fire`, `Arsenal`, `AMMO_MAX` |
| `dudes` | Enemy ("dude") definitions: health, speed, attacks, animation sets | `dude_info`, `DudeInfo`, `Attack`, `ProjKind` |
| `items` | What each pickup type gives | `pickup_of`, `Pickup`, `Timed` |
| `inventory` | Usable items and timed power-ups | `Inventory`, `Powerups`, `InvItem` |
| `triggers` | Blood's object messaging system (channels, busy motion, moving sectors) | `Triggers`, `cmd`, `chan`, `sector_type`, `Obj` |
| `anim` | `SEQ` sprite sequences and `QAV` screen animations | `Seq`, `Qav`, `AnimLib` |

## The `Game` API used by the frontend

```rust
let mut game = Game::load_level(&res, "E1M1")?;      // or load_level_skill(.., skill)
game.update(&res, &input, dt);                        // advance real time dt (fixed ticks inside)
for ev in game.events.drain(..) { /* sounds, messages, level exit, death */ }
let cam = game.camera();                              // interpolated eye position
let sprites = game.sprite_draws(&res, &cam);          // rotation frames resolved, interpolated
let (qav, ticks) = game.weapon_view();                // weapon overlay for the HUD
```

`GameEvent` is the only channel from the game back to the frontend (sounds with optional
world position, on-screen messages, level changes, player death).

## Tuning against the original

Values that affect feel were measured from recordings of the original in DOSBox, not copied
from its code:

- Walking: top speed ≈ 4600 units/s, acceleration response ≈ 20/s (`player::WALK_SPEED`).
- Jump: 0.8 s airtime → `GRAVITY = 200000`, `JUMP_VEL = 80000`.
- Eye height `EYE_STAND = 0x3800`, fitted by correlating renders with captures of the original.
- Axe zombie: approach speed ~1700 units/s (`dudes::SPEED_SCALE` 1.5, from frame-by-frame size
  matching of recordings against renders at known distances), 2.2 s swing cooldown, damage per difficulty
  (`damage_player` scales by skill: 9/16, 13/16, 1, 19/16, 23/16, measured as 10-11, 14-15,
  18, 21-22 and 26 per axe hit at the E4M9 start on skills 0-4).
- Tommy gun: 14.6 rounds/s. Shotgun cultist blast: 7 pellets × 4 damage.

The data-driven tests replay these scenarios; see [Testing](Testing).

## Adding an enemy behaviour or item

- Enemy stats and attacks: `dudes.rs` (`dude_info`), behaviour in `game/ai.rs`.
- Pickup effects: `items.rs` (`pickup_of`) and `Game::apply_pickup`.
- New mutable state must be serializable (`serde`) and, if the save layout changes, bump the
  save version in `game/save.rs`.
