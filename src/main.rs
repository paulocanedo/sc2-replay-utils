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
        /// Diretório com os replays (padrão: sc2replays-pack)
        dir: Option<PathBuf>,
        /// Diretório de saída para os YAMLs (padrão: <dir>/out/)
        #[arg(long)]
        output: Option<PathBuf>,
        /// Imprime todos os YAMLs no stdout em vez de gravar arquivos
        #[arg(long)]
        stdout: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Rename { dir } => commands::cmd_rename(dir),
        Commands::Dump { dir, output, stdout } => commands::cmd_dump(dir, output, stdout),
    }
}
