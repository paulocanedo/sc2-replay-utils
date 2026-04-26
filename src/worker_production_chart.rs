// Extractor de "lanes" de produção de worker por estrutura (townhall).
//
// Produz, para cada jogador, uma lane vertical por Nexus / CommandCenter /
// OrbitalCommand / PlanetaryFortress / Hatchery / Lair / Hive (chaveada
// pelo `tag` do replay — morphs in-place reusam o tag, então CC→Orbital
// vira a mesma lane).
//
// Cada lane tem blocos de tempo classificados:
// - Producing: janela `[finish - build_time, finish]` em que um SCV/Probe/
//   Drone foi treinado nessa estrutura. O stream `entity_events` emite
//   `Started+Finished` no mesmo loop quando o worker nasce (não há
//   janela explícita), então usamos `balance_data::build_time_loops` para
//   reconstruir a janela visual.
// - Morphing: janela `[finish - morph_time, finish]` em que a townhall
//   ficou ocupada com um morph in-place (CC→Orbital/PF, Hatchery→Lair,
//   Lair→Hive). É o "impedimento": a estrutura está viva mas não pode
//   produzir worker.
//
// Resolução worker → townhall:
// 1. SCV/Probe via UnitBorn: o `ProductionStarted` companheiro (mesmo
//    loop+tag, no índice `i-1`) carrega `creator_tag = Some(cc_tag)`. É
//    o caminho preferido.
// 2. Drone via apply_type_change (morph Larva→Drone): o Started
//    companheiro tem `creator_tag = Some(tag)` (próprio tag, ex-larva).
//    Resolvemos via `larva_to_hatch[tag]`, populado quando a Larva
//    nasceu (Started.creator_tag = hatchery_tag).
// 3. Fallback universal: townhall viva mais próxima da posição do
//    worker. Cobre Probe (warp-in via UnitInit, sem creator_tag), drones
//    com larva nascida antes do parser começar a coletar, e qualquer
//    outro caso.

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{EntityEvent, EntityEventKind, PlayerTimeline, ReplayTimeline};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    Producing,
    Morphing,
}

#[derive(Clone, Copy, Debug)]
pub struct ProductionBlock {
    pub start_loop: u32,
    pub end_loop: u32,
    pub kind: BlockKind,
}

#[derive(Clone, Debug)]
pub struct StructureLane {
    pub tag: i64,
    /// Tipo final da estrutura (após morphs). Usado para resolver o
    /// ícone — uma lane CC→Orbital aparece como Orbital ao final.
    pub canonical_type: &'static str,
    pub born_loop: u32,
    /// `None` se a estrutura sobreviveu até o fim do replay.
    pub died_loop: Option<u32>,
    /// Posição (tile) — preserva a última posição vista. Usado pela
    /// resolução por proximidade no fallback Zerg.
    pub pos_x: u8,
    pub pos_y: u8,
    pub blocks: Vec<ProductionBlock>,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerWorkerLanes {
    pub lanes: Vec<StructureLane>,
}

/// Tipos de townhall que viram lane no gráfico.
fn townhall_canonical(name: &str) -> Option<&'static str> {
    match name {
        "Nexus" => Some("Nexus"),
        "CommandCenter" => Some("CommandCenter"),
        "OrbitalCommand" => Some("OrbitalCommand"),
        "PlanetaryFortress" => Some("PlanetaryFortress"),
        "Hatchery" => Some("Hatchery"),
        "Lair" => Some("Lair"),
        "Hive" => Some("Hive"),
        _ => None,
    }
}

fn is_worker_unit(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone")
}

/// Tempo de morph in-place em game loops para os pares cobertos:
/// CC→Orbital/PF, Hatchery→Lair, Lair→Hive. Tenta primeiro a balance
/// data (depende do `base_build` do replay) e cai em constantes
/// hardcoded se a tabela não tiver o nome — espelha o fallback de
/// `finalize.rs`. Retorna 0 quando o tipo não é um morph conhecido.
fn morph_build_loops(new_type: &str, base_build: u32) -> u32 {
    let from_balance = balance_data::build_time_loops(new_type, base_build);
    if from_balance > 0 {
        return from_balance;
    }
    match new_type {
        // Constantes do finalize.rs (Faster speed).
        "OrbitalCommand" => 560,
        "PlanetaryFortress" => 806,
        "Lair" => 1424, // ~57s (balance: 57.1)
        "Hive" => 2160, // ~100s
        _ => 0,
    }
}

