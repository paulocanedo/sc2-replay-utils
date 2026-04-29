use std::collections::HashMap;

use crate::replay::{is_larva_born_army, EntityCategory, EntityEvent, EntityEventKind, PlayerTimeline, ProductionCmd};

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

// ─── Helpers para os testes de resolução de parent de addon ─────────
// (`ev_at` já está definido acima — reusamos.)

fn cmd(game_loop: u32, ability: &str, producer_tag: i64) -> ProductionCmd {
    ProductionCmd {
        game_loop,
        ability: ability.to_string(),
        producer_tags: vec![producer_tag],
        consumed: false,
    }
}

fn player_with_events_and_cmds(
    events: Vec<EntityEvent>,
    cmds: Vec<ProductionCmd>,
    race: &str,
) -> PlayerTimeline {
    let mut p = player_with_events(events, race);
    p.production_cmds = cmds;
    p
}

// ─── Lift/land refresh (C) ──────────────────────────────────────────

/// Barracks nasce em (50, 50), decola, pousa em (80, 80). Depois um
/// Reactor é construído em (83, 80). Pela geometria atualizada o offset
/// (+3, 0) bate com a NOVA posição da Barracks. Sem o refresh em land,
/// a lane.pos ficaria congelada em (50, 50) e a resolução do parent
/// falharia (ou cairia no nearest sem semântica).
#[test]
fn army_terran_barracks_lift_and_land_updates_lane_position() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Lift-off: Died(Barracks) + Started/Finished(BarracksFlying) no mesmo loop.
        ev_at(1000, 1, EntityEventKind::Died, "Barracks", 1, None, 50, 50),
        ev_at(1000, 2, EntityEventKind::ProductionStarted, "BarracksFlying", 1, None, 50, 50),
        ev_at(1000, 3, EntityEventKind::ProductionFinished, "BarracksFlying", 1, None, 50, 50),
        // Land em (80, 80): Died(BarracksFlying) + Started/Finished(Barracks).
        ev_at(2000, 4, EntityEventKind::Died, "BarracksFlying", 1, None, 80, 80),
        ev_at(2000, 5, EntityEventKind::ProductionStarted, "Barracks", 1, None, 80, 80),
        ev_at(2000, 6, EntityEventKind::ProductionFinished, "Barracks", 1, None, 80, 80),
        // Reactor construído em (83, 80) — offset exato (+3, 0) da NOVA pos.
        ev_at(2200, 7, EntityEventKind::ProductionStarted, "BarracksReactor", 2, None, 83, 80),
        ev_at(2600, 8, EntityEventKind::ProductionFinished, "BarracksReactor", 2, None, 83, 80),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    // Posição da lane reflete o pouso, não o born.
    assert_eq!((lane.pos_x, lane.pos_y), (80, 80));
    // Impeded foi atribuído à Barracks (única candidata) corretamente.
    let imp: Vec<_> = lane
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Impeded)
        .collect();
    assert_eq!(imp.len(), 1);
    assert_eq!(imp[0].start_loop, 2200);
    assert_eq!(imp[0].end_loop, 2600);
}

// ─── Swap-skip (addon swap não emite Impeded duplicado) ─────────────

