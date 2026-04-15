// Tradução de tracker events crus do MPQ para `EntityEvent`s
// semânticos, mantendo um `tag_map` interno que rastreia o ciclo de
// vida de cada tag (`InProgress` → `Finished`). Esta é a parte mais
// densa do parser — toda a lógica de morphs, capacidade de workers e
// detecção de cancelamentos vive aqui.

use std::collections::HashMap;

use s2protocol::tracker_events::{unit_tag, ReplayTrackerEvent};

use super::classify::{
    classify_entity, is_armor_upgrade, is_army_producer, is_attack_upgrade, is_worker_producer,
    resource_kind, upgrade_level,
};
use super::types::{
    EntityCategory, EntityEvent, EntityEventKind, PlayerTimeline, ResourceNode, StatsSnapshot,
    UnitPositionSample, UpgradeEntry, UNIT_INIT_MARKER,
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

/// Tipos que, ao receberem `UnitTypeChange` (sem `UnitBorn` correspondente,
/// caso típico dos morphs Terran de CC e do `WarpGate`), devem ser tratados
/// como "build morph": o build_order precisa de um `creator_ability` que
/// passe pelo seu filtro `MorphTo*`. A lista contém apenas transformações
/// que custam tempo/recursos e que um jogador iniciou explicitamente —
/// transformações mecânicas (siege mode, lift/lower, transform) ficam de
/// fora.
fn synthetic_morph_ability(new_type: &str) -> Option<String> {
    matches!(
        new_type,
        "OrbitalCommand"
            | "PlanetaryFortress"
            | "Lair"
            | "Hive"
            | "GreaterSpire"
            | "WarpGate"
    )
    .then(|| format!("MorphTo{new_type}"))
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

/// Mapa global `unit_tag_index → (tag completo, player_idx, último
/// tipo observado)`, populado pelo tracker e consultado por
/// `game::process_game_events`. O game events parser usa essa tabela
/// para descobrir o tipo do produtor (Barracks/Gateway/Forge/etc.) que
/// recebeu um Cmd, alimentando o lookup
/// `resolve_ability_command(producer, ability_id, cmd_index)`.
///
/// O "último tipo observado" segue a regra "last write wins" — para
/// morphs in-place (CC→Orbital, Gateway→WarpGate) o tipo final é o que
/// fica registrado. Cmds emitidos antes do morph podem perder a
/// resolução, mas isso é raro o suficiente pra justificar a
/// simplicidade. Ver `game.rs` para o uso.
pub(super) struct IndexEntry {
    pub tag: i64,
    pub player_idx: usize,
    pub unit_type: String,
}

pub(super) type IndexOwnerMap = HashMap<u32, IndexEntry>;

pub(super) fn process_tracker_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    player_idx: &HashMap<u8, usize>,
    players: &mut [PlayerTimeline],
    index_owner: &mut IndexOwnerMap,
    resources: &mut Vec<ResourceNode>,
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
                    // Nó de recurso neutro: spawna em game_loop = 0 com
                    // player_id neutro. Captura a posição pro minimapa
                    // antes de cair no caminho de tag_map (que só serve
                    // pra resolver UnitDied postumamente).
                    if let Some(kind) = resource_kind(&e.unit_type_name) {
                        resources.push(ResourceNode {
                            x: e.x,
                            y: e.y,
                            kind,
                        });
                    }
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
                index_owner.insert(
                    e.unit_tag_index,
                    IndexEntry {
                        tag,
                        player_idx: idx,
                        unit_type: e.unit_type_name.clone(),
                    },
                );

                let category = classify_entity(&e.unit_type_name);
                let creator_ability = e.creator_ability_name.clone().filter(|s| !s.is_empty());
                // Tag completo do prédio produtor (Gateway/Robo/Stargate/
                // Nexus/CC/etc.). Disponível pra unidades treinadas; vem
                // como None pra spawns iniciais e larvas. O build_order
                // usa esse tag pra encadear `start = max(cmd, prev_finish)`
                // por produtor; quando é None ele cai no fallback antigo.
                let creator_tag = match (e.creator_unit_tag_index, e.creator_unit_tag_recycle) {
                    (Some(ci), Some(cr)) => Some(unit_tag(ci, cr)),
                    _ => None,
                };

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
                                creator_tag,
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
                                creator_tag: None,
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
                        if is_army_producer(&e.unit_type_name) {
                            players[idx].army_capacity.push((game_loop, 1));
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
                        // Em morph in-place o produtor é o próprio tag,
                        // independentemente do `creator_unit_tag` cru.
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
                        if let Some(entry) = index_owner.get_mut(&e.unit_tag_index) {
                            entry.unit_type = e.unit_type_name.clone();
                        }
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
                                creator_tag: None,
                                killer_player_id: None,
                            },
                        );
                        if is_worker_producer(&e.unit_type_name) {
                            players[idx].worker_capacity.push((game_loop, 1));
                        }
                        if is_army_producer(&e.unit_type_name) {
                            players[idx].army_capacity.push((game_loop, 1));
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
                // Sprays continuam fora — são puramente cosméticos.
                if e.unit_type_name.contains("Spray") {
                    continue;
                }
                // Tumors entram no stream com lifecycle completa, mas
                // marcadas com um `creator_ability` próprio para que o
                // filtro de `build_order.rs` (que aceita só
                // UNIT_INIT_MARKER / Train* / MorphTo*) as descarte
                // naturalmente. O `finalize.rs` consome esses eventos
                // pra construir `creep_index` (camada de creep do
                // minimapa).
                let is_tumor = e.unit_type_name.contains("Tumor");
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(&idx) = player_idx.get(&e.control_player_id) else { continue };
                let category = classify_entity(&e.unit_type_name);
                index_owner.insert(
                    e.unit_tag_index,
                    IndexEntry {
                        tag,
                        player_idx: idx,
                        unit_type: e.unit_type_name.clone(),
                    },
                );

                let creator_ability = if is_tumor {
                    // Hardcoded em vez de ler `creator_ability_name` do
                    // evento — só precisamos de algo que não case com o
                    // filtro do build_order, e queremos um nome estável
                    // pra `creep_index` filtrar via match exato.
                    Some("BuildCreepTumor".to_string())
                } else {
                    // Sentinel: marca eventos vindos de UnitInit para
                    // que o build_order possa distinguir warp-ins/
                    // construções de spawns iniciais.
                    Some(UNIT_INIT_MARKER.to_string())
                };

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
                        creator_ability,
                        // UnitInit é a "construção via Probe/SCV" (e
                        // warp-ins). Não há produtor pra encadear; o
                        // build_order cai no fallback antigo
                        // `start = raw + add_build_time`.
                        creator_tag: None,
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
                        creator_tag: None,
                        killer_player_id: None,
                    },
                );
                if is_worker_producer(&entity_type) {
                    players[idx].worker_capacity.push((game_loop, 1));
                }
                if is_army_producer(&entity_type) {
                    players[idx].army_capacity.push((game_loop, 1));
                }
                if let Some(state) = tag_map.get_mut(&tag) {
                    state.lifecycle = Lifecycle::Finished;
                }
            }

            ReplayTrackerEvent::UnitDied(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                // NOTE: deliberadamente NÃO removemos do `index_owner`
                // — `game::process_game_events` ainda vai consultar
                // tipos de prédios mortos quando tentar resolver Cmds
                // antigos. UnitPositions não causa lixo porque o jogo
                // não emite posição pra unidades mortas.
                let Some(state) = tag_map.remove(&tag) else { continue };
                let Some(&idx) = player_idx.get(&state.player_id) else { continue };
                let category = classify_entity(&state.entity_type);

                let kind = if state.lifecycle == Lifecycle::InProgress {
                    EntityEventKind::ProductionCancelled
                } else {
                    if is_worker_producer(&state.entity_type) {
                        players[idx].worker_capacity.push((game_loop, -1));
                    }
                    if is_army_producer(&state.entity_type) {
                        players[idx].army_capacity.push((game_loop, -1));
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
                        creator_tag: None,
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

                let synthetic_ability = synthetic_morph_ability(&e.unit_type_name);
                apply_type_change(
                    &mut players[idx],
                    game_loop,
                    seq,
                    tag,
                    &old_type,
                    &e.unit_type_name,
                    pos_x,
                    pos_y,
                    synthetic_ability,
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
                if let Some(entry) = index_owner.get_mut(&e.unit_tag_index) {
                    entry.unit_type = e.unit_type_name.clone();
                }
            }

            ReplayTrackerEvent::UnitPosition(e) => {
                // O SC2 emite `UnitPositionsEvent` periodicamente com a
                // posição atual de várias unidades móveis. Cada item só
                // carrega o `unit_tag_index` (não o tag completo), então
                // resolvemos o dono via `index_owner`.
                //
                // Escala: `to_unit_positions_vec` já multiplica os
                // deltas brutos por 4 (passando para "world units" do
                // SC2). Para voltar à mesma escala de células usada por
                // `UnitBornEvent.x/y` (`u8`), dividimos por 4 de novo.
                for up in e.to_unit_positions_vec() {
                    let Some(entry) = index_owner.get(&up.tag) else {
                        continue;
                    };
                    let cx = (up.x / 4).clamp(0, 255) as u8;
                    let cy = (up.y / 4).clamp(0, 255) as u8;
                    players[entry.player_idx].unit_positions.push(UnitPositionSample {
                        game_loop,
                        tag: entry.tag,
                        x: cx,
                        y: cy,
                    });
                }
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
            creator_tag: None,
            killer_player_id: None,
        },
    );

    // creator_ability passa-pelo como veio: Some("MorphTo*") quando o
    // evento veio de um UnitBorn de morph real (player issued), None
    // quando veio de UnitTypeChange (siege, lift, lower depot, etc).
    // O build_order só inclui o caso Some("MorphTo*"); transformações
    // mecânicas (None) não devem aparecer no build order.
    //
    // Em morph in-place o "produtor" é o próprio prédio sendo morphado
    // (CC→Orbital, Gateway→WarpGate, etc.). Setando creator_tag=Some(tag)
    // o build_order pode pegar o cmd MorphTo* via FIFO.
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
            creator_tag: Some(tag),
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
            creator_tag: None,
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

    // Army capacity: mesma lógica, cobrindo morphs tipo Gateway→WarpGate
    // (hoje net-zero pois `morph_build_time` não inclui WarpGate, mas o
    // match deixa o lugar certo caso no futuro incluamos morphs com
    // downtime real para army).
    let old_is_army = is_army_producer(old_type);
    let new_is_army = is_army_producer(new_type);
    match (old_is_army, new_is_army) {
        (true, true) => {
            let mt = morph_build_time(new_type);
            if mt > 0 {
                let morph_start = game_loop.saturating_sub(mt);
                player.army_capacity.push((morph_start, -1));
                player.army_capacity.push((game_loop, 1));
            }
        }
        (true, false) => {
            player.army_capacity.push((game_loop, -1));
        }
        (false, true) => {
            player.army_capacity.push((game_loop, 1));
        }
        (false, false) => {}
    }
}
