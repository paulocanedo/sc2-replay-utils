use super::*;

#[test]
fn baseline_single_cc_one_birth() {
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
fn ignore_supply_maxed_override() {
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
