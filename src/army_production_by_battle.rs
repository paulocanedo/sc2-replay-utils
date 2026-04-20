// Eficiência de produção de exército (exceto workers) medida entre
// grandes lutas — consumer puro de `ReplayTimeline`.
//
// Ao contrário da série contínua em `production_efficiency`, aqui cada
// grande luta é um "marco" que fecha o segmento corrente e zera a
// contagem. Retornamos até 3 valores, cada um correspondendo ao
// intervalo entre marcos consecutivos (a primeira janela começa quando
// o jogador tem tech para produzir — i.e. o primeiro producer de army
// ficou pronto).
//
// Diferenças semânticas vs. `production_efficiency::compute_series_army`:
// - Sem bucketização: integração direta sobre janelas arbitrárias.
// - Capacity descontada durante construção de addon (BarracksReactor,
//   BarracksTechLab, FactoryReactor/TechLab, StarportReactor/TechLab):
//   enquanto o addon está em construção, a estrutura-mãe não produz,
//   então não entra como capacity.
//
// Fora de escopo: Zerg. `army_capacity` só cobre Terran/Protoss por
// design (ver `classify::is_army_producer`). Para jogadores Zerg a
// função devolve `segments: []` — a UI trata graciosamente.

use std::collections::HashMap;

use crate::balance_data;
use crate::loss_analysis::{cluster_engagements, player_kills, player_losses};
use crate::production_efficiency::{
    is_warp_gate_unit, ARMY_SUPPLY_MAXED_THRESHOLD, WARP_GATE_CYCLE_LOOPS, WARP_GATE_RESEARCH,
};
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline};

/// Gap (em segundos) que separa engajamentos. Mesmo threshold usado em
/// `insights/army_trades.rs` — mantém a noção de "grande luta"
/// consistente entre cards.
const GAP_SECS: u32 = 15;

/// Valor mínimo total (lost + killed) em recursos pra um engajamento
/// contar como marco. Filtra escaramuças pequenas (1-2 unidades).
const MIN_TOTAL_VALUE: u32 = 300;

/// Número máximo de marcos (e portanto de segmentos) reportados.
pub const MAX_SEGMENTS: usize = 3;

/// Addons cuja construção deixa o produtor-mãe incapacitado enquanto
/// estão em andamento. A estrutura volta a contar como capacidade
/// quando o addon termina (ou é cancelado/morto).
const INCAPACITATING_ADDONS: &[&str] = &[
    "BarracksReactor",
    "BarracksTechLab",
    "FactoryReactor",
    "FactoryTechLab",
    "StarportReactor",
    "StarportTechLab",
];

fn is_incapacitating_addon(name: &str) -> bool {
    INCAPACITATING_ADDONS.iter().any(|a| *a == name)
}

#[derive(Clone, Debug)]
pub struct BattleSegmentEfficiency {
    /// 1-based index do segmento (1 = antes da 1ª luta, ...).
    pub index: u8,
    /// Início do segmento — tech_start_loop para o 1º, start_loop da
    /// luta anterior para os demais.
    pub start_loop: u32,
    /// Fim do segmento = start_loop da luta que o fechou.
    pub end_loop: u32,
    /// ∫ capacity dt dentro do segmento.
    pub capacity_loops: u64,
    /// ∫ min(active, capacity) dt — com override de 100% quando
    /// `supply_high`.
    pub active_loops: u64,
    /// 100 · active_loops / capacity_loops. 100.0 quando `capacity_loops == 0`
    /// (sem capacidade não há ociosidade real).
    pub efficiency_pct: f64,
}

#[derive(Clone, Debug)]
pub struct PlayerBattleEfficiency {
    pub name: String,
    pub segments: Vec<BattleSegmentEfficiency>,
}

