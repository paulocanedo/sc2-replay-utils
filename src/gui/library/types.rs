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
    /// Versão do jogo (formato `5.0.13.92440`) — exibida no card lateral
    /// de detalhes. `None` para entries cacheadas em versões antigas do
    /// app (antes do bump de cache que adicionou o campo).
    pub version: Option<String>,
    /// `m_cacheHandles` do replay — usado para resolver o arquivo do
    /// mapa no Battle.net Cache e renderizar o minimapa no card de
    /// detalhes sem reparsear o replay inteiro. Vazio quando o cache
    /// veio de versões antigas do app.
    pub cache_handles: Vec<String>,
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
            version: Some(timeline.version.clone()),
            cache_handles: timeline.cache_handles.clone(),
            players: timeline
                .players
                .iter()
                .map(|p| PlayerMeta {
                    name: p.name.clone(),
                    race: p.race.clone(),
                    mmr: p.mmr,
                    result: p.result.clone().unwrap_or_default(),
                    opening: OpeningLabel::Pending,
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
    /// "1 Rax FE — Stim Timing", "Gate Expand — Stalker/Sentry"). A
    /// lógica de classificação vive em
    /// `crate::build_order::classify_opening` e roda em background no
    /// pool de enriquecimento do scanner; o resultado é persistido no
    /// cache bincode.
    pub opening: OpeningLabel,
}

/// Estado da classificação de abertura para um jogador.
///
/// O ciclo de vida vai de `Pending` (logo após o `parse_meta` rápido) →
/// `Classified` (sucesso, com a string de display) ou `Unclassifiable`
/// (tentou e o build order não rendeu rótulo — replay curto demais,
/// extração falhou, etc.).
///
/// A distinção entre `Pending` e `Unclassifiable` é load-bearing: sem
/// ela qualquer falha de classificação faz o pool de enriquecimento
/// re-tentar o mesmo replay a cada launch do app, gastando minutos por
/// arquivo para chegar no mesmo `None`. Com a sentinela, replays
/// definitivamente não-classificáveis ficam quietos.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OpeningLabel {
    /// Ainda não tentado — pool de enriquecimento ainda não rodou ou o
    /// resultado não chegou. UI mostra "—" e o scanner pode enfileirar
    /// para enriquecimento.
    Pending,
    /// Classificado com sucesso. String é o display final.
    Classified(String),
    /// Tentado, mas não foi possível extrair um rótulo. Não enfileirar
    /// de novo nesta ou em sessões futuras. UI mostra "—".
    Unclassifiable,
}

impl OpeningLabel {
    /// Devolve o rótulo de display quando `Classified`, ou `None` para
    /// os outros estados (a UI mostra "—" nesse caso).
    pub fn as_classified(&self) -> Option<&str> {
        match self {
            OpeningLabel::Classified(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, OpeningLabel::Pending)
    }
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
