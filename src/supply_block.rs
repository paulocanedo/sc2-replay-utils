use std::collections::HashSet;

use crate::balance_data::supply_cost_x10;
use crate::production_efficiency::{
    is_warp_gate_unit, WARP_GATE_CYCLE_LOOPS, WARP_GATE_RESEARCH,
};
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline};

// ── Structs ───────────────────────────────────────────────────────────────────

pub struct SupplyBlockEntry {
    pub start_loop: u32,
    pub end_loop: u32,
    pub supply: i32, // supply_used no início do bloco
}

// ── Estratégia ────────────────────────────────────────────────────────────────

/// Estratégia para detectar o **início** de um supply block. O fim
/// segue a mesma lógica nas três estratégias (mortes de unidades e
/// conclusão de supply providers).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StartStrategy {
    /// Bloco inicia quando um `ProductionStarted` (Unit/Worker) ocorre
    /// e o supply disponível (`supply_made − supply_used`) é menor que
    /// o custo da unidade.
    ProductionAttempt,
    /// Bloco inicia quando o supply consumido por unidades **já
    /// concluídas** atinge a capacidade total. Não considera produção
    /// em andamento.
    CompletedSupplyCap,
    /// Bloco inicia quando o supply consumido por unidades concluídas
    /// **mais** as em produção atinge a capacidade total.
    TotalSupplyCap,
}

/// Estratégia ativa. Alterar este valor para comparar abordagens.
const ACTIVE_STRATEGY: StartStrategy = StartStrategy::ProductionAttempt;

// ── Constantes ────────────────────────────────────────────────────────────────

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

/// Supply fornecido pelo Calldown Extra Supplies (SupplyDrop).
const SUPPLY_DROP_AMOUNT: i32 = 8;

/// Custo de supply da unidade warpável mais barata (Zealot).
/// Usado como piso do gatilho warpgate-aware: se o supply disponível
/// for menor que isso e houver warpgate pronta, o jogador está
/// bloqueado mesmo sem emitir comando de warp.
const CHEAPEST_WARP_SUPPLY: i32 = 2;

// ── Detecção ──────────────────────────────────────────────────────────────────

