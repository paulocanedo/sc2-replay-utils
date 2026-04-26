// Extractor de "lanes" de produção por estrutura, generalizado para
// dois modos:
//
// - `LaneMode::Workers` — uma lane por townhall (Nexus / CommandCenter /
//   OrbitalCommand / PlanetaryFortress / Hatchery / Lair / Hive). Cada
//   bloco representa uma janela de produção de SCV/Probe/Drone ou um
//   morph in-place impeditivo (CC→Orbital, CC→PF). Hatch→Lair / Lair→
//   Hive não emite bloco — a estrutura continua produzindo drones
//   durante o morph.
//
// - `LaneMode::Army` — uma lane por estrutura produtora de army:
//   - Zerg: Hatchery / Lair / Hive (cada larva-born-army).
//   - Terran: Barracks / Factory / Starport. Janelas de produção de
//     unidade são blocos cheios. Adicionalmente, durante a construção
//     de um addon (Reactor/TechLab) a estrutura-mãe não pode produzir
//     — emitimos um bloco `Impeded` cobrindo essa janela.
//   - Protoss: Gateway / WarpGate (mesma tag — morph in-place),
//     RoboticsFacility, Stargate. Quando uma Gateway morpha em WarpGate,
//     setamos `warpgate_since_loop` na lane; o render distingue blocos
//     pré-WarpGate (cheios, single-track) dos blocos pós-WarpGate
//     (thin sub-tracks, estilo Hatchery).
//
// Resolução unit → producer mantém o pipeline em cascata do worker mode:
// 1. `creator_tag` no `ProductionStarted` companheiro (índice `i-1`).
// 2. Larva-born (Zerg): map `larva_tag → hatch_tag` populado quando a
//    larva nasceu.
// 3. Fallback de proximidade espacial (Probe warp-in).

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{
    is_army_producer, is_incapacitating_addon, is_larva_born_army, is_worker_name, is_zerg_hatch,
    EntityEvent, EntityEventKind, PlayerTimeline, ReplayTimeline,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneMode {
    Workers,
    Army,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    Producing,
    Morphing,
    /// Estrutura existe mas não pode produzir — Terran com addon em
    /// construção. Renderizada com cor distinta de `Producing`/`Morphing`.
    Impeded,
}

#[derive(Clone, Copy, Debug)]
pub struct ProductionBlock {
    pub start_loop: u32,
    pub end_loop: u32,
    pub kind: BlockKind,
    /// Tipo da unidade (ou addon) produzida nesta janela. `None` para
    /// blocos onde o tipo não é interessante (worker mode — o ícone à
    /// esquerda já comunica) ou desconhecido.
    pub produced_type: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct StructureLane {
    pub tag: i64,
    /// Tipo final da estrutura (após morphs).
    pub canonical_type: &'static str,
    pub born_loop: u32,
    pub died_loop: Option<u32>,
    pub pos_x: u8,
    pub pos_y: u8,
    pub blocks: Vec<ProductionBlock>,
    /// Para lanes Protoss: loop em que a Gateway virou WarpGate. Blocos
    /// com `start_loop >= warpgate_since_loop` são renderizados em
    /// estilo "thin sub-tracks" (warp-in discreto). `None` para
    /// estruturas que nunca foram WarpGate.
    pub warpgate_since_loop: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerProductionLanes {
    pub lanes: Vec<StructureLane>,
}

const CONTINUITY_TOLERANCE_LOOPS: u32 = 5;

/// Tipos de townhall (modo Workers).
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

/// Estruturas produtoras de army (modo Army). Inclui Hatch/Lair/Hive
/// como produtoras Zerg.
fn army_producer_canonical(name: &str) -> Option<&'static str> {
    match name {
        "Barracks" => Some("Barracks"),
        "Factory" => Some("Factory"),
        "Starport" => Some("Starport"),
        "Gateway" => Some("Gateway"),
        "WarpGate" => Some("WarpGate"),
        "RoboticsFacility" => Some("RoboticsFacility"),
        "Stargate" => Some("Stargate"),
        "Hatchery" => Some("Hatchery"),
        "Lair" => Some("Lair"),
        "Hive" => Some("Hive"),
        _ => None,
    }
}

fn lane_canonical(name: &str, mode: LaneMode) -> Option<&'static str> {
    match mode {
        LaneMode::Workers => townhall_canonical(name),
        LaneMode::Army => army_producer_canonical(name),
    }
}

fn is_target_unit(name: &str, mode: LaneMode, is_zerg: bool) -> bool {
    match mode {
        LaneMode::Workers => matches!(name, "SCV" | "Probe" | "Drone"),
        LaneMode::Army => {
            if is_worker_name(name) {
                return false;
            }
            if is_zerg {
                is_larva_born_army(name)
            } else {
                // Terran/Protoss: qualquer unidade não-worker, não-larva,
                // não-estrutura é candidata. O resolver de producer
                // descarta unidades que não tenham lane associada.
                !is_zerg_hatch(name) && !is_army_producer(name) && name != "Larva"
            }
        }
    }
}

/// Tempo de morph in-place em game loops. Usado para morph impeditivo
/// CC→Orbital/PF.
fn morph_build_loops(new_type: &str, base_build: u32) -> u32 {
    let from_balance = balance_data::build_time_loops(new_type, base_build);
    if from_balance > 0 {
        return from_balance;
    }
    match new_type {
        "OrbitalCommand" => 560,
        "PlanetaryFortress" => 806,
        "Lair" => 1424,
        "Hive" => 2160,
        _ => 0,
    }
}

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

fn is_morph_died(events: &[EntityEvent], i: usize) -> bool {
    let cur = &events[i];
    let Some(next) = events.get(i + 1) else {
        return false;
    };
    matches!(next.kind, EntityEventKind::ProductionStarted)
        && next.tag == cur.tag
        && next.game_loop == cur.game_loop
}

/// Captura o nome estaticamente embutido pra unidades-alvo. Como
/// `EntityEvent.entity_type` é `String`, precisamos de uma tabela de
/// nomes-com-ciclo-de-vida-`'static` para colocar em `produced_type`.
/// Cobre todas as unidades army (T/P/Z), workers e os addons Terran.
fn intern_unit_name(name: &str) -> Option<&'static str> {
    Some(match name {
        // Terran
        "SCV" => "SCV",
        "MULE" => "MULE",
        "Marine" => "Marine",
        "Marauder" => "Marauder",
        "Reaper" => "Reaper",
        "Ghost" => "Ghost",
        "Hellion" => "Hellion",
        "Hellbat" => "Hellbat",
        "WidowMine" => "WidowMine",
        "SiegeTank" => "SiegeTank",
        "Cyclone" => "Cyclone",
        "Thor" => "Thor",
        "VikingFighter" => "VikingFighter",
        "Medivac" => "Medivac",
        "Liberator" => "Liberator",
        "Banshee" => "Banshee",
        "Raven" => "Raven",
        "Battlecruiser" => "Battlecruiser",
        // Terran addons (modo Army Terran — Impeded)
        "BarracksReactor" => "BarracksReactor",
        "BarracksTechLab" => "BarracksTechLab",
        "FactoryReactor" => "FactoryReactor",
        "FactoryTechLab" => "FactoryTechLab",
        "StarportReactor" => "StarportReactor",
        "StarportTechLab" => "StarportTechLab",
        // Protoss
        "Probe" => "Probe",
        "Zealot" => "Zealot",
        "Stalker" => "Stalker",
        "Sentry" => "Sentry",
        "Adept" => "Adept",
        "HighTemplar" => "HighTemplar",
        "DarkTemplar" => "DarkTemplar",
        "Immortal" => "Immortal",
        "Colossus" => "Colossus",
        "Disruptor" => "Disruptor",
        "Observer" => "Observer",
        "WarpPrism" => "WarpPrism",
        "Phoenix" => "Phoenix",
        "VoidRay" => "VoidRay",
        "Oracle" => "Oracle",
        "Tempest" => "Tempest",
        "Carrier" => "Carrier",
        "Mothership" => "Mothership",
        // Zerg
        "Drone" => "Drone",
        "Overlord" => "Overlord",
        "Zergling" => "Zergling",
        "Queen" => "Queen",
        "Roach" => "Roach",
        "Hydralisk" => "Hydralisk",
        "Infestor" => "Infestor",
        "SwarmHost" => "SwarmHost",
        "SwarmHostMP" => "SwarmHostMP",
        "Mutalisk" => "Mutalisk",
        "Corruptor" => "Corruptor",
        "Viper" => "Viper",
        "Ultralisk" => "Ultralisk",
        _ => return None,
    })
}

