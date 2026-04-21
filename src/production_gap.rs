// Detector de gaps de produção de workers — agora é puro consumer de
// `ReplayTimeline`. Toda a lógica de tag_map / capacidade / nascimentos
// foi absorvida pelo parser single-pass em `src/replay.rs`. Aqui só
// resta o cálculo dos períodos ociosos a partir das séries
// `worker_capacity` e `worker_births` já prontas.

use crate::replay::ReplayTimeline;

// ── Constantes ───────────────────────────────────────────────────────────────

/// Tempo de produção de um worker em game loops (~12s em Faster).
const WORKER_BUILD_TIME: u32 = 272;

/// Mínimo de game loops ociosos para registrar um gap.
const MIN_IDLE_LOOPS: u32 = 20;

// ── Structs de saída ─────────────────────────────────────────────────────────

pub struct ProductionGapEntry {
    pub start_loop: u32,
    pub end_loop: u32,
    pub capacity: u32,
    pub active: u32,
    pub idle_slots: u32,
}

pub struct PlayerProductionGap {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub entries: Vec<ProductionGapEntry>,
    pub is_zerg: bool,
    pub total_idle_loops: u32,
    pub efficiency_pct: f64,
}

pub struct ProductionGapResult {
    pub players: Vec<PlayerProductionGap>,
    pub game_loops: u32,
    pub loops_per_second: f64,
    pub datetime: String,
    pub map_name: String,
}

// ── Classificação ────────────────────────────────────────────────────────────

pub fn is_zerg_race(race: &str) -> bool {
    race.starts_with('Z') || race.starts_with('z')
}

// ── Extração ─────────────────────────────────────────────────────────────────

pub fn extract_production_gaps(
    timeline: &ReplayTimeline,
) -> Result<ProductionGapResult, String> {
    let game_loops = timeline.game_loops;
    let max_loops = if timeline.max_time_seconds == 0 {
        0
    } else {
        (timeline.max_time_seconds as f64 * timeline.loops_per_second).round() as u32
    };

    // Limite efetivo do jogo (game_end usado para fechar gaps abertos).
    let effective_end = if max_loops == 0 {
        game_loops
    } else {
        game_loops.min(max_loops)
    };

    let players = timeline
        .players
        .iter()
        .map(|player| {
            if is_zerg_race(&player.race) {
                return PlayerProductionGap {
                    name: player.name.clone(),
                    race: player.race.clone(),
                    mmr: player.mmr,
                    entries: Vec::new(),
                    is_zerg: true,
                    total_idle_loops: 0,
                    efficiency_pct: 0.0,
                };
            }

            let (entries, total_idle, efficiency) = compute_idle_periods(
                &player.worker_births,
                &player.worker_capacity,
                effective_end,
            );

            PlayerProductionGap {
                name: player.name.clone(),
                race: player.race.clone(),
                mmr: player.mmr,
                entries,
                is_zerg: false,
                total_idle_loops: total_idle,
                efficiency_pct: efficiency,
            }
        })
        .collect();

    Ok(ProductionGapResult {
        players,
        game_loops,
        loops_per_second: timeline.loops_per_second,
        datetime: timeline.datetime.clone(),
        map_name: timeline.map.clone(),
    })
}

// ── Cálculo de períodos ociosos ──────────────────────────────────────────────

/// Tipos de evento na timeline unificada.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EvKind {
    CapacityUp,
    ProdStart,
    ProdEnd,
    CapacityDown,
}

impl EvKind {
    fn order(self) -> u8 {
        match self {
            EvKind::CapacityUp => 0,
            EvKind::ProdStart => 1,
            EvKind::ProdEnd => 2,
            EvKind::CapacityDown => 3,
        }
    }
}

/// Wrapper histórico para worker idle — infere start a partir do birth
/// subtraindo `WORKER_BUILD_TIME` (constante para SCV/Probe). Para army,
/// onde os build times variam por unidade, use `compute_idle_periods_ranges`
/// passando os pares `(start, end)` reais do parser.
pub fn compute_idle_periods(
    worker_births: &[u32],
    capacity_events: &[(u32, i32)],
    game_end: u32,
) -> (Vec<ProductionGapEntry>, u32, f64) {
    let ranges: Vec<(u32, u32)> = worker_births
        .iter()
        .map(|&b| (b.saturating_sub(WORKER_BUILD_TIME), b))
        .collect();
    compute_idle_periods_ranges(&ranges, capacity_events, game_end)
}

