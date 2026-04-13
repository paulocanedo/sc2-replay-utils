# CLAUDE.md

## Project Overview

**sc2-replay-utils** is a StarCraft II replay analysis desktop app built with Rust and egui/eframe. It parses `.SC2Replay` files (Blizzard's MPQ-based format) and provides interactive visualizations: build orders, army value charts, timeline with minimap, and chat viewer.

**Primary language:** English (code, comments, commits, docs). Some older commits are in Portuguese — this is legacy, not the standard going forward.

## Quick Reference

```sh
cargo build --release          # build the binary
cargo test --release           # run all tests
cargo run --release             # run the GUI app
```

Binary output: `target/release/sc2-replay-utils` (or `.exe` on Windows).

## Architecture

### Module Layout

```
src/
├── bin/gui.rs              # Entry point — declares modules via #[path], calls eframe::run_native
├── replay/                 # Core parser (single-pass, streaming)
│   ├── mod.rs              # parse() entry, public API
│   ├── types.rs            # ReplayTimeline, PlayerTimeline, EntityEvent, StatsSnapshot
│   ├── tracker.rs          # Tracker events → semantic EntityEvent vocabulary
│   ├── game.rs             # Game events (Cmd/Selection for production tracking)
│   ├── message.rs          # Chat extraction
│   ├── query.rs            # O(log n) binary-search APIs for timeline scrubbing
│   ├── classify.rs         # Entity classification heuristics (worker/structure/upgrade)
│   └── finalize.rs         # Post-processing and indexing
├── build_order.rs          # Production timeline with Chrono Boost, Inject Larva
├── balance_data.rs         # Build time lookups from generated BalanceData tables
├── army_value.rs           # Army supply/value calculations
├── production_gap.rs       # Idle time analysis
├── supply_block.rs         # Supply constraint detection
├── chat.rs                 # Chat wrapper
├── utils.rs                # Helpers (race letters, replay discovery, name parsing)
├── map_image/              # Minimap extraction from .SC2Map files
│   ├── mod.rs              # Pipeline: cache handles → local fallback
│   ├── decode.rs           # TGA → RGBA8 decoding
│   └── locator.rs          # Map file resolution (Battle.net Cache or local)
└── gui/                    # GUI-exclusive modules
    ├── app.rs              # AppState, eframe::App impl, screen routing
    ├── tabs/               # Analysis tabs
    │   ├── mod.rs          # Tab enum (Timeline, BuildOrder, Charts, Chat)
    │   ├── timeline.rs     # Minimap with scrubbing and camera heatmap
    │   ├── build_order.rs  # Production timeline visualization
    │   ├── charts.rs       # Army value and worker supply charts
    │   └── chat.rs         # Chat viewer
    ├── library.rs          # Replay library browser with filtering/caching
    ├── config.rs           # YAML-based persistent config
    ├── cache.rs            # Metadata cache (bincode serialization)
    ├── replay_state.rs     # LoadedReplay state and UI formatting
    ├── rename.rs           # Batch rename templating UI
    ├── watcher.rs          # File watcher (notify crate) for auto-loading
    ├── locale.rs           # Localization system (en, pt-BR)
    ├── salt.rs             # Color generation for player identification
    ├── ui_settings.rs      # Theme and UI preferences
    └── colors.rs           # Color palette constants
```

### Key Design Decisions

- **Single-pass parser:** All events (tracker, game, message) are processed in one pass over the MPQ archive. This avoids multiple reads and keeps parsing fast.
- **Semantic event vocabulary:** Raw s2protocol events (UnitInit/Born/Done/Died/TypeChange) are translated to `EntityEvent` enums (ProductionStarted, ProductionFinished, Died, etc.) in `tracker.rs`.
- **`ReplayTimeline`** is the central data structure — all extractors (build order, army value, etc.) consume it.
- **Module injection via `#[path]`:** `gui.rs` uses `#[path = "../module.rs"]` to declare domain modules from the binary entry point, since there's no `lib.rs`.

### Build Script (`build.rs`)

Generates `balance_data_generated.rs` at compile time with two static lookup tables:
- `BALANCE_ENTRIES`: `(protocol_version, action_name) → build_loops`
- `ABILITY_ENTRIES`: `(protocol_version, producer, ability_id, cmd_index) → action_name`

These are extracted from s2protocol's bundled BalanceData JSONs. The build script reads them directly from `.cargo/registry` because s2protocol's own `read_balance_data_from_included_assets()` has a Windows path separator bug.

## Conventions

### Commits

Use [Conventional Commits](https://www.conventionalcommits.org/) in English:
- `feat(scope): description` — new feature
- `fix(scope): description` — bug fix
- `refactor(scope): description` — code restructuring
- `chore: description` — maintenance tasks

Common scopes: `build_order`, `timeline`, `charts`, `replay`, `gui`, `library`, `map`, `balance`, `army-value`, `supply-block`

### Versioning & Releases

- Semantic versioning. Version lives in `Cargo.toml`.
- Tag-triggered releases: `git tag v0.x.0 && git push origin master --tags`
- CI builds binaries for Windows and Linux, creates GitHub Release with changelog (git-cliff).

### Testing

- Tests are inline `#[cfg(test)]` modules within source files.
- Golden CSV tests in `src/build_order.rs` compare parser output against `examples/golden/*.csv`.
- To update goldens: `cargo test --bin sc2-replay-utils bless_build_order_goldens -- --ignored`
- Run all tests: `cargo test --release`

### Localization

- Translation files: `data/locale/{en,pt-BR}.txt`
- Format: `MPQ_KEY=Display Name` (one per line)
- Covers units, structures, upgrades, and abilities.

## Files to Know

| File | Why it matters |
|---|---|
| `Cargo.toml` | Version, dependencies, binary target |
| `build.rs` | Balance data code generation — if builds fail, check this first |
| `.env.example` | All configurable environment variables |
| `cliff.toml` | Changelog generation config (git-cliff) |
| `.github/workflows/ci.yml` | CI pipeline (build + test) |
| `.github/workflows/release.yml` | Release pipeline (tag-triggered) |
| `examples/` | Sample replays and golden test outputs |

## Pitfalls

- **`.gitignore` blocks `*.yaml`** — use `.yml` for any committed YAML files (e.g., GitHub workflows).
- **Edition 2024** — requires Rust 1.85+. CI uses `stable` toolchain.
- **Linux builds need `libgtk-3-dev`** — required by the `rfd` crate for native file dialogs.
- **No `lib.rs`** — all modules are declared in `src/bin/gui.rs` via `#[path]`. This is intentional to keep the project as a single binary.