/// BarracksReactor construído em uma Barracks. Player decola a Barracks
/// e pousa uma Factory no mesmo Reactor — tracker emite UnitTypeChange
/// que vira morph (Died + Started + Finished triplet) no MESMO tag do
/// addon, com novo tipo "FactoryReactor". Esperado: apenas 1 Impeded
/// (do construction original); o swap não tem janela impeditiva.
#[test]
fn army_terran_addon_swap_does_not_emit_extra_impeded_block() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        ev_at(105, 1, EntityEventKind::ProductionFinished, "Factory", 2, None, 60, 60),
        // Construção original do Reactor na Barracks (loops 200..600).
        ev_at(200, 2, EntityEventKind::ProductionStarted, "BarracksReactor", 99, Some(1), 53, 50),
        ev_at(600, 3, EntityEventKind::ProductionFinished, "BarracksReactor", 99, None, 53, 50),
        // Swap em loop 2000: Died(BarracksReactor) + Started/Finished(FactoryReactor)
        // mesmo tag/loop. is_pure_morph_finish filtra o Finished e o
        // is_swap (morph_old_type) filtra o Started — nenhum novo
        // Impeded é registrado.
        ev_at(2000, 4, EntityEventKind::Died, "BarracksReactor", 99, None, 53, 50),
        ev_at(2000, 5, EntityEventKind::ProductionStarted, "FactoryReactor", 99, None, 53, 50),
        ev_at(2000, 6, EntityEventKind::ProductionFinished, "FactoryReactor", 99, None, 53, 50),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let total_impeded: usize = out
        .lanes
        .iter()
        .map(|l| l.blocks.iter().filter(|b| b.kind == BlockKind::Impeded).count())
        .sum();
    assert_eq!(
        total_impeded, 1,
        "swap não deve adicionar Impeded; só a construção original conta"
    );
    // E o único Impeded ficou na Barracks (parent original), não na Factory.
    let lane_b = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let lane_f = out.lanes.iter().find(|l| l.tag == 2).unwrap();
    assert_eq!(
        lane_b.blocks.iter().filter(|b| b.kind == BlockKind::Impeded).count(),
        1,
        "Barracks (tag 1) é o parent original; ela ganha o Impeded"
    );
    assert_eq!(
        lane_f.blocks.iter().filter(|b| b.kind == BlockKind::Impeded).count(),
        0,
        "Factory (tag 2) não construiu o addon; não tem Impeded"
    );
}

// ─── Cmd matching como FALLBACK (não primary) ───────────────────────

/// Cmd matching é fallback secundário — usado quando offset exato não
/// bate. Cenário: única Barracks viva, mas o addon spawnou em posição
/// que NÃO bate (+3, 0) — ex. lane com posição ainda stale antes do
/// land detection capturar relocate. Sem candidato exato, a cascata
/// recorre ao cmd; se o cmd dá um producer_tag válido, ele resolve.
#[test]
fn army_terran_addon_uses_cmd_when_no_exact_offset_match() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Reactor em (60, 70) — Δ pra única Barracks = (10, -20), nada de exato (3, 0).
        // Cmd diz que producer é Barracks tag 1.
        ev_at(200, 1, EntityEventKind::ProductionStarted, "BarracksReactor", 99, None, 60, 70),
        ev_at(600, 2, EntityEventKind::ProductionFinished, "BarracksReactor", 99, None, 60, 70),
    ];
    let cmds = vec![cmd(200, "BarracksReactor", 1)];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    assert_eq!(
        lane.blocks.iter().filter(|b| b.kind == BlockKind::Impeded).count(),
        1,
        "sem offset exato, cmd resolve corretamente"
    );
}

/// Cmd com `producer_tag` apontando para uma estrutura **morta** ou
/// inexistente é descartado (cmd órfão). A cascata pula pra
/// proximidade. Cenário: addon em geometria atípica (offset não
/// canônico), cmd inválido — sem essa validação a resolução pararia
/// num cmd ruim e nenhum Impeded seria emitido.
#[test]
fn army_terran_addon_cmd_with_invalid_producer_tag_falls_through_to_proximity() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Addon em (60, 70) — sem offset (+3, 0), proximidade vai cair em B (única).
        ev_at(200, 1, EntityEventKind::ProductionStarted, "BarracksReactor", 99, None, 60, 70),
        ev_at(600, 2, EntityEventKind::ProductionFinished, "BarracksReactor", 99, None, 60, 70),
    ];
    // Cmd com producer_tag=999 (não existe nenhuma lane com esse tag).
    let cmds = vec![cmd(200, "BarracksReactor", 999)];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    assert_eq!(
        lane.blocks.iter().filter(|b| b.kind == BlockKind::Impeded).count(),
        1,
        "cmd inválido descartado, proximidade pega a única Barracks"
    );
}

// ─── Marine queueado durante Reactor não fica sobreposto com Impeded ─

