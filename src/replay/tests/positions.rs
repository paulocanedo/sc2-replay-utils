use super::*;

#[test]
fn unit_positions_collected_and_sorted() {
    let t = load();
    let total: usize = t.players.iter().map(|p| p.unit_positions.len()).sum();
    assert!(
        total > 0,
        "esperava ao menos uma amostra de UnitPositionsEvent agregada",
    );
    for p in &t.players {
        for w in p.unit_positions.windows(2) {
            assert!(
                w[0].game_loop <= w[1].game_loop,
                "unit_positions fora de ordem em {}: {} > {}",
                p.name,
                w[0].game_loop,
                w[1].game_loop,
            );
        }
    }
}

#[test]
fn unit_positions_in_map_scale() {
    let t = load();
    assert!(t.map_size_x > 0 && t.map_size_y > 0);
    let limit_x = (t.map_size_x as u32).saturating_mul(2);
    let limit_y = (t.map_size_y as u32).saturating_mul(2);
    let mut checked = 0usize;
    for p in &t.players {
        for s in &p.unit_positions {
            assert!(
                (s.x as u32) <= limit_x && (s.y as u32) <= limit_y,
                "amostra fora de escala: ({},{}) > 2×({},{})",
                s.x,
                s.y,
                t.map_size_x,
                t.map_size_y,
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "esperava ao menos uma amostra de posição");
}

#[test]
fn last_known_positions_query_matches_walk() {
    let t = load();
    for p in &t.players {
        if p.unit_positions.is_empty() {
            continue;
        }
        let until = t.game_loops;
        let snap = p.last_known_positions(until);
        let mut manual: HashMap<i64, (u8, u8)> = HashMap::new();
        for s in &p.unit_positions {
            manual.insert(s.tag, (s.x, s.y));
        }
        assert_eq!(snap.len(), manual.len());
        for (tag, pos) in &manual {
            assert_eq!(snap.get(tag), Some(pos));
        }
    }
}

#[test]
fn interpolated_positions_match_endpoints_and_midpoint() {
    let t = load();
    let mut tested = 0usize;
    for p in &t.players {
        let mut by_tag: HashMap<i64, Vec<&UnitPositionSample>> = HashMap::new();
        for s in &p.unit_positions {
            by_tag.entry(s.tag).or_default().push(s);
        }
        for samples in by_tag.values() {
            if samples.len() < 2 {
                continue;
            }
            for w in samples.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.x, a.y) == (b.x, b.y) || b.game_loop - a.game_loop < 2 {
                    continue;
                }
                let snap = p.interpolated_positions(a.game_loop);
                let &(x, y) = snap.get(&a.tag).expect("tag presente em snap");
                assert!((x - a.x as f32).abs() < 0.01);
                assert!((y - a.y as f32).abs() < 0.01);
                let mid = (a.game_loop + b.game_loop) / 2;
                let snap = p.interpolated_positions(mid);
                let &(x, y) = snap.get(&a.tag).expect("tag presente em snap mid");
                let frac = (mid - a.game_loop) as f32 / (b.game_loop - a.game_loop) as f32;
                let exp_x = a.x as f32 + (b.x as f32 - a.x as f32) * frac;
                let exp_y = a.y as f32 + (b.y as f32 - a.y as f32) * frac;
                assert!(
                    (x - exp_x).abs() < 0.01,
                    "midpoint x: {} vs {}", x, exp_x,
                );
                assert!(
                    (y - exp_y).abs() < 0.01,
                    "midpoint y: {} vs {}", y, exp_y,
                );
                tested += 1;
                if tested >= 3 {
                    return;
                }
            }
        }
    }
    assert!(
        tested > 0,
        "esperava ao menos um par de amostras com posições distintas",
    );
}

#[test]
fn camera_positions_sorted_and_deduplicated() {
    let t = load();
    let total: usize = t.players.iter().map(|p| p.camera_positions.len()).sum();
    assert!(total > 0, "esperava ao menos uma amostra de câmera");
    for p in &t.players {
        for w in p.camera_positions.windows(2) {
            assert!(
                w[0].game_loop <= w[1].game_loop,
                "camera_positions fora de ordem em {}: {} > {}",
                p.name, w[0].game_loop, w[1].game_loop,
            );
            assert!(
                w[0].x != w[1].x || w[0].y != w[1].y,
                "amostras de câmera consecutivas na mesma posição em {} nos loops {}, {}",
                p.name, w[0].game_loop, w[1].game_loop,
            );
        }
    }
}