/// Detecta períodos de supply block nos stats de um jogador.
///
/// O início do bloco depende de `ACTIVE_STRATEGY`. Para Protoss com
/// `WarpGateResearch` concluído, há um gatilho adicional: quando
/// existe pelo menos uma warpgate fora de cooldown e o supply
/// disponível é menor que o custo do Zealot (unidade warpável mais
/// barata). Isso captura situações onde o jogador nem tenta warpar
/// por estar supply-capped — caso que o gatilho `ProductionAttempt`
/// não enxergaria.
///
/// O fim acontece quando uma estrutura/unidade que fornece supply é
/// concluída (`SupplyDepot`, `Pylon`, `Overlord`, etc.), quando o
/// `SupplyDrop` do Orbital é usado, ou quando uma unidade morre
/// liberando supply.
pub fn extract_supply_blocks(
    player: &PlayerTimeline,
    game_loops: u32,
    base_build: u32,
) -> Vec<SupplyBlockEntry> {
    if player.stats.is_empty() {
        return Vec::new();
    }

    enum Event {
        /// Atualiza o supply conhecido a partir do tracker.
        Snapshot { supply_used: i32, supply_made: i32 },
        /// Início da produção de Unit/Worker (usado por
        /// `ProductionAttempt` e `TotalSupplyCap`).
        ProductionStart { cost_x10: u32 },
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
        /// Warpgate destruída. Se estava em cooldown, o contador
        /// `busy_gates` é clampado para não exceder `alive_warpgates.len()`.
        WarpGateDied { tag: i64 },
        /// Início de um warp-in (unidade warpável, após `WarpGateResearch`).
        /// Ocupa uma warpgate por `WARP_GATE_CYCLE_LOOPS`.
        WarpStart,
        /// Fim do ciclo de uma warpgate (sintético, agendado em
        /// `WarpStart.loop + WARP_GATE_CYCLE_LOOPS`).
        WarpGateReady,
    }

    // Loop em que `WarpGateResearch` ficou pronto. `None` quando o
    // jogador nunca pesquisou — desativa todo o caminho warpgate.
    let warp_research_loop: Option<u32> = player
        .upgrades
        .iter()
        .find(|u| u.name == WARP_GATE_RESEARCH)
        .map(|u| u.game_loop);

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

    for e in &player.entity_events {
        let is_unit = matches!(e.category, EntityCategory::Unit | EntityCategory::Worker);
        let is_warpgate = matches!(e.category, EntityCategory::Structure) && e.entity_type == "WarpGate";
        match e.kind {
            EntityEventKind::ProductionStarted if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                merged.push((e.game_loop, Event::ProductionStart { cost_x10: cost }));

                // Warp-in: após pesquisa concluída e unidade no roster.
                // Ocupa uma warpgate pelo ciclo completo.
                if let Some(r) = warp_research_loop {
                    if e.game_loop >= r && is_warp_gate_unit(&e.entity_type) {
                        merged.push((e.game_loop, Event::WarpStart));
                        let ready_at = e.game_loop.saturating_add(WARP_GATE_CYCLE_LOOPS).min(game_loops);
                        merged.push((ready_at, Event::WarpGateReady));
                    }
                }
            }
            EntityEventKind::ProductionFinished if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if cost > 0 {
                    merged.push((e.game_loop, Event::ProductionFinish { cost_x10: cost }));
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
            EntityEventKind::ProductionCancelled if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if cost > 0 {
                    merged.push((e.game_loop, Event::ProductionCancel { cost_x10: cost }));
                }
            }
            _ => {}
        }
    }

    for cmd in &player.production_cmds {
        if cmd.ability == "SupplyDrop" {
            merged.push((
                cmd.game_loop,
                Event::SupplyReady { amount: SUPPLY_DROP_AMOUNT },
            ));
        }
    }

    // Ordena por game_loop. Dentro do mesmo loop:
    //   Snapshot (0)
    //   → SupplyReady/UnitDied/ProductionCancel/WarpGateSpawn/
    //     WarpGateDied/WarpGateReady (1)
    //   → ProductionFinish (2)
    //   → ProductionStart/WarpStart (3)
    // Snapshot primeiro para sincronizar o supply;
    // Liberações e mudanças de capacidade antes de qualquer produção;
    // ProductionFinish antes de ProductionStart porque uma unidade que
    // termina e a próxima que começa no mesmo loop devem ser avaliadas
    // nessa ordem.
    merged.sort_by_key(|(loop_, e)| {
        let order = match e {
            Event::Snapshot { .. } => 0,
            Event::SupplyReady { .. }
            | Event::UnitDied { .. }
            | Event::ProductionCancel { .. }
            | Event::WarpGateSpawn { .. }
            | Event::WarpGateDied { .. }
            | Event::WarpGateReady => 1,
            Event::ProductionFinish { .. } => 2,
            Event::ProductionStart { .. } | Event::WarpStart => 3,
        };
        (*loop_, order)
    });

    let mut results = Vec::new();
    let mut in_block = false;
    let mut block_start_loop = 0u32;
    let mut block_supply = 0i32;
    let mut last_supply_used = 0i32;
    let mut last_supply_made = 0i32;
    // Supply consumido por unidades **concluídas**, mantido
    // independentemente dos snapshots (que incluem produção em
    // andamento). Usado pela estratégia `CompletedSupplyCap`.
    let mut completed_supply_used = 0i32;
    // Supply consumido por unidades concluídas + em produção. Usado
    // pela estratégia `TotalSupplyCap`.
    let mut total_supply_used = 0i32;

    // Estado warpgate — usado só quando `warp_research_loop.is_some()`.
    // `alive_warpgates` rastreia os tags de warpgates vivas (lifecycle
    // por tag é confiável: `ProductionFinished(WarpGate)` +
    // `Died(WarpGate)` vêm da stream normal do tracker).
    // `busy_gates` é um contador pool — não dá pra amarrar um warp-in
    // a uma warpgate específica porque `UnitInit` vem com
    // `creator_tag: None`. Aceita-se a aproximação (idem
    // `production_efficiency/models.rs`).
    let mut alive_warpgates: HashSet<i64> = HashSet::new();
    let mut busy_gates: u32 = 0;

    for (loop_, event) in &merged {
        match event {
            Event::Snapshot {
                supply_used,
                supply_made,
            } => {
                last_supply_used = *supply_used;
                last_supply_made = *supply_made;
            }
            Event::SupplyReady { amount } => {
                last_supply_made = (last_supply_made + amount).min(200);
                if in_block && supply_freed(
                    last_supply_made,
                    last_supply_used,
                    completed_supply_used,
                    total_supply_used,
                ) {
                    results.push(SupplyBlockEntry {
                        start_loop: block_start_loop,
                        end_loop: *loop_,
                        supply: block_supply,
                    });
                    in_block = false;
                }
            }
            Event::UnitDied { cost_x10 } => {
                let cost = *cost_x10 as i32 / 10;
                last_supply_used = (last_supply_used - cost).max(0);
                completed_supply_used = (completed_supply_used - cost).max(0);
                total_supply_used = (total_supply_used - cost).max(0);
                if in_block && supply_freed(
                    last_supply_made,
                    last_supply_used,
                    completed_supply_used,
                    total_supply_used,
                ) {
                    results.push(SupplyBlockEntry {
                        start_loop: block_start_loop,
                        end_loop: *loop_,
                        supply: block_supply,
                    });
                    in_block = false;
                }
            }
            Event::ProductionCancel { cost_x10 } => {
                let cost = *cost_x10 as i32 / 10;
                total_supply_used = (total_supply_used - cost).max(0);
                if in_block && supply_freed(
                    last_supply_made,
                    last_supply_used,
                    completed_supply_used,
                    total_supply_used,
                ) {
                    results.push(SupplyBlockEntry {
                        start_loop: block_start_loop,
                        end_loop: *loop_,
                        supply: block_supply,
                    });
                    in_block = false;
                }
            }
            Event::ProductionFinish { cost_x10 } => {
                completed_supply_used += *cost_x10 as i32 / 10;

                if ACTIVE_STRATEGY == StartStrategy::CompletedSupplyCap
                    && !in_block
                    && last_supply_made > 0
                    && last_supply_made < 200
                    && completed_supply_used >= last_supply_made
                {
                    in_block = true;
                    block_start_loop = *loop_;
                    block_supply = completed_supply_used;
                }
            }
            Event::ProductionStart { cost_x10 } => {
                total_supply_used += *cost_x10 as i32 / 10;

                match ACTIVE_STRATEGY {
                    StartStrategy::ProductionAttempt => {
                        if in_block {
                            continue;
                        }
                        // Ignora produção antes do primeiro snapshot de stats.
                        if last_supply_made == 0 {
                            continue;
                        }
                        let available_x10 = (last_supply_made - last_supply_used) * 10;
                        if last_supply_made < 200 && available_x10 < *cost_x10 as i32 {
                            in_block = true;
                            block_start_loop = *loop_;
                            block_supply = last_supply_used;
                        }
                    }
                    StartStrategy::TotalSupplyCap => {
                        if !in_block
                            && last_supply_made > 0
                            && last_supply_made < 200
                            && total_supply_used >= last_supply_made
                        {
                            in_block = true;
                            block_start_loop = *loop_;
                            block_supply = total_supply_used;
                        }
                    }
                    StartStrategy::CompletedSupplyCap => {}
                }
            }
            Event::WarpGateSpawn { tag } => {
                alive_warpgates.insert(*tag);
            }
            Event::WarpGateDied { tag } => {
                alive_warpgates.remove(tag);
                // Se a warpgate morreu em cooldown, `busy_gates` pode
                // passar a exceder o número de vivas. Clampa.
                busy_gates = busy_gates.min(alive_warpgates.len() as u32);
            }
            Event::WarpStart => {
                busy_gates = busy_gates.saturating_add(1);
            }
            Event::WarpGateReady => {
                busy_gates = busy_gates.saturating_sub(1);
            }
        }

        // Gatilho warpgate-aware: avaliado após cada evento. Só ativa
        // quando `WarpGateResearch` já foi pesquisado. Não duplica
        // blocos — respeita o `in_block` guard existente.
        if !in_block
            && warp_research_loop.is_some()
            && warpgate_blocked(&alive_warpgates, busy_gates, last_supply_made, last_supply_used)
        {
            in_block = true;
            block_start_loop = *loop_;
            block_supply = last_supply_used;
        }
    }

    // Bloco ainda aberto no fim.
    if in_block {
        results.push(SupplyBlockEntry {
            start_loop: block_start_loop,
            end_loop: game_loops,
            supply: block_supply,
        });
    }

    results
}

