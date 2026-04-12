// Parser single-pass do replay.
//
// Abre o MPQ uma única vez, lê tracker events e message events em
// um único loop cada e produz um `ReplayTimeline` indexado por tempo
// que serve como fonte única de verdade para todos os extractors
// (build_order, army_value, supply_block, production_gap, chat) e
// para a GUI.
//
// O parser **traduz** os eventos crus do replay (UnitInit/UnitBorn/
// UnitDone/UnitDied/UnitTypeChange) para um vocabulário semântico do
// app — `EntityEvent { kind: ProductionStarted | ProductionFinished
// | ProductionCancelled | Died, … }`. Os consumers nunca tocam no
// formato bruto.
//
// Layout do módulo:
// - `types`     — structs/enums expostos publicamente
// - `query`     — API de scrubbing (`stats_at`, `alive_count_at`, …)
// - `classify`  — heurísticas worker/structure/upgrade (privado)
// - `tracker`   — tradução dos tracker events (privado)
// - `message`   — chat (privado)
// - `finalize`  — pós-processamento dos índices (privado)

mod classify;
mod finalize;
mod game;
mod message;
mod query;
mod tracker;
mod types;

pub use types::{
    ChatEntry, EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline, UNIT_INIT_MARKER,
};
// Re-exportados para que `EntityEvent`/`StatsSnapshot`/`UpgradeEntry`/
// `UnitPositionSample`, que aparecem como campos públicos das structs
// acima, sejam alcançáveis via `crate::replay::*` quando consumers
// precisarem nomeá-los explicitamente.
#[allow(unused_imports)]
pub use types::{
    EntityEvent, ProductionCmd, StatsSnapshot, UnitPositionSample, UpgradeEntry,
};

use std::collections::HashMap;
use std::path::Path;

use crate::utils::{extract_clan_and_name, game_speed_to_loops_per_second};

