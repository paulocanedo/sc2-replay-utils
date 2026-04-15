// Série temporal de eficiência de produção (workers ou army) —
// consumer puro do `ReplayTimeline`. Reusa o padrão de merge de
// eventos de `production_gap.rs`, mas ao invés de um escalar + lista
// de gaps, emite uma amostra por bucket de `CHART_BUCKET_SECONDS`
// com a média ponderada pelo tempo do estado dentro do bucket. Isso
// suaviza o gráfico e evita o "serrote" que surge quando produção e
// idle alternam a cada poucos game_loops.
//
// Para workers: reusa `worker_capacity` + `worker_births` (backoff
// de `WORKER_BUILD_TIME` converte nascimentos em intervalos de
// produção ativa).
//
// Para army: usa `army_capacity` + `entity_events` filtrados por
// `EntityCategory::Unit`, pareando `ProductionStarted` com
// `ProductionFinished`/`ProductionCancelled` por `tag`.

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline};

// ── Constantes ───────────────────────────────────────────────────────────────

/// Tempo de produção de um worker em game loops (~12s em Faster).
/// Casa com a constante homônima em `production_gap.rs`.
const WORKER_BUILD_TIME: u32 = 272;

/// Largura em segundos do bucket de amostragem para o gráfico. Cada
/// ponto plotado representa a média ponderada pelo tempo da eficiência
/// naquele intervalo — suaviza transições de estado rápidas e mantém
/// o gráfico legível em partidas longas.
const CHART_BUCKET_SECONDS: f64 = 10.0;

/// Limite de `supply_used` acima do qual a eficiência de produção de
/// army é forçada a 100%. Racional: com supply próximo do cap (200),
/// deixar Barracks/Gateway/etc. ociosos é comportamento esperado —
/// o jogador não *pode* produzir mais army supply. Penalizar idleness
/// nesse regime distorce o gráfico e esconde os momentos realmente
/// importantes (early/mid-game, onde cada segundo de idle importa).
const ARMY_SUPPLY_MAXED_THRESHOLD: i32 = 185;

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

fn is_zerg_race(race: &str) -> bool {
    race.starts_with('Z') || race.starts_with('z')
}

// ── API pública ──────────────────────────────────────────────────────────────

pub fn extract_efficiency_series(
    timeline: &ReplayTimeline,
    target: EfficiencyTarget,
) -> Result<ProductionEfficiencySeries, String> {
    let game_loops = timeline.game_loops;
    let max_loops = if timeline.max_time_seconds == 0 {
        0
    } else {
        (timeline.max_time_seconds as f64 * timeline.loops_per_second).round() as u32
    };
    let effective_end = if max_loops == 0 {
        game_loops
    } else {
        game_loops.min(max_loops)
    };

    // Largura do bucket em game loops. Clampa em ≥ 1 para evitar
    // divisão por zero quando `loops_per_second` vem zerado (replays
    // patológicos).
    let bucket_loops = if timeline.loops_per_second > 0.0 {
        ((CHART_BUCKET_SECONDS * timeline.loops_per_second).round() as u32).max(1)
    } else {
        1
    };

    let players = timeline
        .players
        .iter()
        .map(|player| {
            if is_zerg_race(&player.race) {
                return PlayerEfficiencySeries {
                    name: player.name.clone(),
                    race: player.race.clone(),
                    is_zerg: true,
                    samples: Vec::new(),
                };
            }

            let samples = match target {
                EfficiencyTarget::Workers => {
                    compute_series_workers(player, effective_end, bucket_loops)
                }
                EfficiencyTarget::Army => compute_series_army(
                    player,
                    effective_end,
                    timeline.base_build,
                    bucket_loops,
                ),
            };

            PlayerEfficiencySeries {
                name: player.name.clone(),
                race: player.race.clone(),
                is_zerg: false,
                samples,
            }
        })
        .collect();

    Ok(ProductionEfficiencySeries {
        players,
        target,
        loops_per_second: timeline.loops_per_second,
        game_loops,
    })
}

// ── Merge de eventos ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvKind {
    CapacityUp,
    ProdStart,
    ProdEnd,
    CapacityDown,
    /// Transição de `supply_used` subindo acima de `ARMY_SUPPLY_MAXED_THRESHOLD`.
    SupplyMaxedOn,
    /// Transição de `supply_used` caindo até ≤ `ARMY_SUPPLY_MAXED_THRESHOLD`.
    SupplyMaxedOff,
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

