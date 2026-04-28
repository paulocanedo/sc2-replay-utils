use std::collections::HashMap;

use crate::replay::{is_larva_born_army, EntityCategory, EntityEvent, EntityEventKind, PlayerTimeline};

use super::morph::is_consumable_progenitor;
use super::player::extract_player;
use super::types::{BlockKind, LaneMode};

fn ev(
    gl: u32,
    seq: u32,
    kind: EntityEventKind,
    ty: &str,
    tag: i64,
    creator: Option<i64>,
) -> EntityEvent {
    EntityEvent {
        game_loop: gl,
        seq,
        kind,
        entity_type: ty.to_string(),
        category: if matches!(ty, "SCV" | "Probe" | "Drone") {
            EntityCategory::Worker
        } else if matches!(ty, "Larva")
            || is_larva_born_army(ty)
            || matches!(
                ty,
                "Marine" | "Marauder" | "Reaper" | "Ghost" | "Zealot" | "Stalker"
            )
        {
            EntityCategory::Unit
        } else {
            EntityCategory::Structure
        },
        tag,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: creator,
        killer_player_id: None,
    }
}

fn player_with_events(events: Vec<EntityEvent>, race: &str) -> PlayerTimeline {
    PlayerTimeline {
        name: "p".into(),
        clan: String::new(),
        race: race.into(),
        mmr: None,
        player_id: 1,
        result: None,
        toon: None,
        stats: vec![],
        upgrades: vec![],
        entity_events: events,
        production_cmds: vec![],
        inject_cmds: vec![],
        unit_positions: vec![],
        camera_positions: vec![],
        alive_count: Default::default(),
        worker_capacity: vec![],
        worker_births: vec![],
        army_capacity: vec![],
        army_productions: vec![],
        worker_capacity_cumulative: vec![],
        army_capacity_cumulative: vec![],
        upgrade_cumulative: vec![],
        creep_index: vec![],
    }
}

#[test]
fn workers_terran_cc_morphs_to_orbital_emits_morphing_block() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "CommandCenter",
            1,
            None,
        ),
        ev(1000, 1, EntityEventKind::Died, "CommandCenter", 1, None),
        ev(
            1000,
            2,
            EntityEventKind::ProductionStarted,
            "OrbitalCommand",
            1,
            None,
        ),
        ev(
            1000,
            3,
            EntityEventKind::ProductionFinished,
            "OrbitalCommand",
            1,
            None,
        ),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Workers);
    assert_eq!(out.lanes.len(), 1);
    assert_eq!(out.lanes[0].canonical_type, "OrbitalCommand");
    assert_eq!(out.lanes[0].blocks.len(), 1);
    assert_eq!(out.lanes[0].blocks[0].kind, BlockKind::Morphing);
    // O ícone do destino do morph é desenhado dentro da faixa,
    // então `produced_type` precisa carregar o tipo destino.
    assert_eq!(out.lanes[0].blocks[0].produced_type, Some("OrbitalCommand"));
}

#[test]
fn workers_scv_resolves_via_started_companion() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "CommandCenter",
            1,
            None,
        ),
        ev(
            472,
            1,
            EntityEventKind::ProductionStarted,
            "SCV",
            10,
            Some(1),
        ),
        ev(472, 1, EntityEventKind::ProductionFinished, "SCV", 10, None),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Workers);
    assert_eq!(out.lanes[0].blocks.len(), 1);
    assert_eq!(out.lanes[0].blocks[0].kind, BlockKind::Producing);
}

#[test]
fn workers_zerg_drone_resolves_via_larva_to_hatch_map() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Hatchery",
            1,
            None,
        ),
        ev(150, 1, EntityEventKind::ProductionStarted, "Larva", 5, Some(1)),
        ev(150, 1, EntityEventKind::ProductionFinished, "Larva", 5, None),
        ev(472, 2, EntityEventKind::Died, "Larva", 5, None),
        ev(
            472,
            2,
            EntityEventKind::ProductionStarted,
            "Drone",
            5,
            Some(5),
        ),
        ev(472, 2, EntityEventKind::ProductionFinished, "Drone", 5, None),
    ];
    let p = player_with_events(events, "Zerg");
    let out = extract_player(&p, 0, LaneMode::Workers);
    assert_eq!(out.lanes[0].blocks.len(), 1);
}

