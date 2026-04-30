//! CLI para medir tempo de parse de replay por estágio.
//!
//! Uso:
//!   cargo run --release --bin profile_replay -- replay.SC2Replay [more...]

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: profile_replay <replay.SC2Replay> [more...]");
        std::process::exit(1);
    }
    for arg in args {
        let path = PathBuf::from(&arg);
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| arg.clone());
        match sc2_replay_utils::profile::profile_parse(&path) {
            Ok(r) => {
                println!(
                    "[profile] file={} read={}ms header={}ms tracker={}ms game={}ms message={}ms total={}ms",
                    file_name,
                    r.read_ms,
                    r.header_ms,
                    r.tracker_ms,
                    r.game_ms,
                    r.message_ms,
                    r.total_ms,
                );
            }
            Err(e) => {
                eprintln!("ERROR processing {}: {}", path.display(), e);
            }
        }
    }
}