fn compute_series_workers(
    player: &PlayerTimeline,
    game_end: u32,
    bucket_loops: u32,
) -> Vec<EfficiencySample> {
    let mut evs: Vec<(u32, EvKind)> = Vec::new();
    for &(gl, delta) in &player.worker_capacity {
        evs.push((gl, if delta > 0 { EvKind::CapacityUp } else { EvKind::CapacityDown }));
    }
    for &birth in &player.worker_births {
        let start = birth.saturating_sub(WORKER_BUILD_TIME);
        evs.push((start, EvKind::ProdStart));
        evs.push((birth, EvKind::ProdEnd));
    }
    sweep(evs, game_end, bucket_loops)
}

fn compute_series_army(
    player: &PlayerTimeline,
    game_end: u32,
    base_build: u32,
    bucket_loops: u32,
) -> Vec<EfficiencySample> {
    let mut evs: Vec<(u32, EvKind)> = Vec::new();
    for &(gl, delta) in &player.army_capacity {
        evs.push((gl, if delta > 0 { EvKind::CapacityUp } else { EvKind::CapacityDown }));
    }

    // Pareia ProductionStarted com ProductionFinished/Cancelled por tag,
    // filtrando `category == Unit`. Guardamos (start_loop, entity_type)
    // para back-datar o start quando o tracker emitiu ambos os eventos
    // no mesmo loop (caso típico dos trains Terran vindos via UnitBorn
    // sem UnitInit anterior — veja `tracker::prev_lifecycle == None`).
    // Eventos sem par (órfãos) fecham no game_end.
    let mut starts: HashMap<i64, (u32, String)> = HashMap::new();
    for ev in &player.entity_events {
        if ev.category != EntityCategory::Unit {
            continue;
        }
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                // Aceita apenas o primeiro Started por tag — evita que
                // um eventual re-emit duplique o slot.
                starts
                    .entry(ev.tag)
                    .or_insert_with(|| (ev.game_loop, ev.entity_type.clone()));
            }
            EntityEventKind::ProductionFinished | EntityEventKind::ProductionCancelled => {
                if let Some((start_loop, entity_type)) = starts.remove(&ev.tag) {
                    let start_loop = if start_loop >= ev.game_loop {
                        // Started e Finished/Cancelled no mesmo loop:
                        // back-data pelo build_time do balance data,
                        // análogo ao `WORKER_BUILD_TIME` aplicado aos
                        // `worker_births`. Sem back-data, a produção
                        // apareceria como instantânea (active +1 -1 no
                        // mesmo tick) e a eficiência ficaria zerada.
                        let bt = balance_data::build_time_loops(&entity_type, base_build);
                        ev.game_loop.saturating_sub(bt)
                    } else {
                        start_loop
                    };
                    evs.push((start_loop, EvKind::ProdStart));
                    evs.push((ev.game_loop, EvKind::ProdEnd));
                }
            }
            EntityEventKind::Died => {}
        }
    }
    // Órfãos (sem finish/cancel): fechar em game_end.
    for (_tag, (start_loop, _entity_type)) in starts {
        evs.push((start_loop, EvKind::ProdStart));
        evs.push((game_end, EvKind::ProdEnd));
    }

    // Emite transições de supply para forçar 100% quando o jogador
    // está "supply maxed". Derivado dos `stats` snapshots (amostragem
    // ~10 ticks/min). Entre snapshots, o `supply_used` é assumido
    // constante no valor do último snapshot — aproximação adequada
    // dado que os buckets do gráfico são de 10s.
    let mut supply_was_high = false;
    for s in &player.stats {
        let now_high = s.supply_used > ARMY_SUPPLY_MAXED_THRESHOLD;
        if now_high != supply_was_high {
            evs.push((
                s.game_loop,
                if now_high { EvKind::SupplyMaxedOn } else { EvKind::SupplyMaxedOff },
            ));
            supply_was_high = now_high;
        }
    }

    sweep(evs, game_end, bucket_loops)
}