#[test]
fn army_terran_addon_construction_emits_impeded_block() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Barracks",
            1,
            None,
        ),
        ev(
            200,
            1,
            EntityEventKind::ProductionStarted,
            "BarracksReactor",
            2,
            Some(1),
        ),
        ev(
            600,
            2,
            EntityEventKind::ProductionFinished,
            "BarracksReactor",
            2,
            None,
        ),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    assert_eq!(out.lanes.len(), 1);
    let imp: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Impeded)
        .collect();
    assert_eq!(imp.len(), 1);
    assert_eq!(imp[0].start_loop, 200);
    assert_eq!(imp[0].end_loop, 600);
    assert_eq!(imp[0].produced_type, Some("BarracksReactor"));
}

#[test]
fn army_terran_marine_attributed_to_barracks() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Barracks",
            1,
            None,
        ),
        ev(
            500,
            1,
            EntityEventKind::ProductionStarted,
            "Marine",
            10,
            Some(1),
        ),
        ev(
            500,
            2,
            EntityEventKind::ProductionFinished,
            "Marine",
            10,
            None,
        ),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    let prod: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1);
    assert_eq!(prod[0].produced_type, Some("Marine"));
}

#[test]
fn army_protoss_gateway_morphs_to_warpgate_sets_warpgate_since_loop() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Gateway",
            1,
            None,
        ),
        ev(2000, 1, EntityEventKind::Died, "Gateway", 1, None),
        ev(
            2000,
            2,
            EntityEventKind::ProductionStarted,
            "WarpGate",
            1,
            None,
        ),
        ev(
            2000,
            3,
            EntityEventKind::ProductionFinished,
            "WarpGate",
            1,
            None,
        ),
    ];
    let p = player_with_events(events, "Protoss");
    let out = extract_player(&p, 0, LaneMode::Army);
    assert_eq!(out.lanes.len(), 1);
    assert_eq!(out.lanes[0].canonical_type, "WarpGate");
    assert_eq!(out.lanes[0].warpgate_since_loop, Some(2000));
}

#[test]
fn army_zerg_zergling_attributed_to_hatchery_via_larva() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Hatchery",
            1,
            None,
        ),
        ev(150, 1, EntityEventKind::ProductionStarted, "Larva", 5, Some(1)),
        ev(150, 1, EntityEventKind::ProductionFinished, "Larva", 5, None),
        ev(472, 2, EntityEventKind::Died, "Larva", 5, None),
        ev(
            472,
            2,
            EntityEventKind::ProductionStarted,
            "Zergling",
            5,
            Some(5),
        ),
        ev(
            472,
            2,
            EntityEventKind::ProductionFinished,
            "Zergling",
            5,
            None,
        ),
    ];
    let p = player_with_events(events, "Zerg");
    let out = extract_player(&p, 0, LaneMode::Army);
    let prod: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1);
    assert_eq!(prod[0].produced_type, Some("Zergling"));
}

#[test]
fn army_terran_addon_cancelled_emits_partial_impeded() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Barracks",
            1,
            None,
        ),
        ev(
            200,
            1,
            EntityEventKind::ProductionStarted,
            "BarracksReactor",
            2,
            Some(1),
        ),
        ev(
            400,
            2,
            EntityEventKind::ProductionCancelled,
            "BarracksReactor",
            2,
            None,
        ),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    let imp: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Impeded)
        .collect();
    assert_eq!(imp.len(), 1);
    assert_eq!(imp[0].end_loop, 400);
}

fn ev_at(
    gl: u32,
    seq: u32,
    kind: EntityEventKind,
    ty: &str,
    tag: i64,
    creator: Option<i64>,
    x: u8,
    y: u8,
) -> EntityEvent {
    let mut e = ev(gl, seq, kind, ty, tag, creator);
    e.pos_x = x;
    e.pos_y = y;
    e
}

#[test]
fn army_terran_addon_resolves_parent_via_proximity_when_creator_tag_missing() {
    // O UnitInit do s2protocol não traz creator_tag para addons
    // Terran. O parent precisa ser resolvido por proximidade
    // espacial à Barracks/Factory/Starport mais próxima.
    let events = vec![
        // Duas Barracks: tag 1 perto (50, 50), tag 2 longe (200, 200).
        ev_at(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Barracks",
            1,
            None,
            50,
            50,
        ),
        ev_at(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Barracks",
            2,
            None,
            200,
            200,
        ),
        // Reactor inicia em (53, 50) — adjacente à Barracks 1.
        // creator_tag = None (como vem do parser real).
        ev_at(
            200,
            1,
            EntityEventKind::ProductionStarted,
            "BarracksReactor",
            10,
            None,
            53,
            50,
        ),
        ev_at(
            600,
            2,
            EntityEventKind::ProductionFinished,
            "BarracksReactor",
            10,
            None,
            53,
            50,
        ),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    // Lane 1 (próxima) recebeu o Impeded; lane 2 (longe) não.
    let lane1 = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let lane2 = out.lanes.iter().find(|l| l.tag == 2).unwrap();
    let imp1: Vec<_> = lane1
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Impeded)
        .collect();
    assert_eq!(imp1.len(), 1);
    let imp2: Vec<_> = lane2
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Impeded)
        .collect();
    assert_eq!(imp2.len(), 0);
}

