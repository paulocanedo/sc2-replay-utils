// Algoritmo de sweep: varre eventos ordenados e emite uma amostra por
// bucket de game loops com a média ponderada pelo tempo da eficiência.

use super::types::{EfficiencySample, EvKind, INJECT_EXTRA_SLOTS};

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
pub(super) fn sweep(
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
                    EvKind::InjectOn => capacity += INJECT_EXTRA_SLOTS,
                    EvKind::InjectOff => {
                        capacity = (capacity - INJECT_EXTRA_SLOTS).max(0)
                    }
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