/// Faz o parsing single-pass do replay e devolve um `ReplayTimeline`.
///
/// `max_time_seconds == 0` significa sem limite. `max_time_seconds == 1`
/// é um fast-path usado pela biblioteca da GUI: o parser retorna logo
/// após carregar metadados, sem decodificar tracker/message events.
pub fn parse_replay(path: &Path, max_time_seconds: u32) -> Result<ReplayTimeline, String> {
    let path_str = path.to_str().unwrap_or_default();

    let (mpq, file_contents) =
        s2protocol::read_mpq(path_str).map_err(|e| format!("{:?}", e))?;
    let (_, header) =
        s2protocol::read_protocol_header(&mpq).map_err(|e| format!("{:?}", e))?;
    let details =
        s2protocol::read_details(path_str, &mpq, &file_contents).map_err(|e| format!("{:?}", e))?;
    let init_data = s2protocol::read_init_data(path_str, &mpq, &file_contents).ok();

    let active_count = details.player_list.iter().filter(|p| p.observe == 0).count();
    if active_count < 2 {
        return Err("menos de 2 jogadores".to_string());
    }

    let datetime = s2protocol::transform_to_naivetime(details.time_utc, details.time_local_offset)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| "0000-00-00T00:00:00".to_string());

    let game_loops = header.m_elapsed_game_loops as u32;
    let base_build = header.m_version.m_base_build;
    let loops_per_second = game_speed_to_loops_per_second(&details.game_speed);

    // player_id (1-indexado no player_list completo) → índice no
    // vec de jogadores ativos.
    let player_idx: HashMap<u8, usize> = details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .enumerate()
        .map(|(out_idx, (in_idx, _))| ((in_idx + 1) as u8, out_idx))
        .collect();

    let players: Vec<PlayerTimeline> = details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .map(|(in_idx, p)| {
            let (clan, name) = extract_clan_and_name(&p.name);
            let mmr = init_data
                .as_ref()
                .and_then(|id| find_mmr_for_slot(id, p.working_set_slot_id));
            PlayerTimeline {
                name,
                clan,
                race: p.race.clone(),
                mmr,
                // `player_id` 1-baseado, casando com `player_idx` acima
                // e com o `killer_player_id` dos tracker events.
                player_id: (in_idx + 1) as u8,
                stats: Vec::new(),
                upgrades: Vec::new(),
                entity_events: Vec::new(),
                production_cmds: Vec::new(),
                unit_positions: Vec::new(),
                alive_count: HashMap::new(),
                worker_capacity: Vec::new(),
                worker_births: Vec::new(),
                upgrade_cumulative: Vec::new(),
            }
        })
        .collect();

    // user_id (0-indexado em player_list completo) → display name
    // para correlacionar message events.
    let user_names: HashMap<i64, String> = details
        .player_list
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (_, name) = extract_clan_and_name(&p.name);
            (i as i64, name)
        })
        .collect();

    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let map = details.title.clone();
    let duration_seconds = (game_loops as f64 / loops_per_second).round() as u32;

    let cache_handles = details.cache_handles.clone();

    // Dimensões do mapa vêm do init_data; sem ele caem pra zero e a
    // aba Timeline cai num fallback de aspect 1:1 (raro — `init_data`
    // só falta em replays muito antigos ou corrompidos).
    let (map_size_x, map_size_y) = init_data
        .as_ref()
        .map(|id| {
            let gd = &id.sync_lobby_state.game_description;
            (gd.map_size_x, gd.map_size_y)
        })
        .unwrap_or((0, 0));

    let mut timeline = ReplayTimeline {
        file,
        map,
        datetime,
        game_loops,
        duration_seconds,
        loops_per_second,
        base_build,
        max_time_seconds,
        players,
        chat: Vec::new(),
        cache_handles,
        map_size_x,
        map_size_y,
    };

    // Fast path para metadata-only (usado pela biblioteca da GUI):
    // não decodificamos tracker/message events.
    if max_time_seconds == 1 {
        return Ok(timeline);
    }

    let max_loops = if max_time_seconds == 0 {
        0
    } else {
        (max_time_seconds as f64 * loops_per_second).round() as u32
    };

    let mut index_owner: tracker::IndexOwnerMap = HashMap::new();
    tracker::process_tracker_events(
        path_str,
        &mpq,
        &file_contents,
        &player_idx,
        &mut timeline.players,
        &mut index_owner,
        max_loops,
    )?;

    // user_id (0-baseado em player_list) → player_idx (índice em
    // `timeline.players`). Necessário pra `game::process_game_events`
    // saber em qual player empurrar o `ProductionCmd`.
    let user_to_player_idx: HashMap<i64, usize> = details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .enumerate()
        .map(|(out_idx, (in_idx, _))| (in_idx as i64, out_idx))
        .collect();

    game::process_game_events(
        path_str,
        &mpq,
        &file_contents,
        &user_to_player_idx,
        &index_owner,
        base_build,
        &mut timeline.players,
        max_loops,
    )?;

    message::process_message_events(
        path_str,
        &mpq,
        &file_contents,
        &user_names,
        max_loops,
        &mut timeline.chat,
    )?;

    finalize::finalize_indices(&mut timeline.players);

    Ok(timeline)
}

// ── MMR lookup ──────────────────────────────────────────────────────

