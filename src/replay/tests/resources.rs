use super::*;

#[test]
fn initial_stats_snapshot_prepended_per_race() {
    let t = load();
    for p in &t.players {
        let first = p.stats.first().expect("at least one stat snapshot");
        assert_eq!(first.game_loop, 0, "snapshot inicial deve estar em loop 0 para {}", p.name);
        assert_eq!(first.workers, 12, "workers iniciais: {}", p.name);
        assert_eq!(first.minerals, 50, "minerals iniciais: {}", p.name);
        assert_eq!(first.vespene, 0, "gas inicial: {}", p.name);
        assert_eq!(first.supply_used, 12, "supply usado inicial: {}", p.name);
        let expected_cap = match p.race.as_str() {
            "Zerg" => 14,
            _ => 15,
        };
        assert_eq!(first.supply_made, expected_cap, "supply cap inicial: {}", p.name);
        assert_eq!(first.army_value_minerals, 0);
        assert_eq!(first.army_value_vespene, 0);
    }
}

#[test]
fn resources_captured_with_mix_of_mineral_and_vespene() {
    let t = load();
    let mins = t
        .resources
        .iter()
        .filter(|r| matches!(r.kind, ResourceKind::Mineral | ResourceKind::RichMineral))
        .count();
    let gas = t
        .resources
        .iter()
        .filter(|r| matches!(r.kind, ResourceKind::Vespene | ResourceKind::RichVespene))
        .count();
    assert!(mins >= 8, "poucos mineral fields: {}", mins);
    assert!(gas >= 2, "poucos vespene geysers: {}", gas);
    for r in &t.resources {
        assert!(r.x <= t.map_size_x, "x fora do mapa: {} > {}", r.x, t.map_size_x);
        assert!(r.y <= t.map_size_y, "y fora do mapa: {} > {}", r.y, t.map_size_y);
    }
}
