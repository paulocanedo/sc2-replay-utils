use super::*;

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
fn baseline_one_barracks_one_unit() {
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
fn train_started_and_finished_same_loop_is_back_dated() {
    // Cenário Terran típico: tracker emite Started+Finished no
    // mesmo game_loop porque o UnitBorn não foi precedido de
    // UnitInit (trains Terran vêm direto de UnitBornEvent). Sem
    // back-data, a produção parece instantânea e a eficiência
    // fica zerada durante toda a janela real de produção.
    let mut p = mk_player("Terr");
    p.army_capacity.push((0, 1));
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
    let b_mid = bucket_ending_at(s, 672);
    let b_late = bucket_ending_at(s, 896);
    assert!((b_mid.efficiency_pct - 100.0).abs() < EPS);
    assert!((b_late.efficiency_pct - 100.0).abs() < EPS);
}

#[test]
fn supply_maxed_forces_hundred_percent() {
    let mut p = mk_player("Terr");
    p.army_capacity.push((0, 1));
    p.stats.push(mk_stats(0, 12));
    p.stats.push(mk_stats(200, 100));
    p.stats.push(mk_stats(500, 190));
    p.stats.push(mk_stats(1000, 190));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
    assert_eq!(bucket_ending_at(s, 224).efficiency_pct, 0.0);
    assert_eq!(bucket_ending_at(s, 448).efficiency_pct, 0.0);
    let mixed = bucket_ending_at(s, 672);
    assert!((mixed.efficiency_pct - (172.0 / 224.0 * 100.0)).abs() < EPS);
    assert!((bucket_ending_at(s, 896).efficiency_pct - 100.0).abs() < EPS);
    assert!((bucket_ending_at(s, 1000).efficiency_pct - 100.0).abs() < EPS);
}
