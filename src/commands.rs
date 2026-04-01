use std::fs;
use std::path::PathBuf;
use std::process;

use crate::all_image::write_all_png;
use crate::army_value::{extract_army_value, to_army_value_csv};
use crate::army_value_image::write_army_value_png;
use crate::build_order::{extract_build_order, to_fixed_csv};
use crate::chat::{extract_chat, to_chat_txt};
use crate::supply_block::{extract_supply_blocks, to_supply_block_csv};
use crate::supply_block_image::write_supply_block_png;
use crate::build_order_image::write_build_order_png;
use crate::replay::parse_replay;
use crate::utils::{
    find_latest_replay, list_replays, prepare_out_dir, race_letter, replay_base, resolve_dir,
    resolve_path, sanitize, sc2_default_dir,
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
        let data = match parse_replay(replay_path, 0, false) {
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

pub(crate) fn resolve_dump_path(
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

fn dump_one(replay_path: &std::path::Path, dest: &DumpDest, max_time: u32, include_location: bool) {
    let data = match parse_replay(replay_path, max_time, include_location) {
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
            let base = if data.players.len() >= 2 {
                let p1 = &data.players[0];
                let p2 = &data.players[1];
                replay_base(&data.datetime, &data.map, &p1.name, &p1.race, &p2.name, &p2.race)
            } else {
                replay_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "replay".to_string())
            };
            let out_file = dir.join(format!("{}.yaml", base));
            match fs::write(&out_file, &yaml) {
                Ok(_) => println!("  {} -> {}", replay_path.display(), out_file.display()),
                Err(e) => eprintln!("  ERRO ao gravar {}: {}", out_file.display(), e),
            }
        }
    }
}

fn all_image_one(replay_path: &std::path::Path, out_dir: &std::path::Path, max_time: u32) {
    let army_result = match extract_army_value(replay_path, max_time) {
        Ok(r) => r,
        Err(e) => { eprintln!("  SKIP imagem {}: {}", replay_path.display(), e); return; }
    };
    let bo_result = match extract_build_order(replay_path, max_time) {
        Ok(r) => r,
        Err(e) => { eprintln!("  SKIP imagem {}: {}", replay_path.display(), e); return; }
    };
    let replay_data = match parse_replay(replay_path, max_time, false) {
        Ok(d) => d,
        Err(e) => { eprintln!("  SKIP imagem {}: {}", replay_path.display(), e); return; }
    };

    let [army_p1, army_p2, ..] = army_result.players.as_slice() else {
        eprintln!("  SKIP imagem {}: menos de 2 jogadores", replay_path.display());
        return;
    };
    let [bo_p1, bo_p2, ..] = bo_result.players.as_slice() else {
        eprintln!("  SKIP imagem {}: menos de 2 jogadores na build order", replay_path.display());
        return;
    };

    let effective_end = if max_time == 0 {
        replay_data.game_loops
    } else {
        replay_data.game_loops.min((max_time as f64 * replay_data.loops_per_second).round() as u32)
    };

    let sb_p1 = if replay_data.players.len() >= 1 {
        extract_supply_blocks(&replay_data.players[0].stats_snapshots, effective_end)
    } else { vec![] };
    let sb_p2 = if replay_data.players.len() >= 2 {
        extract_supply_blocks(&replay_data.players[1].stats_snapshots, effective_end)
    } else { vec![] };

    let base = replay_base(
        &army_result.datetime, &army_result.map_name,
        &army_p1.name, &army_p1.race,
        &army_p2.name, &army_p2.race,
    );
    let png_file = out_dir.join(format!("{}_all.png", base));

    match write_all_png(
        army_p1, army_p2,
        &army_result.map_name,
        army_result.game_loops,
        army_result.loops_per_second,
        bo_p1, bo_p2,
        &sb_p1, &sb_p2,
        &png_file,
    ) {
        Ok(_) => println!("  {} -> {}", replay_path.display(), png_file.display()),
        Err(e) => eprintln!("  ERRO ao gerar PNG {}: {}", png_file.display(), e),
    }
}

pub fn cmd_all(
    path: Option<PathBuf>,
    output: Option<PathBuf>,
    stdout: bool,
    max_time: u32,
    include_location: bool,
    latest: bool,
    sc2_replay_dir: Option<PathBuf>,
    image: bool,
) {
    let path = resolve_dump_path(path, latest, sc2_replay_dir);

    if path.is_file() {
        let dest = if stdout {
            DumpDest::Stdout
        } else {
            let dir = output.unwrap_or_else(|| PathBuf::from("."));
            DumpDest::Dir(dir)
        };
        dump_one(&path, &dest, max_time, include_location);
        if image {
            let out_dir = match &dest { DumpDest::Dir(d) => d.clone(), DumpDest::Stdout => PathBuf::from(".") };
            all_image_one(&path, &out_dir, max_time);
        }
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

        let out_dir = match &dest { DumpDest::Dir(d) => d.clone(), DumpDest::Stdout => PathBuf::from(".") };
        for replay_path in &replays {
            dump_one(replay_path, &dest, max_time, include_location);
            if image {
                all_image_one(replay_path, &out_dir, max_time);
            }
        }
    }

    println!("Concluído.");
}

fn build_order_one(
    replay_path: &std::path::Path,
    out_dir: &std::path::Path,
    max_time: u32,
    image: bool,
) {
    let result = match extract_build_order(replay_path, max_time) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  SKIP {}: {}", replay_path.display(), e);
            return;
        }
    };

    let base = if result.players.len() >= 2 {
        let p1 = &result.players[0];
        let p2 = &result.players[1];
        replay_base(&result.datetime, &result.map_name, &p1.name, &p1.race, &p2.name, &p2.race)
    } else {
        replay_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "replay".to_string())
    };

    for (i, player) in result.players.iter().enumerate() {
        let n = i + 1;
        let player_suffix = if player.name.is_empty() {
            format!("p{}", n)
        } else {
            format!("{}({})", sanitize(&player.name), race_letter(&player.race))
        };
        let out_file = out_dir.join(format!("{}_build_{}.csv", base, player_suffix));
        let csv = to_fixed_csv(&player.entries, result.loops_per_second);
        match fs::write(&out_file, &csv) {
            Ok(_) => println!("  {} -> {}", replay_path.display(), out_file.display()),
            Err(e) => eprintln!("  ERRO ao gravar {}: {}", out_file.display(), e),
        }

        if image {
            let png_file = out_dir.join(format!("{}_build_{}.png", base, player_suffix));
            match write_build_order_png(n, &player.name, &player.race, player.mmr, &player.entries, result.loops_per_second, &png_file) {
                Ok(_) => println!("  {} -> {}", replay_path.display(), png_file.display()),
                Err(e) => eprintln!("  ERRO ao gerar PNG {}: {}", png_file.display(), e),
            }
        }
    }
}