/// Calcula até 3 valores de eficiência de produção de army para o
/// jogador indicado, segmentados pelos marcos das 3 primeiras grandes
/// lutas. Jogador sem tech ou sem lutas significativas devolve
/// `segments: []`.
pub fn extract_army_production_by_battle(
    timeline: &ReplayTimeline,
    player_idx: usize,
) -> PlayerBattleEfficiency {
    let Some(player) = timeline.players.get(player_idx) else {
        return PlayerBattleEfficiency {
            name: String::new(),
            segments: Vec::new(),
        };
    };

    // Tech gate: primeiro delta positivo em army_capacity — instante em
    // que o primeiro producer ficou pronto. Zerg não popula
    // army_capacity (ver classify::is_army_producer), então a ausência
    // aqui também filtra raça sem suporte.
    let Some(tech_start_loop) = player
        .army_capacity
        .iter()
        .find(|(_, d)| *d > 0)
        .map(|(gl, _)| *gl)
    else {
        return PlayerBattleEfficiency {
            name: player.name.clone(),
            segments: Vec::new(),
        };
    };

    let lps = timeline.loops_per_second.max(0.0001);
    let gap_loops = (GAP_SECS as f64 * lps).round() as u32;
    let losses = player_losses(timeline, player_idx);
    let kills = player_kills(timeline, player_idx);
    let mut engagements = cluster_engagements(&losses, &kills, gap_loops);
    engagements.retain(|e| e.total_value() >= MIN_TOTAL_VALUE);
    engagements.truncate(MAX_SEGMENTS);

    if engagements.is_empty() {
        return PlayerBattleEfficiency {
            name: player.name.clone(),
            segments: Vec::new(),
        };
    }

    let events = build_events(player, timeline.game_loops, timeline.base_build);

    // Segmentos semi-abertos [prev_boundary, battle.start_loop).
    let mut segments = Vec::with_capacity(engagements.len());
    let mut prev_boundary = tech_start_loop;
    for (i, eng) in engagements.iter().enumerate() {
        let seg_start = prev_boundary;
        // Luta que ocorre antes do tech_start é teoricamente impossível
        // (sem capacidade de army ainda), mas um clamp defensivo evita
        // segmentos de comprimento negativo.
        let seg_end = eng.start_loop.max(seg_start);
        let (cap_sum, act_sum) = integrate_window(&events, seg_start, seg_end);
        let pct = if cap_sum == 0 {
            100.0
        } else {
            100.0 * act_sum as f64 / cap_sum as f64
        };
        segments.push(BattleSegmentEfficiency {
            index: (i as u8) + 1,
            start_loop: seg_start,
            end_loop: seg_end,
            capacity_loops: cap_sum,
            active_loops: act_sum,
            efficiency_pct: pct,
        });
        prev_boundary = eng.start_loop;
    }

    PlayerBattleEfficiency {
        name: player.name.clone(),
        segments,
    }
}

/// Tipos de evento no stream de sweep. Ordem canônica (CapacityUp ->
/// ProdStart -> ProdEnd -> CapacityDown) evita estados transitórios
/// inválidos quando múltiplos eventos caem no mesmo game_loop.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EvKind {
    CapacityUp,
    ProdStart,
    ProdEnd,
    CapacityDown,
    SupplyMaxedOff,
    SupplyMaxedOn,
}

impl EvKind {
    fn order(self) -> u8 {
        match self {
            EvKind::CapacityUp => 0,
            EvKind::ProdStart => 1,
            EvKind::ProdEnd => 2,
            EvKind::CapacityDown => 3,
            EvKind::SupplyMaxedOff => 4,
            EvKind::SupplyMaxedOn => 5,
        }
    }
}