/// Detecta se um `ProductionStarted` no índice `i` faz parte de um
/// triplet de morph in-place: `Died(old, tag, gl)` no `i-1`,
/// `ProductionStarted(new, tag, gl)` no `i`. Retorna `Some(old_type)`
/// quando casa.
fn morph_old_type<'a>(events: &'a [EntityEvent], i: usize) -> Option<&'a str> {
    if i == 0 {
        return None;
    }
    let prev = &events[i - 1];
    let cur = &events[i];
    if matches!(prev.kind, EntityEventKind::Died)
        && prev.tag == cur.tag
        && prev.game_loop == cur.game_loop
    {
        Some(prev.entity_type.as_str())
    } else {
        None
    }
}

/// Indica se este `ProductionFinished` é o terceiro elo de um morph
/// in-place — ver `finalize.rs:361-368`. Usado para evitar criar uma
/// lane nova ao "born" sintético do morph.
fn is_morph_finish(events: &[EntityEvent], i: usize) -> bool {
    if i < 2 {
        return false;
    }
    let cur = &events[i];
    let s = &events[i - 1];
    let d = &events[i - 2];
    matches!(s.kind, EntityEventKind::ProductionStarted)
        && s.tag == cur.tag
        && s.game_loop == cur.game_loop
        && matches!(d.kind, EntityEventKind::Died)
        && d.tag == cur.tag
        && d.game_loop == cur.game_loop
}

/// Indica se este `Died` é o primeiro elo de um morph in-place (i.e.
/// vai ser sucedido por um `ProductionStarted` companheiro). Usado para
/// não fechar a lane com `died_loop` num morph.
fn is_morph_died(events: &[EntityEvent], i: usize) -> bool {
    let cur = &events[i];
    let Some(next) = events.get(i + 1) else {
        return false;
    };
    matches!(next.kind, EntityEventKind::ProductionStarted)
        && next.tag == cur.tag
        && next.game_loop == cur.game_loop
}

