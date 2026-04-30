//! CLI thin para invocar `sc2_replay_utils::debug_addons::run` em um
//! ou mais replays. Usado pela investigação de Phase 0 da resolução de
//! parent de addons.
//!
//! Uso:
//!   cargo run --release --bin debug_addons -- examples/replay1.SC2Replay
//!   cargo run --release --bin debug_addons -- replay1.SC2Replay replay2.SC2Replay

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: debug_addons <replay.SC2Replay> [more...]");
        std::process::exit(1);
    }
    for arg in args {
        let path = PathBuf::from(&arg);
        println!("=========================================");
        println!("=== {}", path.display());
        println!("=========================================");
        if let Err(e) = sc2_replay_utils::debug_addons::run(&path) {
            eprintln!("ERROR processing {}: {}", path.display(), e);
        }
        println!();
    }
}
