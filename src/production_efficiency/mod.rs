// Série temporal de eficiência de produção (workers ou army) —
// consumer puro do `ReplayTimeline`. Reusa o padrão de merge de
// eventos de `production_gap.rs`, mas ao invés de um escalar + lista
// de gaps, emite uma amostra por bucket de `CHART_BUCKET_SECONDS`
// com a média ponderada pelo tempo do estado dentro do bucket. Isso
// suaviza o gráfico e evita o "serrote" que surge quando produção e
// idle alternam a cada poucos game_loops.
//
// Organização:
//   - `types`  — constantes, structs/enums públicos, predicados auxiliares.
//   - `models` — lógica race/target-specific (workers, army, zerg).
//   - `sweep`  — algoritmo de sweep (média ponderada por bucket).

mod models;
mod sweep;
mod types;

#[cfg(test)]
mod tests;

pub use types::{EfficiencySample, EfficiencyTarget, PlayerEfficiencySeries, ProductionEfficiencySeries};
pub(crate) use types::WARP_GATE_RESEARCH;
#[cfg(test)]
pub(crate) use types::{is_warp_gate_unit, WARP_GATE_CYCLE_LOOPS};

use crate::replay::ReplayTimeline;
use models::*;
use types::*;

// ── API pública ──────────────────────────────────────────────────────────────

pub fn extract_efficiency_series(
    timeline: &ReplayTimeline,
    target: EfficiencyTarget,
) -> Result<ProductionEfficiencySeries, String> {
    let game_loops = timeline.game_loops;
    let max_loops = if timeline.max_time_seconds == 0 {
        0
    } else {
        (timeline.max_time_seconds as f64 * timeline.loops_per_second).round() as u32
    };
    let effective_end = if max_loops == 0 {
        game_loops
    } else {
        game_loops.min(max_loops)
    };

    // Largura do bucket em game loops. Clampa em ≥ 1 para evitar
    // divisão por zero quando `loops_per_second` vem zerado (replays
    // patológicos).
    let bucket_loops = if timeline.loops_per_second > 0.0 {
        ((CHART_BUCKET_SECONDS * timeline.loops_per_second).round() as u32).max(1)
    } else {
        1
    };

    let players = timeline
        .players
        .iter()
        .map(|player| {
            let is_zerg = is_zerg_race(&player.race);
            let samples = match (is_zerg, target) {
                (false, EfficiencyTarget::Workers) => {
                    compute_series_workers(player, effective_end, bucket_loops)
                }
                (false, EfficiencyTarget::Army) => compute_series_army(
                    player,
                    effective_end,
                    timeline.base_build,
                    bucket_loops,
                ),
                (true, EfficiencyTarget::Workers) => compute_series_zerg(
                    player,
                    effective_end,
                    bucket_loops,
                    timeline.base_build,
                    is_drone,
                    false, // workers: sem override de supply maxed
                ),
                (true, EfficiencyTarget::Army) => compute_series_zerg(
                    player,
                    effective_end,
                    bucket_loops,
                    timeline.base_build,
                    crate::replay::is_larva_born_army,
                    true, // army: override de supply maxed (convenção)
                ),
            };

            PlayerEfficiencySeries {
                name: player.name.clone(),
                race: player.race.clone(),
                is_zerg,
                samples,
            }
        })
        .collect();

    Ok(ProductionEfficiencySeries {
        players,
        target,
        loops_per_second: timeline.loops_per_second,
        game_loops,
    })
}