fn extract_player(player: &PlayerTimeline, base_build: u32) -> PlayerWorkerLanes {
    let events = &player.entity_events;
    let mut lanes_by_tag: HashMap<i64, StructureLane> = HashMap::new();
    // Mapa larva_tag → hatchery_tag, populado quando a Larva nasce.
    // Como o tag é estável durante o morph Larva→Drone, lookup pelo tag
    // do drone retorna a hatch certa.
    let mut larva_to_hatch: HashMap<i64, i64> = HashMap::new();

    for i in 0..events.len() {
        let ev = &events[i];
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                let new_type = ev.entity_type.as_str();

                // Morph in-place de townhall: CC→Orbital/PF, Hatchery→Lair,
                // Lair→Hive. Atualiza o tipo canônico da lane existente e
                // emite o bloco Morphing retroativo (`finish - morph_time`).
                if let Some(new_canonical) = townhall_canonical(new_type) {
                    if let Some(old_type) = morph_old_type(events, i) {
                        if townhall_canonical(old_type).is_some() {
                            if let Some(lane) = lanes_by_tag.get_mut(&ev.tag) {
                                let mt = morph_build_loops(new_canonical, base_build);
                                if mt > 0 {
                                    let start = ev.game_loop.saturating_sub(mt);
                                    lane.blocks.push(ProductionBlock {
                                        start_loop: start,
                                        end_loop: ev.game_loop,
                                        kind: BlockKind::Morphing,
                                    });
                                }
                                lane.canonical_type = new_canonical;
                            }
                        }
                    }
                }

                // Larva nasce: registra `larva_tag → hatchery_tag` agora,
                // que é onde o creator_tag está populado (no Finished
                // companheiro ele é None — ver tracker.rs:240).
                if new_type == "Larva" {
                    if let Some(creator) = ev.creator_tag {
                        larva_to_hatch.insert(ev.tag, creator);
                    }
                }
            }
            EntityEventKind::ProductionFinished => {
                let new_type = ev.entity_type.as_str();

                // Townhall born (real, não o "finish" sintético do morph
                // in-place): cria a lane.
                if let Some(canonical) = townhall_canonical(new_type) {
                    if !is_morph_finish(events, i) && !lanes_by_tag.contains_key(&ev.tag) {
                        lanes_by_tag.insert(
                            ev.tag,
                            StructureLane {
                                tag: ev.tag,
                                canonical_type: canonical,
                                born_loop: ev.game_loop,
                                died_loop: None,
                                pos_x: ev.pos_x,
                                pos_y: ev.pos_y,
                                blocks: Vec::new(),
                            },
                        );
                    }
                }

                // Worker concluído: reconstruir janela e atribuir lane.
                if is_worker_unit(new_type) {
                    let producer_tag = resolve_worker_producer(
                        events,
                        i,
                        new_type,
                        ev.tag,
                        ev.pos_x,
                        ev.pos_y,
                        ev.game_loop,
                        &lanes_by_tag,
                        &larva_to_hatch,
                    );
                    if let Some(producer) = producer_tag {
                        let build_time = balance_data::build_time_loops(new_type, base_build);
                        // Fallbacks defensivos quando o balance data não
                        // tem o nome (replay com base_build muito antigo
                        // ou unidade exótica): SCV/Probe ~12s, Drone ~12s.
                        let build_time = if build_time > 0 { build_time } else { 272 };
                        let start_loop = ev.game_loop.saturating_sub(build_time);
                        if let Some(lane) = lanes_by_tag.get_mut(&producer) {
                            lane.blocks.push(ProductionBlock {
                                start_loop,
                                end_loop: ev.game_loop,
                                kind: BlockKind::Producing,
                            });
                        }
                    }
                }
            }
            EntityEventKind::ProductionCancelled => {
                // Worker cancelado: nada a fazer (não pareamos por pending).
            }
            EntityEventKind::Died => {
                // Morte real da townhall (não o `Died` sintético do morph
                // in-place): fecha a lane. O `Died` sintético tem um
                // `ProductionStarted` companheiro no mesmo loop+tag e é
                // ignorado aqui.
                if !is_morph_died(events, i) {
                    if let Some(lane) = lanes_by_tag.get_mut(&ev.tag) {
                        lane.died_loop = Some(ev.game_loop);
                    }
                }
            }
        }
    }

    let mut lanes: Vec<StructureLane> = lanes_by_tag.into_values().collect();
    lanes.sort_by_key(|l| (l.born_loop, l.tag));

    // Garante ordenação interna dos blocos (Started fora de ordem em
    // replays patológicos podem violar isso).
    for lane in &mut lanes {
        lane.blocks.sort_by_key(|b| b.start_loop);
    }

    PlayerWorkerLanes { lanes }
}

/// Encontra a townhall que produziu este worker. Cascata:
/// 1. `Started` companheiro (mesmo loop+tag, índice `i-1`) com
///    `creator_tag = Some(t)` onde `t` é uma lane conhecida e ≠ self_tag
///    (caso SCV/Probe via UnitBorn).
/// 2. Drone via morph Larva→Drone: `larva_to_hatch[self_tag]`.
/// 3. Fallback universal: townhall viva mais próxima da posição do
///    worker. Funciona para Probe (warp-in via UnitInit, sem
///    creator_tag), Drones com larva criada antes do parser começar a
///    coletar e qualquer outro caso patológico.
#[allow(clippy::too_many_arguments)]
fn resolve_worker_producer(
    events: &[EntityEvent],
    finished_index: usize,
    unit_type: &str,
    self_tag: i64,
    pos_x: u8,
    pos_y: u8,
    finish_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
    larva_to_hatch: &HashMap<i64, i64>,
) -> Option<i64> {
    // (1) Started companheiro.
    if finished_index > 0 {
        let started = &events[finished_index - 1];
        if matches!(started.kind, EntityEventKind::ProductionStarted)
            && started.tag == self_tag
            && started.game_loop == events[finished_index].game_loop
        {
            if let Some(t) = started.creator_tag {
                if t != self_tag && lanes.contains_key(&t) {
                    return Some(t);
                }
            }
        }
    }

    // (2) Drone via morph Larva→Drone.
    if unit_type == "Drone" {
        if let Some(&hatch) = larva_to_hatch.get(&self_tag) {
            if lanes.contains_key(&hatch) {
                return Some(hatch);
            }
        }
    }

    // (3) Proximidade espacial.
    resolve_by_proximity(lanes, unit_type, finish_loop, pos_x, pos_y)
}

