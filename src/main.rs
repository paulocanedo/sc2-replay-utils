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
        /// Usa o replay mais recente encontrado em --sc2-dir [env: SC2RU_LATEST]
        #[arg(long, env = "SC2RU_LATEST")]
        latest: bool,
        /// Diretório raiz de replays do SC2 para busca com --latest [env: SC2RU_SC2_DIR]
        #[arg(long, env = "SC2RU_SC2_DIR")]
        sc2_dir: Option<PathBuf>,
    },
}

fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    match cli.command {
        Commands::Rename { dir } => commands::cmd_rename(dir),
        Commands::Dump { path, output, stdout, max_time, latest, sc2_dir } => {
            commands::cmd_dump(path, output, stdout, max_time, latest, sc2_dir)
        }
    }
}
