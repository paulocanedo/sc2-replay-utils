use super::*;
use crate::build_order::types::{BuildOrderEntry, EntryOutcome, PlayerBuildOrder};

/// Constrói uma `BuildOrderEntry` mínima para teste. `loop_secs` é o
/// tempo de início em segundos (será convertido pra game loops por
/// game_loop = secs * 22.4 arredondado); `supply` é o supply usado
/// no início; `action` é o nome canônico.
fn entry(action: &str, loop_secs: f64, supply: u16) -> BuildOrderEntry {
    let gl = (loop_secs * 22.4).round() as u32;
    BuildOrderEntry {
        supply,
        supply_made: supply + 2,
        game_loop: gl,
        finish_loop: gl + 100,
        seq: 0,
        action: action.to_string(),
        count: 1,
        is_upgrade: matches!(
            action,
            "Stimpack"
                | "zerglingmovementspeed"
                | "WarpGate"
                | "WarpGateResearch"
                | "BlinkTech"
                | "Blink"
                | "blinktech"
                | "ZerglingMovementSpeed"
        ),
        is_structure: matches!(
            action,
            "SpawningPool"
                | "Hatchery"
                | "Extractor"
                | "Refinery"
                | "Assimilator"
                | "CommandCenter"
                | "Barracks"
                | "Factory"
                | "Starport"
                | "BarracksReactor"
                | "Gateway"
                | "Nexus"
                | "CyberneticsCore"
                | "Forge"
                | "Stargate"
                | "RoboticsFacility"
                | "TwilightCouncil"
                | "DarkShrine"
                | "RoachWarren"
                | "BanelingNest"
                | "Lair"
                | "PhotonCannon"
                | "Bunker"
        ),
        outcome: EntryOutcome::Completed,
        chrono_boosts: 0,
    }
}

fn player_with(race: &str, entries: Vec<BuildOrderEntry>) -> PlayerBuildOrder {
    PlayerBuildOrder {
        name: "Test".to_string(),
        race: race.to_string(),
        mmr: None,
        entries,
    }
}

// ── Zerg ────────────────────────────────────────────────────

