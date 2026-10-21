# Roadmap and Known Issues

## Not implemented yet

- Multiplayer (bloodbath and co-op start channels are parsed but unused).
- The full set of enemy behaviours; health and damage of most enemies are not measured yet.

## Known differences from the original

- **E1M1 starting coffin:** shape, eye height and the floor's rise match DOSBox recordings
  (earlier "taller view" reports came from screenshots taken after the rise had begun: a click
  or fire key starts the level). The crypt seen through the open top is still slightly brighter
  than in the original at the very start.
- **E2M1 start:** the original picks up the sawed-off and a box of shells lying 689 units from
  the player start at once; OpenBlood's pickup reach is 160 units, so they stay on the boat.
  Not changed yet: one screenshot can't tell which rule the original uses.
- **Lifts holding pickups:** a few lifts are stored open in the map with a pickup floating above
  the open floor (E3M1 sectors 174 and 282). OpenBlood closes them at load (state "off") and the
  pickup stays where it was; whether the original starts these sectors open is not measured.
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
