// Modelos de eficiência de produção por raça/target.

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline};

use super::sweep::sweep;
use super::types::*;

pub(super) fn compute_series_workers(
    player: &PlayerTimeline,
    game_end: u32,
    bucket_loops: u32,
) -> Vec<EfficiencySample> {
    let mut evs: Vec<(u32, EvKind)> = Vec::new();
    for &(gl, delta) in &player.worker_capacity {
        evs.push((gl, if delta > 0 { EvKind::CapacityUp } else { EvKind::CapacityDown }));
    }
    for &birth in &player.worker_births {
        let start = birth.saturating_sub(WORKER_BUILD_TIME);
        evs.push((start, EvKind::ProdStart));
        evs.push((birth, EvKind::ProdEnd));
    }
    sweep(evs, game_end, bucket_loops)
}

pub(super) fn compute_series_army(
    player: &PlayerTimeline,
    game_end: u32,
    base_build: u32,
    bucket_loops: u32,
) -> Vec<EfficiencySample> {
    let mut evs: Vec<(u32, EvKind)> = Vec::new();
    for &(gl, delta) in &player.army_capacity {
        evs.push((gl, if delta > 0 { EvKind::CapacityUp } else { EvKind::CapacityDown }));
    }

    // Loop em que `WarpGateResearch` ficou pronto para esse jogador
    // (ou None se nunca pesquisou / jogador não é Protoss). A partir
    // desse ponto, warps de unidades do `WARP_GATE_UNITS` passam a
    // ser tratados com janela estendida (warp-in + cooldown) em vez
    // da janela curta só do warp-in.
    let warp_research_loop: Option<u32> = player
        .upgrades
        .iter()
        .find(|u| u.name == WARP_GATE_RESEARCH)
        .map(|u| u.game_loop);

    // Pareia ProductionStarted com ProductionFinished/Cancelled por tag,
    // filtrando `category == Unit`. Guardamos (start_loop, entity_type)
    // para back-datar o start quando o tracker emitiu ambos os eventos
    // no mesmo loop (caso típico dos trains Terran vindos via UnitBorn
    // sem UnitInit anterior — veja `tracker::prev_lifecycle == None`).
    // Eventos sem par (órfãos) fecham no game_end.
    let mut starts: HashMap<i64, (u32, String)> = HashMap::new();
    for ev in &player.entity_events {
        if ev.category != EntityCategory::Unit {
            continue;
        }
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                // Aceita apenas o primeiro Started por tag — evita que
                // um eventual re-emit duplique o slot.
                starts
                    .entry(ev.tag)
                    .or_insert_with(|| (ev.game_loop, ev.entity_type.clone()));
            }
            EntityEventKind::ProductionFinished | EntityEventKind::ProductionCancelled => {
                if let Some((start_loop, entity_type)) = starts.remove(&ev.tag) {
                    let start_loop = if start_loop >= ev.game_loop {
                        // Started e Finished/Cancelled no mesmo loop:
                        // back-data pelo build_time do balance data,
                        // análogo ao `WORKER_BUILD_TIME` aplicado aos
                        // `worker_births`. Sem back-data, a produção
                        // apareceria como instantânea (active +1 -1 no
                        // mesmo tick) e a eficiência ficaria zerada.
                        let bt = balance_data::build_time_loops(&entity_type, base_build);
                        ev.game_loop.saturating_sub(bt)
                    } else {
                        start_loop
                    };
                    // Detecção de warp-in: pesquisa de WarpGate já
                    // completou E a unidade está no roster de warpáveis
                    // E o start (após eventual back-date) é posterior à
                    // pesquisa. Nesse caso o "fim da produção" não é o
                    // `ProductionFinished` do replay (que sinaliza só o
                    // término da animação de warp-in, ~5s), mas sim o
                    // final do cooldown da WarpGate — modelado como um
                    // ciclo fixo de `WARP_GATE_CYCLE_LOOPS` a partir do
                    // start. Enquanto esse slot estiver "ativo", a
                    // estrutura não pode warpar outra unidade, então é
                    // tratada como ocupada (não idle).
                    let is_warp_in = warp_research_loop
                        .map(|r| start_loop >= r && is_warp_gate_unit(&entity_type))
                        .unwrap_or(false);
                    let end_loop = if is_warp_in {
                        (start_loop + WARP_GATE_CYCLE_LOOPS).min(game_end)
                    } else {
                        ev.game_loop
                    };
                    evs.push((start_loop, EvKind::ProdStart));
                    evs.push((end_loop, EvKind::ProdEnd));
                }
            }
            EntityEventKind::Died => {}
        }
    }
    // Órfãos (sem finish/cancel): fechar em game_end.
    for (_tag, (start_loop, _entity_type)) in starts {
        evs.push((start_loop, EvKind::ProdStart));
        evs.push((game_end, EvKind::ProdEnd));
    }

    // Emite transições de supply para forçar 100% quando o jogador
    // está "supply maxed". Derivado dos `stats` snapshots (amostragem
    // ~10 ticks/min). Entre snapshots, o `supply_used` é assumido
    // constante no valor do último snapshot — aproximação adequada
    // dado que os buckets do gráfico são de 10s.
    let mut supply_was_high = false;
    for s in &player.stats {
        let now_high = s.supply_used > ARMY_SUPPLY_MAXED_THRESHOLD;
        if now_high != supply_was_high {
            evs.push((
                s.game_loop,
                if now_high { EvKind::SupplyMaxedOn } else { EvKind::SupplyMaxedOff },
            ));
            supply_was_high = now_high;
        }
    }

    sweep(evs, game_end, bucket_loops)
}

