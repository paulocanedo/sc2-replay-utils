// Estado derivado de um replay carregado.
//
// Ao abrir um .SC2Replay, a UI chama LoadedReplay::load que invoca
// `parse_replay` UMA vez e em seguida roda todos os extractors puros
// sobre o `ReplayTimeline` resultante. Os resultados ficam em cache
// no struct e as abas leem sem recomputar.

use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use crate::army_value::{self, ArmyValueResult};
use crate::build_order::{self, BuildOrderResult};
use crate::chat::{self, ChatResult};
use crate::map_image::MapImage;
#[cfg(not(target_arch = "wasm32"))]
use crate::map_image;
use crate::production_gap::{self, ProductionGapResult};
use crate::replay::{self, EntityEventKind, ReplayTimeline};
use crate::supply_block::{self, SupplyBlockEntry};

/// Bounds (em células de tile) do retângulo onde os eventos do replay
/// posicionam unidades. Derivado das posições observadas em todos os
/// `entity_events` — é uma aproximação da playable area, já que o
/// `init_data` reporta `map_size_x/y` da grade completa (que inclui
/// uma margem unplayable bem maior do que o jogo mostra).
///
/// Usado pela aba Timeline pra mapear coordenadas de unidade pra
/// coordenadas de tela alinhadas com o `Minimap.tga` (que também
/// representa só a playable area).
#[derive(Clone, Copy, Debug)]
pub struct PlayableBounds {
    pub min_x: u8,
    pub max_x: u8,
    pub min_y: u8,
    pub max_y: u8,
}

pub struct LoadedReplay {
    pub path: PathBuf,
    pub timeline: ReplayTimeline,
    pub build_order: Option<BuildOrderResult>,
    pub chat: Option<ChatResult>,
    pub army: Option<ArmyValueResult>,
    pub production: Option<ProductionGapResult>,
    /// Supply blocks por jogador, mesmo índice que `timeline.players`.
    pub supply_blocks_per_player: Vec<Vec<SupplyBlockEntry>>,
    /// Imagem rasterizada do mapa do replay (Minimap.tga embutido no
    /// `.SC2Map`/`.s2ma`). `None` quando o arquivo do mapa não foi
    /// encontrado em nenhum dos diretórios padrão ou quando a extração
    /// falhou — não é fatal, a aba Timeline cai pro fundo cinza.
    pub map_image: Option<MapImage>,
    /// Bounds da playable area derivados dos eventos do replay. `None`
    /// se nenhum evento posicionou alguma entidade (replay vazio).
    pub playable_bounds: Option<PlayableBounds>,
}