fn build_events(player: &PlayerTimeline, game_end: u32, base_build: u32) -> Vec<(u32, EvKind)> {
    let mut evs: Vec<(u32, EvKind)> = Vec::new();

    // 1. Capacity base via army_capacity (já cobre morph Gate->WarpGate
    //    com backfill em `finalize::derive_capacity_indices`).
    for &(gl, delta) in &player.army_capacity {
        evs.push((
            gl,
            if delta > 0 { EvKind::CapacityUp } else { EvKind::CapacityDown },
        ));
    }

    // 2. Active: pareamento ProductionStarted <-> Finished/Cancelled por
    //    tag para unidades army (category == Unit). Trata warp-in
    //    estendido via WARP_GATE_CYCLE_LOOPS — mesma lógica de
    //    `compute_series_army`.
    let warp_research_loop: Option<u32> = player
        .upgrades
        .iter()
        .find(|u| u.name == WARP_GATE_RESEARCH)
        .map(|u| u.game_loop);

    let mut starts: HashMap<i64, (u32, String)> = HashMap::new();
    for ev in &player.entity_events {
        if ev.category != EntityCategory::Unit {
            continue;
        }
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                starts
                    .entry(ev.tag)
                    .or_insert_with(|| (ev.game_loop, ev.entity_type.clone()));
            }
            EntityEventKind::ProductionFinished | EntityEventKind::ProductionCancelled => {
                if let Some((start_loop, entity_type)) = starts.remove(&ev.tag) {
                    let start_loop = if start_loop >= ev.game_loop {
                        let bt = balance_data::build_time_loops(&entity_type, base_build);
                        ev.game_loop.saturating_sub(bt)
                    } else {
                        start_loop
                    };
                    let is_warp_in = warp_research_loop
                        .map(|r| start_loop >= r && is_warp_gate_unit(&entity_type))
                        .unwrap_or(false);
                    let end_loop = if is_warp_in {
                        (start_loop + WARP_GATE_CYCLE_LOOPS).min(game_end)
                    } else {
                        ev.game_loop
                    };
                    evs.push((start_loop, EvKind::ProdStart));
                    evs.push((end_loop, EvKind::ProdEnd));
                }
            }
            EntityEventKind::Died => {}
        }
    }
    // Órfãos (Started sem Finished/Cancelled): fecham em game_end.
    for (_tag, (start_loop, _entity_type)) in starts {
        evs.push((start_loop, EvKind::ProdStart));
        evs.push((game_end, EvKind::ProdEnd));
    }

    // 3. Addon-incapacitation: para cada addon em construção, derruba a
    //    capacity do produtor-mãe durante [start, finish]. Usa `tag`
    //    (do addon) para parear Started com Finished/Cancelled/Died.
    //    Não usa `creator_tag` explicitamente — o addon sendo construído
    //    implica que a estrutura-mãe está ocupada independentemente de
    //    qual tag específico é o pai, e derrubar +1 capacity em net é
    //    o efeito desejado.
    let mut addon_starts: HashMap<i64, u32> = HashMap::new();
    for ev in &player.entity_events {
        if ev.category != EntityCategory::Structure {
            continue;
        }
        if !is_incapacitating_addon(&ev.entity_type) {
            continue;
        }
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                addon_starts.entry(ev.tag).or_insert(ev.game_loop);
            }
            EntityEventKind::ProductionFinished | EntityEventKind::ProductionCancelled => {
                if let Some(start_loop) = addon_starts.remove(&ev.tag) {
                    if ev.game_loop > start_loop {
                        evs.push((start_loop, EvKind::CapacityDown));
                        evs.push((ev.game_loop, EvKind::CapacityUp));
                    }
                }
            }
            EntityEventKind::Died => {
                // Addon destruído em construção: estrutura-mãe volta a
                // poder produzir no instante da morte. Se já tinha sido
                // finalizado, o Started correspondente já saiu do map.
                if let Some(start_loop) = addon_starts.remove(&ev.tag) {
                    if ev.game_loop > start_loop {
                        evs.push((start_loop, EvKind::CapacityDown));
                        evs.push((ev.game_loop, EvKind::CapacityUp));
                    }
                }
            }
        }
    }
    // Órfãos: addons sem Finished/Died (jogo truncado) fecham em game_end.
    for (_tag, start_loop) in addon_starts {
        if game_end > start_loop {
            evs.push((start_loop, EvKind::CapacityDown));
            evs.push((game_end, EvKind::CapacityUp));
        }
    }

    // 4. Transições de supply-maxed — força 100% durante a janela em
    //    que o jogador está supply capped (consistente com
    //    `compute_series_army`).
    let mut supply_was_high = false;
    for s in &player.stats {
        let now_high = s.supply_used > ARMY_SUPPLY_MAXED_THRESHOLD;
        if now_high != supply_was_high {
            evs.push((
                s.game_loop,
                if now_high {
                    EvKind::SupplyMaxedOn
                } else {
                    EvKind::SupplyMaxedOff
                },
            ));
            supply_was_high = now_high;
        }
    }

    evs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.order().cmp(&b.1.order())));
    evs
}

/// Integra (Σ capacity·dt, Σ min(active, capacity)·dt) na janela
/// semi-aberta [start, end). Aplica o override de supply maxed
/// (active = capacity) durante o intervalo em que a flag está ligada.
/// Eventos em exatamente `start` já contam; eventos em `end` não.
fn integrate_window(events: &[(u32, EvKind)], start: u32, end: u32) -> (u64, u64) {
    if end <= start {
        return (0, 0);
    }

    let mut capacity: i32 = 0;
    let mut active: i32 = 0;
    let mut supply_high: bool = false;

    // Aplica todos os eventos estritamente antes de `start` para
    // estabelecer o estado no início da janela. Eventos em == start
    // são aplicados dentro do loop principal, na ordem canônica.
    let mut i = 0usize;
    while i < events.len() && events[i].0 < start {
        apply_event(events[i].1, &mut capacity, &mut active, &mut supply_high);
        i += 1;
    }

    let mut cursor: u32 = start;
    let mut cap_int: u64 = 0;
    let mut act_int: u64 = 0;

    while i < events.len() && events[i].0 < end {
        let ev_gl = events[i].0;
        if ev_gl > cursor {
            let dt = (ev_gl - cursor) as u64;
            let cap = capacity.max(0) as u64;
            let act = if supply_high {
                cap
            } else {
                active.max(0).min(capacity.max(0)) as u64
            };
            cap_int += cap * dt;
            act_int += act * dt;
        }
        while i < events.len() && events[i].0 == ev_gl {
            apply_event(events[i].1, &mut capacity, &mut active, &mut supply_high);
            i += 1;
        }
        cursor = ev_gl;
    }

    // Cauda [cursor, end).
    if end > cursor {
        let dt = (end - cursor) as u64;
        let cap = capacity.max(0) as u64;
        let act = if supply_high {
            cap
        } else {
            active.max(0).min(capacity.max(0)) as u64
        };
        cap_int += cap * dt;
        act_int += act * dt;
    }

    (cap_int, act_int)
}

