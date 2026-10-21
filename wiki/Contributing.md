# Contributing

Contributions are welcome: bug reports, comparisons with the original, code and docs.

## The one hard rule: no Blood content

OpenBlood must stay free of copyrighted game content and code:

- **Never commit** original files or anything extracted from them: maps, ART tiles,
  palettes, sounds, music, videos, executables, or byte dumps used as test fixtures.
- **No original or decompiled source code**, and no code derived from it. Format knowledge
  comes from public documentation and from inspecting your own copy of the game.
- Tests that need real data read it from `OPENBLOOD_DATA` at runtime and skip when it is not
  set.
- Screenshots of OpenBlood running on your own data are fine for documentation.

`.gitignore` ignores `*.png` and `*.wav` outside `screenshots/` to make accidents less
likely.

## Workflow

1. Fork, create a branch from `master`.
2. Make the change with tests (unit tests with synthetic data; data-driven tests in
   `crates/*/tests/original_data.rs` when behaviour depends on the real files).
3. Run locally:
   ```
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   OPENBLOOD_DATA=/path/to/blood cargo test --workspace --release
   ```
4. Open a pull request. CI must be green (see [CI and Releases](CI-and-Releases)).
5. After merge, a release build waits for a maintainer's approval before it is published.

## Code style

- `rustfmt` with `max_width = 120`.
- Module docs (`//!`) explain *what the module covers*, including file layouts for format
  readers; item docs (`///`) explain non-obvious behaviour and units.
- Use Build units (xy, z = 16 × xy, 2048 angle units) in engine and game code; convert at the
  edges.
- Game state that changes during play must be `serde`-serializable; bump the save version
  when the layout changes.
- Behaviour matched to the original should say how it was measured (comment or test).

## Commit messages

Conventional style, as in the history: `feat: ...`, `fix: ...`, `perf: ...`, `docs: ...`,
`ci: ...`, `test: ...`, `style: ...`. The release notes are built from these subjects, so make
them readable.

## Reporting bugs and differences

Include the level, difficulty, what you did, what happened and what the original does.
Screenshots (F12) and the output of `openblood shotg` or `perf` help. For differences from the
original, a DOSBox capture of the same spot is the best evidence.

## Documentation

The wiki is generated from the `wiki/` folder; change it in the same pull request as the code.
API docs come from the doc comments and are published on every push to `master`.
