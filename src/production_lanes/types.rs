//! Tipos públicos do módulo `production_lanes` + constante de tolerância
//! usada pelo merge de blocos.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneMode {
    Workers,
    Army,
    /// Pesquisas one-shot (Stimpack, Blink, WarpGate, etc.). Lane =
    /// estrutura-de-pesquisa; bloco = janela `[cmd_loop, finish_loop]`
    /// do upgrade não-leveled. Filtra entradas que terminem em
    /// `Level1/2/3` (essas vão para o modo `Upgrades`).
    Research,
    /// Upgrades leveled (Weapons/Armor/Shields 1-3). Mesma estrutura
    /// visual que `Research`, com filtro inverso por sufixo de nível.
    Upgrades,
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
    /// Trilha vertical dentro da lane. 0 = trilha única (full-height) ou
    /// trilha superior. 1 = trilha inferior — usada apenas em lanes
    /// Terran com Reactor anexado, para a SEGUNDA unidade de cada par
    /// paralelo. O renderer pinta em half-height top/bottom quando
    /// `lane.reactor_since_loop.is_some()` e `block.start_loop >=
    /// reactor_since_loop`.
    pub sub_track: u8,
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
    /// Para lanes Terran: loop em que um Reactor terminou de ser
    /// construído nesta estrutura. Blocos `Producing` com `start_loop
    /// >= reactor_since_loop` ganham `sub_track` 0 ou 1 (renderizados
    /// em duas faixas top/bottom representando a capacidade paralela
    /// 2x). `None` se nunca teve reactor.
    pub reactor_since_loop: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerProductionLanes {
    pub lanes: Vec<StructureLane>,
}

pub(super) const CONTINUITY_TOLERANCE_LOOPS: u32 = 5;