fn apply_event(kind: EvKind, capacity: &mut i32, active: &mut i32, supply_high: &mut bool) {
    match kind {
        EvKind::CapacityUp => *capacity += 1,
        EvKind::CapacityDown => *capacity = (*capacity - 1).max(0),
        EvKind::ProdStart => *active += 1,
        EvKind::ProdEnd => *active = (*active - 1).max(0),
        EvKind::SupplyMaxedOn => *supply_high = true,
        EvKind::SupplyMaxedOff => *supply_high = false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(gl: u32, k: EvKind) -> (u32, EvKind) {
        (gl, k)
    }

    fn sorted(mut evs: Vec<(u32, EvKind)>) -> Vec<(u32, EvKind)> {
        evs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.order().cmp(&b.1.order())));
        evs
    }

    #[test]
    fn integrate_full_busy_gives_100pct() {
        // 1 producer, ativo 100% de 0..100.
        let evs = sorted(vec![
            mk(0, EvKind::CapacityUp),
            mk(0, EvKind::ProdStart),
            mk(100, EvKind::ProdEnd),
            mk(100, EvKind::CapacityDown),
        ]);
        let (cap, act) = integrate_window(&evs, 0, 100);
        assert_eq!(cap, 100);
        assert_eq!(act, 100);
    }

    #[test]
    fn integrate_half_busy_gives_50pct() {
        // 1 producer, ativo apenas na primeira metade.
        let evs = sorted(vec![
            mk(0, EvKind::CapacityUp),
            mk(0, EvKind::ProdStart),
            mk(50, EvKind::ProdEnd),
            mk(100, EvKind::CapacityDown),
        ]);
        let (cap, act) = integrate_window(&evs, 0, 100);
        assert_eq!(cap, 100);
        assert_eq!(act, 50);
    }

    #[test]
    fn integrate_zero_capacity_returns_zero() {
        let (cap, act) = integrate_window(&[], 0, 100);
        assert_eq!(cap, 0);
        assert_eq!(act, 0);
    }

    #[test]
    fn integrate_supply_maxed_forces_full() {
        // 2 producers, nenhum ativo, mas supply_high → act = cap.
        let evs = sorted(vec![
            mk(0, EvKind::CapacityUp),
            mk(0, EvKind::CapacityUp),
            mk(0, EvKind::SupplyMaxedOn),
            mk(100, EvKind::SupplyMaxedOff),
        ]);
        let (cap, act) = integrate_window(&evs, 0, 100);
        assert_eq!(cap, 200);
        assert_eq!(act, 200);
    }

    #[test]
    fn integrate_window_starts_mid_state() {
        // Capacity sobe em 10, começa a janela em 20. A janela deve
        // enxergar a capacity já estabelecida.
        let evs = sorted(vec![
            mk(10, EvKind::CapacityUp),
            mk(50, EvKind::CapacityDown),
        ]);
        let (cap, act) = integrate_window(&evs, 20, 40);
        assert_eq!(cap, 20); // 1 cap * 20 loops
        assert_eq!(act, 0);
    }

    #[test]
    fn integrate_clamps_active_to_capacity() {
        // active > capacity: deve ser clampado. Cenário improvável na
        // prática, mas o sweep precisa ser robusto.
        let evs = sorted(vec![
            mk(0, EvKind::CapacityUp),
            mk(0, EvKind::ProdStart),
            mk(0, EvKind::ProdStart), // active = 2, capacity = 1
            mk(100, EvKind::CapacityDown),
        ]);
        let (cap, act) = integrate_window(&evs, 0, 100);
        assert_eq!(cap, 100);
        assert_eq!(act, 100); // min(2, 1) * 100
    }

    #[test]
    fn is_incapacitating_addon_matches_known_addons() {
        assert!(is_incapacitating_addon("BarracksReactor"));
        assert!(is_incapacitating_addon("BarracksTechLab"));
        assert!(is_incapacitating_addon("FactoryReactor"));
        assert!(is_incapacitating_addon("StarportTechLab"));
        assert!(!is_incapacitating_addon("Barracks"));
        assert!(!is_incapacitating_addon("Marine"));
        assert!(!is_incapacitating_addon(""));
    }
}
