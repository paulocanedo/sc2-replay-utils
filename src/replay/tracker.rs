// Tradução de tracker events crus do MPQ para `EntityEvent`s
// semânticos, mantendo um `tag_map` interno que rastreia o ciclo de
// vida de cada tag (`InProgress` → `Finished`). Esta é a parte mais
// densa do parser — toda a lógica de morphs, capacidade de workers e
// detecção de cancelamentos vive aqui.

use std::collections::HashMap;

use s2protocol::tracker_events::{unit_tag, ReplayTrackerEvent};

use super::classify::{
    classify_entity, is_attack_upgrade, is_armor_upgrade, is_worker_producer, upgrade_level,
};
use super::types::{
    EntityCategory, EntityEvent, EntityEventKind, PlayerTimeline, StatsSnapshot, UpgradeEntry,
    UNIT_INIT_MARKER,
};

// ── Constantes de morph ─────────────────────────────────────────────

/// Tempo de morph CC → Orbital Command em game loops (~25s em Faster).
const ORBITAL_MORPH_TIME: u32 = 560;

/// Tempo de morph CC → Planetary Fortress em game loops (~36s em Faster).
const PF_MORPH_TIME: u32 = 806;

fn morph_build_time(unit_type: &str) -> u32 {
    match unit_type {
        "OrbitalCommand" => ORBITAL_MORPH_TIME,
        "PlanetaryFortress" => PF_MORPH_TIME,
        _ => 0,
    }
}

// ── Estado do tag_map ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    InProgress,
    Finished,
}

struct TagState {
    entity_type: String,
    player_id: u8,
    pos_x: u8,
    pos_y: u8,
    lifecycle: Lifecycle,
}

// ── Loop principal ──────────────────────────────────────────────────

