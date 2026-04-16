use super::*;

// ── Helpers Zerg ────────────────────────────────────────────────────

fn mk_hatch_finished(loop_: u32, seq: u32, tag: i64) -> EntityEvent {
    EntityEvent {
        game_loop: loop_,
        seq,
        kind: EntityEventKind::ProductionFinished,
        entity_type: "Hatchery".to_string(),
        category: EntityCategory::Structure,
        tag,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    }
}

fn mk_hatch_died(loop_: u32, seq: u32, tag: i64) -> EntityEvent {
    EntityEvent {
        game_loop: loop_,
        seq,
        kind: EntityEventKind::Died,
        entity_type: "Hatchery".to_string(),
        category: EntityCategory::Structure,
        tag,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    }
}

fn mk_unit_started(
    loop_: u32,
    seq: u32,
    entity_type: &str,
    tag: i64,
    category: EntityCategory,
) -> EntityEvent {
    EntityEvent {
        game_loop: loop_,
        seq,
        kind: EntityEventKind::ProductionStarted,
        entity_type: entity_type.to_string(),
        category,
        tag,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    }
}

fn mk_unit_finished(
    loop_: u32,
    seq: u32,
    entity_type: &str,
    tag: i64,
    category: EntityCategory,
) -> EntityEvent {
    EntityEvent {
        game_loop: loop_,
        seq,
        kind: EntityEventKind::ProductionFinished,
        entity_type: entity_type.to_string(),
        category,
        tag,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    }
}

fn mk_unit_cancelled(
    loop_: u32,
    seq: u32,
    entity_type: &str,
    tag: i64,
    category: EntityCategory,
) -> EntityEvent {
    EntityEvent {
        game_loop: loop_,
        seq,
        kind: EntityEventKind::ProductionCancelled,
        entity_type: entity_type.to_string(),
        category,
        tag,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    }
}

fn mk_inject(loop_: u32) -> crate::replay::InjectCmd {
    crate::replay::InjectCmd {
        game_loop: loop_,
        target_tag_index: 0,
        target_type: "Hatchery".to_string(),
        target_x: 0,
        target_y: 0,
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

// ── Testes ──────────────────────────────────────────────────────────

#[test]
fn workers_single_hatch_continuous_is_100pct() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.entity_events.push(mk_unit_started(28, 1, "Drone", 1, EntityCategory::Worker));
    p.entity_events.push(mk_unit_finished(300, 2, "Drone", 1, EntityCategory::Worker));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    let b0 = bucket_ending_at(s, 224);
    assert!((b0.efficiency_pct - (196.0 / 224.0 * 100.0)).abs() < EPS);
    let b1 = bucket_ending_at(s, 448);
    assert!((b1.efficiency_pct - (76.0 / 224.0 * 100.0)).abs() < EPS);
}

#[test]
fn workers_inject_boost_with_full_spend() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.inject_cmds.push(mk_inject(500));
    for (i, start) in [900u32, 900, 900, 900].iter().enumerate() {
        p.entity_events.push(mk_unit_started(
            *start,
            (i as u32) * 2,
            "Drone",
            (i + 1) as i64,
            EntityCategory::Worker,
        ));
        p.entity_events.push(mk_unit_finished(
            start + 272,
            (i as u32) * 2 + 1,
            "Drone",
            (i + 1) as i64,
            EntityCategory::Worker,
        ));
    }
    let tl = mk_timeline(vec![p], 1500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    let b1 = bucket_ending_at(s, 672);
    assert_eq!(b1.efficiency_pct, 0.0);

    let b_spend = bucket_ending_at(s, 1120);
    assert!(
        (b_spend.efficiency_pct - (880.0 / 1120.0 * 100.0)).abs() < EPS,
        "esperava ~78.57, veio {}",
        b_spend.efficiency_pct
    );
}

#[test]
fn workers_inject_ignored_drops_efficiency() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.inject_cmds.push(mk_inject(500));
    let tl = mk_timeline(vec![p], 1500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    for sample in s {
        assert_eq!(
            sample.efficiency_pct, 0.0,
            "bucket ending at {} esperava 0%, veio {}",
            sample.game_loop, sample.efficiency_pct
        );
    }
}

#[test]
fn army_only_larva_born_counted() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.entity_events
        .push(mk_unit_started(100, 1, "Zergling", 1, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 2, "Zergling", 1, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_started(100, 3, "Baneling", 2, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 4, "Baneling", 2, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_started(100, 5, "Queen", 3, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 6, "Queen", 3, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_started(100, 7, "Overlord", 4, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 8, "Overlord", 4, EntityCategory::Unit));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army)
        .unwrap()
        .players[0]
        .samples;

    let b0 = bucket_ending_at(s, 224);
    assert!(
        (b0.efficiency_pct - (124.0 / 224.0 * 100.0)).abs() < EPS,
        "bucket 224 esperava {}, veio {}",
        124.0 / 224.0 * 100.0,
        b0.efficiency_pct
    );
    assert_eq!(b0.active, 1);
    assert_eq!(b0.capacity, 1);
}

#[test]
fn hatch_morph_lair_no_capacity_gap() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.entity_events.push(mk_hatch_died(1000, 1, 100));
    p.entity_events.push(EntityEvent {
        game_loop: 1000,
        seq: 2,
        kind: EntityEventKind::ProductionFinished,
        entity_type: "Lair".to_string(),
        category: EntityCategory::Structure,
        tag: 100,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    });
    let tl = mk_timeline(vec![p], 1500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    for sample in s {
        assert_eq!(
            sample.capacity, 1,
            "bucket em {} caiu para cap={}; esperava manter 1 durante o morph",
            sample.game_loop, sample.capacity
        );
    }
}

#[test]
fn hatch_destroyed_cancels_inflight_morph() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.entity_events
        .push(mk_unit_started(300, 1, "Drone", 7, EntityCategory::Worker));
    p.entity_events.push(mk_hatch_died(400, 2, 100));
    p.entity_events
        .push(mk_unit_cancelled(400, 3, "Drone", 7, EntityCategory::Worker));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    let late = bucket_ending_at(s, 672);
    assert_eq!(late.capacity, 0);
    assert_eq!(late.active, 0);
    assert_eq!(late.efficiency_pct, 100.0);
}

#[test]
fn army_supply_maxed_forces_hundred_percent() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.stats.push(mk_stats(0, 12));
    p.stats.push(mk_stats(200, 100));
    p.stats.push(mk_stats(500, 190));
    p.stats.push(mk_stats(1000, 190));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army)
        .unwrap()
        .players[0]
        .samples;

    assert_eq!(bucket_ending_at(s, 224).efficiency_pct, 0.0);
    assert_eq!(bucket_ending_at(s, 448).efficiency_pct, 0.0);
    let mixed = bucket_ending_at(s, 672);
    assert!((mixed.efficiency_pct - (172.0 / 224.0 * 100.0)).abs() < EPS);
    assert!((bucket_ending_at(s, 896).efficiency_pct - 100.0).abs() < EPS);
}

#[test]
fn workers_ignore_supply_maxed_override() {
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.stats.push(mk_stats(0, 190));
    let tl = mk_timeline(vec![p], 500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    for sample in s {
        assert_eq!(
            sample.efficiency_pct, 0.0,
            "workers Zerg não deveriam pegar override; bucket {} veio {}",
            sample.game_loop, sample.efficiency_pct
        );
    }
}
