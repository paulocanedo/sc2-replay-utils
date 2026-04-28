//! Tipos públicos do módulo `production_lanes` + constante de tolerância
//! usada pelo merge de blocos.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneMode {
    Workers,
    Army,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    Producing,
    Morphing,
    /// Estrutura existe mas não pode produzir — Terran com addon
    /// (Reactor ou TechLab) em construção. Renderizada com cor distinta
    /// de `Producing`/`Morphing`.
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

pub(super) const CONTINUITY_TOLERANCE_LOOPS: u32 = 5;
