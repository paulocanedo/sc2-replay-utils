// Detecção de supply blocks — camada pura sobre `PlayerTimeline`.
//
// Consome `stats`, `entity_events`, `upgrades` e `production_cmds` do
// jogador (já produzidos pelo parser single-pass) e emite uma lista
// de `SupplyBlockEntry` cobrindo cada intervalo em que o jogador
// ficou supply-capped.
//
// Organização:
//   - `types`    — `SupplyBlockEntry`, `StartStrategy`, `ACTIVE_STRATEGY`.
//   - `events`   — `Event` enum + `build_events()` (timeline ordenada).
//   - `detector` — `BlockDetector` (state machine).

mod detector;
mod events;
mod types;

#[cfg(test)]
mod tests;

pub use types::SupplyBlockEntry;
// `StartStrategy` faz parte da API pública do módulo mas hoje só é
// consumida internamente pelo `ACTIVE_STRATEGY`. Mantemos re-exportada
// para permitir seleção externa futura sem quebrar a convenção de
// silenciar o warning de unused.
#[allow(unused_imports)]
pub use types::StartStrategy;

use crate::production_efficiency::WARP_GATE_RESEARCH;
use crate::replay::PlayerTimeline;

use detector::BlockDetector;
use events::build_events;

/// Detecta períodos de supply block nos stats de um jogador.
///
/// O início do bloco depende de `ACTIVE_STRATEGY`. Para Protoss com
/// `WarpGateResearch` concluído, há um gatilho adicional: quando
/// existe pelo menos uma warpgate fora de cooldown e o supply
/// disponível é menor que o custo do Zealot (unidade warpável mais
/// barata). Isso captura situações onde o jogador nem tenta warpar
/// por estar supply-capped — caso que o gatilho `ProductionAttempt`
/// não enxergaria.
///
/// O fim acontece quando uma estrutura/unidade que fornece supply é
/// concluída (`SupplyDepot`, `Pylon`, `Overlord`, etc.), quando o
/// `SupplyDrop` do Orbital é usado, ou quando uma unidade morre
/// liberando supply.
pub fn extract_supply_blocks(
    player: &PlayerTimeline,
    game_loops: u32,
    base_build: u32,
) -> Vec<SupplyBlockEntry> {
    if player.stats.is_empty() {
        return Vec::new();
    }

    let warp_research_done = player
        .upgrades
        .iter()
        .any(|u| u.name == WARP_GATE_RESEARCH);

    let events = build_events(player, base_build);

    let mut detector = BlockDetector::new(warp_research_done);
    for (loop_, event) in &events {
        detector.process(*loop_, event);
    }
    detector.finish(game_loops)
}