pub(super) fn process_tracker_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    player_idx: &HashMap<u8, usize>,
    players: &mut [PlayerTimeline],
    max_loops: u32,
) -> Result<(), String> {
    let tracker_events = s2protocol::read_tracker_events(path_str, mpq, file_contents)
        .map_err(|e| format!("{:?}", e))?;

    let mut tag_map: HashMap<i64, TagState> = HashMap::new();
    let mut cur_attack: Vec<u8> = vec![0; players.len()];
    let mut cur_armor: Vec<u8> = vec![0; players.len()];

    let mut game_loop: u32 = 0;
    // Sequência monotônica usada como tiebreaker entre eventos do
    // mesmo `game_loop`. Compartilhada entre `entity_events` e
    // `upgrades` para preservar a interleavação original do tracker.
    let mut seq: u32 = 0;

    for ev in tracker_events {
        game_loop += ev.delta;
        if max_loops != 0 && game_loop > max_loops {
            break;
        }
        seq = seq.wrapping_add(1);

        match ev.event {
            ReplayTrackerEvent::PlayerStats(e) => {
                let Some(&idx) = player_idx.get(&e.player_id) else { continue };
                let s = &e.stats;
                players[idx].stats.push(StatsSnapshot {
                    game_loop,
                    minerals: s.minerals_current,
                    vespene: s.vespene_current,
                    minerals_rate: s.minerals_collection_rate,
                    vespene_rate: s.vespene_collection_rate,
                    workers: s.workers_active_count,
                    supply_used: s.food_used,
                    supply_made: s.food_made,
                    army_value_minerals: s.minerals_used_active_forces,
                    army_value_vespene: s.vespene_used_active_forces,
                    minerals_lost_army: s.minerals_lost_army,
                    vespene_lost_army: s.vespene_lost_army,
                    minerals_killed_army: s.minerals_killed_army,
                    vespene_killed_army: s.vespene_killed_army,
                });
            }

            ReplayTrackerEvent::Upgrade(e) => {
                let Some(&idx) = player_idx.get(&e.player_id) else { continue };
                if e.upgrade_type_name.contains("Spray") {
                    continue;
                }
                let level = upgrade_level(&e.upgrade_type_name);
                if is_attack_upgrade(&e.upgrade_type_name) && level > 0 {
                    cur_attack[idx] = cur_attack[idx].max(level);
                }
                if is_armor_upgrade(&e.upgrade_type_name) && level > 0 {
                    cur_armor[idx] = cur_armor[idx].max(level);
                }
                players[idx].upgrades.push(UpgradeEntry {
                    game_loop,
                    seq,
                    name: e.upgrade_type_name,
                });
                players[idx]
                    .upgrade_cumulative
                    .push((game_loop, cur_attack[idx], cur_armor[idx]));
            }

            ReplayTrackerEvent::UnitBorn(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(&idx) = player_idx.get(&e.control_player_id) else {
                    // Insere mesmo assim para resolução posterior se vier UnitDied.
                    tag_map.insert(
                        tag,
                        TagState {
                            entity_type: e.unit_type_name.clone(),
                            player_id: e.control_player_id,
                            pos_x: e.x,
                            pos_y: e.y,
                            lifecycle: Lifecycle::Finished,
                        },
                    );
                    continue;
                };

                let category = classify_entity(&e.unit_type_name);
                let creator_ability = e.creator_ability_name.clone().filter(|s| !s.is_empty());

                // Decisão baseada no estado atual da tag.
                let prev_lifecycle = tag_map.get(&tag).map(|s| (s.entity_type.clone(), s.lifecycle));

                match prev_lifecycle {
                    None => {
                        // Tag novo: spawn instantâneo. Started+Finished
                        // no mesmo loop.
                        push_event(
                            &mut players[idx],
                            EntityEvent {
                                game_loop,
                                seq,
                                kind: EntityEventKind::ProductionStarted,
                                entity_type: e.unit_type_name.clone(),
                                category,
                                tag,
                                pos_x: e.x,
                                pos_y: e.y,
                                creator_ability: creator_ability.clone(),
                                killer_player_id: None,
                            },
                        );
                        push_event(
                            &mut players[idx],
                            EntityEvent {
                                game_loop,
                                seq,
                                kind: EntityEventKind::ProductionFinished,
                                entity_type: e.unit_type_name.clone(),
                                category,
                                tag,
                                pos_x: e.x,
                                pos_y: e.y,
                                creator_ability: None,
                                killer_player_id: None,
                            },
                        );

                        // Worker nascido via Train (SCV/Probe).
                        if matches!(category, EntityCategory::Worker)
                            && creator_ability.as_deref().unwrap_or("").contains("Train")
                            && matches!(e.unit_type_name.as_str(), "SCV" | "Probe")
                        {
                            players[idx].worker_births.push(game_loop);
                        }

                        // Estrutura produtora aparecendo (ex: CC inicial).
                        if is_worker_producer(&e.unit_type_name) {
                            players[idx].worker_capacity.push((game_loop, 1));
                        }

                        tag_map.insert(
                            tag,
                            TagState {
                                entity_type: e.unit_type_name.clone(),
                                player_id: e.control_player_id,
                                pos_x: e.x,
                                pos_y: e.y,
                                lifecycle: Lifecycle::Finished,
                            },
                        );
                    }
                    Some((prev_type, _)) if prev_type != e.unit_type_name => {
                        // Tag existente, type diferente: morph completion
                        // (UnitTypeChange pode ou não ter chegado antes).
                        apply_type_change(
                            &mut players[idx],
                            game_loop,
                            seq,
                            tag,
                            &prev_type,
                            &e.unit_type_name,
                            e.x,
                            e.y,
                            creator_ability,
                        );
                        tag_map.insert(
                            tag,
                            TagState {
                                entity_type: e.unit_type_name.clone(),
                                player_id: e.control_player_id,
                                pos_x: e.x,
                                pos_y: e.y,
                                lifecycle: Lifecycle::Finished,
                            },
                        );
                    }
                    Some((_, Lifecycle::InProgress)) => {
                        // Init prévio + Born agora = produção concluída.
                        push_event(
                            &mut players[idx],
                            EntityEvent {
                                game_loop,
                                seq,
                                kind: EntityEventKind::ProductionFinished,
                                entity_type: e.unit_type_name.clone(),
                                category,
                                tag,
                                pos_x: e.x,
                                pos_y: e.y,
                                creator_ability: None,
                                killer_player_id: None,
                            },
                        );
                        if is_worker_producer(&e.unit_type_name) {
                            players[idx].worker_capacity.push((game_loop, 1));
                        }
                        if let Some(state) = tag_map.get_mut(&tag) {
                            state.lifecycle = Lifecycle::Finished;
                        }
                    }
                    Some((_, Lifecycle::Finished)) => {
                        // Tag re-emitido, mesmo tipo, já Finished — duplicata, ignorar.
                    }
                }
            }

            ReplayTrackerEvent::UnitInit(e) => {
                // Cosméticos / placements táticos não são build order.
                if e.unit_type_name.contains("Tumor") || e.unit_type_name.contains("Spray") {
                    continue;
                }
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(&idx) = player_idx.get(&e.control_player_id) else { continue };
                let category = classify_entity(&e.unit_type_name);

                push_event(
                    &mut players[idx],
                    EntityEvent {
                        game_loop,
                        seq,
                        kind: EntityEventKind::ProductionStarted,
                        entity_type: e.unit_type_name.clone(),
                        category,
                        tag,
                        pos_x: e.x,
                        pos_y: e.y,
                        // Sentinel: marca eventos vindos de UnitInit para
                        // que o build_order possa distinguir warp-ins/
                        // construções de spawns iniciais.
                        creator_ability: Some(UNIT_INIT_MARKER.to_string()),
                        killer_player_id: None,
                    },
                );

                tag_map.insert(
                    tag,
                    TagState {
                        entity_type: e.unit_type_name.clone(),
                        player_id: e.control_player_id,
                        pos_x: e.x,
                        pos_y: e.y,
                        lifecycle: Lifecycle::InProgress,
                    },
                );
            }

            ReplayTrackerEvent::UnitDone(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(state) = tag_map.get_mut(&tag) else { continue };
                let Some(&idx) = player_idx.get(&state.player_id) else { continue };
                let category = classify_entity(&state.entity_type);
                let entity_type = state.entity_type.clone();
                let pos_x = state.pos_x;
                let pos_y = state.pos_y;

                push_event(
                    &mut players[idx],
                    EntityEvent {
                        game_loop,
                        seq,
                        kind: EntityEventKind::ProductionFinished,
                        entity_type: entity_type.clone(),
                        category,
                        tag,
                        pos_x,
                        pos_y,
                        creator_ability: None,
                        killer_player_id: None,
                    },
                );
                if is_worker_producer(&entity_type) {
                    players[idx].worker_capacity.push((game_loop, 1));
                }
                if let Some(state) = tag_map.get_mut(&tag) {
                    state.lifecycle = Lifecycle::Finished;
                }
            }

            ReplayTrackerEvent::UnitDied(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(state) = tag_map.remove(&tag) else { continue };
                let Some(&idx) = player_idx.get(&state.player_id) else { continue };
                let category = classify_entity(&state.entity_type);

                let kind = if state.lifecycle == Lifecycle::InProgress {
                    EntityEventKind::ProductionCancelled
                } else {
                    if is_worker_producer(&state.entity_type) {
                        players[idx].worker_capacity.push((game_loop, -1));
                    }
                    EntityEventKind::Died
                };

                push_event(
                    &mut players[idx],
                    EntityEvent {
                        game_loop,
                        seq,
                        kind,
                        entity_type: state.entity_type,
                        category,
                        tag,
                        pos_x: e.x,
                        pos_y: e.y,
                        creator_ability: None,
                        killer_player_id: e.killer_player_id,
                    },
                );
            }

            ReplayTrackerEvent::UnitTypeChange(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(state) = tag_map.get(&tag) else { continue };
                if state.entity_type == e.unit_type_name {
                    continue; // dedupe vs UnitBorn de morph
                }
                let pid = state.player_id;
                let pos_x = state.pos_x;
                let pos_y = state.pos_y;
                let old_type = state.entity_type.clone();
                let Some(&idx) = player_idx.get(&pid) else { continue };

                apply_type_change(
                    &mut players[idx],
                    game_loop,
                    seq,
                    tag,
                    &old_type,
                    &e.unit_type_name,
                    pos_x,
                    pos_y,
                    None,
                );

                tag_map.insert(
                    tag,
                    TagState {
                        entity_type: e.unit_type_name.clone(),
                        player_id: pid,
                        pos_x,
                        pos_y,
                        lifecycle: Lifecycle::Finished,
                    },
                );
            }

            _ => {}
        }
    }

    Ok(())
}

