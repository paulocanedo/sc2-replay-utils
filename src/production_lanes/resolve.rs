//! Resolução `unit → producer` (cascade de três etapas) e merge de
//! blocos contíguos. Lógica genérica usada para todas as raças.

use std::collections::HashMap;

use crate::replay::{EntityEvent, EntityEventKind, ProductionCmd};

use super::types::{LaneMode, ProductionBlock, StructureLane, CONTINUITY_TOLERANCE_LOOPS};

/// Procura o cmd não-consumido emitido pelo `producer_tag` cuja
/// `ability` bate com `action` E cujo `game_loop` satisfaz a constraint
/// de causalidade `cmd_loop <= max_cmd_loop`. Quando há múltiplos
/// candidatos, escolhe o **mais recente** (maior `game_loop`) — é o
/// que tem maior probabilidade de ter realmente produzido a unidade
/// concluída em `finish_loop`. Cmds "fantasma" mais antigos (cliques
/// cancelados, double-clicks, queue cheia) ficam não-consumidos e não
/// poluem a atribuição de produções subsequentes.
///
/// **Nota**: a versão homônima em `build_order::extract` faz FIFO
/// (primeiro cmd válido). As duas pipelines não compartilham este
/// helper deliberadamente — o build_order mostra o **clique** do
/// jogador (semântica de input do player), enquanto o
/// `production_lanes` mostra o **trabalho efetivo** da estrutura
/// (semântica de timing visual). Cmds fantasma representados em
/// build_order não devem mover blocos do gráfico para o passado.
pub(super) fn consume_producer_cmd(
    by_producer: &HashMap<i64, Vec<usize>>,
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    producer_tag: i64,
    action: &str,
    max_cmd_loop: u32,
) -> Option<u32> {
    let queue = by_producer.get(&producer_tag)?;
    // Queue está em ordem cronológica (insertion order via iteração
    // sequencial sobre `production_cmds`, que já vem ordenado por
    // game_loop). Iteramos até bater no primeiro cmd > max_cmd_loop
    // e retornamos o ÚLTIMO válido visto até lá — filtra phantoms
    // (cliques antigos cancelados ou dobrados que ficariam à frente
    // da fila e roubariam o cmd da unidade real).
    let mut last_valid: Option<usize> = None;
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
        last_valid = Some(i);
    }
    if let Some(i) = last_valid {
        consumed[i] = true;
        return Some(cmds[i].game_loop);
    }
    None
}

/// Como `consume_global_cmd` do `build_order::extract`, mas devolve
/// também o primeiro `producer_tag` do cmd (via `Vec::first`) — modos
/// Research/Upgrades roteiam o bloco para a lane do produtor.
///
/// Match global por nome de ação (sem filtrar por produtor) — pesquisas
/// não enfileiram, então o primeiro cmd disponível com a ability certa
/// que respeite a constraint de causalidade `cmd_loop <= max_cmd_loop`
/// é o match correto. FIFO em vez de last-valid: pesquisas one-shot e
/// progressões de níveis são raramente canceladas, e cmds-fantasma
/// "esquecidos" no início do replay não disputam slots porque a
/// constraint causal os filtra naturalmente.
pub(super) fn consume_global_cmd_with_producer(
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    action: &str,
    max_cmd_loop: u32,
) -> Option<(u32, Option<i64>)> {
    for (i, cmd) in cmds.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        if cmd.ability != action {
            continue;
        }
        if cmd.game_loop > max_cmd_loop {
            continue;
        }
        consumed[i] = true;
        return Some((cmd.game_loop, cmd.producer_tags.first().copied()));
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
            // Não mesclar blocos com sub_track diferente: a 2ª unidade
            // do par paralelo (sub_track=1) precisa ficar como bloco
            // distinto pra ser pintada na metade inferior. Sem este
            // guarda os 2 blocos do par mesclam num único e o efeito
            // visual de duas faixas paralelas se perde.
            if prev.sub_track != b.sub_track {
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
        // Research/Upgrades não usam proximidade — atribuição é direta
        // via `producer_tag` do cmd. `resolve_producer` não é chamado
        // nesses modos, mas mantemos exaustividade do match.
        (LaneMode::Research | LaneMode::Upgrades, _) => return None,
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