fn extract_player(
    player: &PlayerTimeline,
    base_build: u32,
    mode: LaneMode,
) -> PlayerProductionLanes {
    let events = &player.entity_events;
    let mut lanes_by_tag: HashMap<i64, StructureLane> = HashMap::new();
    let mut larva_to_hatch: HashMap<i64, i64> = HashMap::new();
    // Modo Army Terran: addon_tag → (parent_tag, start_loop, name).
    // Ao ver Finished/Cancelled/Died do addon, fechamos a janela.
    let mut pending_addon: HashMap<i64, (i64, u32, &'static str)> = HashMap::new();

    let is_zerg = matches!(player.race.as_str(), "Zerg");

    for i in 0..events.len() {
        let ev = &events[i];
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                let new_type = ev.entity_type.as_str();

                // Morph in-place de estrutura — atualiza canonical_type
                // ou emite bloco Morphing impeditivo (CC→Orbital/PF).
                if let Some(new_canonical) = lane_canonical(new_type, mode) {
                    if let Some(old_type) = morph_old_type(events, i) {
                        if lane_canonical(old_type, mode).is_some() {
                            if let Some(lane) = lanes_by_tag.get_mut(&ev.tag) {
                                let is_impeditive_morph = matches!(
                                    new_canonical,
                                    "OrbitalCommand" | "PlanetaryFortress"
                                );
                                if mode == LaneMode::Workers && is_impeditive_morph {
                                    let mt = morph_build_loops(new_canonical, base_build);
                                    if mt > 0 {
                                        let start = ev.game_loop.saturating_sub(mt);
                                        lane.blocks.push(ProductionBlock {
                                            start_loop: start,
                                            end_loop: ev.game_loop,
                                            kind: BlockKind::Morphing,
                                            produced_type: None,
                                        });
                                    }
                                }
                                // Detecta Gateway → WarpGate. A pesquisa
                                // de Warpgate dispara esse morph na
                                // mesma tag, simultaneamente em todas
                                // as Gateways do jogador.
                                if new_canonical == "WarpGate" && old_type == "Gateway" {
                                    lane.warpgate_since_loop = Some(ev.game_loop);
                                }
                                lane.canonical_type = new_canonical;
                            }
                        }
                    }
                }

                // Larva nasce: registra para resolução posterior de
                // unidades larva-born (Drone em workers, ou army units
                // em Zerg).
                if new_type == "Larva" {
                    if let Some(creator) = ev.creator_tag {
                        larva_to_hatch.insert(ev.tag, creator);
                    }
                }

                // Modo Army Terran: addon começou. Abre janela.
                if mode == LaneMode::Army && is_incapacitating_addon(new_type) {
                    if let Some(parent) = ev.creator_tag {
                        if let Some(name) = intern_unit_name(new_type) {
                            pending_addon.insert(ev.tag, (parent, ev.game_loop, name));
                        }
                    }
                }
            }
            EntityEventKind::ProductionFinished => {
                let new_type = ev.entity_type.as_str();

                // Born real de uma estrutura-lane: cria a lane.
                if let Some(canonical) = lane_canonical(new_type, mode) {
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
                                warpgate_since_loop: None,
                            },
                        );
                    }
                }

                // Unidade-alvo concluída.
                if is_target_unit(new_type, mode, is_zerg) {
                    let producer_tag = resolve_producer(
                        events,
                        i,
                        new_type,
                        ev.tag,
                        ev.pos_x,
                        ev.pos_y,
                        ev.game_loop,
                        &lanes_by_tag,
                        &larva_to_hatch,
                        mode,
                    );
                    if let Some(producer) = producer_tag {
                        let build_time = balance_data::build_time_loops(new_type, base_build);
                        let build_time = if build_time > 0 { build_time } else { 272 };
                        let start_loop = ev.game_loop.saturating_sub(build_time);
                        if let Some(lane) = lanes_by_tag.get_mut(&producer) {
                            lane.blocks.push(ProductionBlock {
                                start_loop,
                                end_loop: ev.game_loop,
                                kind: BlockKind::Producing,
                                produced_type: intern_unit_name(new_type),
                            });
                        }
                    }
                }

                // Modo Army Terran: addon terminou.
                if mode == LaneMode::Army && is_incapacitating_addon(new_type) {
                    if let Some((parent, start, name)) = pending_addon.remove(&ev.tag) {
                        if let Some(lane) = lanes_by_tag.get_mut(&parent) {
                            lane.blocks.push(ProductionBlock {
                                start_loop: start,
                                end_loop: ev.game_loop,
                                kind: BlockKind::Impeded,
                                produced_type: Some(name),
                            });
                        }
                    }
                }
            }
            EntityEventKind::ProductionCancelled => {
                if mode == LaneMode::Army {
                    if let Some((parent, start, name)) = pending_addon.remove(&ev.tag) {
                        if let Some(lane) = lanes_by_tag.get_mut(&parent) {
                            lane.blocks.push(ProductionBlock {
                                start_loop: start,
                                end_loop: ev.game_loop,
                                kind: BlockKind::Impeded,
                                produced_type: Some(name),
                            });
                        }
                    }
                }
            }
            EntityEventKind::Died => {
                if !is_morph_died(events, i) {
                    if let Some(lane) = lanes_by_tag.get_mut(&ev.tag) {
                        lane.died_loop = Some(ev.game_loop);
                    }
                    // Addon morto antes de terminar: trata como cancel.
                    if mode == LaneMode::Army {
                        if let Some((parent, start, name)) = pending_addon.remove(&ev.tag) {
                            if let Some(lane) = lanes_by_tag.get_mut(&parent) {
                                lane.blocks.push(ProductionBlock {
                                    start_loop: start,
                                    end_loop: ev.game_loop,
                                    kind: BlockKind::Impeded,
                                    produced_type: Some(name),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let mut lanes: Vec<StructureLane> = lanes_by_tag.into_values().collect();
    lanes.sort_by_key(|l| (l.born_loop, l.tag));

    for lane in &mut lanes {
        lane.blocks.sort_by_key(|b| b.start_loop);
        // Em estruturas com paralelismo real (Hatch/Lair/Hive em qualquer
        // modo, ou WarpGate pós-research onde unidades chegam em rajada
        // simultânea entre múltiplas warpgates), preservamos overlaps.
        // Aqui, a lane é per-estrutura, então mesmo Hatch só tem
        // paralelismo via larvas distintas — preservamos overlap só
        // pra Zerg hatch.
        let parallel_lane = is_zerg_hatch(lane.canonical_type);
        lane.blocks = merge_continuous(std::mem::take(&mut lane.blocks), parallel_lane);
    }

    PlayerProductionLanes { lanes }
}

fn merge_continuous(
    blocks: Vec<ProductionBlock>,
    parallel_lane: bool,
) -> Vec<ProductionBlock> {
    let mut out: Vec<ProductionBlock> = Vec::with_capacity(blocks.len());
    for b in blocks {
        let mut merged = false;
        for prev in out.iter_mut().rev() {
            if prev.kind != b.kind {
                continue;
            }
            // Não mesclar blocos com produced_type diferente: preserva
            // distinção visual entre unidades sequenciais (ícone muda).
            if prev.produced_type != b.produced_type {
                break;
            }
            let overlap = b.start_loop < prev.end_loop;
            if overlap {
                if parallel_lane {
                    continue;
                }
                prev.end_loop = prev.end_loop.max(b.end_loop);
                merged = true;
                break;
            }
            if b.start_loop.saturating_sub(prev.end_loop) <= CONTINUITY_TOLERANCE_LOOPS {
                prev.end_loop = prev.end_loop.max(b.end_loop);
                merged = true;
                break;
            }
            break;
        }
        if !merged {
            out.push(b);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn resolve_producer(
    events: &[EntityEvent],
    finished_index: usize,
    unit_type: &str,
    self_tag: i64,
    pos_x: u8,
    pos_y: u8,
    finish_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
    larva_to_hatch: &HashMap<i64, i64>,
    mode: LaneMode,
) -> Option<i64> {
    // (1) Started companheiro com creator_tag.
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

    // (2) Larva-born (Zerg).
    if let Some(&hatch) = larva_to_hatch.get(&self_tag) {
        if lanes.contains_key(&hatch) {
            return Some(hatch);
        }
    }

    // (3) Fallback de proximidade.
    resolve_by_proximity(lanes, unit_type, finish_loop, pos_x, pos_y, mode)
}

fn resolve_by_proximity(
    lanes: &HashMap<i64, StructureLane>,
    unit_type: &str,
    at_loop: u32,
    x: u8,
    y: u8,
    mode: LaneMode,
) -> Option<i64> {
    let allowed: &[&str] = match (mode, unit_type) {
        (LaneMode::Workers, "SCV") => &["CommandCenter", "OrbitalCommand", "PlanetaryFortress"],
        (LaneMode::Workers, "Probe") => &["Nexus"],
        (LaneMode::Workers, "Drone") => &["Hatchery", "Lair", "Hive"],
        (LaneMode::Workers, _) => return None,
        (LaneMode::Army, _) => &[
            "Barracks",
            "Factory",
            "Starport",
            "Gateway",
            "WarpGate",
            "RoboticsFacility",
            "Stargate",
            "Hatchery",
            "Lair",
            "Hive",
        ],
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
/// de `timeline.players`.
pub fn extract(timeline: &ReplayTimeline, mode: LaneMode) -> Vec<PlayerProductionLanes> {
    timeline
        .players
        .iter()
        .map(|p| extract_player(p, timeline.base_build, mode))
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
            } else if matches!(ty, "Larva")
                || is_larva_born_army(ty)
                || matches!(
                    ty,
                    "Marine" | "Marauder" | "Reaper" | "Ghost" | "Zealot" | "Stalker"
                )
            {
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

    fn player_with_events(events: Vec<EntityEvent>, race: &str) -> PlayerTimeline {
        PlayerTimeline {
            name: "p".into(),
            clan: String::new(),
            race: race.into(),
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
    fn workers_terran_cc_morphs_to_orbital_emits_morphing_block() {
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
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Workers);
        assert_eq!(out.lanes.len(), 1);
        assert_eq!(out.lanes[0].canonical_type, "OrbitalCommand");
        assert_eq!(out.lanes[0].blocks.len(), 1);
        assert_eq!(out.lanes[0].blocks[0].kind, BlockKind::Morphing);
    }

    #[test]
    fn workers_scv_resolves_via_started_companion() {
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
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Workers);
        assert_eq!(out.lanes[0].blocks.len(), 1);
        assert_eq!(out.lanes[0].blocks[0].kind, BlockKind::Producing);
    }

    #[test]
    fn workers_zerg_drone_resolves_via_larva_to_hatch_map() {
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
        let p = player_with_events(events, "Zerg");
        let out = extract_player(&p, 0, LaneMode::Workers);
        assert_eq!(out.lanes[0].blocks.len(), 1);
    }

    #[test]
    fn army_terran_addon_construction_emits_impeded_block() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                600,
                2,
                EntityEventKind::ProductionFinished,
                "BarracksReactor",
                2,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes.len(), 1);
        let imp: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Impeded)
            .collect();
        assert_eq!(imp.len(), 1);
        assert_eq!(imp[0].start_loop, 200);
        assert_eq!(imp[0].end_loop, 600);
        assert_eq!(imp[0].produced_type, Some("BarracksReactor"));
    }

    #[test]
    fn army_terran_marine_attributed_to_barracks() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                500,
                1,
                EntityEventKind::ProductionStarted,
                "Marine",
                10,
                Some(1),
            ),
            ev(
                500,
                2,
                EntityEventKind::ProductionFinished,
                "Marine",
                10,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1);
        assert_eq!(prod[0].produced_type, Some("Marine"));
    }

    #[test]
    fn army_protoss_gateway_morphs_to_warpgate_sets_warpgate_since_loop() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Gateway",
                1,
                None,
            ),
            ev(2000, 1, EntityEventKind::Died, "Gateway", 1, None),
            ev(
                2000,
                2,
                EntityEventKind::ProductionStarted,
                "WarpGate",
                1,
                None,
            ),
            ev(
                2000,
                3,
                EntityEventKind::ProductionFinished,
                "WarpGate",
                1,
                None,
            ),
        ];
        let p = player_with_events(events, "Protoss");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes.len(), 1);
        assert_eq!(out.lanes[0].canonical_type, "WarpGate");
        assert_eq!(out.lanes[0].warpgate_since_loop, Some(2000));
    }

    #[test]
    fn army_zerg_zergling_attributed_to_hatchery_via_larva() {
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
                "Zergling",
                5,
                Some(5),
            ),
            ev(
                472,
                2,
                EntityEventKind::ProductionFinished,
                "Zergling",
                5,
                None,
            ),
        ];
        let p = player_with_events(events, "Zerg");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1);
        assert_eq!(prod[0].produced_type, Some("Zergling"));
    }

    #[test]
    fn army_terran_addon_cancelled_emits_partial_impeded() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                400,
                2,
                EntityEventKind::ProductionCancelled,
                "BarracksReactor",
                2,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let imp: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Impeded)
            .collect();
        assert_eq!(imp.len(), 1);
        assert_eq!(imp[0].end_loop, 400);
    }
}
