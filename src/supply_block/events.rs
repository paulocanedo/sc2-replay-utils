// Timeline de eventos consumida pelo `BlockDetector`. Traduz stats,
// entity_events e production_cmds do `PlayerTimeline` em uma sequência
// ordenada de `Event` já preparada para o state machine.

use std::collections::HashMap;

use crate::balance_data::supply_cost_x10;
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline};

/// Supply fornecido pelo Calldown Extra Supplies (SupplyDrop).
const SUPPLY_DROP_AMOUNT: i32 = 8;

/// Retorna quanto supply uma estrutura/unidade fornece ao terminar.
/// Zero significa que não é um supply provider.
/// Morphs que não alteram supply (Hatch→Lair→Hive, CC→Orbital/PF,
/// Overlord→OverlordTransport) NÃO estão nesta lista.
fn supply_provided(entity_type: &str) -> i32 {
    match entity_type {
        "SupplyDepot" => 8,
        "CommandCenter" => 15,
        "Pylon" => 8,
        "Nexus" => 15,
        "Hatchery" => 6,
        "Overlord" => 8,
        _ => 0,
    }
}

pub(super) enum Event {
    /// Atualiza o supply conhecido a partir do tracker.
    Snapshot { supply_used: i32, supply_made: i32 },
    /// Início da produção de Unit/Worker.
    ///
    /// `reserve`:
    /// - `true`  — reserva nova: incrementa `last_supply_used`
    ///   virtualmente. Usado quando `Started` e `Finished` caem em
    ///   loops distintos (caso normal: UnitInit no comando + UnitBorn
    ///   ao nascer), OU para órfãos (sem Finished).
    /// - `false` — supply já estava no snapshot anterior, NÃO
    ///   incrementa virtual. Usado para same-loop pairs (caso típico
    ///   de trains Terran e morphs que o tracker só vê pelo
    ///   `UnitBorn`, sem `UnitInit` prévio — a unidade foi construída
    ///   ao longo dos ~build_time loops anteriores e seu custo já
    ///   aparece em todos os snapshots desse intervalo).
    ///
    /// O check de `ProductionAttempt` (supply disponível < custo)
    /// roda nos dois casos, sempre no loop em que a unidade
    /// efetivamente apareceu — é o sinal mais recente que temos.
    ProductionStart { cost_x10: u32, reserve: bool },
    /// Conclusão de produção de Unit/Worker (usado por
    /// `CompletedSupplyCap` para incrementar supply concluído).
    ProductionFinish { cost_x10: u32 },
    /// Produção cancelada (unidade morreu antes de concluir) —
    /// libera o supply reservado.
    ProductionCancel { cost_x10: u32 },
    /// Conclusão de uma estrutura/unidade que fornece supply.
    SupplyReady { amount: i32 },
    /// Morte de Unit/Worker já concluída — libera supply_used.
    UnitDied { cost_x10: u32 },
    /// Warpgate entra em jogo (morph Gateway→WarpGate ou build
    /// direto). Entra pronta para warpar.
    WarpGateSpawn { tag: i64 },
    /// Warpgate destruída.
    WarpGateDied { tag: i64 },
}

impl Event {
    /// Ordenação secundária dentro do mesmo game_loop:
    ///   Snapshot (0)
    ///   → SupplyReady/UnitDied/ProductionCancel/WarpGateSpawn/
    ///     WarpGateDied (1)
    ///   → ProductionFinish (2)
    ///   → ProductionStart (3)
    /// Snapshot primeiro para sincronizar o supply;
    /// liberações e mudanças de capacidade antes de qualquer produção;
    /// ProductionFinish antes de ProductionStart porque uma unidade que
    /// termina e a próxima que começa no mesmo loop devem ser avaliadas
    /// nessa ordem.
    fn sort_order(&self) -> u8 {
        match self {
            Event::Snapshot { .. } => 0,
            Event::SupplyReady { .. }
            | Event::UnitDied { .. }
            | Event::ProductionCancel { .. }
            | Event::WarpGateSpawn { .. }
            | Event::WarpGateDied { .. } => 1,
            Event::ProductionFinish { .. } => 2,
            Event::ProductionStart { .. } => 3,
        }
    }
}