/// Integration: a contagem de unidades de army por jogador no
/// gráfico tem que bater com o que o build_order extrai do mesmo
/// replay. As duas pipelines consomem os mesmos `production_cmds`
/// + `entity_events` e devem chegar nos mesmos eventos pareados.
#[test]
fn army_lanes_match_build_order_counts_on_real_terran_replay() {
    use crate::build_order::extract_build_order;
    use crate::replay::{is_worker_name, parse_replay};
    use std::path::PathBuf;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/replay1.SC2Replay");
    let timeline = parse_replay(&path, 0).expect("parse replay");
    let bo = extract_build_order(&timeline).expect("build_order");
    let lanes_per_player = super::extract(&timeline, LaneMode::Army);

    // Para cada jogador Terran, conta unidades de army produzidas
    // (Marine/Marauder/etc.) em ambas as pipelines e compara.
    let mut compared_any = false;
    for (p_idx, player) in timeline.players.iter().enumerate() {
        if player.race != "Terran" {
            continue;
        }
        let bo_player = &bo.players[p_idx];
        let lanes_player = &lanes_per_player[p_idx];

        // Conta por tipo no build_order: entries de army (não
        // estrutura, não upgrade, completed) com count agregado.
        let mut bo_counts: HashMap<String, usize> = HashMap::new();
        for entry in &bo_player.entries {
            if entry.is_structure || entry.is_upgrade {
                continue;
            }
            if entry.outcome != crate::build_order::EntryOutcome::Completed {
                continue;
            }
            if is_worker_name(&entry.action) {
                continue;
            }
            if !matches!(
                entry.action.as_str(),
                "Marine"
                    | "Marauder"
                    | "Reaper"
                    | "Ghost"
                    | "Hellion"
                    | "Hellbat"
                    | "WidowMine"
                    | "SiegeTank"
                    | "Cyclone"
                    | "Thor"
                    | "VikingFighter"
                    | "Medivac"
                    | "Liberator"
                    | "Banshee"
                    | "Raven"
                    | "Battlecruiser"
            ) {
                continue;
            }
            *bo_counts.entry(entry.action.clone()).or_default() += entry.count as usize;
        }

        // Conta por tipo nas lanes: cada bloco Producing é uma
        // unidade. Como `merge_continuous` mescla blocos contíguos
        // do mesmo tipo, a contagem aqui pode ser ≤ build_order.
        // Mas o conjunto de tipos produzidos tem que ser o mesmo, e
        // a cardinalidade não pode ser zero quando build_order tem
        // entradas.
        let mut lanes_types: HashMap<&'static str, usize> = HashMap::new();
        for lane in &lanes_player.lanes {
            for block in &lane.blocks {
                if block.kind != BlockKind::Producing {
                    continue;
                }
                if let Some(t) = block.produced_type {
                    if matches!(
                        t,
                        "Marine"
                            | "Marauder"
                            | "Reaper"
                            | "Ghost"
                            | "Hellion"
                            | "Hellbat"
                            | "WidowMine"
                            | "SiegeTank"
                            | "Cyclone"
                            | "Thor"
                            | "VikingFighter"
                            | "Medivac"
                            | "Liberator"
                            | "Banshee"
                            | "Raven"
                            | "Battlecruiser"
                    ) {
                        *lanes_types.entry(t).or_default() += 1;
                    }
                }
            }
        }

        // Validação: todo tipo presente no build_order tem que
        // aparecer nas lanes, e vice-versa.
        for (action, _) in &bo_counts {
            assert!(
                lanes_types.contains_key(action.as_str()),
                "Player {}: build_order tem '{}' mas lanes não",
                player.name,
                action,
            );
        }
        for action in lanes_types.keys() {
            assert!(
                bo_counts.contains_key(*action),
                "Player {}: lanes tem '{}' mas build_order não",
                player.name,
                action,
            );
        }

        // Sem blocos `Producing` com `produced_type=None`: como
        // `is_target_unit` agora gateia em `intern_unit_name.is_some()`,
        // todo bloco aceito tem nome canônico. Se este invariante
        // quebra, é sinal de que `is_target_unit` está deixando algo
        // passar (ou `intern_unit_name` divergiu).
        for lane in &lanes_player.lanes {
            for block in &lane.blocks {
                if block.kind == BlockKind::Producing {
                    assert!(
                        block.produced_type.is_some(),
                        "Player {}: bloco Producing em lane '{}' com produced_type=None — divergência entre is_target_unit e intern_unit_name",
                        player.name,
                        lane.canonical_type,
                    );
                }
            }
        }
        compared_any = true;
    }
    assert!(compared_any, "replay1.SC2Replay não tem jogador Terran");
}

