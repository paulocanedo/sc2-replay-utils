use super::*;

fn mk_upgrade(game_loop: u32, name: &str) -> UpgradeEntry {
    UpgradeEntry { game_loop, seq: 0, name: name.to_string() }
}

/// Nome do upgrade da WarpGate — reusado da constante em `types.rs`.
const WARP_GATE_RESEARCH: &str = "WarpGateResearch";

#[test]
fn extends_busy_window_with_cooldown() {
    let mut p = mk_player("Prot");
    p.army_capacity.push((0, 1));
    p.upgrades.push(mk_upgrade(100, WARP_GATE_RESEARCH));
    p.entity_events.push(EntityEvent {
        game_loop: 500,
        seq: 0,
        kind: EntityEventKind::ProductionStarted,
        entity_type: "Zealot".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: Some("Zealot".to_string()),
        creator_tag: None,
        killer_player_id: None,
    });
    p.entity_events.push(EntityEvent {
        game_loop: 612,
        seq: 1,
        kind: EntityEventKind::ProductionFinished,
        entity_type: "Zealot".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    });
    let tl = mk_timeline(vec![p], 1500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;

    let b = bucket_ending_at(s, 672);
    assert!((b.efficiency_pct - (172.0 / 224.0 * 100.0)).abs() < EPS);
    assert!((bucket_ending_at(s, 896).efficiency_pct - 100.0).abs() < EPS);
    let partial = bucket_ending_at(s, 1120);
    assert!(
        (partial.efficiency_pct - (164.0 / 224.0 * 100.0)).abs() < EPS,
        "bucket 1120 esperava {}, veio {}",
        164.0 / 224.0 * 100.0,
        partial.efficiency_pct
    );
    assert_eq!(bucket_ending_at(s, 1344).efficiency_pct, 0.0);
}

#[test]
fn cycle_not_applied_before_research() {
    let mut p = mk_player("Prot");
    p.army_capacity.push((0, 1));
    p.upgrades.push(mk_upgrade(800, WARP_GATE_RESEARCH));
    p.entity_events.push(EntityEvent {
        game_loop: 200,
        seq: 0,
        kind: EntityEventKind::ProductionStarted,
        entity_type: "Zealot".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: Some("Zealot".to_string()),
        creator_tag: None,
        killer_player_id: None,
    });
    p.entity_events.push(EntityEvent {
        game_loop: 500,
        seq: 1,
        kind: EntityEventKind::ProductionFinished,
        entity_type: "Zealot".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    });
    let tl = mk_timeline(vec![p], 1500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
    let b = bucket_ending_at(s, 672);
    assert!((b.efficiency_pct - (52.0 / 224.0 * 100.0)).abs() < EPS);
    assert_eq!(bucket_ending_at(s, 896).efficiency_pct, 0.0);
}

#[test]
fn only_extends_for_warp_gate_units() {
    let mut p = mk_player("Prot");
    p.army_capacity.push((0, 1));
    p.upgrades.push(mk_upgrade(100, WARP_GATE_RESEARCH));
    p.entity_events.push(EntityEvent {
        game_loop: 300,
        seq: 0,
        kind: EntityEventKind::ProductionStarted,
        entity_type: "Immortal".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: Some("Immortal".to_string()),
        creator_tag: None,
        killer_player_id: None,
    });
    p.entity_events.push(EntityEvent {
        game_loop: 500,
        seq: 1,
        kind: EntityEventKind::ProductionFinished,
        entity_type: "Immortal".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    });
    let tl = mk_timeline(vec![p], 1200);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
    assert_eq!(bucket_ending_at(s, 896).efficiency_pct, 0.0);
}
