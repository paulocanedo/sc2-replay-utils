//! Classificação de abertura (opening label) para o build order.
//!
//! Produz uma string curta e legível no vocabulário da comunidade SC2
//! (`"Hatch First — Ling/Queen"`, `"3 Rax Reaper Expand"`,
//! `"Gate Expand — Stalker/Sentry"`) a partir do `PlayerBuildOrder` já
//! extraído pelo módulo irmão `extract.rs`.
//!
//! # Princípios de design
//!
//! - **Fonte única de verdade**: varre apenas `player.entries` — o
//!   mesmo stream canônico que alimenta a GUI, o CSV golden e os
//!   charts. Nada de re-decodificar tracker events.
//! - **Janela temporal curta**: `T_FOLLOW_UP_END = 5 min` de game
//!   time. Compatível com o `max_time_seconds = 300` que a biblioteca
//!   passa ao parser (scanner.rs) — assim a classificação roda sem
//!   parsear o replay inteiro.
//! - **Fallback honesto**: quando nenhuma heurística casa, devolvemos
//!   uma *assinatura de supply* feita dos dados reais
//!   (`"13 Pool, 15 Hatch"`) em vez de inventar um rótulo errado.
//! - **Nomenclatura em inglês** tanto em en quanto em pt-BR.
//!   Jogadores brasileiros também falam "Hatch First", "3 Rax",
//!   "Gate Expand". Só o fallback genérico é traduzido.
//!
//! # Organização
//!
//! - `types`     — `Confidence`, `OpeningLabel` (API pública).
//! - `facts`     — `WindowFacts` + coleta numa passada sobre `entries`.
//! - `zerg`/`terran`/`protoss` — classificadores por raça.
//! - `signature` — fallback baseado nos primeiros marcos de supply.

mod facts;
mod protoss;
mod signature;
mod terran;
mod types;
mod zerg;

#[cfg(test)]
mod tests;

pub use types::{Confidence, OpeningLabel};

use super::types::PlayerBuildOrder;
use facts::collect_window_facts;

/// Ponto de corte que separa **abertura** (escolha de pool/rax/gateway
/// vs. expansão vs. gás) de **follow-up** (primeiras unidades, upgrades
/// e tech). 3 min game time (normal speed: 3×60×22.4 ≈ 4032 loops).
/// Computado em loops a partir do `lps` real do replay.
const T_OPENING_END_SECS: u32 = 180;

/// Ponto de corte do follow-up. 5 min de game time — pega Stim
/// timing, primeira leva de unidades, primeiro upgrade de speed, etc.
const T_FOLLOW_UP_END_SECS: u32 = 300;

/// Se o replay sequer tem algum evento de build order nos primeiros
/// `T_OPENING_END_SECS`, devolvemos `Insufficient` com a string genérica
/// configurável pelo caller via i18n. Internamente usamos "Too short"
/// como placeholder — o caller que integra o label à UI (scanner.rs)
/// decide se traduz para `"Muito curto"`.
const INSUFFICIENT_PLACEHOLDER: &str = "Too short";

/// Classifica a abertura de um jogador. Não toca disk/network — só
/// lê `player.entries`, já ordenado cronologicamente por start_loop.
///
/// `lps` é o `loops_per_second` do replay (normal 22.4 no LotV, outros
/// valores em replays legados). Usado para converter os cortes de
/// tempo em loops.
pub fn classify_opening(player: &PlayerBuildOrder, lps: f64) -> OpeningLabel {
    let open_end = (T_OPENING_END_SECS as f64 * lps).round() as u32;
    let follow_end = (T_FOLLOW_UP_END_SECS as f64 * lps).round() as u32;

    let facts = collect_window_facts(&player.entries, &player.race, open_end, follow_end);

    // Sem qualquer evento relevante até o fim da janela da abertura?
    // O replay provavelmente parou cedo (GG em 30s, replay corrompido).
    if !facts.has_any_before_opening_end {
        return OpeningLabel {
            opening: INSUFFICIENT_PLACEHOLDER.to_string(),
            follow_up: None,
            confidence: Confidence::Insufficient,
        };
    }

    let named = match player.race.as_str() {
        "Zerg" => zerg::classify(&facts),
        "Terran" => terran::classify(&facts),
        "Protoss" => protoss::classify(&facts),
        _ => None,
    };

    named.unwrap_or_else(|| signature::fallback(&facts))
}
