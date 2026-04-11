// Pós-processamento do parser: ordena timelines fora de ordem e
// constrói o índice cumulativo `alive_count` por tipo de entidade.

use std::collections::HashMap;

use super::types::{EntityEventKind, PlayerTimeline};

pub(super) fn finalize_indices(players: &mut [PlayerTimeline]) {
    for player in players.iter_mut() {
        // Eventos podem ter sido emitidos fora de ordem por morphs
        // (apply_type_change empilha múltiplos no mesmo loop). A
        // ordenação é estável, então a ordem relativa entre eventos
        // do mesmo loop é preservada.
        player.entity_events.sort_by_key(|e| e.game_loop);
        player.worker_capacity.sort_by_key(|(l, _)| *l);
        player.worker_births.sort_unstable();
        // `unit_positions` chega na ordem natural do tracker, então
        // já está ordenado — mas um sort estável defensivo garante
        // o invariante esperado pelos consumers (`last_known_positions`).
        player.unit_positions.sort_by_key(|s| s.game_loop);

        // alive_count: ProductionFinished ++; Died --; ignora
        // Started/Cancelled.
        let mut counts: HashMap<String, i32> = HashMap::new();
        for ev in &player.entity_events {
            match ev.kind {
                EntityEventKind::ProductionFinished => {
                    let c = counts.entry(ev.entity_type.clone()).or_insert(0);
                    *c += 1;
                    let v = player
                        .alive_count
                        .entry(ev.entity_type.clone())
                        .or_default();
                    v.push((ev.game_loop, *c));
                }
                EntityEventKind::Died => {
                    let c = counts.entry(ev.entity_type.clone()).or_insert(0);
                    *c -= 1;
                    let v = player
                        .alive_count
                        .entry(ev.entity_type.clone())
                        .or_default();
                    v.push((ev.game_loop, *c));
                }
                _ => {}
            }
        }
    }
}
