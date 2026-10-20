# Roadmap and Known Issues

## Not implemented yet

- Multiplayer (bloodbath and co-op start channels are parsed but unused).
- The full set of enemy behaviours; health and damage of most enemies are not measured yet.

## Known differences from the original

- **E1M1 starting coffin:** the view above the coffin walls still differs (a room-over-room
  view). Eye height and vertical proportions elsewhere were fitted to screenshots of the
  original and match closely.
- Some enemy values are estimates: cultist health and tommy-gun cultist damage (measuring them
  needs aimed shots at moving targets in DOSBox, which keyboard input can't do reliably).
  Difficulty damage scaling is measured for all five skills.
- Axe-zombie hit spacing at the E4M9 start varies between runs in both games (it depends on how
  many zombies reach the player); single recordings are not enough to tune it further.
- Saves are tied to the save-format version; saves from older OpenBlood builds cannot be
  loaded after a format change.

## Ideas

- More measured enemy behaviour from DOSBox recordings.

Have something to add? See [Contributing](Contributing).
