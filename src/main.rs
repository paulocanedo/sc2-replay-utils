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
        /// Diretório com os replays (padrão: sc2replays-pack)
        dir: Option<PathBuf>,
    },
    /// Exporta metadados de replays para YAML (um arquivo por replay)
    Dump {
        /// Arquivo .SC2Replay ou diretório com replays (padrão: sc2replays-pack)
        path: Option<PathBuf>,
        /// Diretório de saída para os YAMLs (padrão: <dir>/out/)
        #[arg(long)]
        output: Option<PathBuf>,
        /// Imprime todos os YAMLs no stdout em vez de gravar arquivos
        #[arg(long)]
        stdout: bool,
        /// Rastreia eventos até este limite em minutos (0 = sem limite, padrão: 5)
        #[arg(long, default_value_t = 5)]
        max_time: u32,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Rename { dir } => commands::cmd_rename(dir),
        Commands::Dump { path, output, stdout, max_time } => commands::cmd_dump(path, output, stdout, max_time),
    }
}