fn resolve_by_proximity(
    lanes: &HashMap<i64, StructureLane>,
    unit_type: &str,
    at_loop: u32,
    x: u8,
    y: u8,
) -> Option<i64> {
    // Restringe os candidatos por raça do worker para evitar atribuição
    // cruzada em replays com mais de 2 jogadores ou bases muito
    // próximas.
    let allowed: &[&str] = match unit_type {
        "SCV" => &["CommandCenter", "OrbitalCommand", "PlanetaryFortress"],
        "Probe" => &["Nexus"],
        "Drone" => &["Hatchery", "Lair", "Hive"],
        _ => return None,
    };

    let mut best: Option<(i64, i32)> = None;
    for lane in lanes.values() {
        if lane.born_loop > at_loop {
            continue;
        }
        if lane.died_loop.map(|d| d <= at_loop).unwrap_or(false) {
            continue;
        }
        if !allowed.contains(&lane.canonical_type) {
            continue;
        }
        let dx = lane.pos_x as i32 - x as i32;
        let dy = lane.pos_y as i32 - y as i32;
        let d2 = dx * dx + dy * dy;
        if best.map(|(_, b)| d2 < b).unwrap_or(true) {
            best = Some((lane.tag, d2));
        }
    }
    best.map(|(tag, _)| tag)
}

