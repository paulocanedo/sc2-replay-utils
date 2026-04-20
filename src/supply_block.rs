use std::collections::{HashMap, HashSet};

use crate::balance_data::{build_time_loops, supply_cost_x10};
use crate::production_efficiency::WARP_GATE_RESEARCH;
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
        /// Warpgate destruída.
        WarpGateDied { tag: i64 },
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

    // Pareia `ProductionStarted` com `ProductionFinished`/`ProductionCancelled`
    // por tag. Quando a mesma tag recebe Started e Finished no mesmo loop
    // (caso típico quando o tracker só vê a unidade pelo `UnitBorn` — sem
    // `UnitInit` prévio), back-data o start pelo `build_time` da unidade.
    //
    // Por que back-data: sem ele, a produção apareceria como instantânea e
    // o virtual-tracking de supply somaria o custo DEPOIS do snapshot que
    // já contava essa unidade — drift que disparava supply blocks falsos
    // (ex: Colossus no Tourmaline LE 21 em 11:29, construído há ~600 loops
    // mas visto pelo tracker só em loop 15451). Com back-date, o Start
    // acontece antes do snapshot, que resyncha o virtual no loop certo.
    //
    // Mesma lógica usada em `compute_series_army`.
    let mut open_starts: HashMap<i64, (u32, String)> = HashMap::new();

    for e in &player.entity_events {
        let is_unit = matches!(e.category, EntityCategory::Unit | EntityCategory::Worker);
        let is_warpgate = matches!(e.category, EntityCategory::Structure) && e.entity_type == "WarpGate";
        match e.kind {
            EntityEventKind::ProductionStarted if is_unit => {
                open_starts
                    .entry(e.tag)
                    .or_insert_with(|| (e.game_loop, e.entity_type.clone()));
            }
            EntityEventKind::ProductionFinished if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if let Some((start_loop, entity_type)) = open_starts.remove(&e.tag) {
                    let start_loop = if start_loop >= e.game_loop {
                        let bt = build_time_loops(&entity_type, base_build);
                        e.game_loop.saturating_sub(bt)
                    } else {
                        start_loop
                    };
                    merged.push((start_loop, Event::ProductionStart { cost_x10: cost }));
                }
                if cost > 0 {
                    merged.push((e.game_loop, Event::ProductionFinish { cost_x10: cost }));
                }
            }
            EntityEventKind::ProductionCancelled if is_unit => {
                let cost = supply_cost_x10(&e.entity_type, base_build);
                if let Some((start_loop, entity_type)) = open_starts.remove(&e.tag) {
                    let start_loop = if start_loop >= e.game_loop {
                        let bt = build_time_loops(&entity_type, base_build);
                        e.game_loop.saturating_sub(bt)
                    } else {
                        start_loop
                    };
                    merged.push((start_loop, Event::ProductionStart { cost_x10: cost }));
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

    // Órfãos (Started sem Finished/Cancelled até o fim): emite o Start
    // no loop original. O virtual-tracking reserva o supply e permanece
    // reservado até o game_end — coerente com a semântica de bloco aberto
    // ao longo do resto da partida caso a produção nunca complete.
    for (_tag, (start_loop, entity_type)) in open_starts {
        let cost = supply_cost_x10(&entity_type, base_build);
        merged.push((start_loop, Event::ProductionStart { cost_x10: cost }));
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
    //     WarpGateDied (1)
    //   → ProductionFinish (2)
    //   → ProductionStart (3)
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
            | Event::WarpGateDied { .. } => 1,
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

    // Estado warpgate — usado só quando `warp_research_loop.is_some()`.
    // Rastreamos apenas o set de tags vivas; não modelamos cooldowns
    // individuais. A semântica do gatilho warpgate-aware é: "jogador
    // Protoss em modo warpgate está supply-capped (não cobre Zealot)".
    // Supply cap É supply cap independente de qual gate está ocupada
    // no microinstante — gates que estão em cooldown logo estarão
    // prontas e o jogador perde o warp ali também.
    let mut alive_warpgates: HashSet<i64> = HashSet::new();

    for (loop_, event) in &merged {
        match event {
            Event::Snapshot {
                supply_used,
                supply_made,
            } => {
                last_supply_used = *supply_used;
                last_supply_made = *supply_made;
                // Safety net: virtual tracking pode ter drift acumulado
                // (p.ex. se uma unidade morreu sem `Died` capturado e o
                // bloco abriu por esse motivo). O snapshot é autoridade
                // do tracker; se ele mostra supply livre, fechamos.
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
                // Virtual tracking: libera o supply reservado no
                // `ProductionStart` correspondente. Resync com snapshot
                // no próximo tick.
                last_supply_used = (last_supply_used - cost).max(0);
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
                        // Checa ANTES de atualizar `last_supply_used`,
                        // porque a pergunta é "o jogador conseguiu
                        // iniciar esta produção?". Uma produção com
                        // custo exatamente igual ao supply disponível
                        // (`avail == cost`) passa — é warpada/treinada
                        // com sucesso.
                        if !in_block
                            && last_supply_made > 0
                            && last_supply_made < 200
                        {
                            let available_x10 = (last_supply_made - last_supply_used) * 10;
                            if available_x10 < *cost_x10 as i32 {
                                in_block = true;
                                block_start_loop = *loop_;
                                block_supply = last_supply_used;
                            }
                        }

                        // Virtual tracking: reserva o supply desta
                        // produção para que eventos subsequentes entre
                        // snapshots (warpgate-aware, outro
                        // ProductionStart) vejam o estado correto. O
                        // próximo snapshot resyncará com o valor do
                        // tracker (que também inclui reservas).
                        last_supply_used += *cost_x10 as i32 / 10;
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
            }
        }

        // Gatilho warpgate-aware: avaliado após cada evento. Só ativa
        // quando `WarpGateResearch` já foi pesquisado e há pelo menos
        // uma warpgate viva. Não duplica blocos — respeita o
        // `in_block` guard. Usa `last_supply_used` virtual (atualizado
        // em ProductionStart/UnitDied/ProductionCancel), então capta
        // caps que ocorrem entre snapshots — caso típico do replay
        // Tourmaline onde o Sentry warp consumia a última fatia de
        // supply e só víamos o cap ~4s depois no próximo snapshot.
        if !in_block
            && warp_research_loop.is_some()
            && warpgate_blocked(&alive_warpgates, last_supply_made, last_supply_used)
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

/// Verifica se o jogador está supply-blocked em modo warpgate.
/// Retorna true quando há pelo menos uma warpgate viva e o supply
/// disponível é menor que o custo do Zealot. Não olha cooldowns
/// individuais — a presença de qualquer warpgate viva + supply cap
/// já caracteriza o bloqueio (o cooldown é resolvido em segundos e
/// o jogador perde o warp quando ela ficar pronta).
fn warpgate_blocked(
    alive: &HashSet<i64>,
    supply_made: i32,
    supply_used: i32,
) -> bool {
    !alive.is_empty()
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

    // (e) Warpgate morre, nova nasce depois: bloco só dispara quando
    // uma warpgate volta a existir. Entre a morte e o renascimento
    // (alive=0) o supply pode estar capado mas não é bloqueio em modo
    // warpgate — não há estrutura pra ficar ociosa.
    #[test]
    fn block_only_while_warpgate_alive() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        p.entity_events.push(entity_event(
            200,
            1,
            "WarpGate",
            EntityEventKind::Died,
            EntityCategory::Structure,
        ));
        // Supply cap enquanto não há warpgate viva (entre 200 e 700).
        p.stats.push(snapshot(300, 20, 20));
        // Nova warpgate nasce — nesse instante há alive=1 e supply capado,
        // então o gatilho warpgate-aware dispara exatamente em 700.
        p.entity_events.push(entity_event(
            700,
            2,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        p.entity_events.push(entity_event(
            900,
            3,
            "Pylon",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_loop, 700, "dispara no instante em que a warpgate renasce");
        assert_eq!(blocks[0].end_loop, 900);
    }

    // (g) Virtual supply tracking: ProductionStart entre snapshots
    // atualiza o supply_used virtual, disparando o bloco warpgate-aware
    // imediatamente quando a produção consome a última fatia disponível.
    // Reproduz o cenário Tourmaline: snapshot mostra avail=2, um warp-in
    // de custo 2 acontece, e o próximo snapshot só chega ~10s depois.
    #[test]
    fn virtual_supply_tracking_catches_cap_between_snapshots() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        // Snapshot em 100: used=68, made=70 (avail=2 — ainda dá pra warpar).
        p.stats.push(snapshot(100, 68, 70));
        // Sentry warp em 200: custo=2, consome a última fatia. Sem virtual
        // tracking, o parser só detectaria o cap no snapshot seguinte.
        p.entity_events.push(entity_event(
            200,
            999,
            "Sentry",
            EntityEventKind::ProductionStarted,
            EntityCategory::Unit,
        ));
        // Próximo snapshot muito depois (mimica cadência ~10s).
        p.stats.push(snapshot(500, 70, 70));
        p.entity_events.push(entity_event(
            600,
            2,
            "Pylon",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert_eq!(blocks.len(), 1, "virtual tracking deve detectar o cap em 200, não 500");
        assert_eq!(
            blocks[0].start_loop, 200,
            "bloco inicia no warp do Sentry (virtual_used atualizado in-flight)"
        );
        assert_eq!(blocks[0].end_loop, 600);
    }

    // (h) Back-date de same-loop Start+Finish: regressão do falso
    // positivo de 22s no Tourmaline LE (21) em 11:29. Um Colossus
    // com Started e Finished no mesmo loop (tracker só viu pelo
    // UnitBorn — sem UnitInit anterior) não deve disparar bloco,
    // porque o snapshot anterior já contabilizou seu supply. Sem
    // back-date, o virtual tracking somaria 6 de supply "fantasma"
    // e o gatilho warpgate-aware dispararia com drift.
    #[test]
    fn same_loop_start_finish_is_back_dated() {
        let mut p = mk_player();
        p.upgrades.push(upgrade(40, WARP_GATE_RESEARCH));
        p.entity_events.push(entity_event(
            50,
            1,
            "WarpGate",
            EntityEventKind::ProductionFinished,
            EntityCategory::Structure,
        ));
        // Snapshot em 1000: used=188, made=195 (avail=7 — fica de boa
        // pra um Colossus custo 6). O snapshot já inclui o Colossus
        // que está sendo construído há ~600 loops.
        p.stats.push(snapshot(1000, 188, 195));
        // Colossus com Started e Finished no MESMO loop (1100). Sem
        // back-date, o virtual somaria +6 → 194, avail=1<2 → bloco
        // falso positivo warpgate-aware.
        p.entity_events.push(entity_event(
            1100,
            999,
            "Colossus",
            EntityEventKind::ProductionStarted,
            EntityCategory::Unit,
        ));
        p.entity_events.push(entity_event(
            1100,
            999,
            "Colossus",
            EntityEventKind::ProductionFinished,
            EntityCategory::Unit,
        ));
        p.stats.push(snapshot(1500, 190, 195));
        let blocks = extract_supply_blocks(&p, 10_000, 80000);
        assert!(
            blocks.is_empty(),
            "same-loop Start+Finish deve ser back-dated e não disparar bloco; blocos={:?}",
            blocks.iter().map(|b| (b.start_loop, b.end_loop)).collect::<Vec<_>>()
        );
    }

    // Diagnóstico do replay Tourmaline LE (21) — dump da janela 5:10-5:16.
    // Rodar com: cargo test --release -- dump_tourmaline_supply_block --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_tourmaline_supply_block() {
        use crate::production_efficiency::{is_warp_gate_unit, WARP_GATE_CYCLE_LOOPS};
        use crate::replay::parse_replay;
        use std::path::Path;

        let path = std::env::var("HOME").unwrap_or_else(|_| std::env::var("USERPROFILE").unwrap());
        let replay = format!("{}/Downloads/Tourmaline LE (21).SC2Replay", path);
        let tl = parse_replay(Path::new(&replay), 0).expect("parse");

        let lps = tl.loops_per_second;
        let loop_of = |s: f64| (s * lps).round() as u32;
        // Janela padrão: 5:05-5:20 (cenário original). Pode ser
        // sobrescrita via env `WIN="<start_s>,<end_s>"` — útil pra
        // investigar outros blocos (p.ex. WIN="680,720" olha 11:20-12:00).
        let (win_start, win_end) = std::env::var("WIN")
            .ok()
            .and_then(|v| {
                let mut parts = v.split(',');
                let s = parts.next()?.parse::<f64>().ok()?;
                let e = parts.next()?.parse::<f64>().ok()?;
                Some((loop_of(s), loop_of(e)))
            })
            .unwrap_or_else(|| (loop_of(305.0), loop_of(320.0)));

        eprintln!("== Replay: {} (loops/s = {}) ==", replay, lps);
        eprintln!("  game_loops={}, base_build={}", tl.game_loops, tl.base_build);
        eprintln!("  janela: [{}, {}] loops ({}s-{}s)", win_start, win_end, 305, 320);

        for (idx, p) in tl.players.iter().enumerate() {
            if !p.race.starts_with('P') && !p.race.starts_with('p') {
                continue;
            }
            eprintln!("\n-- Player {} ({}) race={} --", idx, p.name, p.race);

            let has_wgr = p.upgrades.iter().find(|u| u.name == WARP_GATE_RESEARCH);
            eprintln!(
                "  WarpGateResearch: {:?}",
                has_wgr.map(|u| (u.game_loop, u.game_loop as f64 / lps))
            );

            eprintln!("  Stats na janela:");
            for s in &p.stats {
                if s.game_loop >= win_start && s.game_loop <= win_end {
                    eprintln!(
                        "    loop={:>5} ({:>5.1}s) used={:>3} made={:>3} avail={}",
                        s.game_loop,
                        s.game_loop as f64 / lps,
                        s.supply_used,
                        s.supply_made,
                        s.supply_made - s.supply_used
                    );
                }
            }

            eprintln!("  Entity events na janela (Unit/Worker + Pylon/WarpGate):");
            for e in &p.entity_events {
                if e.game_loop < win_start || e.game_loop > win_end {
                    continue;
                }
                let is_unit = matches!(e.category, EntityCategory::Unit | EntityCategory::Worker);
                let relevant = is_unit || e.entity_type == "WarpGate" || e.entity_type == "Pylon";
                if !relevant {
                    continue;
                }
                let cost = crate::balance_data::supply_cost_x10(&e.entity_type, tl.base_build);
                eprintln!(
                    "    loop={:>5} ({:>5.1}s) {:?} {} tag={} cost_x10={}",
                    e.game_loop,
                    e.game_loop as f64 / lps,
                    e.kind,
                    e.entity_type,
                    e.tag,
                    cost
                );
            }

            eprintln!("  Warpgates vivas imediatamente antes da janela:");
            let mut alive: std::collections::HashSet<i64> = std::collections::HashSet::new();
            for e in &p.entity_events {
                if e.game_loop >= win_start {
                    break;
                }
                if e.entity_type == "WarpGate" && e.category == EntityCategory::Structure {
                    match e.kind {
                        EntityEventKind::ProductionFinished => {
                            alive.insert(e.tag);
                        }
                        EntityEventKind::Died => {
                            alive.remove(&e.tag);
                        }
                        _ => {}
                    }
                }
            }
            eprintln!("    count={}, tags={:?}", alive.len(), alive);

            // Warps iniciados antes da janela — quais ainda estão em cooldown?
            let wgr_loop = has_wgr.map(|u| u.game_loop).unwrap_or(u32::MAX);
            eprintln!("  Warps iniciados antes da janela (e se estão em cooldown em {}):", win_start);
            for e in &p.entity_events {
                if e.game_loop >= win_start {
                    break;
                }
                if e.kind != EntityEventKind::ProductionStarted {
                    continue;
                }
                if !is_warp_gate_unit(&e.entity_type) || e.game_loop < wgr_loop {
                    continue;
                }
                let ready_at = e.game_loop + WARP_GATE_CYCLE_LOOPS;
                let in_cd = ready_at > win_start;
                eprintln!(
                    "    start={:>5} ({:>5.1}s) unit={} ready_at={} in_cd_at_win_start={}",
                    e.game_loop,
                    e.game_loop as f64 / lps,
                    e.entity_type,
                    ready_at,
                    in_cd
                );
            }

            let blocks = extract_supply_blocks(p, tl.game_loops, tl.base_build);
            eprintln!("  Blocos detectados (todos):");
            for b in &blocks {
                eprintln!(
                    "    [{:>5}-{:>5}] ({:>5.1}s-{:>5.1}s) supply={}",
                    b.start_loop,
                    b.end_loop,
                    b.start_loop as f64 / lps,
                    b.end_loop as f64 / lps,
                    b.supply
                );
            }
        }
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
