# Roadmap and Known Issues

## Not implemented yet

- Multiplayer (bloodbath and co-op start channels are parsed but unused).
- The full set of enemy behaviours; health and damage of most enemies are not measured yet.

## Known differences from the original

- **E1M1 starting coffin:** shape, eye height and the floor's rise match DOSBox recordings
  (earlier "taller view" reports came from screenshots taken after the rise had begun: a click
  or fire key starts the level). Still different: the crypt seen through the open top is
  brighter than in the original, and its carved panels look gold instead of reddish.
- Some enemy values are estimates: cultist health and tommy-gun cultist damage (measuring them
  needs aimed shots at moving targets in DOSBox, which keyboard input can't do reliably).
  Difficulty damage scaling is measured for all five skills.
- E4M9 axe-zombie ambush: approach speed, first hit and death on skill 2 match the original
  (deterministic DOSBox recordings: hits at 4.2, 5.0, 5.6, 6.7, 7.5 s). Still different: OpenBlood's
  zombies settle 30-90 degrees beside the player, so holding the tommy gun kills none where the
  original kills three; on skill 0 the player survives ~3 s longer, on skills 3-4 dies sooner.
- Saves are tied to the save-format version; saves from older OpenBlood builds cannot be
  loaded after a format change.

## Ideas

- More measured enemy behaviour from DOSBox recordings.

Have something to add? See [Contributing](Contributing).