/// Constrói as lanes para todos os jogadores do replay, na mesma ordem
/// de `timeline.players`. Cada chamada percorre `entity_events` uma
/// única vez por jogador.
pub fn extract(timeline: &ReplayTimeline) -> Vec<PlayerWorkerLanes> {
    timeline
        .players
        .iter()
        .map(|p| extract_player(p, timeline.base_build))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{EntityCategory, EntityEvent, EntityEventKind};

    fn ev(
        gl: u32,
        seq: u32,
        kind: EntityEventKind,
        ty: &str,
        tag: i64,
        creator: Option<i64>,
    ) -> EntityEvent {
        EntityEvent {
            game_loop: gl,
            seq,
            kind,
            entity_type: ty.to_string(),
            category: if matches!(ty, "SCV" | "Probe" | "Drone") {
                EntityCategory::Worker
            } else if matches!(ty, "Larva") {
                EntityCategory::Unit
            } else {
                EntityCategory::Structure
            },
            tag,
            pos_x: 0,
            pos_y: 0,
            creator_ability: None,
            creator_tag: creator,
            killer_player_id: None,
        }
    }

    fn player_with_events(events: Vec<EntityEvent>) -> PlayerTimeline {
        PlayerTimeline {
            name: "p".into(),
            clan: String::new(),
            race: "Terran".into(),
            mmr: None,
            player_id: 1,
            result: None,
            toon: None,
            stats: vec![],
            upgrades: vec![],
            entity_events: events,
            production_cmds: vec![],
            inject_cmds: vec![],
            unit_positions: vec![],
            camera_positions: vec![],
            alive_count: Default::default(),
            worker_capacity: vec![],
            worker_births: vec![],
            army_capacity: vec![],
            army_productions: vec![],
            worker_capacity_cumulative: vec![],
            army_capacity_cumulative: vec![],
            upgrade_cumulative: vec![],
            creep_index: vec![],
        }
    }

    #[test]
    fn terran_cc_morphs_to_orbital_emits_morphing_block() {
        // CC nasce em loop=100 (tag=1).
        // Em loop=1000 morpha pra Orbital: Died(CC) + Started(Orbital) +
        // Finished(Orbital), todos no mesmo loop+tag.
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "CommandCenter",
                1,
                None,
            ),
            ev(1000, 1, EntityEventKind::Died, "CommandCenter", 1, None),
            ev(
                1000,
                2,
                EntityEventKind::ProductionStarted,
                "OrbitalCommand",
                1,
                None,
            ),
            ev(
                1000,
                3,
                EntityEventKind::ProductionFinished,
                "OrbitalCommand",
                1,
                None,
            ),
        ];
        let p = player_with_events(events);
        let out = extract_player(&p, 0);
        assert_eq!(out.lanes.len(), 1);
        let lane = &out.lanes[0];
        assert_eq!(lane.canonical_type, "OrbitalCommand");
        assert_eq!(lane.born_loop, 100);
        assert_eq!(lane.died_loop, None);
        assert_eq!(lane.blocks.len(), 1);
        assert_eq!(lane.blocks[0].kind, BlockKind::Morphing);
        assert_eq!(lane.blocks[0].end_loop, 1000);
        // Morph time > 0 — start anterior ao finish.
        assert!(lane.blocks[0].start_loop < 1000);
    }

    #[test]
    fn worker_training_resolves_via_started_companion() {
        // CC nasce; SCV nasce instantaneamente em loop=472 — tracker
        // emite Started+Finished no mesmo loop. Started.creator_tag=Some(1)
        // (CC), Finished.creator_tag=None.
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "CommandCenter",
                1,
                None,
            ),
            ev(
                472,
                1,
                EntityEventKind::ProductionStarted,
                "SCV",
                10,
                Some(1),
            ),
            ev(472, 1, EntityEventKind::ProductionFinished, "SCV", 10, None),
        ];
        let p = player_with_events(events);
        let out = extract_player(&p, 0);
        let lane = &out.lanes[0];
        assert_eq!(lane.blocks.len(), 1);
        assert_eq!(lane.blocks[0].kind, BlockKind::Producing);
        // start = finish - 272 (fallback build_time).
        assert_eq!(lane.blocks[0].start_loop, 200);
        assert_eq!(lane.blocks[0].end_loop, 472);
    }

    #[test]
    fn zerg_drone_resolves_via_larva_to_hatch_map() {
        // Hatchery (tag=1), Larva nasce com Started.creator_tag=Some(1)
        // — populamos `larva_to_hatch` aí. Drone vem via apply_type_change
        // com Died(Larva) + Started(Drone, creator_tag=Some(self)) +
        // Finished(Drone). Resolução via mapa.
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Hatchery",
                1,
                None,
            ),
            ev(150, 1, EntityEventKind::ProductionStarted, "Larva", 5, Some(1)),
            ev(150, 1, EntityEventKind::ProductionFinished, "Larva", 5, None),
            ev(472, 2, EntityEventKind::Died, "Larva", 5, None),
            ev(
                472,
                2,
                EntityEventKind::ProductionStarted,
                "Drone",
                5,
                Some(5),
            ),
            ev(472, 2, EntityEventKind::ProductionFinished, "Drone", 5, None),
        ];
        let p = player_with_events(events);
        let out = extract_player(&p, 0);
        assert_eq!(out.lanes.len(), 1);
        let lane = &out.lanes[0];
        assert_eq!(lane.canonical_type, "Hatchery");
        assert_eq!(lane.blocks.len(), 1);
        assert_eq!(lane.blocks[0].kind, BlockKind::Producing);
        assert_eq!(lane.blocks[0].end_loop, 472);
    }

    #[test]
    fn probe_warp_in_falls_back_to_proximity() {
        // Probe não tem creator_tag em nenhum dos eventos (warp-in via
        // UnitInit). Mesmo assim deve casar com o Nexus mais próximo
        // pela posição.
        let mut nexus_born = ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Nexus",
            1,
            None,
        );
        nexus_born.pos_x = 50;
        nexus_born.pos_y = 50;
        let mut probe_started = ev(472, 1, EntityEventKind::ProductionStarted, "Probe", 10, None);
        probe_started.pos_x = 52;
        probe_started.pos_y = 51;
        let mut probe_finished = ev(472, 1, EntityEventKind::ProductionFinished, "Probe", 10, None);
        probe_finished.pos_x = 52;
        probe_finished.pos_y = 51;
        let events = vec![nexus_born, probe_started, probe_finished];
        let p = player_with_events(events);
        let out = extract_player(&p, 0);
        let lane = &out.lanes[0];
        assert_eq!(lane.blocks.len(), 1);
        assert_eq!(lane.blocks[0].kind, BlockKind::Producing);
    }

    #[test]
    fn townhall_died_sets_died_loop() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Nexus",
                1,
                None,
            ),
            ev(2000, 1, EntityEventKind::Died, "Nexus", 1, None),
        ];
        let p = player_with_events(events);
        let out = extract_player(&p, 0);
        assert_eq!(out.lanes[0].died_loop, Some(2000));
    }
}
