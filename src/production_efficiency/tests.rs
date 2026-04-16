use super::*;
use crate::replay::{
    EntityCategory, EntityEvent, EntityEventKind, PlayerTimeline, ReplayTimeline, UpgradeEntry,
};

fn mk_timeline(players: Vec<PlayerTimeline>, game_loops: u32) -> ReplayTimeline {
    ReplayTimeline {
        file: String::new(),
        map: String::new(),
        datetime: String::new(),
        game_loops,
        duration_seconds: game_loops / 16,
        loops_per_second: 22.4,
        base_build: 0,
        max_time_seconds: 0,
        players,
        chat: Vec::new(),
        cache_handles: Vec::new(),
        map_size_x: 0,
        map_size_y: 0,
        resources: Vec::new(),
    }
}

fn mk_player(race: &str) -> PlayerTimeline {
    PlayerTimeline {
        name: "P".to_string(),
        clan: String::new(),
        race: race.to_string(),
        mmr: None,
        player_id: 1,
        result: None,
        toon: None,
        stats: Vec::new(),
        upgrades: Vec::new(),
        entity_events: Vec::new(),
        production_cmds: Vec::new(),
        inject_cmds: Vec::new(),
        unit_positions: Vec::new(),
        camera_positions: Vec::new(),
        alive_count: std::collections::HashMap::new(),
        worker_capacity: Vec::new(),
        worker_births: Vec::new(),
        army_capacity: Vec::new(),
        worker_capacity_cumulative: Vec::new(),
        army_capacity_cumulative: Vec::new(),
        upgrade_cumulative: Vec::new(),
        creep_index: Vec::new(),
    }
}

/// Tolerância padrão para comparações de pct (erros de ponto
/// flutuante no cálculo da média ponderada).
const EPS: f64 = 1e-6;

/// Localiza a amostra cujo `game_loop` (fim do bucket) bate
/// exatamente com `t`. Retorna `None` se o bucket não existe —
/// usado nos testes para asserts em boundaries conhecidos.
fn bucket_ending_at(samples: &[EfficiencySample], t: u32) -> EfficiencySample {
    *samples
        .iter()
        .find(|s| s.game_loop == t)
        .unwrap_or_else(|| panic!("no bucket ending at game_loop={t}"))
}

#[test]
fn workers_baseline_single_cc_one_birth() {
    // lps=22.4 → bucket_loops = 224 (10s). game_loops=1000.
    // worker_capacity [(0, +1)], worker_births [300].
    // ProdStart = 300 − 272 = 28, ProdEnd = 300.
    let mut p = mk_player("Terr");
    p.worker_capacity.push((0, 1));
    p.worker_births.push(300);
    let tl = mk_timeline(vec![p], 1000);

    let series = extract_efficiency_series(&tl, EfficiencyTarget::Workers).unwrap();
    let s = &series.players[0].samples;

    // Buckets devem terminar em 224, 448, 672, 896 e 1000 (parcial).
    assert_eq!(s[0].game_loop, 224);
    // [0, 224): cap=1, active=0 em [0,28) e active=1 em [28,224).
    //   cap_int = 224, act_int = 196 → 87.5%.
    assert!((s[0].efficiency_pct - (196.0 / 224.0 * 100.0)).abs() < EPS);
    // [224, 448): cap=1; active=1 em [224,300), active=0 em [300,448).
    //   cap_int = 224, act_int = 76 → 33.93%.
    assert_eq!(s[1].game_loop, 448);
    assert!((s[1].efficiency_pct - (76.0 / 224.0 * 100.0)).abs() < EPS);
    // [448, 672) e seguintes: cap=1, active=0 → 0%.
    assert_eq!(s[2].game_loop, 672);
    assert_eq!(s[2].efficiency_pct, 0.0);
    // Último bucket é parcial (ends at game_end=1000).
    assert_eq!(s.last().unwrap().game_loop, 1000);
}

