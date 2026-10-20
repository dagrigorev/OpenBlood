# Roadmap and Known Issues

## Not implemented yet

- Multiplayer (bloodbath and co-op start channels are parsed but unused).
- MIDI music fallback when no CD tracks were ripped.
- The full set of enemy behaviours; health and damage of most enemies are not measured yet.

## Known differences from the original

- **E1M1 starting coffin:** the view above the coffin walls still differs (a room-over-room
  view). Eye height and vertical proportions elsewhere were fitted to screenshots of the
  original and match closely.
- Some enemy values (cultist health, damage at difficulties 1, 3 and 4) are estimates.
- Saves are tied to the save-format version; saves from older OpenBlood builds cannot be
  loaded after a format change.

## Ideas

- More platforms in the release pipeline (macOS, Windows ARM64).
- Enforcing clippy in CI once the remaining warnings are fixed.
- More measured enemy behaviour from DOSBox recordings.

Have something to add? See [Contributing](Contributing).