/// Calcula períodos ociosos dado uma lista de `(start_loop, end_loop)`
/// de produções e os deltas de capacidade. Usado para worker (via
/// `compute_idle_periods`) e army (direto com `PlayerTimeline::army_productions`).
pub fn compute_idle_periods_ranges(
    productions: &[(u32, u32)],
    capacity_events: &[(u32, i32)],
    game_end: u32,
) -> (Vec<ProductionGapEntry>, u32, f64) {
    // Montar timeline de eventos
    let mut timeline: Vec<(u32, EvKind)> = Vec::new();

    for &(gl, delta) in capacity_events {
        if delta > 0 {
            timeline.push((gl, EvKind::CapacityUp));
        } else {
            timeline.push((gl, EvKind::CapacityDown));
        }
    }

    for &(start, end) in productions {
        timeline.push((start, EvKind::ProdStart));
        timeline.push((end, EvKind::ProdEnd));
    }

    // Ordenar: por game_loop, desempate por ordem do tipo
    timeline.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.order().cmp(&b.1.order())));

    let mut capacity: i32 = 0;
    let mut active: i32 = 0;
    let mut entries: Vec<ProductionGapEntry> = Vec::new();

    // Estado do gap aberto
    let mut gap_start: Option<(u32, u32, u32)> = None; // (start_loop, capacity, active)

    // Para cálculo de eficiência: acumular (capacity_slot_time, idle_slot_time)
    let mut sum_capacity_time: u64 = 0;
    let mut sum_idle_time: u64 = 0;
    let mut prev_loop: u32 = 0;

    for &(gl, kind) in &timeline {
        if gl > game_end {
            break;
        }

        // Acumular tempos do segmento anterior
        let dt = (gl.min(game_end) - prev_loop) as u64;
        if capacity > 0 && dt > 0 {
            sum_capacity_time += capacity as u64 * dt;
            let idle = (capacity - active).max(0) as u64;
            sum_idle_time += idle * dt;
        }

        // Atualizar contadores
        match kind {
            EvKind::CapacityUp => capacity += 1,
            EvKind::CapacityDown => capacity = (capacity - 1).max(0),
            EvKind::ProdStart => active += 1,
            EvKind::ProdEnd => active = (active - 1).max(0),
        }

        let idle_now = (capacity - active.min(capacity)).max(0);

        // Gerenciar gaps
        if idle_now > 0 && capacity > 0 {
            if gap_start.is_none() {
                gap_start = Some((gl, capacity as u32, active.max(0) as u32));
            }
        } else if let Some((start, cap, act)) = gap_start.take() {
            if gl.saturating_sub(start) >= MIN_IDLE_LOOPS {
                entries.push(ProductionGapEntry {
                    start_loop: start,
                    end_loop: gl,
                    capacity: cap,
                    active: act,
                    idle_slots: cap.saturating_sub(act),
                });
            }
        }

        prev_loop = gl;
    }

    // Segmento final até game_end
    let dt = game_end.saturating_sub(prev_loop) as u64;
    if capacity > 0 && dt > 0 {
        sum_capacity_time += capacity as u64 * dt;
        let idle = (capacity - active).max(0) as u64;
        sum_idle_time += idle * dt;
    }

    // Fechar gap aberto
    if let Some((start, cap, act)) = gap_start.take() {
        if game_end.saturating_sub(start) >= MIN_IDLE_LOOPS {
            entries.push(ProductionGapEntry {
                start_loop: start,
                end_loop: game_end,
                capacity: cap,
                active: act,
                idle_slots: cap.saturating_sub(act),
            });
        }
    }

    let total_idle: u32 = entries
        .iter()
        .map(|e| e.end_loop.saturating_sub(e.start_loop))
        .sum();

    let efficiency = if sum_capacity_time > 0 {
        100.0 * (1.0 - sum_idle_time as f64 / sum_capacity_time as f64)
    } else {
        100.0
    };

    (entries, total_idle, efficiency)
}

