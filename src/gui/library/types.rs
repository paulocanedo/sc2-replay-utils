//! Tipos de dados do catálogo de replays.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::replay::ReplayTimeline;

/// Metadados mínimos exibidos na biblioteca.
#[derive(Clone)]
pub struct ParsedMeta {
    pub map: String,
    pub datetime: String,
    pub duration_seconds: u32,
    pub game_loops: u32,
    pub players: Vec<PlayerMeta>,
}

impl ParsedMeta {
    /// Deriva `ParsedMeta` de um `ReplayTimeline` já parseado, sem abrir
    /// o `.SC2Replay` de novo. Mesma transformação que
    /// `scanner::parse_meta` faz — sourced do stream canônico em vez de
    /// re-parsear. Retorna `None` se não for 1v1.
    pub fn from_timeline(timeline: &ReplayTimeline) -> Option<Self> {
        if timeline.players.len() != 2 {
            return None;
        }
        Some(Self {
            map: timeline.map.clone(),
            datetime: timeline.datetime.clone(),
            duration_seconds: timeline.duration_seconds,
            game_loops: timeline.game_loops,
            players: timeline
                .players
                .iter()
                .map(|p| PlayerMeta {
                    name: p.name.clone(),
                    race: p.race.clone(),
                    mmr: p.mmr,
                    result: p.result.clone().unwrap_or_default(),
                    opening: None,
                })
                .collect(),
        })
    }
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
    /// vive em `crate::build_order::classify_opening` e roda em background
    /// no pool de enriquecimento do scanner, com o resultado persistido
    /// no cache bincode.
    ///
    /// `None` significa "ainda não calculado" (transiente) ou "não foi
    /// possível extrair o build order" (raro). A UI renderiza `None`
    /// como "—" via `library.opening.unknown`; o pool de enriquecimento
    /// acaba preenchendo em seguida e o cache passa a servir o rótulo.
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