/// Encontra o `scaled_rating` de um jogador no InitData usando
/// `working_set_slot_id`. O índice em `user_initial_data` é a posição
/// do slot em `lobby_state.slots` cujo `working_set_slot_id` bate.
fn find_mmr_for_slot(
    init: &s2protocol::InitData,
    working_set_slot_id: Option<u8>,
) -> Option<i32> {
    let wsid = working_set_slot_id?;
    let slot_idx = init
        .sync_lobby_state
        .lobby_state
        .slots
        .iter()
        .position(|s| s.working_set_slot_id == Some(wsid))?;
    init.sync_lobby_state
        .user_initial_data
        .get(slot_idx)?
        .scaled_rating
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Caminho para o replay de exemplo (Terran vs Protoss, com morphs
    /// CC→Orbital e uma estrutura cancelada). Usado como golden em
    /// vários testes.
    fn example_replay() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/replay1.SC2Replay")
    }

    fn load() -> ReplayTimeline {
        parse_replay(&example_replay(), 0).expect("parse_replay")
    }

    #[test]
    fn timeline_loads() {
        let t = load();
        assert_eq!(t.players.len(), 2);
        assert!(t.game_loops > 0);
        assert!(t.loops_per_second > 0.0);
        assert!(!t.players[0].name.is_empty());
        assert!(!t.players[1].name.is_empty());
    }

    #[test]
    fn metadata_only_fast_path_skips_events() {
        let t = parse_replay(&example_replay(), 1).expect("parse_replay fast");
        assert_eq!(t.players.len(), 2);
        // Fast path: nada de tracker/message events.
        for p in &t.players {
            assert!(p.stats.is_empty(), "stats deveria estar vazio no fast path");
            assert!(
                p.entity_events.is_empty(),
                "entity_events deveria estar vazio no fast path",
            );
            assert!(p.upgrades.is_empty());
        }
        assert!(t.chat.is_empty());
    }

    #[test]
    fn stats_at_returns_latest_le() {
        let t = load();
        let p = &t.players[0];
        assert!(!p.stats.is_empty());

        // Antes do primeiro snapshot → None.
        assert!(p.stats_at(0).is_none() || p.stats[0].game_loop == 0);

        // No próprio loop do primeiro snapshot, deve devolvê-lo.
        let first = &p.stats[0];
        let s = p.stats_at(first.game_loop).unwrap();
        assert_eq!(s.game_loop, first.game_loop);

        // No meio do replay, devolve o snapshot mais recente <= alvo.
        let mid = p.stats[p.stats.len() / 2].game_loop;
        let s = p.stats_at(mid + 1).unwrap();
        assert!(s.game_loop <= mid + 1);

        // Depois do último snapshot, devolve o último.
        let last = p.stats.last().unwrap().game_loop;
        let s = p.stats_at(last + 1_000_000).unwrap();
        assert_eq!(s.game_loop, last);
    }

    #[test]
    fn upgrades_until_is_prefix() {
        let t = load();
        let p = &t.players[0];
        // 0 → vazio.
        assert!(p.upgrades_until(0).is_empty() || p.upgrades[0].game_loop == 0);
        // ∞ → todos.
        let all = p.upgrades_until(u32::MAX);
        assert_eq!(all.len(), p.upgrades.len());
        // Monotônico em loop.
        for w in p.upgrades.windows(2) {
            assert!(w[0].game_loop <= w[1].game_loop);
        }
    }

    #[test]
    fn alive_count_monotonic_for_morphs() {
        let t = load();
        // O contador acumulado por tipo nunca pode ficar negativo.
        for p in &t.players {
            for (kind, series) in &p.alive_count {
                for (loop_, count) in series {
                    assert!(
                        *count >= 0,
                        "alive_count negativo para {} no loop {}: {}",
                        kind, loop_, count
                    );
                }
            }
        }
    }

    #[test]
    fn cancellation_emitted_when_in_progress_dies() {
        let t = load();
        // O replay de exemplo tem um CommandCenter (tag 95682561) que
        // teve UnitInit no loop 7953 e UnitDied no loop 8450 — esse
        // par precisa virar ProductionCancelled, não Died.
        let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
        let cancellations: Vec<_> = terran
            .entity_events
            .iter()
            .filter(|e| e.kind == EntityEventKind::ProductionCancelled)
            .collect();
        assert!(
            !cancellations.is_empty(),
            "esperava ao menos um ProductionCancelled (CC interrompido)",
        );
        assert!(
            cancellations
                .iter()
                .any(|e| e.entity_type == "CommandCenter" && e.game_loop == 8450),
            "esperava o ProductionCancelled específico do CC tag=95682561 no loop 8450",
        );
    }

    #[test]
    fn instant_units_emit_started_and_finished_same_loop() {
        let t = load();
        // SCVs treinados a partir do CC nascem instantaneamente do
        // ponto de vista do tracker (UnitBorn cru). O parser deve
        // emitir Started+Finished no MESMO game_loop para essas tags.
        let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
        let mut by_tag: HashMap<i64, Vec<(u32, EntityEventKind)>> = HashMap::new();
        for ev in &terran.entity_events {
            if ev.entity_type == "SCV" {
                by_tag.entry(ev.tag).or_default().push((ev.game_loop, ev.kind));
            }
        }
        let mut found = 0;
        for (_, evs) in &by_tag {
            let started = evs.iter().find(|(_, k)| *k == EntityEventKind::ProductionStarted);
            let finished = evs.iter().find(|(_, k)| *k == EntityEventKind::ProductionFinished);
            if let (Some(s), Some(f)) = (started, finished) {
                assert_eq!(s.0, f.0, "Started/Finished deveriam estar no mesmo loop para SCV");
                found += 1;
            }
        }
        assert!(found > 0, "esperava ao menos um SCV com Started+Finished no mesmo loop");
    }

    #[test]
    fn morph_emits_started_and_finished_for_new_type() {
        let t = load();
        // O replay tem CC→OrbitalCommand (apply_type_change). Para os
        // morphs, esperamos Died do tipo antigo + Started + Finished do
        // tipo novo no mesmo game_loop e mesmo tag. Filtramos os
        // ProductionStarted de Orbital que vieram de morph (i.e., têm um
        // Died de CC no mesmo loop+tag) e checamos o Finished pareado.
        let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
        let morph_starts: Vec<_> = terran
            .entity_events
            .iter()
            .filter(|e| {
                e.entity_type == "OrbitalCommand"
                    && e.kind == EntityEventKind::ProductionStarted
                    && terran.entity_events.iter().any(|d| {
                        d.tag == e.tag
                            && d.game_loop == e.game_loop
                            && d.kind == EntityEventKind::Died
                            && d.entity_type == "CommandCenter"
                    })
            })
            .collect();
        assert!(
            !morph_starts.is_empty(),
            "esperava ao menos um morph CC→OrbitalCommand (Died+Started no mesmo loop+tag)",
        );
        for s in &morph_starts {
            let finished = terran.entity_events.iter().any(|e| {
                e.tag == s.tag
                    && e.kind == EntityEventKind::ProductionFinished
                    && e.game_loop == s.game_loop
                    && e.entity_type == "OrbitalCommand"
            });
            assert!(
                finished,
                "morph sem ProductionFinished de OrbitalCommand pareado em {}",
                s.game_loop,
            );
        }
    }

    #[test]
    fn worker_capacity_never_negative() {
        // O parser pode empurrar -1 mesmo quando a capacidade
        // observada estaria 0; o consumer (production_gap) clampa.
        // Mas a soma cumulativa correta nunca deveria ficar negativa
        // se a parser-side ignorar Cancelled (estrutura nunca +1'd).
        let t = load();
        for p in &t.players {
            let mut cum: i32 = 0;
            let mut events = p.worker_capacity.clone();
            events.sort_by_key(|(l, _)| *l);
            for (_, delta) in &events {
                cum += delta;
                assert!(
                    cum >= 0,
                    "worker_capacity acumulado ficou negativo em {}: {:?}",
                    p.name, events,
                );
            }
        }
    }

    #[test]
    fn state_at_loop_zero_returns_no_stats() {
        let t = load();
        let p = &t.players[0];
        // Stats começam após o loop 0 (snapshot inicial); stats_at(0)
        // pode devolver Some se o primeiro snapshot é exatamente em
        // loop 0, ou None caso contrário. Em ambos os casos não deve
        // panicar.
        let _ = p.stats_at(0);
        let _ = p.upgrades_until(0);
        let _ = p.worker_capacity_at(0);
    }

    #[test]
    fn state_at_loop_past_end_returns_last() {
        let t = load();
        let p = &t.players[0];
        let last_stat = p.stats.last().unwrap();
        let s = p.stats_at(u32::MAX).unwrap();
        assert_eq!(s.game_loop, last_stat.game_loop);

        // worker_capacity após o fim deve devolver o último valor
        // acumulado (não 0).
        let last_cap = p
            .worker_capacity
            .iter()
            .fold(0i32, |acc, (_, d)| acc + d);
        assert_eq!(p.worker_capacity_at(u32::MAX), last_cap);
    }

    #[test]
    fn entity_events_sorted_by_loop() {
        let t = load();
        for p in &t.players {
            for w in p.entity_events.windows(2) {
                assert!(
                    w[0].game_loop <= w[1].game_loop,
                    "entity_events fora de ordem em {}: {} > {}",
                    p.name, w[0].game_loop, w[1].game_loop,
                );
            }
        }
    }

    #[test]
    fn unit_positions_collected_and_sorted() {
        let t = load();
        // Pelo menos um jogador deve ter recebido amostras de
        // movimento — o replay de exemplo tem combate suficiente.
        let total: usize = t.players.iter().map(|p| p.unit_positions.len()).sum();
        assert!(
            total > 0,
            "esperava ao menos uma amostra de UnitPositionsEvent agregada",
        );
        // Sort defensivo do finalize: cada player deve estar ordenado.
        for p in &t.players {
            for w in p.unit_positions.windows(2) {
                assert!(
                    w[0].game_loop <= w[1].game_loop,
                    "unit_positions fora de ordem em {}: {} > {}",
                    p.name,
                    w[0].game_loop,
                    w[1].game_loop,
                );
            }
        }
    }

    #[test]
    fn unit_positions_in_map_scale() {
        // Sanity da escala de coordenadas: a divisão por 4 em
        // `tracker.rs` (ramo `UnitPosition`) deve produzir valores na
        // mesma faixa que `UnitBornEvent.x/y`. Se a escala estivesse
        // errada por um fator >1, a maioria das amostras estouraria
        // o `map_size_x`/`map_size_y`. Tolerância de 2× a dimensão do
        // mapa para absorver overflow do clamp e qualquer margem
        // fora da playable area.
        let t = load();
        assert!(t.map_size_x > 0 && t.map_size_y > 0);
        let limit_x = (t.map_size_x as u32).saturating_mul(2);
        let limit_y = (t.map_size_y as u32).saturating_mul(2);
        let mut checked = 0usize;
        for p in &t.players {
            for s in &p.unit_positions {
                assert!(
                    (s.x as u32) <= limit_x && (s.y as u32) <= limit_y,
                    "amostra fora de escala: ({},{}) > 2×({},{})",
                    s.x,
                    s.y,
                    t.map_size_x,
                    t.map_size_y,
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "esperava ao menos uma amostra de posição");
    }

    #[test]
    fn unit_positions_show_movement() {
        // Para alguma unidade, a posição muda entre amostras —
        // garantia de que estamos realmente coletando movimento e
        // não só repetindo o ponto de nascimento.
        let t = load();
        let mut moved = false;
        'outer: for p in &t.players {
            let mut by_tag: HashMap<i64, (u8, u8)> = HashMap::new();
            for s in &p.unit_positions {
                if let Some(&(px, py)) = by_tag.get(&s.tag) {
                    if px != s.x || py != s.y {
                        moved = true;
                        break 'outer;
                    }
                }
                by_tag.insert(s.tag, (s.x, s.y));
            }
        }
        assert!(
            moved,
            "esperava ao menos uma unidade mudando de posição entre amostras",
        );
    }

    #[test]
    fn last_known_positions_query_matches_walk() {
        let t = load();
        for p in &t.players {
            if p.unit_positions.is_empty() {
                continue;
            }
            // No último loop do replay, o snapshot do helper deve
            // bater com o resultado do walk manual sobre todas as
            // amostras (uma por tag, a mais recente).
            let until = t.game_loops;
            let snap = p.last_known_positions(until);
            let mut manual: HashMap<i64, (u8, u8)> = HashMap::new();
            for s in &p.unit_positions {
                manual.insert(s.tag, (s.x, s.y));
            }
            assert_eq!(snap.len(), manual.len());
            for (tag, pos) in &manual {
                assert_eq!(snap.get(tag), Some(pos));
            }
        }
    }

    #[test]
    fn morph_only_unit_type_change_carries_synthetic_ability() {
        // OrbitalCommand e WarpGate só emitem `UnitTypeChange` (sem
        // `UnitBorn` correspondente). O parser precisa injetar um
        // `creator_ability=Some("MorphTo*")` sintético para que o
        // build_order não filtre esses eventos por falta de ability.
        let t = load();
        for p in &t.players {
            for ev in &p.entity_events {
                if ev.kind != EntityEventKind::ProductionStarted {
                    continue;
                }
                if matches!(
                    ev.entity_type.as_str(),
                    "OrbitalCommand" | "PlanetaryFortress" | "WarpGate"
                ) {
                    let ability = ev.creator_ability.as_deref().unwrap_or("");
                    assert!(
                        ability.starts_with("MorphTo"),
                        "esperava creator_ability=MorphTo* para {} no loop {}, achei {:?}",
                        ev.entity_type, ev.game_loop, ev.creator_ability,
                    );
                }
            }
        }
    }
}
