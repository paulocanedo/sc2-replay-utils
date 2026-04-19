//! Tipos de dados do catálogo de replays.

use std::path::PathBuf;
use std::time::SystemTime;

/// Metadados mínimos exibidos na biblioteca.
#[derive(Clone)]
pub struct ParsedMeta {
    pub map: String,
    pub datetime: String,
    pub duration_seconds: u32,
    pub game_loops: u32,
    pub players: Vec<PlayerMeta>,
}

#[derive(Clone)]
pub struct PlayerMeta {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub result: String,
    /// Rótulo estratégico de abertura (ex: "Hatch First — Ling/Queen",
    /// "1 Rax FE — Stim Timing", "Gate Expand — Stalker/Sentry").
    /// String já formatada para display — a lógica de classificação
    /// vive em `crate::build_order::classify_opening` e roda uma única
    /// vez no scanner, com o resultado persistido no cache bincode.
    /// `None` quando o replay não pôde ser parseado para extrair o
    /// build order (raro — só em replays curtíssimos ou corrompidos).
    pub opening: Option<String>,
}

#[derive(Clone)]
pub enum MetaState {
    Pending,
    Parsed(ParsedMeta),
    /// Replay válido, porém com número de jogadores ≠ 2. O app só
    /// suporta 1v1, então esses entries ficam visíveis mas não
    /// clicáveis. A string contém uma descrição curta (e.g.
    /// "não é 1v1 (4 jogadores)") para exibir na UI.
    Unsupported(String),
    Failed(String),
}

impl MetaState {
    pub(super) fn is_loadable(&self) -> bool {
        matches!(self, MetaState::Parsed(_))
    }
}

pub struct LibraryEntry {
    pub path: PathBuf,
    pub filename: String,
    pub mtime: Option<SystemTime>,
    pub meta: MetaState,
}