#[test]
fn army_baseline_one_barracks_one_unit() {
    let mut p = mk_player("Terr");
    p.army_capacity.push((100, 1));
    p.entity_events.push(EntityEvent {
        game_loop: 200,
        seq: 0,
        kind: EntityEventKind::ProductionStarted,
        entity_type: "Marine".to_string(),
        category: EntityCategory::Unit,
        tag: 42,
        pos_x: 0,
        pos_y: 0,
        creator_ability: Some("TrainMarine".to_string()),
        creator_tag: None,
        killer_player_id: None,
    });
    p.entity_events.push(EntityEvent {
        game_loop: 500,
        seq: 1,
        kind: EntityEventKind::ProductionFinished,
        entity_type: "Marine".to_string(),
        category: EntityCategory::Unit,
        tag: 42,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    });
    let tl = mk_timeline(vec![p], 1000);

    let series = extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap();
    let s = &series.players[0].samples;

    // [0, 224): cap=0 em [0,100), cap=1 em [100,224); active=1 em [200,224).
    //   cap_int = 124, act_int = 24 → ≈ 19.35%.
    let b0 = bucket_ending_at(s, 224);
    assert!((b0.efficiency_pct - (24.0 / 124.0 * 100.0)).abs() < EPS);
    // [224, 448): cap=1, active=1 → 100%.
    let b1 = bucket_ending_at(s, 448);
    assert!((b1.efficiency_pct - 100.0).abs() < EPS);
    // [448, 672): active=1 em [448,500), active=0 em [500,672).
    //   cap_int = 224, act_int = 52 → ≈ 23.21%.
    let b2 = bucket_ending_at(s, 672);
    assert!((b2.efficiency_pct - (52.0 / 224.0 * 100.0)).abs() < EPS);
    // Buckets seguintes: cap=1, active=0 → 0%.
    let b3 = bucket_ending_at(s, 896);
    assert_eq!(b3.efficiency_pct, 0.0);
}

// ── Helpers para testes Zerg ─────────────────────────────────────

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

#[test]
fn zerg_workers_single_hatch_continuous_is_100pct() {
    // 1 Hatch born em t=0; 1 Drone morfando de forma contínua
    // (Started em 28, Finished em 300 = ciclo de 272 loops).
    // Durante [28, 300), capacity=1, active=1 → 100%.
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.entity_events.push(mk_unit_started(28, 1, "Drone", 1, EntityCategory::Worker));
    p.entity_events.push(mk_unit_finished(300, 2, "Drone", 1, EntityCategory::Worker));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    // [0, 224): capacity=1 o tempo todo; active=1 em [28, 224) = 196 loops.
    //   cap_int=224, act_int=196 → 87.5%.
    let b0 = bucket_ending_at(s, 224);
    assert!((b0.efficiency_pct - (196.0 / 224.0 * 100.0)).abs() < EPS);
    // [224, 448): active=1 em [224, 300) = 76 loops; depois 0.
    //   cap_int=224, act_int=76 → ~33.9%.
    let b1 = bucket_ending_at(s, 448);
    assert!((b1.efficiency_pct - (76.0 / 224.0 * 100.0)).abs() < EPS);
}

#[test]
fn zerg_workers_inject_boost_with_full_spend() {
    // 1 Hatch desde t=0. Inject em t=500 → capacity vira 5 durante
    // [500, 1150). Rajada de 4 drones consumindo entre t=900 e t=1100
    // (build 272 loops cada). Buckets dentro da janela antes dos
    // drones spendados ficam em active=0,capacity=5 → 0% ou ~baixo;
    // quando os 4 drones estão morfando em paralelo, active=4,
    // capacity=5 → 80%.
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.inject_cmds.push(mk_inject(500));
    // 4 drones, todos starteando na janela de inject e terminando
    // dentro dela.
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

    // [448, 672): inject liga em 500 → cap=1 em [448,500) (52 loops)
    // e cap=5 em [500,672) (172 loops). active=0 todo o tempo.
    //   cap_int = 52*1 + 172*5 = 52 + 860 = 912; act=0 → 0%.
    let b1 = bucket_ending_at(s, 672);
    assert_eq!(b1.efficiency_pct, 0.0);

    // [896, 1120): cap=5 o tempo todo (inject ativo até 500+650=1150).
    // active=4 em [900, 1120) = 220 loops; active=0 em [896, 900) = 4.
    //   cap_int = 224*5 = 1120; act_int = 220*4 = 880 → ~78.57%.
    let b_spend = bucket_ending_at(s, 1120);
    assert!(
        (b_spend.efficiency_pct - (880.0 / 1120.0 * 100.0)).abs() < EPS,
        "esperava ~78.57, veio {}",
        b_spend.efficiency_pct
    );
}