/// Em SC2 o jogador pode enfileirar Train_Marine enquanto a Barracks
/// está construindo addon (Reactor/TechLab). O cmd entra em
/// `production_cmds` no instante do clique, mas o treino real só
/// começa quando a janela impeditiva termina. Sem o ajuste
/// `start_loop = max(raw_start, Impeded.end_loop)`, o bloco
/// `Producing` apareceria sobreposto com o `Impeded` no chart,
/// passando a impressão visual de que a Barracks estava produzindo
/// Marine enquanto construía Reactor — o "Marine fantasma" relatado
/// no replay Winter Madness LE.
#[test]
fn army_terran_marine_queued_during_reactor_starts_after_impeded_ends() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Reactor: Impeded de 200 a 600.
        ev_at(200, 1, EntityEventKind::ProductionStarted, "BarracksReactor", 99, None, 53, 50),
        ev_at(600, 2, EntityEventKind::ProductionFinished, "BarracksReactor", 99, None, 53, 50),
        // Marine completa em 1000. cmd queueado em 400 (DURANTE o Reactor).
        ev_at(1000, 3, EntityEventKind::ProductionStarted, "Marine", 10, Some(1), 0, 0),
        ev_at(1000, 4, EntityEventKind::ProductionFinished, "Marine", 10, None, 0, 0),
    ];
    let cmds = vec![cmd(400, "Marine", 1)];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let prod: Vec<_> = lane
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1, "deve haver um bloco Producing pra esse Marine");
    // O cmd_loop foi 400 (durante Impeded 200..600). Sem o ajuste, o
    // bloco começaria em 400 — sobrepondo com o Impeded. Com o
    // ajuste, começa em 600 (fim do Impeded).
    assert_eq!(
        prod[0].start_loop, 600,
        "start_loop deve ser empurrado para o fim do Impeded, não o cmd_loop raw"
    );
    assert_eq!(prod[0].end_loop, 1000);

    // Sanity: o Impeded continua intacto (200..600).
    let imp: Vec<_> = lane
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Impeded)
        .collect();
    assert_eq!(imp.len(), 1);
    assert_eq!(imp[0].start_loop, 200);
    assert_eq!(imp[0].end_loop, 600);
}

// ─── Cmd matching prefere o mais recente sobre fantasmas antigos ────

/// Regression: o jogador clica `Train Marine` em algum momento (cmd
/// fantasma — pode ter sido double-click, queue cheia, ou cancelado),
/// depois clica de novo no instante real do treino. Se o `consume_
/// producer_cmd` fizesse FIFO, o cmd fantasma seria emparelhado com
/// o Marine real, fazendo o bloco `Producing` começar muito antes do
/// treino real. A política latest-within-window prefere o cmd mais
/// recente válido — o fantasma fica não-consumido, o real produz a
/// atribuição correta.
///
/// Cenário concreto (visto no Winter Madness LE):
///   Reactor terminou em ~8:14, Marines reais treinaram 8:31..8:48.
///   Existe um cmd fantasma em ~7:50 que sem este fix seria pareado
///   com o Marine, fazendo o bloco aparecer 7:50..8:48 (ou 8:14..8:48
///   após o push-past-Impeded).
#[test]
fn army_terran_marine_paired_with_latest_cmd_not_phantom_old_cmd() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Marine completa em 1500. Train time típico ~380 loops, então
        // bt_fallback/2 ≈ 190 → max_cmd_loop = 1500 - 190 = 1310.
        // Tanto o cmd fantasma (200) quanto o real (1100) entram na
        // janela; FIFO pegaria 200, latest pega 1100.
        ev_at(1500, 1, EntityEventKind::ProductionStarted, "Marine", 10, Some(1), 0, 0),
        ev_at(1500, 2, EntityEventKind::ProductionFinished, "Marine", 10, None, 0, 0),
    ];
    let cmds = vec![
        cmd(200, "Marine", 1),  // fantasma — clique antigo sem unidade real
        cmd(1100, "Marine", 1), // real — clique no instante do treino
    ];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let prod: Vec<_> = lane
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 1);
    assert_eq!(
        prod[0].start_loop, 1100,
        "deve usar o cmd mais recente (1100), não o fantasma (200)"
    );
    assert_eq!(prod[0].end_loop, 1500);
}

