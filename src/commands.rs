use std::fs;
use std::path::PathBuf;
use std::process;

use crate::replay::parse_replay;
use crate::utils::{
    find_latest_replay, list_replays, prepare_out_dir, race_letter, resolve_dir, resolve_path,
    sanitize, sc2_default_dir,
};

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
        let data = match parse_replay(replay_path, 0) {
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

fn resolve_dump_path(
    path: Option<PathBuf>,
    latest: bool,
    sc2_replay_dir: Option<PathBuf>,
) -> PathBuf {
    // Caminho explícito tem prioridade sobre --latest
    if let Some(p) = path {
        if !p.exists() {
            eprintln!("Erro: '{}' não encontrado", p.display());
            process::exit(1);
        }
        return p;
    }

    if latest {
        let base = sc2_replay_dir
            .or_else(sc2_default_dir)
            .unwrap_or_else(|| {
                eprintln!(
                    "Erro: diretório de replays do SC2 não encontrado. \
                     Use --sc2-replay-dir ou SC2_REPLAY_DIR."
                );
                process::exit(1);
            });

        return find_latest_replay(&base).unwrap_or_else(|| {
            eprintln!("Nenhum replay encontrado em '{}'", base.display());
            process::exit(1);
        });
    }

    // Fallback padrão
    resolve_path(None)
}

fn dump_one(replay_path: &std::path::Path, dest: &DumpDest, max_time: u32) {
    let data = match parse_replay(replay_path, max_time) {
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

pub fn cmd_dump(
    path: Option<PathBuf>,
    output: Option<PathBuf>,
    stdout: bool,
    max_time: u32,
    latest: bool,
    sc2_replay_dir: Option<PathBuf>,
) {
    let path = resolve_dump_path(path, latest, sc2_replay_dir);

    if path.is_file() {
        let dest = if stdout {
            DumpDest::Stdout
        } else {
            let dir = output.unwrap_or_else(|| PathBuf::from("."));
            DumpDest::Dir(dir)
        };
        dump_one(&path, &dest, max_time);
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
            let dir = output.unwrap_or_else(|| PathBuf::from("out"));
            prepare_out_dir(&dir);
            DumpDest::Dir(dir)
        };

        for replay_path in &replays {
            dump_one(replay_path, &dest, max_time);
        }
    }

    println!("Concluído.");
}