#[test]
fn zerg_workers_inject_ignored_drops_efficiency() {
    // 1 Hatch desde t=0, 1 inject em t=500, nenhum drone produzido.
    // Janela do inject: [500, 1150). Dentro: capacity=5, active=0 → 0%.
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    p.inject_cmds.push(mk_inject(500));
    let tl = mk_timeline(vec![p], 1500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers)
        .unwrap()
        .players[0]
        .samples;

    // Todo bucket dentro ou fora da janela: active=0 → 0%. (Sentinel
    // 100% não aplica porque capacity > 0 sempre.)
    for sample in s {
        assert_eq!(
            sample.efficiency_pct, 0.0,
            "bucket ending at {} esperava 0%, veio {}",
            sample.game_loop, sample.efficiency_pct
        );
    }
}

#[test]
fn zerg_army_only_larva_born_counted() {
    // 1 Hatch; eventos Started/Finished para Zergling (larva-born),
    // Baneling (morph de Zergling — fora), Queen (morph da Hatch —
    // fora) e Overlord (larva-born — entra).
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    // Zergling: entra na série.
    p.entity_events
        .push(mk_unit_started(100, 1, "Zergling", 1, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 2, "Zergling", 1, EntityCategory::Unit));
    // Baneling: ignorar.
    p.entity_events
        .push(mk_unit_started(100, 3, "Baneling", 2, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 4, "Baneling", 2, EntityCategory::Unit));
    // Queen: ignorar.
    p.entity_events
        .push(mk_unit_started(100, 5, "Queen", 3, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 6, "Queen", 3, EntityCategory::Unit));
    // Overlord: entra.
    p.entity_events
        .push(mk_unit_started(100, 7, "Overlord", 4, EntityCategory::Unit));
    p.entity_events
        .push(mk_unit_finished(300, 8, "Overlord", 4, EntityCategory::Unit));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army)
        .unwrap()
        .players[0]
        .samples;

    // [0, 224): capacity=1; active=2 (Zergling + Overlord) em
    // [100, 224) = 124 loops. Clampa em min(active,capacity)=1.
    //   cap_int=224, act_int=124 → ~55.36%.
    let b0 = bucket_ending_at(s, 224);
    assert!(
        (b0.efficiency_pct - (124.0 / 224.0 * 100.0)).abs() < EPS,
        "bucket 224 esperava {}, veio {}",
        124.0 / 224.0 * 100.0,
        b0.efficiency_pct
    );
    // Se Baneling/Queen estivessem sendo contados, active seria 4
    // (clampado a 1), não mudaria o resultado. Para verificar que
    // estão sendo FILTRADOS, checar o sample struct.
    assert_eq!(b0.active, 1);
    assert_eq!(b0.capacity, 1);
}

#[test]
fn zerg_hatch_morph_lair_no_capacity_gap() {
    // Hatch born em t=0; morpha para Lair em t=1000. Pelo
    // apply_type_change, o replay gera Died(Hatchery) +
    // ProductionFinished(Lair) no mesmo loop. Net capacity change
    // = 0. Antes e depois, capacity=1.
    let mut p = mk_player("Zerg");
    p.entity_events.push(mk_hatch_finished(0, 0, 100));
    // Morph: Died(Hatchery) no mesmo loop que ProductionFinished(Lair).
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

    // Qualquer bucket depois de t=0: capacity=1.
    for sample in s {
        assert_eq!(
            sample.capacity, 1,
            "bucket em {} caiu para cap={}; esperava manter 1 durante o morph",
            sample.game_loop, sample.capacity
        );
    }
}