/// Verifica se há supply disponível para sair do bloco. A medida de
/// "supply usado" depende da estratégia ativa.
fn supply_freed(supply_made: i32, supply_used: i32, completed: i32, total: i32) -> bool {
    let used = match ACTIVE_STRATEGY {
        StartStrategy::ProductionAttempt => supply_used,
        StartStrategy::CompletedSupplyCap => completed,
        StartStrategy::TotalSupplyCap => total,
    };
    supply_made > used
}

/// Verifica se o jogador está supply-blocked com warpgate pronta.
/// Retorna true quando há pelo menos uma warpgate fora de cooldown e
/// o supply disponível é menor que o custo do Zealot.
fn warpgate_blocked(
    alive: &HashSet<i64>,
    busy: u32,
    supply_made: i32,
    supply_used: i32,
) -> bool {
    let ready = (alive.len() as u32).saturating_sub(busy);
    ready > 0
        && supply_made > 0
        && supply_made < 200
        && (supply_made - supply_used) < CHEAPEST_WARP_SUPPLY
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::replay::{EntityEvent, PlayerTimeline, StatsSnapshot, UpgradeEntry};

    // Helpers ────────────────────────────────────────────────────────────────

    fn mk_player() -> PlayerTimeline {
        PlayerTimeline {
            name: String::new(),
            clan: String::new(),
            race: "Prot".to_string(),
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
            alive_count: HashMap::new(),
            worker_capacity: Vec::new(),
            worker_births: Vec::new(),
            army_capacity: Vec::new(),
            worker_capacity_cumulative: Vec::new(),
            army_capacity_cumulative: Vec::new(),
            upgrade_cumulative: Vec::new(),
            creep_index: Vec::new(),
        }
    }

    fn snapshot(gl: u32, used: i32, made: i32) -> StatsSnapshot {
        StatsSnapshot {
            game_loop: gl,
            minerals: 0,
            vespene: 0,
            minerals_rate: 0,
            vespene_rate: 0,
            workers: 0,
            supply_used: used,
            supply_made: made,
            army_value_minerals: 0,
            army_value_vespene: 0,
            minerals_lost_army: 0,
            vespene_lost_army: 0,
            minerals_killed_army: 0,
            vespene_killed_army: 0,
        }
    }

    fn entity_event(
        gl: u32,
        tag: i64,
        entity_type: &str,
        kind: EntityEventKind,
        category: EntityCategory,
    ) -> EntityEvent {
        EntityEvent {
            game_loop: gl,
            seq: 0,
            tag,
            entity_type: entity_type.to_string(),
            category,
            kind,
            pos_x: 0,
            pos_y: 0,
            creator_ability: None,
            creator_tag: None,
            killer_player_id: None,
        }
    }

    fn upgrade(gl: u32, name: &str) -> UpgradeEntry {
        UpgradeEntry {
            game_loop: gl,
            seq: 0,
            name: name.to_string(),
        }
    }

    // (a) Sem WarpGateResearch: comportamento inalterado — mesmo com
    // warpgate viva e supply zerado, não dispara bloco warpgate-aware.
    #[test]
    fn no_warp_research_no_warpgate_block() {
        let mut p = mk_player();
        p.stats.push(snapshot(100, 20, 20)); // supply cheio
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        // Sem upgrades.
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert!(blocks.is_empty(), "não deve detectar bloco sem WarpGateResearch");
    }

    // (b) Warp research done + warpgate pronta + supply sobrando: não bloqueia.
    #[test]
    fn warp_ready_but_supply_ok() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        p.stats.push(snapshot(100, 10, 20)); // 10 disponível ≥ 2
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert!(blocks.is_empty(), "supply disponível (10) ≥ Zealot (2), sem bloco");
    }

    // (c) Warp research done + warpgate pronta + supply 0: bloqueia; fim no SupplyReady.
    #[test]
    fn warp_ready_supply_zero_blocks() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        p.stats.push(snapshot(100, 20, 20)); // 0 disponível < 2
        p.entity_events.push(entity_event(
            500,
            2,
            "Pylon",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert_eq!(blocks.len(), 1, "deveria detectar 1 bloco");
        assert_eq!(blocks[0].start_loop, 100, "bloco inicia no snapshot que detectou supply cap");
        assert_eq!(blocks[0].end_loop, 500, "bloco termina quando Pylon conclui");
        assert_eq!(blocks[0].supply, 20);
    }

    // (d) 2 warpgates, 1 em cooldown (warp em andamento), 1 pronta, supply 0.
    #[test]
    fn two_warpgates_one_busy_still_blocks() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        // 2 warpgates nascem cedo.
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        p.entity_events.push(entity_event(
            50,
            2,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        // Warp de um Zealot em loop 80 (uma warpgate ocupada por 560 loops).
        p.entity_events.push(entity_event(
            80,
            100,
            "Zealot",
            EntityEventKind::ProductionStarted,
            EntityCategory::Unit,
        ));
        // Supply cap em loop 100.
        p.stats.push(snapshot(100, 20, 20));
        // SupplyReady em 500 (ainda dentro do cooldown de 560 da warpgate).
        p.entity_events.push(entity_event(
            500,
            3,
            "Pylon",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert_eq!(blocks.len(), 1, "a warpgate #2 (pronta) deve disparar bloco");
        assert_eq!(blocks[0].start_loop, 100);
        assert_eq!(blocks[0].end_loop, 500);
    }

    // (e) Warpgate morre durante cooldown: busy_gates não excede alive.
    #[test]
    fn warpgate_dies_during_cooldown_no_stuck_counter() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        // 1 warpgate nasce.
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        // Warp começa no loop 80 → busy=1, cooldown até 640.
        p.entity_events.push(entity_event(
            80,
            100,
            "Zealot",
            EntityEventKind::ProductionStarted,
            EntityCategory::Unit,
        ));
        // Warpgate morre no loop 200 → alive=0; busy clampeado para 0.
        p.entity_events.push(entity_event(
            200,
            1,
            "WarpGate",
            EntityEventKind::Died,
            EntityCategory::Structure,
        ));
        // Nova warpgate nasce no loop 700 (após o "WarpGateReady" agendado
        // em 640 ter tentado decrementar).
        p.entity_events.push(entity_event(
            700,
            2,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        // Snapshot em 800 com supply cap.
        p.stats.push(snapshot(800, 20, 20));
        p.entity_events.push(entity_event(
            900,
            3,
            "Pylon",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert_eq!(blocks.len(), 1, "nova warpgate deve estar pronta — contador não travado");
        assert_eq!(blocks[0].start_loop, 800);
    }

    // (f) ProductionAttempt + warpgate-ready sobrepostos: bloco não duplica.
    #[test]
    fn no_duplicate_block_from_overlapping_triggers() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        p.stats.push(snapshot(100, 20, 20)); // supply cap: dispara warpgate-aware
        // ProductionAttempt também dispararia aqui, mas o `in_block` guard impede.
        p.entity_events.push(entity_event(
            150,
            10,
            "Stalker",
            EntityEventKind::ProductionStarted,
            EntityCategory::Unit,
        ));
        p.entity_events.push(entity_event(
            500,
            2,
            "Pylon",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert_eq!(blocks.len(), 1, "blocos sobrepostos devem resultar em um único bloco");
    }
}