// ─── Par paralelo do Reactor (1 cmd → 2 unidades simultâneas) ───────

/// Numa Barracks com Reactor, dois Marines com cmds simultâneos
/// (1 click cada, ambos no mesmo loop) entram em produção em paralelo
/// — slot 0 e slot 1. Cada Marine consome seu próprio cmd (1 click =
/// 1 unidade) e ambos compartilham o `start_loop` natural (=cmd).
/// O renderer pinta sub_track=0 (top) e sub_track=1 (bottom).
#[test]
fn army_terran_reactor_parallel_pair_simultaneous_cmds() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Reactor instalado: ProductionStarted (com creator_tag=Barracks)
        // + ProductionFinished. Após o finish, lane.reactor_since_loop=600.
        ev_at(200, 1, EntityEventKind::ProductionStarted, "BarracksReactor", 2, Some(1), 53, 50),
        ev_at(600, 2, EntityEventKind::ProductionFinished, "BarracksReactor", 2, None, 53, 50),
        // Dois Marines do mesmo loop — cliques simultâneos do jogador.
        ev_at(2000, 3, EntityEventKind::ProductionStarted, "Marine", 10, Some(1), 0, 0),
        ev_at(2000, 4, EntityEventKind::ProductionFinished, "Marine", 10, None, 0, 0),
        ev_at(2000, 5, EntityEventKind::ProductionStarted, "Marine", 11, Some(1), 0, 0),
        ev_at(2000, 6, EntityEventKind::ProductionFinished, "Marine", 11, None, 0, 0),
    ];
    // Dois cmds Train_Marine no mesmo loop — cliques simultâneos.
    let cmds = vec![cmd(1500, "Marine", 1), cmd(1500, "Marine", 1)];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let prod: Vec<_> = lane
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 2, "par paralelo mantém 2 blocks distintos (sub_track 0 e 1)");
    assert_eq!(prod[0].start_loop, 1500);
    assert_eq!(prod[0].end_loop, 2000);
    assert_eq!(prod[1].start_loop, 1500);
    assert_eq!(prod[1].end_loop, 2000);
    let mut tracks: Vec<u8> = prod.iter().map(|b| b.sub_track).collect();
    tracks.sort();
    assert_eq!(tracks, vec![0, 1], "primeiro slot=0 (top), segundo slot=1 (bottom)");
}

/// Regressão do bug onde o segundo Marine sumia quando o jogador
/// produzia dois Marines com **cliques espaçados** (gap entre cmds
/// muito maior que a antiga PARALLEL_PAIR_TOLERANCE=50 loops). A
/// heurística antiga tratava isso como sequencial e fundia ambos num
/// único bar via `merge_continuous`. Com slot-tracking, o segundo
/// Marine ocupa o slot livre (sub_track=1) com seu próprio cmd_loop.
///
/// Cmds são posicionados em janelas de causalidade distintas
/// (`max_cmd_loop = finish - bt_fallback/2 = finish - 136`):
///  - cmd1=1100 só é válido para M1 (max=1364 em finish=1500).
///  - cmd2=1380 só é válido para M2 (max=1564 em finish=1700; 1380 >
///    1364, então é filtrado da janela do M1). Garante atribuição
///    correta sob LAST-valid sem cruzamento.
#[test]
fn army_terran_reactor_parallel_pair_with_large_cmd_gap() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        ev_at(200, 1, EntityEventKind::ProductionStarted, "BarracksReactor", 2, Some(1), 53, 50),
        ev_at(600, 2, EntityEventKind::ProductionFinished, "BarracksReactor", 2, None, 53, 50),
        ev_at(1500, 3, EntityEventKind::ProductionStarted, "Marine", 10, Some(1), 0, 0),
        ev_at(1500, 4, EntityEventKind::ProductionFinished, "Marine", 10, None, 0, 0),
        ev_at(1700, 5, EntityEventKind::ProductionStarted, "Marine", 11, Some(1), 0, 0),
        ev_at(1700, 6, EntityEventKind::ProductionFinished, "Marine", 11, None, 0, 0),
    ];
    let cmds = vec![cmd(1100, "Marine", 1), cmd(1380, "Marine", 1)];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let prod: Vec<_> = lane
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    assert_eq!(prod.len(), 2, "dois Marines distintos, NÃO mesclados em uma bar");
    // Ordenados por (start_loop, sub_track) em finalize.
    assert_eq!(prod[0].start_loop, 1100, "M1 começa no seu cmd_loop");
    assert_eq!(prod[0].end_loop, 1500);
    assert_eq!(prod[0].sub_track, 0);
    assert_eq!(prod[1].start_loop, 1380, "M2 começa no SEU cmd_loop, não herdado");
    assert_eq!(prod[1].end_loop, 1700);
    assert_eq!(prod[1].sub_track, 1);
}

