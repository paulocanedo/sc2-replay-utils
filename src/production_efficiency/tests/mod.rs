mod army;
mod warpgate;
mod workers;
mod zerg;

use super::*;
use crate::replay::{
    EntityCategory, EntityEvent, EntityEventKind, PlayerTimeline, ReplayTimeline, UpgradeEntry,
};

/// Tolerância padrão para comparações de pct (erros de ponto
/// flutuante no cálculo da média ponderada).
pub(super) const EPS: f64 = 1e-6;

pub(super) fn mk_timeline(players: Vec<PlayerTimeline>, game_loops: u32) -> ReplayTimeline {
    ReplayTimeline {
        file: String::new(),
        map: String::new(),
        datetime: String::new(),
        game_loops,
        duration_seconds: game_loops / 16,
        loops_per_second: 22.4,
        base_build: 0,
        version: String::new(),
        max_time_seconds: 0,
        players,
        chat: Vec::new(),
        cache_handles: Vec::new(),
        map_size_x: 0,
        map_size_y: 0,
        resources: Vec::new(),
    }
}

pub(super) fn mk_player(race: &str) -> PlayerTimeline {
    PlayerTimeline {
        name: "P".to_string(),
        clan: String::new(),
        race: race.to_string(),
        mmr: None,
        player_id: 1,
        result: None,
        toon: None,
        stats: Vec::new(),
        upgrades: Vec::new(),
        entity_events: Vec::new(),
        production_cmds: Vec::new(),
        inject_cmds: Vec::new(),
        unit_positions: Vec::new(),
        camera_positions: Vec::new(),
        alive_count: std::collections::HashMap::new(),
        worker_capacity: Vec::new(),
        worker_births: Vec::new(),
        army_capacity: Vec::new(),
        army_productions: Vec::new(),
        worker_capacity_cumulative: Vec::new(),
        army_capacity_cumulative: Vec::new(),
        upgrade_cumulative: Vec::new(),
        creep_index: Vec::new(),
    }
}

/// Localiza a amostra cujo `game_loop` (fim do bucket) bate
/// exatamente com `t`. Retorna `None` se o bucket não existe —
/// usado nos testes para asserts em boundaries conhecidos.
pub(super) fn bucket_ending_at(samples: &[EfficiencySample], t: u32) -> EfficiencySample {
    *samples
        .iter()
        .find(|s| s.game_loop == t)
        .unwrap_or_else(|| panic!("no bucket ending at game_loop={t}"))
}