/// Constrói a timeline ordenada de eventos a partir dos stats,
/// entity events e production cmds do jogador.
///
/// Pareia `ProductionStarted` com `ProductionFinished`/`ProductionCancelled`
/// por tag. O único propósito do pareamento é detectar same-loop pairs —
/// quando Started e Finished caem no mesmo loop, a unidade não é uma
/// reserva NOVA (o tracker só a viu agora pelo `UnitBorn`, mas seu
/// supply já vinha sendo contado pelos snapshots ao longo dos últimos
/// ~build_time loops). Para esses, emitimos `reserve=false` pra não
/// duplicar o supply no virtual tracking — evita o falso positivo do
/// Colossus no Tourmaline 11:29 sem mover o loop do Start (o que
/// quebraria o check de ProductionAttempt, que depende do snapshot
/// atual pra avaliar se o supply está apertado AGORA).
///
/// Trains Terran são o caso mais comum de same-loop: a maioria vem
/// via `UnitBorn` sem `UnitInit` anterior (veja comentário em
/// `compute_series_army` sobre `prev_lifecycle == None`).
pub(super) fn build_events(player: &PlayerTimeline, base_build: u32) -> Vec<(u32, Event)> {
    let mut merged: Vec<(u32, Event)> = Vec::new();

    for s in &player.stats {
        if s.supply_used == 0 && s.supply_made == 0 {
            continue;
        }
        merged.push((
            s.game_loop,
            Event::Snapshot {
                supply_used: s.supply_used,
                supply_made: s.supply_made,
            },
        ));
    }

    let mut open_starts: HashMap<i64, (u32, String)> = HashMap::new();

    for e in &player.entity_events {
        let is_unit = matches!(e.category, EntityCategory::Unit | EntityCategory::Worker);
        let is_warpgate =
            matches!(e.category, EntityCategory::Structure) && e.entity_type == "WarpGate";
        match e.kind {
            EntityEventKind::ProductionStarted if is_unit => {
                open_starts
                    .entry(e.tag)
                    .or_insert_with(|| (e.game_loop, e.entity_type.clone()));
            }
            EntityEventKind::ProductionFinished if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if let Some((start_loop, _)) = open_starts.remove(&e.tag) {
                    // Same-loop pair → snapshot já tem o supply; reserve=false.
                    // Pair normal (Start em loop anterior) → nova reserva; reserve=true.
                    let reserve = start_loop < e.game_loop;
                    merged.push((
                        start_loop.min(e.game_loop),
                        Event::ProductionStart { cost_x10: cost, reserve },
                    ));
                }
                if cost > 0 {
                    merged.push((e.game_loop, Event::ProductionFinish { cost_x10: cost }));
                }
            }
            EntityEventKind::ProductionCancelled if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if let Some((start_loop, _)) = open_starts.remove(&e.tag) {
                    let reserve = start_loop < e.game_loop;
                    merged.push((
                        start_loop.min(e.game_loop),
                        Event::ProductionStart { cost_x10: cost, reserve },
                    ));
                }
                if cost > 0 {
                    merged.push((e.game_loop, Event::ProductionCancel { cost_x10: cost }));
                }
            }
            EntityEventKind::ProductionFinished if is_warpgate => {
                merged.push((e.game_loop, Event::WarpGateSpawn { tag: e.tag }));
            }
            EntityEventKind::ProductionFinished => {
                let amount = supply_provided(&e.entity_type);
                if amount > 0 {
                    merged.push((e.game_loop, Event::SupplyReady { amount }));
                }
            }
            EntityEventKind::Died if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if cost > 0 {
                    merged.push((e.game_loop, Event::UnitDied { cost_x10: cost }));
                }
            }
            EntityEventKind::Died if is_warpgate => {
                merged.push((e.game_loop, Event::WarpGateDied { tag: e.tag }));
            }
            _ => {}
        }
    }

    // Órfãos (Started sem Finished/Cancelled até o fim): reserva nova
    // que nunca concluiu — mantém `reserve=true` pro virtual tracking.
    for (_tag, (start_loop, entity_type)) in open_starts {
        let cost = supply_cost_x10(&entity_type, base_build);
        merged.push((
            start_loop,
            Event::ProductionStart { cost_x10: cost, reserve: true },
        ));
    }

    for cmd in &player.production_cmds {
        if cmd.ability == "SupplyDrop" {
            merged.push((
                cmd.game_loop,
                Event::SupplyReady { amount: SUPPLY_DROP_AMOUNT },
            ));
        }
    }

    merged.sort_by_key(|(loop_, e)| (*loop_, e.sort_order()));
    merged
}
