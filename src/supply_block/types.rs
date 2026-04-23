// Tipos públicos + configuração da estratégia de detecção.

pub struct SupplyBlockEntry {
    pub start_loop: u32,
    pub end_loop: u32,
    pub supply: i32, // supply_used no início do bloco
}

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
pub(super) const ACTIVE_STRATEGY: StartStrategy = StartStrategy::ProductionAttempt;
