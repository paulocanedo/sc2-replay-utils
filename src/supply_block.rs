use crate::balance_data::supply_cost_x10;
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

// ── Detecção ──────────────────────────────────────────────────────────────────

/// Detecta períodos de supply block nos stats de um jogador.
///
/// O início do bloco depende de `ACTIVE_STRATEGY`. O fim acontece
/// quando uma estrutura/unidade que fornece supply é concluída
/// (`SupplyDepot`, `Pylon`, `Overlord`, etc.), quando o `SupplyDrop`
/// do Orbital é usado, ou quando uma unidade morre liberando supply.
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
    }

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
        match e.kind {
            EntityEventKind::ProductionStarted if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                merged.push((e.game_loop, Event::ProductionStart { cost_x10: cost }));
            }
            EntityEventKind::ProductionFinished if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if cost > 0 {
                    merged.push((e.game_loop, Event::ProductionFinish { cost_x10: cost }));
                }
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
    //   Snapshot (0) → SupplyReady/UnitDied (1) → ProductionFinish (2) → ProductionStart (3)
    // Snapshot primeiro para sincronizar o supply;
    // SupplyReady/UnitDied antes de qualquer produção para liberar supply;
    // ProductionFinish antes de ProductionStart porque uma unidade que
    // termina e a próxima que começa no mesmo loop devem ser avaliadas
    // nessa ordem.
    merged.sort_by_key(|(loop_, e)| {
        let order = match e {
            Event::Snapshot { .. } => 0,
            Event::SupplyReady { .. }
            | Event::UnitDied { .. }
            | Event::ProductionCancel { .. } => 1,
            Event::ProductionFinish { .. } => 2,
            Event::ProductionStart { .. } => 3,
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