#[test]
fn zerg_14_pool_detects_supply_and_labels() {
    let p = player_with(
        "Zerg",
        vec![
            entry("SpawningPool", 55.0, 14),
            entry("Extractor", 75.0, 15),
            entry("Hatchery", 110.0, 16),
            entry("Zergling", 140.0, 18),
            entry("Zergling", 142.0, 20),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "14 Pool");
    assert_eq!(lbl.confidence, Confidence::Named);
}

#[test]
fn zerg_12_pool_differs_from_14_pool_by_supply() {
    let p = player_with(
        "Zerg",
        vec![
            entry("SpawningPool", 45.0, 12),
            entry("Hatchery", 140.0, 14),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "12 Pool");
}

#[test]
fn zerg_hatch_first_labeled_correctly() {
    let p = player_with(
        "Zerg",
        vec![
            entry("Hatchery", 50.0, 17),
            entry("SpawningPool", 80.0, 17),
            entry("Extractor", 100.0, 18),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "Hatch First");
}

#[test]
fn zerg_hatch_gas_pool_when_gas_before_pool() {
    let p = player_with(
        "Zerg",
        vec![
            entry("Hatchery", 40.0, 17),
            entry("Extractor", 55.0, 17),
            entry("SpawningPool", 80.0, 18),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "Hatch Gas Pool");
}

#[test]
fn zerg_speedling_requires_metabolic_boost_and_lings() {
    let mut entries = vec![
        entry("SpawningPool", 55.0, 14),
        entry("Hatchery", 110.0, 16),
        entry("zerglingmovementspeed", 180.0, 22),
    ];
    for s in 0..9 {
        entries.push(entry("Zergling", 140.0 + s as f64, 18 + s as u16));
    }
    let p = player_with("Zerg", entries);
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.follow_up.as_deref(), Some("Speedling"));
}

#[test]
fn zerg_baneling_bust_needs_nest_and_four_banes() {
    let mut entries = vec![
        entry("SpawningPool", 55.0, 14),
        entry("Hatchery", 110.0, 16),
        entry("BanelingNest", 180.0, 22),
    ];
    for _ in 0..4 {
        entries.push(entry("Baneling", 220.0, 28));
    }
    let p = player_with("Zerg", entries);
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.follow_up.as_deref(), Some("Baneling Bust"));
}

// ── Terran ──────────────────────────────────────────────────

#[test]
fn terran_1_rax_fe_named_correctly() {
    let p = player_with(
        "Terran",
        vec![
            entry("Barracks", 70.0, 15),
            entry("Refinery", 95.0, 16),
            entry("CommandCenter", 180.0, 19),
            entry("Marine", 200.0, 20),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "1 Rax FE");
    assert_eq!(lbl.confidence, Confidence::Named);
}

#[test]
fn terran_3_rax_pressure_labeled() {
    let p = player_with(
        "Terran",
        vec![
            entry("Barracks", 70.0, 15),
            entry("Barracks", 100.0, 17),
            entry("Barracks", 140.0, 19),
            entry("Marine", 200.0, 21),
            entry("Marine", 210.0, 22),
            entry("Marine", 220.0, 23),
            entry("Marine", 230.0, 24),
            entry("Marine", 240.0, 25),
            entry("Marine", 250.0, 26),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "3 Rax");
}

#[test]
fn terran_reaper_expand_requires_reactor_and_reaper() {
    let p = player_with(
        "Terran",
        vec![
            entry("Barracks", 70.0, 15),
            entry("BarracksReactor", 100.0, 16),
            entry("Reaper", 130.0, 17),
            entry("CommandCenter", 200.0, 20),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "Reaper Expand");
}

#[test]
fn terran_cc_first_when_cc_before_rax() {
    let p = player_with(
        "Terran",
        vec![
            entry("CommandCenter", 40.0, 14),
            entry("Barracks", 80.0, 16),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "CC First");
}

#[test]
fn terran_stim_timing_follow_up() {
    let p = player_with(
        "Terran",
        vec![
            entry("Barracks", 70.0, 15),
            entry("Refinery", 95.0, 16),
            entry("CommandCenter", 180.0, 19),
            entry("Stimpack", 250.0, 24),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.follow_up.as_deref(), Some("Stim Timing"));
}

// ── Protoss ─────────────────────────────────────────────────

#[test]
fn protoss_gate_expand_named() {
    let p = player_with(
        "Protoss",
        vec![
            entry("Gateway", 60.0, 14),
            entry("Assimilator", 85.0, 15),
            entry("CyberneticsCore", 100.0, 16),
            entry("Nexus", 180.0, 19),
            entry("Stalker", 210.0, 22),
            entry("Stalker", 220.0, 23),
            entry("Stalker", 225.0, 24),
            entry("Sentry", 230.0, 25),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "Gate Expand");
    assert_eq!(lbl.follow_up.as_deref(), Some("Stalker/Sentry"));
}

#[test]
fn protoss_ffe_detected_when_forge_before_nexus() {
    let p = player_with(
        "Protoss",
        vec![
            entry("Forge", 50.0, 13),
            entry("Nexus", 90.0, 15),
            entry("Gateway", 140.0, 18),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "Nexus First (FFE)");
}

#[test]
fn protoss_nexus_first_without_forge() {
    let p = player_with(
        "Protoss",
        vec![
            entry("Nexus", 70.0, 14),
            entry("Gateway", 150.0, 18),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "Nexus First");
}

#[test]
fn protoss_4_gate_labeled_when_four_gateways() {
    let p = player_with(
        "Protoss",
        vec![
            entry("Gateway", 60.0, 14),
            entry("Gateway", 110.0, 17),
            entry("Gateway", 150.0, 19),
            entry("Gateway", 200.0, 21),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "4 Gate");
}

#[test]
fn protoss_cannon_rush_when_forge_before_gateway_and_cannon_built() {
    let p = player_with(
        "Protoss",
        vec![
            entry("Forge", 30.0, 11),
            entry("PhotonCannon", 80.0, 13),
            entry("Gateway", 140.0, 15),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.opening, "Cannon Rush");
}

// ── Fallback ────────────────────────────────────────────────

#[test]
fn unknown_race_falls_back_to_signature() {
    let p = player_with(
        "Random",
        vec![
            entry("Barracks", 70.0, 15),
            entry("Refinery", 95.0, 16),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.confidence, Confidence::Signature);
    assert!(lbl.opening.contains("Rax"));
}

#[test]
fn signature_fallback_has_at_most_three_marks() {
    let p = player_with(
        "Random",
        vec![
            entry("Barracks", 50.0, 14),
            entry("Refinery", 60.0, 15),
            entry("CommandCenter", 90.0, 17),
            entry("Barracks", 100.0, 18),
            entry("Barracks", 120.0, 19),
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    // 3 marcos separados por ", ": 2 vírgulas
    assert_eq!(lbl.opening.matches(',').count(), 2);
}

#[test]
fn replay_with_no_entries_before_window_is_insufficient() {
    // Entries vazio → has_any_before_opening_end = false.
    let p = player_with("Zerg", vec![]);
    let lbl = classify_opening(&p, 22.4);
    assert_eq!(lbl.confidence, Confidence::Insufficient);
}

#[test]
fn entries_beyond_follow_up_end_are_ignored() {
    // Pool às 05:30 (tardíssimo), fora da janela de 5 min.
    let p = player_with(
        "Zerg",
        vec![
            entry("Extractor", 60.0, 15),     // dentro da janela (supply marcado)
            entry("SpawningPool", 330.0, 40), // 5:30 — fora da janela
        ],
    );
    let lbl = classify_opening(&p, 22.4);
    // Sem pool dentro da janela, cai no fallback de signature.
    assert_eq!(lbl.confidence, Confidence::Signature);
}

// ── Formatação ──────────────────────────────────────────────

#[test]
fn display_string_combines_opening_and_follow_up() {
    let lbl = OpeningLabel {
        opening: "1 Rax FE".to_string(),
        follow_up: Some("Stim Timing".to_string()),
        confidence: Confidence::Named,
    };
    assert_eq!(lbl.to_display_string(), "1 Rax FE — Stim Timing");
}

#[test]
fn display_string_uses_opening_alone_when_no_follow_up() {
    let lbl = OpeningLabel {
        opening: "CC First".to_string(),
        follow_up: None,
        confidence: Confidence::Named,
    };
    assert_eq!(lbl.to_display_string(), "CC First");
}

// ── Smoke sobre replays reais ───────────────────────────────

/// Sanity check: os replays em `examples/` devem gerar rótulos
/// com confidence `Named` para os dois jogadores. Não validamos
/// o conteúdo exato do rótulo aqui (isso fica para os golden
/// tests); só garantimos que a classificação não degenera em
/// signature/insufficient para replays bem-formados.
#[test]
fn smoke_golden_replay_produces_named_labels() {
    use crate::build_order::extract_build_order;
    use crate::replay::parse_replay;
    use std::path::PathBuf;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/old_republic_50.SC2Replay");
    let timeline = parse_replay(&path, 0).expect("parse");
    let bo = extract_build_order(&timeline).expect("extract");
    let lps = bo.loops_per_second;
    assert!(!bo.players.is_empty());
    for player in &bo.players {
        let lbl = classify_opening(player, lps);
        eprintln!(
            "  {:>10} ({:>7}): {}  [{:?}]",
            player.name,
            player.race,
            lbl.to_display_string(),
            lbl.confidence,
        );
        assert_eq!(
            lbl.confidence,
            Confidence::Named,
            "esperava Named para {} em replay golden, veio {:?} ({})",
            player.name,
            lbl.confidence,
            lbl.to_display_string(),
        );
    }
}