/// Varre os eventos ordenados e emite uma amostra por bucket de
/// `bucket_loops` game loops. Cada amostra reporta a **média
/// ponderada pelo tempo** da eficiência dentro do bucket:
///
/// ```text
/// pct = 100 × Σ min(active, capacity)·dt  /  Σ capacity·dt
/// ```
///
/// Se o bucket inteiro teve `capacity == 0`, devolvemos 100% (mesmo
/// sentinel usado antes — sem capacidade não há ociosidade real).
/// O `game_loop` da amostra é o **fim** do bucket; o último pode
/// ser parcial (trunca em `game_end`). Buckets seguem a convenção
/// semi-aberta `[bucket_start, bucket_end)` — eventos exatamente em
/// `bucket_end` caem no próximo bucket.
fn sweep(
    mut events: Vec<(u32, EvKind)>,
    game_end: u32,
    bucket_loops: u32,
) -> Vec<EfficiencySample> {
    events.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.order().cmp(&b.1.order())));

    if game_end == 0 || bucket_loops == 0 {
        return Vec::new();
    }

    let mut samples: Vec<EfficiencySample> = Vec::new();
    let mut capacity: i32 = 0;
    let mut active: i32 = 0;
    // `supply_high` é alternado por SupplyMaxedOn/Off — quando ligado,
    // a integração trata `active = capacity` para o dt (jogador não
    // pode produzir mais army supply, não penaliza ociosidade).
    let mut supply_high: bool = false;
    let mut i = 0usize;
    let mut cursor: u32 = 0;
    let mut bucket_start: u32 = 0;

    while bucket_start < game_end {
        let bucket_end = (bucket_start + bucket_loops).min(game_end);
        let mut cap_integral: u64 = 0;
        let mut act_integral: u64 = 0;

        // Processa todos os eventos dentro de [bucket_start, bucket_end).
        while i < events.len() && events[i].0 < bucket_end {
            let ev_gl = events[i].0;
            if ev_gl > cursor {
                let dt = (ev_gl - cursor) as u64;
                let cap = capacity.max(0) as u64;
                let act = if supply_high {
                    cap
                } else {
                    active.max(0).min(capacity.max(0)) as u64
                };
                cap_integral += cap * dt;
                act_integral += act * dt;
            }
            // Aplica todos os eventos deste `ev_gl` (empate resolvido
            // pela ordem canônica em `EvKind::order`).
            while i < events.len() && events[i].0 == ev_gl {
                match events[i].1 {
                    EvKind::CapacityUp => capacity += 1,
                    EvKind::CapacityDown => capacity = (capacity - 1).max(0),
                    EvKind::ProdStart => active += 1,
                    EvKind::ProdEnd => active = (active - 1).max(0),
                    EvKind::SupplyMaxedOn => supply_high = true,
                    EvKind::SupplyMaxedOff => supply_high = false,
                }
                i += 1;
            }
            cursor = ev_gl;
        }

        // Cauda [cursor, bucket_end) com o estado corrente.
        if bucket_end > cursor {
            let dt = (bucket_end - cursor) as u64;
            let cap = capacity.max(0) as u64;
            let act = if supply_high {
                cap
            } else {
                active.max(0).min(capacity.max(0)) as u64
            };
            cap_integral += cap * dt;
            act_integral += act * dt;
            cursor = bucket_end;
        }

        let pct = if cap_integral == 0 {
            100.0
        } else {
            100.0 * act_integral as f64 / cap_integral as f64
        };
        // `capacity`/`active` reportam o estado ao fim do bucket —
        // não são usados pelo gráfico (só `efficiency_pct`), mas
        // continuam expostos no struct para eventual inspeção.
        samples.push(EfficiencySample {
            game_loop: bucket_end,
            capacity: capacity.max(0) as u32,
            active: active.max(0).min(capacity.max(0)) as u32,
            efficiency_pct: pct,
        });

        bucket_start = bucket_end;
    }

    samples
}