/// Série temporal de eficiência de produção **Zerg**. Modelo "larva
/// bandwidth":
///
/// - Cada Hatchery/Lair/Hive viva contribui com **1 slot** de
///   capacidade (limite sustentável pela regen de larva, não o cap
///   visual de 3). Alinha a escala com Terran/Protoss.
/// - Cada Inject Larva ativo adiciona **+4 slots temporários** por
///   `INJECT_WINDOW_LOOPS` (~29s). Injetar sem spend cai a
///   eficiência — comportamento desejado.
/// - `active` conta os morphs em curso do target (Drone ou unidades
///   larva-born). Queens, Banelings, Ravagers, Lurkers, BroodLords
///   e Overseers ficam fora (não consomem slot de larva).
/// - `apply_supply_override = true` para Army (reusa o comportamento
///   "supply maxed → 100%" dos outros races); `false` para Workers
///   (Zerg com supply cheio ainda deveria gastar larva em Overlord).
pub(super) fn compute_series_zerg(
    player: &PlayerTimeline,
    game_end: u32,
    bucket_loops: u32,
    base_build: u32,
    is_target_unit: fn(&str) -> bool,
    apply_supply_override: bool,
) -> Vec<EfficiencySample> {
    let mut evs: Vec<(u32, EvKind)> = Vec::new();

    // 1. Capacity base (+1 por hatch vivo). Derivado on-the-fly de
    //    `entity_events` — não há cache persistido de hatch_capacity
    //    (simétrico ao tratamento de army capacity no módulo).
    //    Morph Hatchery→Lair→Hive: `apply_type_change` emite
    //    `Died(old)` + `ProductionStarted(new)` + `ProductionFinished(new)`
    //    no mesmo loop; ouvimos só `Died` e `ProductionFinished`, net
    //    = -1 + 1 = 0 na capacity.
    for ev in &player.entity_events {
        if ev.category != EntityCategory::Structure {
            continue;
        }
        if !crate::replay::is_zerg_hatch(&ev.entity_type) {
            continue;
        }
        match ev.kind {
            EntityEventKind::ProductionFinished => {
                evs.push((ev.game_loop, EvKind::CapacityUp));
            }
            EntityEventKind::Died => {
                evs.push((ev.game_loop, EvKind::CapacityDown));
            }
            _ => {}
        }
    }

    // 2. Inject boost (+4 por inject ativo, por `INJECT_WINDOW_LOOPS`).
    //    Injects sobrepostos (Hatches diferentes, ou múltiplas Queens
    //    no mesmo hatch em loops próximos) somam no pool global —
    //    aproximação adequada dado o bucket de 10s.
    for cmd in &player.inject_cmds {
        evs.push((cmd.game_loop, EvKind::InjectOn));
        let end = cmd.game_loop.saturating_add(INJECT_WINDOW_LOOPS).min(game_end);
        evs.push((end, EvKind::InjectOff));
    }

    // 3. Active (±1 por morph do target em curso). Mesma lógica de
    //    pareamento `ProductionStarted`/`ProductionFinished`/
    //    `ProductionCancelled` por tag usada em `compute_series_army`.
    let mut starts: HashMap<i64, (u32, String)> = HashMap::new();
    for ev in &player.entity_events {
        // Drone é EntityCategory::Worker; unidades larva-born são
        // EntityCategory::Unit. Aceita ambas.
        if !matches!(
            ev.category,
            EntityCategory::Unit | EntityCategory::Worker
        ) {
            continue;
        }
        if !is_target_unit(&ev.entity_type) {
            continue;
        }
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                starts
                    .entry(ev.tag)
                    .or_insert_with(|| (ev.game_loop, ev.entity_type.clone()));
            }
            EntityEventKind::ProductionFinished | EntityEventKind::ProductionCancelled => {
                if let Some((start_loop, entity_type)) = starts.remove(&ev.tag) {
                    let start_loop = if start_loop >= ev.game_loop {
                        // Started e Finished no mesmo loop — back-data
                        // pelo build_time conhecido. Drone usa
                        // `DRONE_BUILD_LOOPS` direto (constante); resto
                        // consulta balance data.
                        let bt = if entity_type == "Drone" {
                            DRONE_BUILD_LOOPS
                        } else {
                            balance_data::build_time_loops(&entity_type, base_build)
                        };
                        ev.game_loop.saturating_sub(bt)
                    } else {
                        start_loop
                    };
                    evs.push((start_loop, EvKind::ProdStart));
                    evs.push((ev.game_loop, EvKind::ProdEnd));
                }
            }
            EntityEventKind::Died => {}
        }
    }
    // Órfãos (Started sem Finished/Cancelled): fechar em `game_end`.
    for (_tag, (start_loop, _entity_type)) in starts {
        evs.push((start_loop, EvKind::ProdStart));
        evs.push((game_end, EvKind::ProdEnd));
    }

    // 4. Supply-maxed override — só para Army. Workers ignoram
    //    porque um Zerg perto do cap ainda deveria estar gastando
    //    larva em Overlord para abrir supply.
    if apply_supply_override {
        let mut supply_was_high = false;
        for s in &player.stats {
            let now_high = s.supply_used > ARMY_SUPPLY_MAXED_THRESHOLD;
            if now_high != supply_was_high {
                evs.push((
                    s.game_loop,
                    if now_high {
                        EvKind::SupplyMaxedOn
                    } else {
                        EvKind::SupplyMaxedOff
                    },
                ));
                supply_was_high = now_high;
            }
        }
    }

    sweep(evs, game_end, bucket_loops)
}