#[test]
fn zerg_hatch_destroyed_cancels_inflight_morph() {
    // Hatch born em t=0. Drone morph Started em t=300 (tag X). Hatch
    // Died em t=400; Drone morph Cancelled em t=400 (mesmo tag). Após
    // t=400: capacity=0, active=0, sem underflow.
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

    // [448, 672) e seguintes: capacity=0, active=0 → sentinel 100%.
    let late = bucket_ending_at(s, 672);
    assert_eq!(late.capacity, 0);
    assert_eq!(late.active, 0);
    assert_eq!(late.efficiency_pct, 100.0);
}

#[test]
fn zerg_army_supply_maxed_forces_hundred_percent() {
    // Cenário: 1 Hatch desde t=0, nenhuma produção. supply > 185
    // a partir de t=500 → buckets dentro do regime em 100%.
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

    // Antes de supply cruzar: active=0 → 0%.
    assert_eq!(bucket_ending_at(s, 224).efficiency_pct, 0.0);
    assert_eq!(bucket_ending_at(s, 448).efficiency_pct, 0.0);
    // [448, 672): supply cruza em 500; [500, 672) = 172 loops em 100%.
    //   cap_int=224, act_int=172 → ~76.8%.
    let mixed = bucket_ending_at(s, 672);
    assert!((mixed.efficiency_pct - (172.0 / 224.0 * 100.0)).abs() < EPS);
    // Buckets seguintes inteiramente em supply-high: 100%.
    assert!((bucket_ending_at(s, 896).efficiency_pct - 100.0).abs() < EPS);
}