/// Sanity: produção sequencial NÃO é par paralelo, mesmo com 2
/// Marines no mesmo producer. `finish_loop` diferente por bem mais
/// que zero — segunda Marine começa quando primeira termina.
#[test]
fn army_terran_sequential_marines_chain_normally() {
    let events = vec![
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Marine 1 finisha em 1500.
        ev_at(1500, 1, EntityEventKind::ProductionStarted, "Marine", 10, Some(1), 0, 0),
        ev_at(1500, 2, EntityEventKind::ProductionFinished, "Marine", 10, None, 0, 0),
        // Marine 2 finisha em 1800 (300 loops depois).
        ev_at(1800, 3, EntityEventKind::ProductionStarted, "Marine", 11, Some(1), 0, 0),
        ev_at(1800, 4, EntityEventKind::ProductionFinished, "Marine", 11, None, 0, 0),
    ];
    let cmds = vec![
        cmd(1100, "Marine", 1),
        cmd(1400, "Marine", 1),
    ];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let prod: Vec<_> = lane
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Producing)
        .collect();
    // Marine 1: start=1100, end=1500.
    // Marine 2: start=max(1400, 1500)=1500, end=1800.
    // Após merge: bloco contínuo [1100, 1800] (continuity_tolerance ≥ 0).
    assert!(!prod.is_empty());
    assert_eq!(prod[0].start_loop, 1100);
    assert_eq!(prod.last().unwrap().end_loop, 1800);
}

// ─── Bug control-group (Winter Madness LE — TvT) ────────────────────

