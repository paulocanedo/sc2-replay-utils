use super::*;

#[test]
fn datetime_is_local_not_utc_plus_offset() {
    // `examples/replay1.SC2Replay` foi gravado em UTC-3 (time_local_offset
    // = -108_000_000_000 FILETIME units). O UTC do replay cai em
    // 2025-10-11 00:52:16, então o horário LOCAL correto é
    // 2025-10-10 21:52:16. Se `transform_to_naivetime` voltar a
    // subtrair o offset, o teste pega o valor errado
    // (03:52 em 2025-10-11, 6h no futuro).
    let t = load();
    assert_eq!(t.datetime, "2025-10-10T21:52:16");
}

#[test]
fn timeline_loads() {
    let t = load();
    assert_eq!(t.players.len(), 2);
    assert!(t.game_loops > 0);
    assert!(t.loops_per_second > 0.0);
    assert!(!t.players[0].name.is_empty());
    assert!(!t.players[1].name.is_empty());
}

#[test]
fn metadata_only_fast_path_skips_events() {
    let t = parse_replay(&example_replay(), 1).expect("parse_replay fast");
    assert_eq!(t.players.len(), 2);
    // Fast path: nada de tracker/message events.
    for p in &t.players {
        assert!(p.stats.is_empty(), "stats deveria estar vazio no fast path");
        assert!(
            p.entity_events.is_empty(),
            "entity_events deveria estar vazio no fast path",
        );
        assert!(p.upgrades.is_empty());
    }
    assert!(t.chat.is_empty());
}
