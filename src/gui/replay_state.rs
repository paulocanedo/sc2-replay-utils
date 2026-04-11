// Estado derivado de um replay carregado.
//
// Ao abrir um .SC2Replay, a UI chama LoadedReplay::load que invoca
// parse_replay + todas as funções extract_* de uma vez. Os resultados
// ficam em cache no struct e as abas leem sem recomputar.

use std::path::{Path, PathBuf};

use crate::army_value::{self, ArmyValueResult};
use crate::build_order::{self, BuildOrderResult};
use crate::chat::{self, ChatResult};
use crate::production_gap::{self, ProductionGapResult};
use crate::replay::{self, ReplayData};
use crate::supply_block::{self, SupplyBlockEntry};

pub struct LoadedReplay {
    pub path: PathBuf,
    pub raw: ReplayData,
    pub build_order: Option<BuildOrderResult>,
    pub chat: Option<ChatResult>,
    pub army: Option<ArmyValueResult>,
    pub production: Option<ProductionGapResult>,
    /// Supply blocks por jogador, mesmo índice que `raw.players`.
    pub supply_blocks_per_player: Vec<Vec<SupplyBlockEntry>>,
}

impl LoadedReplay {
    pub fn load(path: &Path, max_time: u32) -> Result<Self, String> {
        let raw = replay::parse_replay(path, max_time, false)?;

        // O app só suporta 1v1. Rejeitamos aqui também (além do filtro
        // da biblioteca) para cobrir carregamentos diretos via diálogo
        // de arquivo ou via file watcher.
        if raw.players.len() != 2 {
            return Err(format!(
                "replay não suportado: só 1v1 é suportado ({} jogadores)",
                raw.players.len()
            ));
        }

        let build_order = match build_order::extract_build_order(path, max_time) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("build_order: {e}");
                None
            }
        };
        let chat = match chat::extract_chat(path, max_time) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("chat: {e}");
                None
            }
        };
        let army = match army_value::extract_army_value(path, max_time) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("army_value: {e}");
                None
            }
        };
        let production = match production_gap::extract_production_gaps(path, max_time) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("production_gap: {e}");
                None
            }
        };
        let supply_blocks_per_player = raw
            .players
            .iter()
            .map(|p| supply_block::extract_supply_blocks(&p.stats_snapshots, raw.game_loops))
            .collect();

        Ok(Self {
            path: path.to_path_buf(),
            raw,
            build_order,
            chat,
            army,
            production,
            supply_blocks_per_player,
        })
    }

    /// Índice do jogador que bate com algum nickname do usuário (case-insensitive).
    pub fn user_player_index(&self, nicknames: &[String]) -> Option<usize> {
        self.raw.players.iter().position(|p| {
            nicknames
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&p.name))
        })
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Formata um game_loop como "mm:ss" dado loops_per_second.
pub fn fmt_time(game_loop: u32, lps: f64) -> String {
    let secs = if lps > 0.0 {
        (game_loop as f64 / lps) as u32
    } else {
        0
    };
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// Game loop → segundos (f64) para plots.
pub fn loop_to_secs(game_loop: u32, lps: f64) -> f64 {
    if lps > 0.0 {
        game_loop as f64 / lps
    } else {
        0.0
    }
}
