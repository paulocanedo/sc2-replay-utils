// State machine que consome a timeline de eventos e emite blocos.
// Mantém contabilidade virtual do supply para capturar caps entre
// snapshots (ver comentários inline em `try_open_warpgate_aware` e
// `handle_production_start`).

use std::collections::HashSet;

use super::events::Event;
use super::types::{ACTIVE_STRATEGY, StartStrategy, SupplyBlockEntry};

/// Custo de supply da unidade warpável mais barata (Zealot).
/// Usado como piso do gatilho warpgate-aware: se o supply disponível
/// for menor que isso e houver warpgate pronta, o jogador está
/// bloqueado mesmo sem emitir comando de warp.
const CHEAPEST_WARP_SUPPLY: i32 = 2;

/// Máquina de estado que consome a timeline de eventos e emite
/// `SupplyBlockEntry`s. Toda a contabilidade virtual do supply
/// (incluindo drift de reservas em produção) vive aqui.
pub(super) struct BlockDetector {
    warp_research_done: bool,
    in_block: bool,
    block_start_loop: u32,
    block_supply: i32,
    last_supply_used: i32,
    last_supply_made: i32,
    /// Supply consumido por unidades **concluídas**, mantido
    /// independentemente dos snapshots (que incluem produção em
    /// andamento). Usado pela estratégia `CompletedSupplyCap`.
    completed_supply_used: i32,
    /// Supply consumido por unidades concluídas + em produção. Usado
    /// pela estratégia `TotalSupplyCap`.
    total_supply_used: i32,
    /// Set de warpgates vivas; não modelamos cooldowns individuais.
    /// A semântica do gatilho warpgate-aware é: "jogador Protoss em
    /// modo warpgate está supply-capped (não cobre Zealot)". Supply
    /// cap É supply cap independente de qual gate está ocupada no
    /// microinstante — gates em cooldown logo estarão prontas e o
    /// jogador perde o warp ali também.
    alive_warpgates: HashSet<i64>,
    blocks: Vec<SupplyBlockEntry>,
}

impl BlockDetector {
    pub(super) fn new(warp_research_done: bool) -> Self {
        Self {
            warp_research_done,
            in_block: false,
            block_start_loop: 0,
            block_supply: 0,
            last_supply_used: 0,
            last_supply_made: 0,
            completed_supply_used: 0,
            total_supply_used: 0,
            alive_warpgates: HashSet::new(),
            blocks: Vec::new(),
        }
    }

    pub(super) fn process(&mut self, loop_: u32, event: &Event) {
        match event {
            Event::Snapshot { supply_used, supply_made } => {
                self.last_supply_used = *supply_used;
                self.last_supply_made = *supply_made;
                // Safety net: virtual tracking pode ter drift acumulado
                // (p.ex. se uma unidade morreu sem `Died` capturado e o
                // bloco abriu por esse motivo). O snapshot é autoridade
                // do tracker; se ele mostra supply livre, fechamos.
                self.try_close_block(loop_);
            }
            Event::SupplyReady { amount } => {
                self.last_supply_made = (self.last_supply_made + amount).min(200);
                self.try_close_block(loop_);
            }
            Event::UnitDied { cost_x10 } => {
                let cost = *cost_x10 as i32 / 10;
                self.last_supply_used = (self.last_supply_used - cost).max(0);
                self.completed_supply_used = (self.completed_supply_used - cost).max(0);
                self.total_supply_used = (self.total_supply_used - cost).max(0);
                self.try_close_block(loop_);
            }
            Event::ProductionCancel { cost_x10 } => {
                let cost = *cost_x10 as i32 / 10;
                // Virtual tracking: libera o supply reservado no
                // `ProductionStart` correspondente. Resync com snapshot
                // no próximo tick.
                self.last_supply_used = (self.last_supply_used - cost).max(0);
                self.total_supply_used = (self.total_supply_used - cost).max(0);
                self.try_close_block(loop_);
            }
            Event::ProductionFinish { cost_x10 } => {
                self.completed_supply_used += *cost_x10 as i32 / 10;
                if ACTIVE_STRATEGY == StartStrategy::CompletedSupplyCap
                    && !self.in_block
                    && self.last_supply_made > 0
                    && self.last_supply_made < 200
                    && self.completed_supply_used >= self.last_supply_made
                {
                    self.open_block(loop_, self.completed_supply_used);
                }
            }
            Event::ProductionStart { cost_x10, reserve } => {
                self.total_supply_used += *cost_x10 as i32 / 10;
                self.handle_production_start(loop_, *cost_x10, *reserve);
            }
            Event::WarpGateSpawn { tag } => {
                self.alive_warpgates.insert(*tag);
            }
            Event::WarpGateDied { tag } => {
                self.alive_warpgates.remove(tag);
            }
        }

        // Gatilho warpgate-aware: avaliado após cada evento. Só ativa
        // quando `WarpGateResearch` já foi pesquisado e há pelo menos
        // uma warpgate viva. Não duplica blocos — respeita o `in_block`
        // guard. Usa `last_supply_used` virtual (atualizado em
        // ProductionStart/UnitDied/ProductionCancel), então capta caps
        // que ocorrem entre snapshots — caso típico do replay
        // Tourmaline onde o Sentry warp consumia a última fatia de
        // supply e só víamos o cap ~4s depois no próximo snapshot.
        self.try_open_warpgate_aware(loop_);
    }