fn supply_block_one(
    replay_path: &std::path::Path,
    out_dir: &std::path::Path,
    max_time: u32,
    image: bool,
) {
    let data = match parse_replay(replay_path, max_time, false) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  SKIP {}: {}", replay_path.display(), e);
            return;
        }
    };

    let base = if data.players.len() >= 2 {
        let p1 = &data.players[0];
        let p2 = &data.players[1];
        replay_base(&data.datetime, &data.map, &p1.name, &p1.race, &p2.name, &p2.race)
    } else {
        replay_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "replay".to_string())
    };

    let effective_end = if max_time == 0 {
        data.game_loops
    } else {
        data.game_loops.min((max_time as f64 * data.loops_per_second).round() as u32)
    };

    for (i, player) in data.players.iter().enumerate() {
        let n = i + 1;
        let player_suffix = if player.name.is_empty() {
            format!("p{}", n)
        } else {
            format!("{}({})", sanitize(&player.name), race_letter(&player.race))
        };

        let entries = extract_supply_blocks(&player.stats_snapshots, effective_end);
        let csv = to_supply_block_csv(&entries, data.loops_per_second);

        let out_file = out_dir.join(format!("{}_supply_{}.csv", base, player_suffix));
        match fs::write(&out_file, &csv) {
            Ok(_) => println!("  {} -> {}", replay_path.display(), out_file.display()),
            Err(e) => eprintln!("  ERRO ao gravar {}: {}", out_file.display(), e),
        }

        if image {
            let png_file = out_dir.join(format!("{}_supply_{}.png", base, player_suffix));
            match write_supply_block_png(n, &player.name, &player.race, player.mmr, &entries, effective_end, data.loops_per_second, &png_file) {
                Ok(_) => println!("  {} -> {}", replay_path.display(), png_file.display()),
                Err(e) => eprintln!("  ERRO ao gerar PNG {}: {}", png_file.display(), e),
            }
        }
    }
}

