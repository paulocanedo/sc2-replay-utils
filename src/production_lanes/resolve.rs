//! Resolução `unit → producer` (cascade de três etapas) e merge de
//! blocos contíguos. Lógica genérica usada para todas as raças.

use std::collections::HashMap;

use crate::replay::{EntityEvent, EntityEventKind, ProductionCmd};

use super::types::{LaneMode, ProductionBlock, StructureLane, CONTINUITY_TOLERANCE_LOOPS};

/// Procura o primeiro cmd não-consumido emitido pelo `producer_tag`
/// cuja `ability` bate com `action` E cujo `game_loop` satisfaz a
/// constraint de causalidade `cmd_loop <= max_cmd_loop`. Idêntico ao
/// helper homônimo em `build_order::extract` — manter as duas
/// pipelines com a mesma lógica de pareamento garante que o gráfico de
/// produção e a aba de build order mostrem o mesmo conjunto de eventos
/// pareados aos mesmos cmds.
pub(super) fn consume_producer_cmd(
    by_producer: &HashMap<i64, Vec<usize>>,
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    producer_tag: i64,
    action: &str,
    max_cmd_loop: u32,
) -> Option<u32> {
    let queue = by_producer.get(&producer_tag)?;
    for &i in queue {
        if consumed[i] {
            continue;
        }
        if cmds[i].ability != action {
            continue;
        }
        if cmds[i].game_loop > max_cmd_loop {
            break;
        }
        consumed[i] = true;
        return Some(cmds[i].game_loop);
    }
    None
}

pub(super) fn merge_continuous(
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
pub(super) fn resolve_producer(
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
