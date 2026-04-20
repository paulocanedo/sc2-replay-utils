// Tipos públicos, constantes e predicados auxiliares de classificação.

// ── Constantes ───────────────────────────────────────────────────────────────

/// Tempo de produção de um worker em game loops (~12s em Faster).
/// Casa com a constante homônima em `production_gap.rs`.
pub(super) const WORKER_BUILD_TIME: u32 = 272;

/// Largura em segundos do bucket de amostragem para o gráfico. Cada
/// ponto plotado representa a média ponderada pelo tempo da eficiência
/// naquele intervalo — suaviza transições de estado rápidas e mantém
/// o gráfico legível em partidas longas.
pub(super) const CHART_BUCKET_SECONDS: f64 = 10.0;

/// Limite de `supply_used` acima do qual a eficiência de produção de
/// army é forçada a 100%. Racional: com supply próximo do cap (200),
/// deixar Barracks/Gateway/etc. ociosos é comportamento esperado —
/// o jogador não *pode* produzir mais army supply. Penalizar idleness
/// nesse regime distorce o gráfico e esconde os momentos realmente
/// importantes (early/mid-game, onde cada segundo de idle importa).
pub(crate) const ARMY_SUPPLY_MAXED_THRESHOLD: i32 = 185;

/// Duração total (em game loops) do ciclo de warp de uma WarpGate,
/// contado a partir do instante em que o jogador emite o comando
/// de warp: ~5s de animação de warp-in + ~20s de cooldown até a
/// estrutura ficar disponível para warpar novamente (~25s no total
/// em Faster, que corresponde a 560 loops @ 22.4 lps).
///
/// Racional da modelagem: após a pesquisa de Warp Gate, a semântica
/// de "ociosa" muda — a estrutura só é ociosa quando está pronta e
/// o jogador não está warpando. Durante warp-in + cooldown, o
/// jogador *não pode* warpar, então não faz sentido penalizar como
/// idle. Aproximação por constante única (sem discriminar por tipo
/// de unidade) é deliberada — suficiente para o gráfico, refinação
/// por unidade fica pendente.
pub(crate) const WARP_GATE_CYCLE_LOOPS: u32 = 560;

/// Nome do upgrade que habilita o modo WarpGate. Após essa pesquisa
/// completar, gateways morpham automaticamente para warpgates e as
/// produções de unidades do roster abaixo passam a ser warp-ins em
/// vez de trains convencionais.
pub(crate) const WARP_GATE_RESEARCH: &str = "WarpGateResearch";

/// Unidades que podem ser warpadas por uma WarpGate. Usado para
/// identificar, dado um `ProductionStarted` posterior ao término de
/// `WarpGateResearch`, se ele corresponde a um warp-in (recebendo
/// tratamento de ciclo estendido) ou não.
const WARP_GATE_UNITS: &[&str] = &[
    "Zealot",
    "Stalker",
    "Sentry",
    "Adept",
    "HighTemplar",
    "DarkTemplar",
];

pub(crate) fn is_warp_gate_unit(name: &str) -> bool {
    WARP_GATE_UNITS.iter().any(|u| *u == name)
}

/// Duração da janela em que um inject fornece capacidade extra ao
/// hatch alvo. 650 loops ≈ 29s em Faster, cobrindo o delay entre o
/// comando de SpawnLarva e o momento em que os 4 larvae extras
/// idealmente já foram consumidos. Após isso, ou foram usados ou se
/// perderam no cap — a janela do bônus encerra.
pub(super) const INJECT_WINDOW_LOOPS: u32 = 650;

/// Slots extras por inject ativo. Corresponde aos 4 larvae que a
/// mecânica SpawnLarva gera de uma vez.
pub(super) const INJECT_EXTRA_SLOTS: i32 = 4;

/// Drone morph time (~12s em Faster). Equivalente ao
/// `WORKER_BUILD_TIME` dos non-Zerg; reusado no pareamento
/// `ProductionStarted`/`ProductionFinished` quando caem no mesmo
/// loop (morph de larva frequentemente nasce "pronto" sem
/// `UnitInit` anterior).
pub(super) const DRONE_BUILD_LOOPS: u32 = 272;

/// Filtro para a vertente Workers do Zerg — o único larva-born
/// worker é o Drone. Usado como `is_target_unit` em
/// `compute_series_zerg`.
pub(super) fn is_drone(name: &str) -> bool {
    name == "Drone"
}

// ── Tipos públicos ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EfficiencyTarget {
    Workers,
    Army,
}

#[derive(Clone, Copy, Debug)]
pub struct EfficiencySample {
    pub game_loop: u32,
    pub capacity: u32,
    pub active: u32,
    pub efficiency_pct: f64,
}

pub struct PlayerEfficiencySeries {
    pub name: String,
    pub race: String,
    pub is_zerg: bool,
    pub samples: Vec<EfficiencySample>,
}

pub struct ProductionEfficiencySeries {
    pub players: Vec<PlayerEfficiencySeries>,
    pub target: EfficiencyTarget,
    pub loops_per_second: f64,
    pub game_loops: u32,
}

// ── Classificação ────────────────────────────────────────────────────────────

pub(super) fn is_zerg_race(race: &str) -> bool {
    race.starts_with('Z') || race.starts_with('z')
}

// ── Merge de eventos ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EvKind {
    CapacityUp,
    /// Inject Larva ativo: capacidade +4 (usado só no path Zerg).
    InjectOn,
    ProdStart,
    ProdEnd,
    CapacityDown,
    /// Fim da janela de inject: capacidade −4 (usado só no path Zerg).
    InjectOff,
    /// Transição de `supply_used` subindo acima de `ARMY_SUPPLY_MAXED_THRESHOLD`.
    SupplyMaxedOn,
    /// Transição de `supply_used` caindo até ≤ `ARMY_SUPPLY_MAXED_THRESHOLD`.
    SupplyMaxedOff,
}

impl EvKind {
    pub(super) fn order(self) -> u8 {
        // Capacity up (base + inject) entra antes de produção para
        // evitar `active > capacity` transitório; capacity down
        // (base + inject) sai depois de produção pelo mesmo motivo.
        match self {
            EvKind::CapacityUp => 0,
            EvKind::InjectOn => 1,
            EvKind::ProdStart => 2,
            EvKind::ProdEnd => 3,
            EvKind::CapacityDown => 4,
            EvKind::InjectOff => 5,
            EvKind::SupplyMaxedOff => 6,
            EvKind::SupplyMaxedOn => 7,
        }
    }
}