pub fn cmd_supply_block(
    path: Option<PathBuf>,
    output: Option<PathBuf>,
    max_time: u32,
    latest: bool,
    sc2_replay_dir: Option<PathBuf>,
    image: bool,
) {
    let path = resolve_dump_path(path, latest, sc2_replay_dir);

    if path.is_file() {
        let out_dir = output.unwrap_or_else(|| PathBuf::from("."));
        supply_block_one(&path, &out_dir, max_time, image);
    } else {
        let replays = list_replays(&path);
        if replays.is_empty() {
            eprintln!("Nenhum arquivo .SC2Replay encontrado em '{}'", path.display());
            process::exit(1);
        }
        println!("Encontrados {} replays", replays.len());

        let out_dir = output.unwrap_or_else(|| PathBuf::from("out"));
        prepare_out_dir(&out_dir);

        for replay_path in &replays {
            supply_block_one(replay_path, &out_dir, max_time, image);
        }
    }

    println!("Concluído.");
}

fn army_value_one(
    replay_path: &std::path::Path,
    out_dir: &std::path::Path,
    max_time: u32,
    image: bool,
) {
    let result = match extract_army_value(replay_path, max_time) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  SKIP {}: {}", replay_path.display(), e);
            return;
        }
    };

    let base = if result.players.len() >= 2 {
        let p1 = &result.players[0];
        let p2 = &result.players[1];
        replay_base(&result.datetime, &result.map_name, &p1.name, &p1.race, &p2.name, &p2.race)
    } else {
        replay_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "replay".to_string())
    };

    for (i, player) in result.players.iter().enumerate() {
        let n = i + 1;
        let player_suffix = if player.name.is_empty() {
            format!("p{}", n)
        } else {
            format!("{}({})", sanitize(&player.name), race_letter(&player.race))
        };
        let out_file = out_dir.join(format!("{}_army_{}.csv", base, player_suffix));
        let csv = to_army_value_csv(&player.snapshots, result.loops_per_second);
        match fs::write(&out_file, &csv) {
            Ok(_) => println!("  {} -> {}", replay_path.display(), out_file.display()),
            Err(e) => eprintln!("  ERRO ao gravar {}: {}", out_file.display(), e),
        }
    }

    if image {
        if let [p1, p2, ..] = result.players.as_slice() {
            let png_file = out_dir.join(format!("{}_army.png", base));
            match write_army_value_png(p1, p2, &result.map_name, result.game_loops, result.loops_per_second, &png_file) {
                Ok(_) => println!("  {} -> {}", replay_path.display(), png_file.display()),
                Err(e) => eprintln!("  ERRO ao gerar PNG {}: {}", png_file.display(), e),
            }
        }
    }
}