/// Regression: o player tem duas Barracks B-A e B-B em control group;
/// emite um único `Build_BarracksReactor` cmd, e o engine SC2 despacha
/// a ordem para AMBAS, gerando dois UnitInits de Reactor quase
/// simultâneos. Mas o cmd stream só registra UM `producer_tag` (a
/// primeira da seleção). Se o cmd fosse primary, ambos os Reactors
/// seriam atribuídos à mesma Barracks e o outro ficaria sem Impeded.
///
/// Com offset exato (+3, 0) como primary, cada Reactor encontra sua
/// própria Barracks pelo encaixe físico, independente do cmd.
#[test]
fn army_terran_two_addons_built_simultaneously_via_control_group() {
    let events = vec![
        // B-A em (50, 50) — fonte do "cmd's producer_tag".
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // B-B em (60, 60) — também recebeu a ordem mas não está no cmd.
        ev_at(110, 1, EntityEventKind::ProductionFinished, "Barracks", 2, None, 60, 60),
        // Reactor R1 fisicamente colado em B-A — Δ=(+3, 0) exato.
        ev_at(200, 2, EntityEventKind::ProductionStarted, "BarracksReactor", 91, None, 53, 50),
        // Reactor R2 fisicamente colado em B-B — Δ=(+3, 0) exato.
        ev_at(202, 3, EntityEventKind::ProductionStarted, "BarracksReactor", 92, None, 63, 60),
        ev_at(600, 4, EntityEventKind::ProductionFinished, "BarracksReactor", 91, None, 53, 50),
        ev_at(602, 5, EntityEventKind::ProductionFinished, "BarracksReactor", 92, None, 63, 60),
    ];
    // Único cmd, com producer_tag = B-A. O segundo Reactor não tem cmd
    // dedicado — o despacho do control group não gera dois cmds.
    let cmds = vec![cmd(200, "BarracksReactor", 1)];
    let p = player_with_events_and_cmds(events, cmds, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane_a = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let lane_b = out.lanes.iter().find(|l| l.tag == 2).unwrap();
    let count = |l: &super::types::StructureLane| {
        l.blocks.iter().filter(|b| b.kind == BlockKind::Impeded).count()
    };
    assert_eq!(
        count(lane_a),
        1,
        "B-A deve ganhar EXATAMENTE 1 Impeded (R1, casado por offset exato), não dois"
    );
    assert_eq!(
        count(lane_b),
        1,
        "B-B deve ganhar 1 Impeded (R2, casado por offset exato) — sem isso o bug do Winter Madness reaparece"
    );
}

// ─── Offset (+3, 0) preferido sobre nearest (B refinado) ────────────

/// Cenário discriminante: a lane geometricamente mais próxima por d²
/// NÃO é a que tem o offset canônico (+3, 0). Sem cmd disponível,
/// o resolver deve preferir o offset exato sobre o nearest.
#[test]
fn army_terran_addon_resolver_prefers_canonical_offset_over_nearest() {
    let events = vec![
        // Barracks-A em (50, 50): Reactor em (53, 50) → Δ=(3, 0) ✓ canônico, d²=9.
        ev_at(100, 0, EntityEventKind::ProductionFinished, "Barracks", 1, None, 50, 50),
        // Barracks-B em (51, 51): Δ=(2, -1) → d²=5 (mais perto por d²!), mas
        // offset não-canônico.
        ev_at(110, 1, EntityEventKind::ProductionFinished, "Barracks", 2, None, 51, 51),
        // Reactor sem cmd associado (creator_tag=None, sem production_cmds).
        ev_at(200, 2, EntityEventKind::ProductionStarted, "BarracksReactor", 99, None, 53, 50),
        ev_at(600, 3, EntityEventKind::ProductionFinished, "BarracksReactor", 99, None, 53, 50),
    ];
    let p = player_with_events(events, "Terran");
    let out = extract_player(&p, 0, LaneMode::Army);

    let lane_a = out.lanes.iter().find(|l| l.tag == 1).unwrap();
    let lane_b = out.lanes.iter().find(|l| l.tag == 2).unwrap();
    let count = |l: &super::types::StructureLane| {
        l.blocks.iter().filter(|b| b.kind == BlockKind::Impeded).count()
    };
    assert_eq!(count(lane_a), 1, "A tem offset canônico (+3, 0); deveria ganhar mesmo sendo mais distante por d²");
    assert_eq!(count(lane_b), 0, "B é mais próxima por d² mas offset errado");
}

// ── Research / Upgrades ─────────────────────────────────────────────

fn upgrade(gl: u32, seq: u32, name: &str) -> crate::replay::UpgradeEntry {
    crate::replay::UpgradeEntry {
        game_loop: gl,
        seq,
        name: name.into(),
    }
}

#[test]
fn research_emits_block_on_producer_lane() {
    // BarracksTechLab viva pesquisa Stimpack. Stimpack tem build_time
    // de ~1760 loops no balance_data — escolhemos cmd/finish com gap
    // suficiente pra satisfazer `cmd_loop <= finish_loop - bt/2`.
    let events = vec![
        ev(100, 0, EntityEventKind::ProductionFinished, "BarracksTechLab", 1, None),
    ];
    let mut p = player_with_events(events, "Terran");
    p.upgrades = vec![upgrade(2200, 10, "Stimpack")];
    p.production_cmds = vec![cmd(1000, "Stimpack", 1)];

    let out = extract_player(&p, 0, LaneMode::Research);
    assert_eq!(out.lanes.len(), 1);
    let lane = &out.lanes[0];
    assert_eq!(lane.canonical_type, "BarracksTechLab");
    assert_eq!(lane.blocks.len(), 1);
    assert_eq!(lane.blocks[0].kind, BlockKind::Producing);
    assert_eq!(lane.blocks[0].start_loop, 1000, "start vem do cmd_loop");
    assert_eq!(lane.blocks[0].end_loop, 2200, "end vem do upgrade.game_loop");
}

#[test]
fn upgrades_emits_sequential_blocks_on_producer_lane() {
    // Forge fazendo W1 → W2. Build_time real do W1 é ~4032 loops, W2
    // ~4928 — precisamos de gap mínimo bt/2 entre cmd e finish para
    // satisfazer a constraint causal.
    let events = vec![
        ev(100, 0, EntityEventKind::ProductionFinished, "Forge", 1, None),
    ];
    let mut p = player_with_events(events, "Protoss");
    p.upgrades = vec![
        upgrade(5000, 10, "ProtossGroundWeaponsLevel1"),
        upgrade(11000, 11, "ProtossGroundWeaponsLevel2"),
    ];
    p.production_cmds = vec![
        cmd(1000, "ProtossGroundWeaponsLevel1", 1),
        cmd(6000, "ProtossGroundWeaponsLevel2", 1),
    ];

    let out = extract_player(&p, 0, LaneMode::Upgrades);
    assert_eq!(out.lanes.len(), 1);
    let lane = &out.lanes[0];
    assert_eq!(lane.canonical_type, "Forge");
    assert_eq!(lane.blocks.len(), 2);
    assert_eq!(lane.blocks[0].start_loop, 1000);
    assert_eq!(lane.blocks[0].end_loop, 5000);
    assert_eq!(lane.blocks[1].start_loop, 6000);
    assert_eq!(lane.blocks[1].end_loop, 11000);
}

#[test]
fn research_mode_filters_out_leveled_upgrades() {
    // Mesma fixture com leveled upgrade — em modo Research, lane some
    // (sem blocos = sem lane se ela não recebeu nenhum upgrade do tipo
    // certo; mas a estrutura é criada como lane porque
    // `lane_canonical("Forge", Research) = Some`. Então ela aparece
    // mas sem blocos).
    let events = vec![
        ev(100, 0, EntityEventKind::ProductionFinished, "Forge", 1, None),
    ];
    let mut p = player_with_events(events, "Protoss");
    p.upgrades = vec![upgrade(900, 10, "ProtossGroundWeaponsLevel1")];
    p.production_cmds = vec![cmd(500, "ProtossGroundWeaponsLevel1", 1)];

    let out = extract_player(&p, 0, LaneMode::Research);
    assert_eq!(out.lanes.len(), 1);
    assert!(
        out.lanes[0].blocks.is_empty(),
        "leveled upgrade não deve aparecer no modo Research"
    );
}

#[test]
fn upgrades_mode_filters_out_one_shot_research() {
    let events = vec![
        ev(100, 0, EntityEventKind::ProductionFinished, "EngineeringBay", 1, None),
    ];
    let mut p = player_with_events(events, "Terran");
    p.upgrades = vec![upgrade(800, 10, "Stimpack")];
    p.production_cmds = vec![cmd(500, "Stimpack", 1)];

    let out = extract_player(&p, 0, LaneMode::Upgrades);
    assert_eq!(out.lanes.len(), 1);
    assert!(
        out.lanes[0].blocks.is_empty(),
        "research one-shot não deve aparecer no modo Upgrades"
    );
}

#[test]
fn research_orphan_cmd_drops_block_silently() {
    // Cmd sem producer_tags (cmd órfão) — não conseguimos rotear pra
    // uma lane, então o bloco é descartado silenciosamente em vez de
    // criar uma lane fantasma.
    let events = vec![
        ev(100, 0, EntityEventKind::ProductionFinished, "EngineeringBay", 1, None),
    ];
    let mut p = player_with_events(events, "Terran");
    p.upgrades = vec![upgrade(800, 10, "Stimpack")];
    p.production_cmds = vec![ProductionCmd {
        game_loop: 500,
        ability: "Stimpack".into(),
        producer_tags: vec![],
        consumed: false,
    }];

    let out = extract_player(&p, 0, LaneMode::Research);
    // EngineeringBay vira lane (lane_canonical reconhece em modo
    // Research) mas sem blocos atribuídos.
    assert_eq!(out.lanes.len(), 1);
    assert!(out.lanes[0].blocks.is_empty());
}
