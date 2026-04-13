# sc2-replay-utils

A StarCraft II replay analysis tool with a graphical interface, built in Rust.

## Features

- **Replay Library** — browse and manage replays with persistent metadata cache and file watcher for auto-loading new replays
- **Build Order Analysis** — detailed production timeline with Chrono Boost, Inject Larva, and hatchery target tracking
- **Army Value Charts** — army and worker supply over time, with supply block visualization
- **Timeline** — interactive timeline with camera heatmap and minimap viewport overlay
- **Chat Viewer** — in-game chat messages with timestamps
- **Batch Rename** — rename replay files based on metadata
- **Localization** — English and Brazilian Portuguese

## Screenshots

<!-- TODO: add screenshots -->

## Installation

### Pre-built binaries

Download the latest release for your platform from the [Releases](../../releases) page.

- **Windows:** download the `.zip`, extract, and run `sc2-replay-utils.exe`
- **Linux:** download the `.tar.gz`, extract, and run `sc2-replay-utils`

### Building from source

**Requirements:** Rust 1.85+ (edition 2024)

On Linux, install the GTK3 development libraries first:

```sh
sudo apt-get install -y libgtk-3-dev
```

Then build:

```sh
cargo build --release
```

The binary will be at `target/release/sc2-replay-utils` (or `sc2-replay-utils.exe` on Windows).

## Configuration

Copy `.env.example` to `.env` and adjust the paths to match your system. See the file for available options.

## License

[MIT](LICENSE)
