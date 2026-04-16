//! Invariantes dos índices derivados — cada índice derivado deve
//! bater com o que é derivável do stream canônico correspondente.

use super::*;

const WORKER_PRODUCERS: &[&str] = &["CommandCenter", "OrbitalCommand", "PlanetaryFortress", "Nexus"];
const ARMY_PRODUCERS: &[&str] = &[
    "Barracks", "Factory", "Starport", "Gateway", "WarpGate", "RoboticsFacility", "Stargate",
];

/// Soma cumulativa de um `Vec<(loop, delta)>` em um dado `loop`.
fn capacity_sum_at(capacity: &[(u32, i32)], loop_: u32) -> i32 {
    capacity.iter().filter(|(l, _)| *l <= loop_).map(|(_, d)| *d).sum()
}

/// Soma de `alive_count` para uma lista de tipos num dado loop.
fn alive_sum_at(p: &PlayerTimeline, types: &[&str], loop_: u32) -> i32 {
    types
        .iter()
        .map(|t| p.alive_count_at(t, loop_).max(0))
        .sum()
}

#[test]
fn worker_capacity_matches_alive_producers() {
    let t = load();
    for p in &t.players {
        let mut loops: Vec<u32> = p.worker_capacity.iter().map(|(l, _)| *l).collect();
        loops.sort();
        loops.dedup();
        for loop_ in loops {
            let cap = capacity_sum_at(&p.worker_capacity, loop_);
            let alive = alive_sum_at(p, WORKER_PRODUCERS, loop_);
            assert!(cap >= 0, "worker_capacity negativo em loop {}: {}", loop_, cap);
            assert!(
                cap <= alive,
                "worker_capacity ({}) > alive producers ({}) em loop {}",
                cap,
                alive,
                loop_
            );
        }
        let end = t.game_loops;
        let cap_end = capacity_sum_at(&p.worker_capacity, end);
        let alive_end = alive_sum_at(p, WORKER_PRODUCERS, end);
        assert_eq!(
            cap_end, alive_end,
            "worker_capacity ({}) != alive_producers ({}) no fim do replay (player {})",
            cap_end, alive_end, p.name
        );
    }
}

#[test]
fn capacity_cumulative_matches_delta_sum() {
    let t = load();
    for p in &t.players {
        assert_eq!(
            p.worker_capacity.len(),
            p.worker_capacity_cumulative.len(),
            "worker cumulative len != deltas len (player {})", p.name,
        );
        let mut sum = 0i32;
        for (i, &(loop_, delta)) in p.worker_capacity.iter().enumerate() {
            sum += delta;
            let (cum_loop, cum_sum) = p.worker_capacity_cumulative[i];
            assert_eq!(cum_loop, loop_, "worker cumulative loop mismatch at {}", i);
            assert_eq!(cum_sum, sum, "worker cumulative sum mismatch at {}", i);
        }

        assert_eq!(
            p.army_capacity.len(),
            p.army_capacity_cumulative.len(),
            "army cumulative len != deltas len (player {})", p.name,
        );
        let mut sum = 0i32;
        for (i, &(loop_, delta)) in p.army_capacity.iter().enumerate() {
            sum += delta;
            let (cum_loop, cum_sum) = p.army_capacity_cumulative[i];
            assert_eq!(cum_loop, loop_, "army cumulative loop mismatch at {}", i);
            assert_eq!(cum_sum, sum, "army cumulative sum mismatch at {}", i);
        }
    }
}

#[test]
fn army_capacity_matches_alive_producers() {
    let t = load();
    for p in &t.players {
        let end = t.game_loops;
        let cap_end = capacity_sum_at(&p.army_capacity, end);
        let alive_end = alive_sum_at(p, ARMY_PRODUCERS, end);
        assert_eq!(
            cap_end, alive_end,
            "army_capacity ({}) != alive_producers ({}) no fim do replay (player {})",
            cap_end, alive_end, p.name
        );
        for (loop_, _) in &p.army_capacity {
            let cap = capacity_sum_at(&p.army_capacity, *loop_);
            let alive = alive_sum_at(p, ARMY_PRODUCERS, *loop_);
            assert!(cap >= 0, "army_capacity negativo em loop {}", loop_);
            assert!(cap <= alive, "army_capacity ({}) > alive ({}) em loop {}", cap, alive, loop_);
        }
    }
}

#[test]
fn worker_births_matches_train_events() {
    let t = load();
    for p in &t.players {
        let trained: Vec<u32> = p
            .entity_events
            .iter()
            .filter(|e| {
                matches!(e.kind, EntityEventKind::ProductionStarted)
                    && matches!(e.entity_type.as_str(), "SCV" | "Probe")
                    && e
                        .creator_ability
                        .as_deref()
                        .map(|a| a.contains("Train"))
                        .unwrap_or(false)
            })
            .map(|e| e.game_loop)
            .collect();
        assert_eq!(
            p.worker_births.len(),
            trained.len(),
            "worker_births.len() != #Train events (player {})",
            p.name
        );
        let mut births = p.worker_births.clone();
        let mut expected = trained;
        births.sort();
        expected.sort();
        assert_eq!(births, expected, "worker_births não bate com Train events");
    }
}

#[test]
fn upgrade_cumulative_monotonic() {
    let t = load();
    for p in &t.players {
        assert_eq!(
            p.upgrade_cumulative.len(),
            p.upgrades.len(),
            "upgrade_cumulative len != upgrades len (player {})",
            p.name
        );
        for w in p.upgrade_cumulative.windows(2) {
            let (l0, a0, r0) = w[0];
            let (l1, a1, r1) = w[1];
            assert!(l0 <= l1, "upgrade_cumulative não ordenado: {} > {}", l0, l1);
            assert!(a1 >= a0, "attack level diminuiu em loop {}: {} → {}", l1, a0, a1);
            assert!(r1 >= r0, "armor level diminuiu em loop {}: {} → {}", l1, r0, r1);
        }
    }
}

#[test]
fn morph_backfill_cc_to_orbital() {
    let t = load();
    let terran = t.players.iter().find(|p| p.race == "Terran").expect("Terran player");
    let events = &terran.entity_events;
    let mut morph_loops: Vec<u32> = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let prev = &events[i - 1];
        if matches!(ev.kind, EntityEventKind::ProductionStarted)
            && ev.entity_type == "OrbitalCommand"
            && matches!(prev.kind, EntityEventKind::Died)
            && prev.entity_type == "CommandCenter"
            && prev.tag == ev.tag
            && prev.game_loop == ev.game_loop
        {
            morph_loops.push(ev.game_loop);
        }
    }
    assert!(
        !morph_loops.is_empty(),
        "replay1.SC2Replay deveria ter ao menos um morph CC→Orbital"
    );
    for finish_loop in morph_loops {
        let expected_start = finish_loop.saturating_sub(560);
        let has_minus = terran
            .worker_capacity
            .iter()
            .any(|&(l, d)| l == expected_start && d == -1);
        let has_plus = terran
            .worker_capacity
            .iter()
            .any(|&(l, d)| l == finish_loop && d == 1);
        assert!(
            has_minus,
            "worker_capacity deveria ter (-1) em loop {} (morph_start = finish {} - 560)",
            expected_start, finish_loop
        );
        assert!(
            has_plus,
            "worker_capacity deveria ter (+1) em loop {} (morph finish)",
            finish_loop
        );
    }
}
