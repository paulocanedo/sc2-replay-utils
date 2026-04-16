use super::*;

#[test]
fn alive_count_monotonic_for_morphs() {
    let t = load();
    // O contador acumulado por tipo nunca pode ficar negativo.
    for p in &t.players {
        for (kind, series) in &p.alive_count {
            for (loop_, count) in series {
                assert!(
                    *count >= 0,
                    "alive_count negativo para {} no loop {}: {}",
                    kind, loop_, count
                );
            }
        }
    }
}

#[test]
fn cancellation_emitted_when_in_progress_dies() {
    let t = load();
    let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
    let cancellations: Vec<_> = terran
        .entity_events
        .iter()
        .filter(|e| e.kind == EntityEventKind::ProductionCancelled)
        .collect();
    assert!(
        !cancellations.is_empty(),
        "esperava ao menos um ProductionCancelled (CC interrompido)",
    );
    assert!(
        cancellations
            .iter()
            .any(|e| e.entity_type == "CommandCenter" && e.game_loop == 8450),
        "esperava o ProductionCancelled específico do CC tag=95682561 no loop 8450",
    );
}

#[test]
fn instant_units_emit_started_and_finished_same_loop() {
    let t = load();
    let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
    let mut by_tag: HashMap<i64, Vec<(u32, EntityEventKind)>> = HashMap::new();
    for ev in &terran.entity_events {
        if ev.entity_type == "SCV" {
            by_tag.entry(ev.tag).or_default().push((ev.game_loop, ev.kind));
        }
    }
    let mut found = 0;
    for (_, evs) in &by_tag {
        let started = evs.iter().find(|(_, k)| *k == EntityEventKind::ProductionStarted);
        let finished = evs.iter().find(|(_, k)| *k == EntityEventKind::ProductionFinished);
        if let (Some(s), Some(f)) = (started, finished) {
            assert_eq!(s.0, f.0, "Started/Finished deveriam estar no mesmo loop para SCV");
            found += 1;
        }
    }
    assert!(found > 0, "esperava ao menos um SCV com Started+Finished no mesmo loop");
}

#[test]
fn morph_emits_started_and_finished_for_new_type() {
    let t = load();
    let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
    let morph_starts: Vec<_> = terran
        .entity_events
        .iter()
        .filter(|e| {
            e.entity_type == "OrbitalCommand"
                && e.kind == EntityEventKind::ProductionStarted
                && terran.entity_events.iter().any(|d| {
                    d.tag == e.tag
                        && d.game_loop == e.game_loop
                        && d.kind == EntityEventKind::Died
                        && d.entity_type == "CommandCenter"
                })
        })
        .collect();
    assert!(
        !morph_starts.is_empty(),
        "esperava ao menos um morph CC→OrbitalCommand (Died+Started no mesmo loop+tag)",
    );
    for s in &morph_starts {
        let finished = terran.entity_events.iter().any(|e| {
            e.tag == s.tag
                && e.kind == EntityEventKind::ProductionFinished
                && e.game_loop == s.game_loop
                && e.entity_type == "OrbitalCommand"
        });
        assert!(
            finished,
            "morph sem ProductionFinished de OrbitalCommand pareado em {}",
            s.game_loop,
        );
    }
}

#[test]
fn worker_capacity_never_negative() {
    let t = load();
    for p in &t.players {
        let mut cum: i32 = 0;
        let mut events = p.worker_capacity.clone();
        events.sort_by_key(|(l, _)| *l);
        for (_, delta) in &events {
            cum += delta;
            assert!(
                cum >= 0,
                "worker_capacity acumulado ficou negativo em {}: {:?}",
                p.name, events,
            );
        }
    }
}

#[test]
fn entity_events_sorted_by_loop() {
    let t = load();
    for p in &t.players {
        for w in p.entity_events.windows(2) {
            assert!(
                w[0].game_loop <= w[1].game_loop,
                "entity_events fora de ordem em {}: {} > {}",
                p.name, w[0].game_loop, w[1].game_loop,
            );
        }
    }
}

#[test]
fn morph_only_unit_type_change_carries_synthetic_ability() {
    let t = load();
    for p in &t.players {
        for ev in &p.entity_events {
            if ev.kind != EntityEventKind::ProductionStarted {
                continue;
            }
            if matches!(
                ev.entity_type.as_str(),
                "OrbitalCommand" | "PlanetaryFortress" | "WarpGate"
            ) {
                let ability = ev.creator_ability.as_deref().unwrap_or("");
                assert!(
                    ability.starts_with("MorphTo"),
                    "esperava creator_ability=MorphTo* para {} no loop {}, achei {:?}",
                    ev.entity_type, ev.game_loop, ev.creator_ability,
                );
            }
        }
    }
}