    fn handle_production_start(&mut self, loop_: u32, cost_x10: u32, reserve: bool) {
        match ACTIVE_STRATEGY {
            StartStrategy::ProductionAttempt => {
                // Checa ANTES de atualizar `last_supply_used`, porque a
                // pergunta é "o jogador conseguiu iniciar esta produção?".
                // Uma produção com custo exatamente igual ao supply
                // disponível (`avail == cost`) passa — é warpada/treinada
                // com sucesso. Rodamos o check em AMBOS os casos
                // (reserve true/false) porque o snapshot atual é o melhor
                // proxy de "supply está apertado agora?" — e mesmo
                // same-loop pairs (UnitBorn fallback) sinalizam que nesse
                // instante o jogador tem a próxima unidade contada.
                if !self.in_block && self.last_supply_made > 0 && self.last_supply_made < 200 {
                    let available_x10 = (self.last_supply_made - self.last_supply_used) * 10;
                    if available_x10 < cost_x10 as i32 {
                        self.open_block(loop_, self.last_supply_used);
                    }
                }

                // Virtual tracking: só incrementa quando é reserva nova
                // (`reserve=true`). Same-loop pairs (reserve=false) já
                // têm o custo refletido nos snapshots — incrementar aqui
                // causa drift e dispara warpgate-aware falsamente
                // (regressão do Colossus no Tourmaline 11:29).
                if reserve {
                    self.last_supply_used += cost_x10 as i32 / 10;
                }
            }
            StartStrategy::TotalSupplyCap => {
                if !self.in_block
                    && self.last_supply_made > 0
                    && self.last_supply_made < 200
                    && self.total_supply_used >= self.last_supply_made
                {
                    self.open_block(loop_, self.total_supply_used);
                }
            }
            StartStrategy::CompletedSupplyCap => {}
        }
    }

    fn open_block(&mut self, loop_: u32, supply: i32) {
        self.in_block = true;
        self.block_start_loop = loop_;
        self.block_supply = supply;
    }

    fn try_close_block(&mut self, loop_: u32) {
        if self.in_block && self.supply_freed() {
            self.blocks.push(SupplyBlockEntry {
                start_loop: self.block_start_loop,
                end_loop: loop_,
                supply: self.block_supply,
            });
            self.in_block = false;
        }
    }

    fn try_open_warpgate_aware(&mut self, loop_: u32) {
        if self.in_block
            || !self.warp_research_done
            || self.alive_warpgates.is_empty()
            || self.last_supply_made <= 0
            || self.last_supply_made >= 200
        {
            return;
        }
        if (self.last_supply_made - self.last_supply_used) < CHEAPEST_WARP_SUPPLY {
            self.open_block(loop_, self.last_supply_used);
        }
    }

    /// Verifica se há supply disponível para sair do bloco. A medida
    /// de "supply usado" depende da estratégia ativa.
    fn supply_freed(&self) -> bool {
        let used = match ACTIVE_STRATEGY {
            StartStrategy::ProductionAttempt => self.last_supply_used,
            StartStrategy::CompletedSupplyCap => self.completed_supply_used,
            StartStrategy::TotalSupplyCap => self.total_supply_used,
        };
        self.last_supply_made > used
    }

    pub(super) fn finish(mut self, game_loops: u32) -> Vec<SupplyBlockEntry> {
        // Bloco ainda aberto no fim do jogo.
        if self.in_block {
            self.blocks.push(SupplyBlockEntry {
                start_loop: self.block_start_loop,
                end_loop: game_loops,
                supply: self.block_supply,
            });
        }
        self.blocks
    }
}