impl LoadedReplay {
    /// Native-only path-based loader. Reads bytes from disk, delegates the
    /// parsing/extraction work to `from_bytes`, then attempts the minimap
    /// lookup (which requires the local Battle.net Cache and is therefore
    /// native-only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: &Path, max_time: u32) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut me = Self::from_bytes(file_name, &bytes, max_time)?;
        me.path = path.to_path_buf();
        me.map_image = match map_image::load_for_replay(&me.timeline.map, &me.timeline.cache_handles)
        {
            Ok(img) => Some(img),
            Err(e) => {
                eprintln!("map_image: {e}");
                None
            }
        };
        Ok(me)
    }

    /// Bytes-in-memory loader. Used by the web build (FileReader upload)
    /// and shared as the implementation core of `load`. Does NOT attempt
    /// to resolve the minimap — that requires the local Battle.net Cache
    /// and is the caller's responsibility on native.
    pub fn from_bytes(file_name: String, bytes: &[u8], max_time: u32) -> Result<Self, String> {
        let timeline = replay::parse_replay_from_bytes(&file_name, bytes, max_time)?;

        // O app só suporta 1v1. Rejeitamos aqui também (além do filtro
        // da biblioteca) para cobrir carregamentos diretos via diálogo
        // de arquivo ou via file watcher.
        if timeline.players.len() != 2 {
            return Err(format!(
                "replay não suportado: só 1v1 é suportado ({} jogadores)",
                timeline.players.len()
            ));
        }

        let build_order = match build_order::extract_build_order(&timeline) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("build_order: {e}");
                None
            }
        };
        let chat = match chat::extract_chat(&timeline) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("chat: {e}");
                None
            }
        };
        let army = match army_value::extract_army_value(&timeline) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("army_value: {e}");
                None
            }
        };
        let production = match production_gap::extract_production_gaps(&timeline) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("production_gap: {e}");
                None
            }
        };
        let supply_blocks_per_player = timeline
            .players
            .iter()
            .map(|p| supply_block::extract_supply_blocks(p, timeline.game_loops, timeline.base_build))
            .collect();

        let playable_bounds = compute_playable_bounds(&timeline);

        Ok(Self {
            path: PathBuf::from(&file_name),
            timeline,
            build_order,
            chat,
            army,
            production,
            supply_blocks_per_player,
            map_image: None,
            playable_bounds,
        })
    }

    /// Índice do jogador que bate com algum nickname do usuário (case-insensitive).
    pub fn user_player_index(&self, nicknames: &[String]) -> Option<usize> {
        self.timeline.players.iter().position(|p| {
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

/// Calcula os bounds da playable area a partir das posições observadas
/// nos `entity_events` de todos os jogadores. Adiciona uma pequena
/// margem (`MARGIN`) em cada lado pra deixar respiro visual quando as
/// unidades estão exatamente no canto da área jogável.
///
/// `None` quando o replay não tem nenhum evento posicionado (replay
/// vazio ou parser sem rastreamento).
fn compute_playable_bounds(timeline: &ReplayTimeline) -> Option<PlayableBounds> {
    const MARGIN: u8 = 4;
    let mut min_x = u8::MAX;
    let mut max_x = 0u8;
    let mut min_y = u8::MAX;
    let mut max_y = 0u8;
    let mut any = false;
    for p in &timeline.players {
        for ev in &p.entity_events {
            if !matches!(
                ev.kind,
                EntityEventKind::ProductionFinished | EntityEventKind::Died
            ) {
                continue;
            }
            min_x = min_x.min(ev.pos_x);
            max_x = max_x.max(ev.pos_x);
            min_y = min_y.min(ev.pos_y);
            max_y = max_y.max(ev.pos_y);
            any = true;
        }
        // Inclui também as amostras de movimento — sem isso, unidades
        // que se afastam da nuvem de spawns podem ser clipadas no
        // mini-mapa.
        for s in &p.unit_positions {
            min_x = min_x.min(s.x);
            max_x = max_x.max(s.x);
            min_y = min_y.min(s.y);
            max_y = max_y.max(s.y);
            any = true;
        }
    }
    // Inclui os recursos (mineral fields + geysers). Normalmente já
    // estão dentro dos bounds das unidades porque cada jogador tem
    // uma base próxima, mas mapas com expansões remotas sem visitas
    // antecipadas podem ter patches fora dessa nuvem.
    for r in &timeline.resources {
        min_x = min_x.min(r.x);
        max_x = max_x.max(r.x);
        min_y = min_y.min(r.y);
        max_y = max_y.max(r.y);
        any = true;
    }
    if !any || max_x <= min_x || max_y <= min_y {
        return None;
    }
    Some(PlayableBounds {
        min_x: min_x.saturating_sub(MARGIN),
        max_x: max_x.saturating_add(MARGIN),
        min_y: min_y.saturating_sub(MARGIN),
        max_y: max_y.saturating_add(MARGIN),
    })
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

/// Matchup code ("PvT", "ZvP" …) from the replay's player slots.
/// Returns an em dash when the player count isn't the usual two.
pub fn build_matchup(players: &[replay::PlayerTimeline]) -> String {
    if players.len() >= 2 {
        format!(
            "{}v{}",
            crate::utils::race_letter(&players[0].race),
            crate::utils::race_letter(&players[1].race),
        )
    } else {
        String::from("—")
    }
}

/// Formats "2026-04-10T17:46:40" → e.g. "10 apr 2026" / "10 abr 2026"
/// depending on the active UI language.
pub fn format_date_short(datetime: &str, lang: crate::locale::Language) -> String {
    let date_part = datetime.split('T').next().unwrap_or(datetime);
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() == 3 {
        let key = format!("month.{}", parts[1]);
        let month = crate::locale::t(&key, lang);
        let day = parts[2].trim_start_matches('0');
        format!("{day} {month} {}", parts[0])
    } else {
        date_part.to_string()
    }
}