fn push_event(player: &mut PlayerTimeline, ev: EntityEvent) {
    player.entity_events.push(ev);
}

/// Aplica uma transição de tipo (UnitTypeChange ou UnitBorn de morph).
///
/// Emite `Died` para o tipo antigo e `ProductionStarted`+`ProductionFinished`
/// para o novo tipo, ambos no `game_loop` atual (a back-data do `Started`
/// é evitada porque o build_order da GUI já faz o ajuste de start-time
/// via `build_time_seconds`). A lógica de capacidade de workers, que
/// precisa do morph downtime real, ainda usa o instante back-dated.
fn apply_type_change(
    player: &mut PlayerTimeline,
    game_loop: u32,
    seq: u32,
    tag: i64,
    old_type: &str,
    new_type: &str,
    pos_x: u8,
    pos_y: u8,
    creator_ability: Option<String>,
) {
    let old_category = classify_entity(old_type);
    let new_category = classify_entity(new_type);

    push_event(
        player,
        EntityEvent {
            game_loop,
            seq,
            kind: EntityEventKind::Died,
            entity_type: old_type.to_string(),
            category: old_category,
            tag,
            pos_x,
            pos_y,
            creator_ability: None,
            killer_player_id: None,
        },
    );

    // creator_ability passa-pelo como veio: Some("MorphTo*") quando o
    // evento veio de um UnitBorn de morph real (player issued), None
    // quando veio de UnitTypeChange (siege, lift, lower depot, etc).
    // O build_order só inclui o caso Some("MorphTo*"); transformações
    // mecânicas (None) não devem aparecer no build order.
    push_event(
        player,
        EntityEvent {
            game_loop,
            seq,
            kind: EntityEventKind::ProductionStarted,
            entity_type: new_type.to_string(),
            category: new_category,
            tag,
            pos_x,
            pos_y,
            creator_ability,
            killer_player_id: None,
        },
    );
    push_event(
        player,
        EntityEvent {
            game_loop,
            seq,
            kind: EntityEventKind::ProductionFinished,
            entity_type: new_type.to_string(),
            category: new_category,
            tag,
            pos_x,
            pos_y,
            creator_ability: None,
            killer_player_id: None,
        },
    );

    // Worker capacity: replica a lógica de production_gap.rs, com o
    // backfill de morph downtime para CC→Orbital/PF.
    let old_is_prod = is_worker_producer(old_type);
    let new_is_prod = is_worker_producer(new_type);
    match (old_is_prod, new_is_prod) {
        (true, true) => {
            let mt = morph_build_time(new_type);
            if mt > 0 {
                let morph_start = game_loop.saturating_sub(mt);
                player.worker_capacity.push((morph_start, -1));
                player.worker_capacity.push((game_loop, 1));
            }
        }
        (true, false) => {
            player.worker_capacity.push((game_loop, -1));
        }
        (false, true) => {
            player.worker_capacity.push((game_loop, 1));
        }
        (false, false) => {}
    }
}
