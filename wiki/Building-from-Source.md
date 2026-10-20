# Building from Source

## Requirements

- **Rust** 1.88 or newer (edition 2021) via [rustup](https://rustup.rs).
- **Windows:** the MSVC toolchain (Visual Studio Build Tools with the C++ workload).
- **Linux:** a C toolchain plus ALSA headers for audio:
  `sudo apt install build-essential pkg-config libasound2-dev`.

No other system libraries are needed: windowing (winit), presentation (softbuffer) and audio
(cpal) are pure Rust or load the platform libraries at runtime.

## Build and run

```
git clone https://github.com/dagrigorev/OpenBlood.git
cd OpenBlood
cargo build --release
cargo run --release -- --data /path/to/blood --windowed
```

The binary is `target/release/openblood(.exe)`. Release builds keep line-level debug info
(`[profile.release] debug = 1`) so profilers show function names.

Debug builds are optimized too (`opt-level = 1` for the workspace, `3` for dependencies), so
`cargo run` is playable while still compiling quickly.

## Tests

```
cargo test --workspace                                        # unit tests, no game data needed
OPENBLOOD_DATA=/path/to/blood cargo test --workspace --release # + tests against your own files
```

See [Testing](Testing) for what the data-driven tests check.

## Formatting, lints and docs

```
cargo fmt --all            # rustfmt.toml: max_width = 120
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps --document-private-items --open
```

CI runs all of these on every pull request; see [CI and Releases](CI-and-Releases). The
published API documentation is at <https://dagrigorev.github.io/OpenBlood/>.

## Workspace layout

```
Cargo.toml                 workspace: version, edition, profiles
crates/
  engine/   ob-engine      formats, geometry, collision, renderer, Smacker decoder
  game/     ob-game        Blood game logic (depends on ob-engine)
  openblood/               the executable (depends on both)
screenshots/               images used by the README and this wiki
wiki/                      source of this wiki (synced by CI)
.github/workflows/         CI, release and docs pipelines
```

Details: [Architecture](Architecture).
