use std::fs;
use std::path::PathBuf;
use std::process;

use crate::replay::parse_replay;
use crate::utils::{list_replays, prepare_out_dir, race_letter, resolve_dir, resolve_path, sanitize};

pub fn cmd_rename(dir: Option<PathBuf>) {
    let input_dir = resolve_dir(dir);
    let out_dir = input_dir.join("out");
    prepare_out_dir(&out_dir);

    let replays = list_replays(&input_dir);
    if replays.is_empty() {
        eprintln!("Nenhum arquivo .SC2Replay encontrado em '{}'", input_dir.display());
        process::exit(1);
    }
    println!("Encontrados {} replays", replays.len());

    for replay_path in &replays {
        let data = match parse_replay(replay_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  SKIP {}: {}", replay_path.display(), e);
                continue;
            }
        };

        let [p1, p2, ..] = data.players.as_slice() else {
            eprintln!("  SKIP {}: menos de 2 jogadores", replay_path.display());
            continue;
        };

        // "2025-12-18T06:44:53" → "202512180644"
        let datetime_compact = data.datetime.replace(['-', ':', 'T'], "");
        let datetime_compact = &datetime_compact[..12];

        let new_name = format!(
            "{}_{}-{}({})_vs_{}({})_{}.SC2Replay",
            datetime_compact,
            sanitize(&data.map),
            sanitize(&p1.name),
            race_letter(&p1.race),
            sanitize(&p2.name),
            race_letter(&p2.race),
            data.game_loops,
        );

        let dest = out_dir.join(&new_name);
        match fs::copy(replay_path, &dest) {
            Ok(_) => println!("  {} -> {}", replay_path.display(), new_name),
            Err(e) => eprintln!("  ERRO ao copiar {}: {}", replay_path.display(), e),
        }
    }

    println!("Concluído.");
}

enum DumpDest {
    Stdout,
    Dir(PathBuf),
}

fn dump_one(replay_path: &std::path::Path, dest: &DumpDest) {
    let data = match parse_replay(replay_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  SKIP {}: {}", replay_path.display(), e);
            return;
        }
    };

    let yaml = match serde_yml::to_string(&data) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  ERRO ao serializar {}: {}", replay_path.display(), e);
            return;
        }
    };

    match dest {
        DumpDest::Stdout => {
            println!("---");
            print!("{}", yaml);
        }
        DumpDest::Dir(dir) => {
            let stem = replay_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "replay".to_string());
            let out_file = dir.join(format!("{}.yaml", stem));
            match fs::write(&out_file, &yaml) {
                Ok(_) => println!("  {} -> {}", replay_path.display(), out_file.display()),
                Err(e) => eprintln!("  ERRO ao gravar {}: {}", out_file.display(), e),
            }
        }
    }
}

pub fn cmd_dump(path: Option<PathBuf>, output: Option<PathBuf>, stdout: bool) {
    let path = resolve_path(path);

    if path.is_file() {
        let dest = if stdout {
            DumpDest::Stdout
        } else {
            let dir = output.unwrap_or_else(|| {
                path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf()
            });
            DumpDest::Dir(dir)
        };
        dump_one(&path, &dest);
    } else {
        let replays = list_replays(&path);
        if replays.is_empty() {
            eprintln!("Nenhum arquivo .SC2Replay encontrado em '{}'", path.display());
            process::exit(1);
        }
        println!("Encontrados {} replays", replays.len());

        let dest = if stdout {
            DumpDest::Stdout
        } else {
            let dir = output.unwrap_or_else(|| path.join("out"));
            prepare_out_dir(&dir);
            DumpDest::Dir(dir)
        };

        for replay_path in &replays {
            dump_one(replay_path, &dest);
        }
    }

    println!("Concluído.");
}