#[test]
fn zerg_workers_ignore_supply_maxed_override() {
    // Zerg workers não aplicam o override — supply cheio ainda
    // deveria estar gastando larva em Overlord. Cenário: 1 Hatch,
    // nenhum drone, supply maxed o jogo todo → 0% em todos os
    // buckets (não 100%).
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

#[test]
fn capacity_loss_during_production_no_underflow() {
    // Barracks morre no mesmo loop em que a produção é cancelada.
    let mut p = mk_player("Terr");
    p.army_capacity.push((100, 1));
    p.army_capacity.push((300, -1));
    p.entity_events.push(EntityEvent {
        game_loop: 200,
        seq: 0,
        kind: EntityEventKind::ProductionStarted,
        entity_type: "Marine".to_string(),
        category: EntityCategory::Unit,
        tag: 7,
        pos_x: 0,
        pos_y: 0,
        creator_ability: Some("TrainMarine".to_string()),
        creator_tag: None,
        killer_player_id: None,
    });
    p.entity_events.push(EntityEvent {
        game_loop: 300,
        seq: 1,
        kind: EntityEventKind::ProductionCancelled,
        entity_type: "Marine".to_string(),
        category: EntityCategory::Unit,
        tag: 7,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    });
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
    // [224, 448): em 300 ProdEnd (ordem 2) aplica antes de CapacityDown (ordem 3),
    // então não houve underflow. active=1 em [224,300) e cap=0 em [300,448).
    //   cap_int = 76, act_int = 76 → 100%.
    let mid = bucket_ending_at(s, 448);
    assert!((mid.efficiency_pct - 100.0).abs() < EPS);
    // Bucket após a morte do Barracks: cap_int inteiro = 0 → sentinel 100%.
    let late = bucket_ending_at(s, 672);
    assert_eq!(late.capacity, 0);
    assert_eq!(late.active, 0);
    assert_eq!(late.efficiency_pct, 100.0);
}

#[test]
fn army_train_started_and_finished_same_loop_is_back_dated() {
    // Cenário Terran típico: tracker emite Started+Finished no
    // mesmo game_loop porque o UnitBorn não foi precedido de
    // UnitInit (trains Terran vêm direto de UnitBornEvent). Sem
    // back-data, a produção parece instantânea e a eficiência
    // fica zerada durante toda a janela real de produção.
    let mut p = mk_player("Terr");
    p.army_capacity.push((0, 1));
    // "Marine" existe no balance data — build_time_loops devolve
    // ~272 loops. Usamos um entity_type conhecido para que o
    // lookup retorne um valor > 0; o teste só precisa garantir
    // que a eficiência é 100% em algum instante entre a janela
    // back-dated e o finish.
    let finish = 500u32;
    p.entity_events.push(EntityEvent {
        game_loop: finish,
        seq: 0,
        kind: EntityEventKind::ProductionStarted,
        entity_type: "Marine".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: Some("TrainMarine".to_string()),
        creator_tag: None,
        killer_player_id: None,
    });
    p.entity_events.push(EntityEvent {
        game_loop: finish,
        seq: 1,
        kind: EntityEventKind::ProductionFinished,
        entity_type: "Marine".to_string(),
        category: EntityCategory::Unit,
        tag: 1,
        pos_x: 0,
        pos_y: 0,
        creator_ability: None,
        creator_tag: None,
        killer_player_id: None,
    });
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
    // Sem back-data o Started/Finished coincidiriam no loop 500 e
    // nenhum tempo de produção entraria nos buckets — eficiência 0%
    // em todos eles. Com back-data para 228 (500 − 272), o bucket
    // [224, 448) fica quase inteiro em produção (active=1 em
    // [228, 448) = 220 loops de 224) → ~98%.
    let back_dated = bucket_ending_at(s, 448);
    assert!(
        back_dated.efficiency_pct > 50.0,
        "back-data deveria levar o bucket a >50%, veio {}",
        back_dated.efficiency_pct
    );
}

#[test]
fn orphan_started_closes_at_game_end() {
    let mut p = mk_player("Terr");
    p.army_capacity.push((100, 1));
    p.entity_events.push(EntityEvent {
        game_loop: 200,
        seq: 0,
        kind: EntityEventKind::ProductionStarted,
        entity_type: "Marine".to_string(),
        category: EntityCategory::Unit,
        tag: 11,
        pos_x: 0,
        pos_y: 0,
        creator_ability: Some("TrainMarine".to_string()),
        creator_tag: None,
        killer_player_id: None,
    });
    // Sem Finished nem Cancelled.
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
    // Buckets inteiramente dentro de [200, 1000] devem ficar em 100%
    // — se o órfão não tivesse sido fechado em game_end, `active`
    // não seria decrementado e igualmente daria 100%; então esse
    // teste garante sobretudo que o sweep atravessa o stream até o
    // fim sem parar.
    let b_mid = bucket_ending_at(s, 672);
    let b_late = bucket_ending_at(s, 896);
    assert!((b_mid.efficiency_pct - 100.0).abs() < EPS);
    assert!((b_late.efficiency_pct - 100.0).abs() < EPS);
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

#[test]
fn army_supply_maxed_forces_hundred_percent() {
    // Cenário: 1 Barracks desde t=0, nenhuma produção ocorrendo.
    // Sem o override, a eficiência seria 0% em todos os buckets.
    // Com supply_used > 185 a partir de t=500, os buckets dentro
    // desse regime devem ficar em 100%.
    let mut p = mk_player("Terr");
    p.army_capacity.push((0, 1));
    // Snapshots: supply sobe para 190 em t=500 e fica lá.
    p.stats.push(mk_stats(0, 12));
    p.stats.push(mk_stats(200, 100));
    p.stats.push(mk_stats(500, 190));
    p.stats.push(mk_stats(1000, 190));
    let tl = mk_timeline(vec![p], 1000);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Army).unwrap().players[0].samples;
    // [0, 224) e [224, 448): supply baixo → Barracks ocioso → 0%.
    assert_eq!(bucket_ending_at(s, 224).efficiency_pct, 0.0);
    assert_eq!(bucket_ending_at(s, 448).efficiency_pct, 0.0);
    // [448, 672): supply cruza 185 em 500 → parte inicial (52
    // loops) conta como idle, restante (172 loops) como 100%.
    //   cap_int = 224, act_int = 172 → ~76.8%.
    let mixed = bucket_ending_at(s, 672);
    assert!((mixed.efficiency_pct - (172.0 / 224.0 * 100.0)).abs() < EPS);
    // [672, 896) e [896, 1000): supply_high o bucket inteiro → 100%.
    assert!((bucket_ending_at(s, 896).efficiency_pct - 100.0).abs() < EPS);
    assert!((bucket_ending_at(s, 1000).efficiency_pct - 100.0).abs() < EPS);
}

#[test]
fn workers_ignore_supply_maxed_override() {
    // O override é só para army. Workers em idle com supply maxed
    // continuam contando como idle (o jogador pode colocar workers
    // em gás, refinery extra, etc. — idleness de CC é real).
    let mut p = mk_player("Terr");
    p.worker_capacity.push((0, 1));
    // Nenhum worker sendo produzido, supply maxed desde o começo.
    p.stats.push(mk_stats(0, 190));
    let tl = mk_timeline(vec![p], 500);

    let s = &extract_efficiency_series(&tl, EfficiencyTarget::Workers).unwrap().players[0].samples;
    // Todos os buckets: capacity=1, active=0 → 0%.
    for sample in s {
        assert_eq!(sample.efficiency_pct, 0.0);
    }
}

fn mk_upgrade(game_loop: u32, name: &str) -> UpgradeEntry {
    UpgradeEntry { game_loop, seq: 0, name: name.to_string() }
}

#[test]
fn warpgate_extends_busy_window_with_cooldown() {
    // Cenário: Protoss com 1 WarpGate (registrado em army_capacity
    // desde t=0), WarpGateResearch completando em t=100, e um
    // warp-in de Zealot começando em t=500. O replay emite
    // ProductionFinished em t=612 (~5s de warp-in), mas a janela
    // real de ocupação do slot vai até t=500+560=1060 (~25s,
    // incluindo cooldown).
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

    // [448, 672): active=1 em [500, 672) = 172 loops (cap_int=224).
    //   pct = 172/224 ≈ 76.8%.
    let b = bucket_ending_at(s, 672);
    assert!((b.efficiency_pct - (172.0 / 224.0 * 100.0)).abs() < EPS);
    // [672, 896) e [896, 1120): dentro da janela estendida (vai
    // até 1060). O bucket 1120 inclui active=1 em [896, 1060) =
    //   164 loops. [1120, 1344): idle, 0%.
    assert!((bucket_ending_at(s, 896).efficiency_pct - 100.0).abs() < EPS);
    let partial = bucket_ending_at(s, 1120);
    assert!(
        (partial.efficiency_pct - (164.0 / 224.0 * 100.0)).abs() < EPS,
        "bucket 1120 esperava {}, veio {}",
        164.0 / 224.0 * 100.0,
        partial.efficiency_pct
    );
    // Após o cooldown: slot ocioso.
    assert_eq!(bucket_ending_at(s, 1344).efficiency_pct, 0.0);
}

#[test]
fn warpgate_cycle_not_applied_before_research() {
    // Mesmo cenário do teste acima, mas o warp-in acontece antes
    // da pesquisa completar. Deve usar a janela curta (normal
    // gateway train) em vez da janela estendida.
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
    // [448, 672): active=1 apenas em [448, 500) = 52 loops. Sem
    // a extensão de WarpGate, retorna a 0 após 500. pct = 52/224.
    let b = bucket_ending_at(s, 672);
    assert!((b.efficiency_pct - (52.0 / 224.0 * 100.0)).abs() < EPS);
    // Bucket seguinte: idle.
    assert_eq!(bucket_ending_at(s, 896).efficiency_pct, 0.0);
}

#[test]
fn warpgate_only_extends_for_warp_gate_units() {
    // Cenário: Robotics Facility produzindo Immortal depois de
    // WarpGateResearch. Immortal não está no roster de warpáveis,
    // então mantém a janela normal (não leva a extensão).
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
    // Após 500 deve voltar a idle (sem extensão por cooldown).
    assert_eq!(bucket_ending_at(s, 896).efficiency_pct, 0.0);
}
