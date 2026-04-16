use super::*;

#[test]
fn stats_at_returns_latest_le() {
    let t = load();
    let p = &t.players[0];
    assert!(!p.stats.is_empty());

    // Antes do primeiro snapshot → None.
    assert!(p.stats_at(0).is_none() || p.stats[0].game_loop == 0);

    // No próprio loop do primeiro snapshot, deve devolvê-lo.
    let first = &p.stats[0];
    let s = p.stats_at(first.game_loop).unwrap();
    assert_eq!(s.game_loop, first.game_loop);

    // No meio do replay, devolve o snapshot mais recente <= alvo.
    let mid = p.stats[p.stats.len() / 2].game_loop;
    let s = p.stats_at(mid + 1).unwrap();
    assert!(s.game_loop <= mid + 1);

    // Depois do último snapshot, devolve o último.
    let last = p.stats.last().unwrap().game_loop;
    let s = p.stats_at(last + 1_000_000).unwrap();
    assert_eq!(s.game_loop, last);
}

#[test]
fn upgrades_until_is_prefix() {
    let t = load();
    let p = &t.players[0];
    // 0 → vazio.
    assert!(p.upgrades_until(0).is_empty() || p.upgrades[0].game_loop == 0);
    // ∞ → todos.
    let all = p.upgrades_until(u32::MAX);
    assert_eq!(all.len(), p.upgrades.len());
    // Monotônico em loop.
    for w in p.upgrades.windows(2) {
        assert!(w[0].game_loop <= w[1].game_loop);
    }
}

#[test]
fn state_at_loop_zero_returns_no_stats() {
    let t = load();
    let p = &t.players[0];
    // Stats começam após o loop 0 (snapshot inicial); stats_at(0)
    // pode devolver Some se o primeiro snapshot é exatamente em
    // loop 0, ou None caso contrário. Em ambos os casos não deve
    // panicar.
    let _ = p.stats_at(0);
    let _ = p.upgrades_until(0);
    let _ = p.worker_capacity_at(0);
}

#[test]
fn camera_at_returns_latest_le() {
    let t = load();
    let p = &t.players[0];
    if p.camera_positions.is_empty() {
        return;
    }
    if p.camera_positions[0].game_loop > 0 {
        assert!(p.camera_at(0).is_none());
    }
    let first = &p.camera_positions[0];
    let c = p.camera_at(first.game_loop).unwrap();
    assert_eq!(c.game_loop, first.game_loop);
    let last = p.camera_positions.last().unwrap();
    let c = p.camera_at(u32::MAX).unwrap();
    assert_eq!(c.game_loop, last.game_loop);
}