/// Terran transform: Hellion ↔ Hellbat. Sem o filtro
/// `is_pure_morph_finish`, o `Finished(Hellbat)` viraria um bloco
/// fantasma — a Hellion já foi contada quando nasceu da Factory.
#[test]
fn army_terran_hellion_hellbat_morph_does_not_create_phantom_block() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Factory",
            1,
            None,
        ),
        // Hellion produzido na Factory (UnitBorn fresh tag).
        ev(500, 1, EntityEventKind::ProductionStarted, "Hellion", 10, Some(1)),
        ev(500, 1, EntityEventKind::ProductionFinished, "Hellion", 10, None),
        // Morph Hellion → Hellbat via UnitTypeChange (apply_type_change
        // emite Died+Started+Finished com creator_ability=None).
        ev(2000, 2, EntityEventKind::Died, "Hellion", 10, None),
        ev(2000, 3, EntityEventKind::ProductionStarted, "Hellbat", 10, Some(10)),
        ev(2000, 3, EntityEventKind::ProductionFinished, "Hellbat", 10, None),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    let prod: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1, "esperava 1 bloco (Hellion), Hellbat morph não conta");
    assert_eq!(prod[0].produced_type, Some("Hellion"));
}

/// Terran toggle: SiegeTank ↔ SiegeTankSieged. Em uma partida típica
/// o jogador siege/unsiege dezenas de vezes. Cada toggle emite
/// Died→Started→Finished e geraria 2 blocos fantasmas por ciclo
/// sem o fix.
#[test]
fn army_terran_siegetank_siege_cycle_zero_extra_blocks() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Factory",
            1,
            None,
        ),
        ev(800, 1, EntityEventKind::ProductionStarted, "SiegeTank", 20, Some(1)),
        ev(800, 1, EntityEventKind::ProductionFinished, "SiegeTank", 20, None),
        // Siege.
        ev(2000, 2, EntityEventKind::Died, "SiegeTank", 20, None),
        ev(2000, 3, EntityEventKind::ProductionStarted, "SiegeTankSieged", 20, Some(20)),
        ev(2000, 3, EntityEventKind::ProductionFinished, "SiegeTankSieged", 20, None),
        // Unsiege.
        ev(3000, 4, EntityEventKind::Died, "SiegeTankSieged", 20, None),
        ev(3000, 5, EntityEventKind::ProductionStarted, "SiegeTank", 20, Some(20)),
        ev(3000, 5, EntityEventKind::ProductionFinished, "SiegeTank", 20, None),
        // Re-siege.
        ev(4000, 6, EntityEventKind::Died, "SiegeTank", 20, None),
        ev(4000, 7, EntityEventKind::ProductionStarted, "SiegeTankSieged", 20, Some(20)),
        ev(4000, 7, EntityEventKind::ProductionFinished, "SiegeTankSieged", 20, None),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    let prod: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1, "esperava 1 bloco (Tank), 3 toggles não contam");
    assert_eq!(prod[0].produced_type, Some("SiegeTank"));
}

/// Terran toggle: VikingFighter ↔ VikingAssault.
#[test]
fn army_terran_viking_transform_zero_extra_blocks() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Starport",
            1,
            None,
        ),
        ev(800, 1, EntityEventKind::ProductionStarted, "VikingFighter", 30, Some(1)),
        ev(800, 1, EntityEventKind::ProductionFinished, "VikingFighter", 30, None),
        // Transform para assault mode.
        ev(2000, 2, EntityEventKind::Died, "VikingFighter", 30, None),
        ev(2000, 3, EntityEventKind::ProductionStarted, "VikingAssault", 30, Some(30)),
        ev(2000, 3, EntityEventKind::ProductionFinished, "VikingAssault", 30, None),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    let prod: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1);
    assert_eq!(prod[0].produced_type, Some("VikingFighter"));
}