// ── Testes ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{EntityEvent, ReplayTimeline};

    fn mk_timeline(players: Vec<PlayerTimeline>, game_loops: u32) -> ReplayTimeline {
        ReplayTimeline {
            file: String::new(),
            map: String::new(),
            datetime: String::new(),
            game_loops,
            duration_seconds: game_loops / 16,
            loops_per_second: 22.4,
            base_build: 0,
            max_time_seconds: 0,
            players,
            chat: Vec::new(),
            cache_handles: Vec::new(),
            map_size_x: 0,
            map_size_y: 0,
            resources: Vec::new(),
        }
    }

    fn mk_player(race: &str) -> PlayerTimeline {
        PlayerTimeline {
            name: "P".to_string(),
            clan: String::new(),
            race: race.to_string(),
            mmr: None,
            player_id: 1,
            result: None,
            stats: Vec::new(),
            upgrades: Vec::new(),
            entity_events: Vec::new(),
            production_cmds: Vec::new(),
            inject_cmds: Vec::new(),
            unit_positions: Vec::new(),
            camera_positions: Vec::new(),
            alive_count: std::collections::HashMap::new(),
            worker_capacity: Vec::new(),
            worker_births: Vec::new(),
            army_capacity: Vec::new(),
            upgrade_cumulative: Vec::new(),
            creep_index: Vec::new(),
        }
    }

    /// Tolerância padrão para comparações de pct (erros de ponto
    /// flutuante no cálculo da média ponderada).
    const EPS: f64 = 1e-6;

    /// Localiza a amostra cujo `game_loop` (fim do bucket) bate
    /// exatamente com `t`. Retorna `None` se o bucket não existe —
    /// usado nos testes para asserts em boundaries conhecidos.
    fn bucket_ending_at(samples: &[EfficiencySample], t: u32) -> EfficiencySample {
        *samples
            .iter()
            .find(|s| s.game_loop == t)
            .unwrap_or_else(|| panic!("no bucket ending at game_loop={t}"))
    }

    #[test]
    fn workers_baseline_single_cc_one_birth() {
        // lps=22.4 → bucket_loops = 224 (10s). game_loops=1000.
        // worker_capacity [(0, +1)], worker_births [300].
        // ProdStart = 300 − 272 = 28, ProdEnd = 300.
        let mut p = mk_player("Terr");
        p.worker_capacity.push((0, 1));
        p.worker_births.push(300);
        let tl = mk_timeline(vec![p], 1000);

        let series = extract_efficiency_series(&tl, EfficiencyTarget::Workers).unwrap();
        let s = &series.players[0].samples;

        // Buckets devem terminar em 224, 448, 672, 896 e 1000 (parcial).
        assert_eq!(s[0].game_loop, 224);
        // [0, 224): cap=1, active=0 em [0,28) e active=1 em [28,224).
        //   cap_int = 224, act_int = 196 → 87.5%.
        assert!((s[0].efficiency_pct - (196.0 / 224.0 * 100.0)).abs() < EPS);
        // [224, 448): cap=1; active=1 em [224,300), active=0 em [300,448).
        //   cap_int = 224, act_int = 76 → 33.93%.
        assert_eq!(s[1].game_loop, 448);
        assert!((s[1].efficiency_pct - (76.0 / 224.0 * 100.0)).abs() < EPS);
        // [448, 672) e seguintes: cap=1, active=0 → 0%.
        assert_eq!(s[2].game_loop, 672);
        assert_eq!(s[2].efficiency_pct, 0.0);
        // Último bucket é parcial (ends at game_end=1000).
        assert_eq!(s.last().unwrap().game_loop, 1000);
    }

    #[test]
    fn army_baseline_one_barracks_one_unit() {
        let mut p = mk_player("Terr");
        p.army_capacity.push((100, 1));
        p.entity_events.push(EntityEvent {
            game_loop: 200,
            seq: 0,
            kind: EntityEventKind::ProductionStarted,
            entity_type: "Marine".to_string(),
            category: EntityCategory::Unit,
            tag: 42,
            pos_x: 0,
            pos_y: 0,
            creator_ability: Some("TrainMarine".to_string()),
            creator_tag: None,
            killer_player_id: None,
        });
        p.entity_events.push(EntityEvent {
            game_loop: 500,
            seq: 1,
            kind: EntityEventKind::ProductionFinished,
            entity_type: "Marine".to_string(),
            category: EntityCategory::Unit,
            tag: 42,
            pos_x: 0,
            pos_y: 0,
            creator_ability: None,
            creator_tag: None,
            killer_player_id: None,
        });
        let tl = mk_timeline(vec![p], 1000);

        let series = extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap();
        let s = &series.players[0].samples;

        // [0, 224): cap=0 em [0,100), cap=1 em [100,224); active=1 em [200,224).
        //   cap_int = 124, act_int = 24 → ≈ 19.35%.
        let b0 = bucket_ending_at(s, 224);
        assert!((b0.efficiency_pct - (24.0 / 124.0 * 100.0)).abs() < EPS);
        // [224, 448): cap=1, active=1 → 100%.
        let b1 = bucket_ending_at(s, 448);
        assert!((b1.efficiency_pct - 100.0).abs() < EPS);
        // [448, 672): active=1 em [448,500), active=0 em [500,672).
        //   cap_int = 224, act_int = 52 → ≈ 23.21%.
        let b2 = bucket_ending_at(s, 672);
        assert!((b2.efficiency_pct - (52.0 / 224.0 * 100.0)).abs() < EPS);
        // Buckets seguintes: cap=1, active=0 → 0%.
        let b3 = bucket_ending_at(s, 896);
        assert_eq!(b3.efficiency_pct, 0.0);
    }

    #[test]
    fn zerg_returns_empty_samples() {
        let mut p = mk_player("Zerg");
        p.army_capacity.push((100, 1));
        let tl = mk_timeline(vec![p], 1000);

        let w = extract_efficiency_series(&tl, EfficiencyTarget::Workers).unwrap();
        let a = extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap();
        assert!(w.players[0].is_zerg);
        assert!(a.players[0].is_zerg);
        assert!(w.players[0].samples.is_empty());
        assert!(a.players[0].samples.is_empty());
    }

    #[test]
    fn capacity_loss_during_production_no_underflow() {
        // Barracks morre no mesmo loop em que a produção é cancelada.
        let mut p = mk_player("Terr");
        p.army_capacity.push((100, 1));
        p.army_capacity.push((300, -1));
        p.entity_events.push(EntityEvent {
            game_loop: 200,
            seq: 0,
            kind: EntityEventKind::ProductionStarted,
            entity_type: "Marine".to_string(),
            category: EntityCategory::Unit,
            tag: 7,
            pos_x: 0,
            pos_y: 0,
            creator_ability: Some("TrainMarine".to_string()),
            creator_tag: None,
            killer_player_id: None,
        });
        p.entity_events.push(EntityEvent {
            game_loop: 300,
            seq: 1,
            kind: EntityEventKind::ProductionCancelled,
            entity_type: "Marine".to_string(),
            category: EntityCategory::Unit,
            tag: 7,
            pos_x: 0,
            pos_y: 0,
            creator_ability: None,
            creator_tag: None,
            killer_player_id: None,
        });
        let tl = mk_timeline(vec![p], 1000);

        let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
        // [224, 448): em 300 ProdEnd (ordem 2) aplica antes de CapacityDown (ordem 3),
        // então não houve underflow. active=1 em [224,300) e cap=0 em [300,448).
        //   cap_int = 76, act_int = 76 → 100%.
        let mid = bucket_ending_at(s, 448);
        assert!((mid.efficiency_pct - 100.0).abs() < EPS);
        // Bucket após a morte do Barracks: cap_int inteiro = 0 → sentinel 100%.
        let late = bucket_ending_at(s, 672);
        assert_eq!(late.capacity, 0);
        assert_eq!(late.active, 0);
        assert_eq!(late.efficiency_pct, 100.0);
    }

    #[test]
    fn army_train_started_and_finished_same_loop_is_back_dated() {
        // Cenário Terran típico: tracker emite Started+Finished no
        // mesmo game_loop porque o UnitBorn não foi precedido de
        // UnitInit (trains Terran vêm direto de UnitBornEvent). Sem
        // back-data, a produção parece instantânea e a eficiência
        // fica zerada durante toda a janela real de produção.
        let mut p = mk_player("Terr");
        p.army_capacity.push((0, 1));
        // "Marine" existe no balance data — build_time_loops devolve
        // ~272 loops. Usamos um entity_type conhecido para que o
        // lookup retorne um valor > 0; o teste só precisa garantir
        // que a eficiência é 100% em algum instante entre a janela
        // back-dated e o finish.
        let finish = 500u32;
        p.entity_events.push(EntityEvent {
            game_loop: finish,
            seq: 0,
            kind: EntityEventKind::ProductionStarted,
            entity_type: "Marine".to_string(),
            category: EntityCategory::Unit,
            tag: 1,
            pos_x: 0,
            pos_y: 0,
            creator_ability: Some("TrainMarine".to_string()),
            creator_tag: None,
            killer_player_id: None,
        });
        p.entity_events.push(EntityEvent {
            game_loop: finish,
            seq: 1,
            kind: EntityEventKind::ProductionFinished,
            entity_type: "Marine".to_string(),
            category: EntityCategory::Unit,
            tag: 1,
            pos_x: 0,
            pos_y: 0,
            creator_ability: None,
            creator_tag: None,
            killer_player_id: None,
        });
        let tl = mk_timeline(vec![p], 1000);

        let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
        // Sem back-data o Started/Finished coincidiriam no loop 500 e
        // nenhum tempo de produção entraria nos buckets — eficiência 0%
        // em todos eles. Com back-data para 228 (500 − 272), o bucket
        // [224, 448) fica quase inteiro em produção (active=1 em
        // [228, 448) = 220 loops de 224) → ~98%.
        let back_dated = bucket_ending_at(s, 448);
        assert!(
            back_dated.efficiency_pct > 50.0,
            "back-data deveria levar o bucket a >50%, veio {}",
            back_dated.efficiency_pct
        );
    }

    #[test]
    fn orphan_started_closes_at_game_end() {
        let mut p = mk_player("Terr");
        p.army_capacity.push((100, 1));
        p.entity_events.push(EntityEvent {
            game_loop: 200,
            seq: 0,
            kind: EntityEventKind::ProductionStarted,
            entity_type: "Marine".to_string(),
            category: EntityCategory::Unit,
            tag: 11,
            pos_x: 0,
            pos_y: 0,
            creator_ability: Some("TrainMarine".to_string()),
            creator_tag: None,
            killer_player_id: None,
        });
        // Sem Finished nem Cancelled.
        let tl = mk_timeline(vec![p], 1000);

        let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
        // Buckets inteiramente dentro de [200, 1000] devem ficar em 100%
        // — se o órfão não tivesse sido fechado em game_end, `active`
        // não seria decrementado e igualmente daria 100%; então esse
        // teste garante sobretudo que o sweep atravessa o stream até o
        // fim sem parar.
        let b_mid = bucket_ending_at(s, 672);
        let b_late = bucket_ending_at(s, 896);
        assert!((b_mid.efficiency_pct - 100.0).abs() < EPS);
        assert!((b_late.efficiency_pct - 100.0).abs() < EPS);
    }

    fn mk_stats(game_loop: u32, supply_used: i32) -> crate::replay::StatsSnapshot {
        crate::replay::StatsSnapshot {
            game_loop,
            minerals: 0,
            vespene: 0,
            minerals_rate: 0,
            vespene_rate: 0,
            workers: 0,
            supply_used,
            supply_made: 200,
            army_value_minerals: 0,
            army_value_vespene: 0,
            minerals_lost_army: 0,
            vespene_lost_army: 0,
            minerals_killed_army: 0,
            vespene_killed_army: 0,
        }
    }

    #[test]
    fn army_supply_maxed_forces_hundred_percent() {
        // Cenário: 1 Barracks desde t=0, nenhuma produção ocorrendo.
        // Sem o override, a eficiência seria 0% em todos os buckets.
        // Com supply_used > 185 a partir de t=500, os buckets dentro
        // desse regime devem ficar em 100%.
        let mut p = mk_player("Terr");
        p.army_capacity.push((0, 1));
        // Snapshots: supply sobe para 190 em t=500 e fica lá.
        p.stats.push(mk_stats(0, 12));
        p.stats.push(mk_stats(200, 100));
        p.stats.push(mk_stats(500, 190));
        p.stats.push(mk_stats(1000, 190));
        let tl = mk_timeline(vec![p], 1000);

        let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
        // [0, 224) e [224, 448): supply baixo → Barracks ocioso → 0%.
        assert_eq!(bucket_ending_at(s, 224).efficiency_pct, 0.0);
        assert_eq!(bucket_ending_at(s, 448).efficiency_pct, 0.0);
        // [448, 672): supply cruza 185 em 500 → parte inicial (52
        // loops) conta como idle, restante (172 loops) como 100%.
        //   cap_int = 224, act_int = 172 → ~76.8%.
        let mixed = bucket_ending_at(s, 672);
        assert!((mixed.efficiency_pct - (172.0 / 224.0 * 100.0)).abs() < EPS);
        // [672, 896) e [896, 1000): supply_high o bucket inteiro → 100%.
        assert!((bucket_ending_at(s, 896).efficiency_pct - 100.0).abs() < EPS);
        assert!((bucket_ending_at(s, 1000).efficiency_pct - 100.0).abs() < EPS);
    }

    #[test]
    fn workers_ignore_supply_maxed_override() {
        // O override é só para army. Workers em idle com supply maxed
        // continuam contando como idle (o jogador pode colocar workers
        // em gás, refinery extra, etc. — idleness de CC é real).
        let mut p = mk_player("Terr");
        p.worker_capacity.push((0, 1));
        // Nenhum worker sendo produzido, supply maxed desde o começo.
        p.stats.push(mk_stats(0, 190));
        let tl = mk_timeline(vec![p], 500);

        let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers).unwrap().players[0].samples;
        // Todos os buckets: capacity=1, active=0 → 0%.
        for sample in s {
            assert_eq!(sample.efficiency_pct, 0.0);
        }
    }
}