pub fn cmd_army_value(
    path: Option<PathBuf>,
    output: Option<PathBuf>,
    max_time: u32,
    latest: bool,
    sc2_replay_dir: Option<PathBuf>,
    image: bool,
) {
    let path = resolve_dump_path(path, latest, sc2_replay_dir);

    if path.is_file() {
        let out_dir = output.unwrap_or_else(|| PathBuf::from("."));
        army_value_one(&path, &out_dir, max_time, image);
    } else {
        let replays = list_replays(&path);
        if replays.is_empty() {
            eprintln!("Nenhum arquivo .SC2Replay encontrado em '{}'", path.display());
            process::exit(1);
        }
        println!("Encontrados {} replays", replays.len());

        let out_dir = output.unwrap_or_else(|| PathBuf::from("out"));
        prepare_out_dir(&out_dir);

        for replay_path in &replays {
            army_value_one(replay_path, &out_dir, max_time, image);
        }
    }

    println!("Concluído.");
}

fn chat_one(replay_path: &std::path::Path, out_dir: &std::path::Path, max_time: u32) {
    let rd = match parse_replay(replay_path, max_time, false) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  SKIP {}: {}", replay_path.display(), e);
            return;
        }
    };

    let result = match extract_chat(replay_path, max_time) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  SKIP {}: {}", replay_path.display(), e);
            return;
        }
    };

    let (base, p1_label, p2_label) = if rd.players.len() >= 2 {
        let p1 = &rd.players[0];
        let p2 = &rd.players[1];
        let base = replay_base(&rd.datetime, &rd.map, &p1.name, &p1.race, &p2.name, &p2.race);
        let p1l = format!("{}({})", sanitize(&p1.name), race_letter(&p1.race));
        let p2l = format!("{}({})", sanitize(&p2.name), race_letter(&p2.race));
        (base, p1l, p2l)
    } else {
        let base = replay_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "replay".to_string());
        (base, "P1".to_string(), "P2".to_string())
    };

    let out_file = out_dir.join(format!("{}_chat.txt", base));
    let txt = to_chat_txt(&result, (&p1_label, &p2_label));
    match fs::write(&out_file, &txt) {
        Ok(_) => println!("  {} -> {}", replay_path.display(), out_file.display()),
        Err(e) => eprintln!("  ERRO ao gravar {}: {}", out_file.display(), e),
    }
}

pub fn cmd_chat(
    path: Option<PathBuf>,
    output: Option<PathBuf>,
    max_time: u32,
    latest: bool,
    sc2_replay_dir: Option<PathBuf>,
) {
    let path = resolve_dump_path(path, latest, sc2_replay_dir);

    if path.is_file() {
        let out_dir = output.unwrap_or_else(|| PathBuf::from("."));
        chat_one(&path, &out_dir, max_time);
    } else {
        let replays = list_replays(&path);
        if replays.is_empty() {
            eprintln!("Nenhum arquivo .SC2Replay encontrado em '{}'", path.display());
            process::exit(1);
        }
        println!("Encontrados {} replays", replays.len());

        let out_dir = output.unwrap_or_else(|| PathBuf::from("out"));
        prepare_out_dir(&out_dir);

        for replay_path in &replays {
            chat_one(replay_path, &out_dir, max_time);
        }
    }

    println!("Concluído.");
}

pub fn cmd_build_order(
    path: Option<PathBuf>,
    output: Option<PathBuf>,
    max_time: u32,
    latest: bool,
    sc2_replay_dir: Option<PathBuf>,
    image: bool,
) {
    let path = resolve_dump_path(path, latest, sc2_replay_dir);

    if path.is_file() {
        let out_dir = output.unwrap_or_else(|| PathBuf::from("."));
        build_order_one(&path, &out_dir, max_time, image);
    } else {
        let replays = list_replays(&path);
        if replays.is_empty() {
            eprintln!("Nenhum arquivo .SC2Replay encontrado em '{}'", path.display());
            process::exit(1);
        }
        println!("Encontrados {} replays", replays.len());

        let out_dir = output.unwrap_or_else(|| PathBuf::from("out"));
        prepare_out_dir(&out_dir);

        for replay_path in &replays {
            build_order_one(replay_path, &out_dir, max_time, image);
        }
    }

    println!("Concluído.");
}