/// Terran toggle: WidowMine ↔ WidowMineBurrowed.
#[test]
fn army_terran_widowmine_burrow_zero_extra_blocks() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Factory",
            1,
            None,
        ),
        ev(800, 1, EntityEventKind::ProductionStarted, "WidowMine", 40, Some(1)),
        ev(800, 1, EntityEventKind::ProductionFinished, "WidowMine", 40, None),
        // Burrow.
        ev(1500, 2, EntityEventKind::Died, "WidowMine", 40, None),
        ev(1500, 3, EntityEventKind::ProductionStarted, "WidowMineBurrowed", 40, Some(40)),
        ev(1500, 3, EntityEventKind::ProductionFinished, "WidowMineBurrowed", 40, None),
        // Unburrow.
        ev(2500, 4, EntityEventKind::Died, "WidowMineBurrowed", 40, None),
        ev(2500, 5, EntityEventKind::ProductionStarted, "WidowMine", 40, Some(40)),
        ev(2500, 5, EntityEventKind::ProductionFinished, "WidowMine", 40, None),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);
    let prod: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1);
    assert_eq!(prod[0].produced_type, Some("WidowMine"));
}

/// Helper logic test: `is_consumable_progenitor` deve aceitar Larva,
/// cocoons (BanelingCocoon, RavagerCocoon, BroodLordCocoon,
/// OverlordCocoon) e eggs (LurkerMPEgg) — confirmados em
/// `s2protocol-3.5.2/assets/BalanceData/`. E rejeitar tudo o mais.
#[test]
fn is_consumable_progenitor_accepts_larva_cocoons_and_eggs() {
    // Aceitar.
    assert!(is_consumable_progenitor("Larva"));
    assert!(is_consumable_progenitor("BanelingCocoon"));
    assert!(is_consumable_progenitor("BroodLordCocoon"));
    assert!(is_consumable_progenitor("OverlordCocoon"));
    assert!(is_consumable_progenitor("RavagerCocoon"));
    assert!(is_consumable_progenitor("LurkerMPEgg"));
    // Rejeitar transforms Terran.
    assert!(!is_consumable_progenitor("Hellion"));
    assert!(!is_consumable_progenitor("Hellbat"));
    assert!(!is_consumable_progenitor("SiegeTank"));
    assert!(!is_consumable_progenitor("SiegeTankSieged"));
    assert!(!is_consumable_progenitor("VikingFighter"));
    assert!(!is_consumable_progenitor("VikingAssault"));
    assert!(!is_consumable_progenitor("WidowMine"));
    assert!(!is_consumable_progenitor("WidowMineBurrowed"));
    assert!(!is_consumable_progenitor("Liberator"));
    assert!(!is_consumable_progenitor("LiberatorAG"));
    // Rejeitar unidades Zerg base (não progenitoras).
    assert!(!is_consumable_progenitor("Zergling"));
    assert!(!is_consumable_progenitor("Roach"));
    assert!(!is_consumable_progenitor("Overlord"));
}

/// Zerg via Cocoon: Larva → OverlordCocoon → Overlord. Quando o
/// tracker emite a transição final (Cocoon → Overlord), o
/// `is_consumable_progenitor("OverlordCocoon")` é `true` e o
/// `Finished(Overlord)` continua gerando bloco. Confirma que o fix
/// não regrediu Zerg que passa por intermediário cocoon.
#[test]
fn army_zerg_overlord_via_cocoon_creates_block() {
    let events = vec![
        ev(
            100,
            0,
            EntityEventKind::ProductionFinished,
            "Hatchery",
            1,
            None,
        ),
        ev(150, 1, EntityEventKind::ProductionStarted, "Larva", 5, Some(1)),
        ev(150, 1, EntityEventKind::ProductionFinished, "Larva", 5, None),
        // Larva → OverlordCocoon (intermediário, descartado por
        // is_target_unit pra Zerg porque OverlordCocoon não está em
        // is_larva_born_army).
        ev(472, 2, EntityEventKind::Died, "Larva", 5, None),
        ev(472, 3, EntityEventKind::ProductionStarted, "OverlordCocoon", 5, Some(5)),
        ev(472, 3, EntityEventKind::ProductionFinished, "OverlordCocoon", 5, None),
        // OverlordCocoon → Overlord (final morph, IS em is_larva_born_army).
        ev(900, 4, EntityEventKind::Died, "OverlordCocoon", 5, None),
        ev(900, 5, EntityEventKind::ProductionStarted, "Overlord", 5, Some(5)),
        ev(900, 5, EntityEventKind::ProductionFinished, "Overlord", 5, None),
    ];
    let p = player_with_events(events, "Zerg");
    let out = extract_player(&p, 0, LaneMode::Army);
    let prod: Vec<_> = out.lanes[0]
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1, "Overlord via cocoon deve gerar 1 bloco");
    assert_eq!(prod[0].produced_type, Some("Overlord"));
}
