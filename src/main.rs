use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod commands;
mod replay;
mod utils;

#[derive(Parser)]
#[command(name = "sc2-replay-utils", version, about = "Utilitários para replays de StarCraft II")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Renomeia replays copiando para out/
    Rename {
        /// Diretório com os replays (padrão: sc2replays-pack) [env: SC2RU_DIR]
        #[arg(env = "SC2RU_DIR")]
        dir: Option<PathBuf>,
    },
    /// Exporta metadados de replays para YAML (um arquivo por replay)
    Dump {
        /// Arquivo .SC2Replay ou diretório com replays (padrão: sc2replays-pack) [env: SC2RU_PATH]
        #[arg(env = "SC2RU_PATH")]
        path: Option<PathBuf>,
        /// Diretório de saída para os YAMLs (padrão: <dir>/out/) [env: SC2RU_OUTPUT]
        #[arg(long, env = "SC2RU_OUTPUT")]
        output: Option<PathBuf>,
        /// Imprime todos os YAMLs no stdout em vez de gravar arquivos [env: SC2RU_STDOUT]
        #[arg(long, env = "SC2RU_STDOUT")]
        stdout: bool,
        /// Rastreia eventos até este limite em minutos (0 = sem limite, padrão: 5) [env: SC2RU_MAX_TIME]
        #[arg(long, default_value_t = 5, env = "SC2RU_MAX_TIME")]
        max_time: u32,
        /// Omite campos de localização (pos_x, pos_y) dos eventos [env: SC2RU_NO_LOCATION]
        #[arg(long, env = "SC2RU_NO_LOCATION")]
        no_location: bool,
        /// Usa o replay mais recente encontrado em --sc2-replay-dir [env: SC2RU_LATEST]
        #[arg(long, env = "SC2RU_LATEST")]
        latest: bool,
        /// Diretório onde o SC2 salva os replays, usado com --latest [env: SC2_REPLAY_DIR]
        #[arg(long, env = "SC2_REPLAY_DIR")]
        sc2_replay_dir: Option<PathBuf>,
    },
}

fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    match cli.command {
        Commands::Rename { dir } => commands::cmd_rename(dir),
        Commands::Dump { path, output, stdout, max_time, no_location, latest, sc2_replay_dir } => {
            commands::cmd_dump(path, output, stdout, max_time, !no_location, latest, sc2_replay_dir)
        }
    }
}
