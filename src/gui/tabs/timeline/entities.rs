//! Agregações derivadas sobre `entity_events` + `camera_positions`:
//! reconstrução das entidades vivas em um dado instante
//! (`alive_entities_at`) e a métrica de atenção a estruturas
//! (`structure_attention_at`).

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline};

/// Lado base (px) do quadrado de uma unidade de 1 supply no minimapa.
/// Unidades com mais supply escalam a partir daqui via
/// `unit_scale_for_supply`.
const UNIT_BASE_SIZE: f32 = 4.0;

/// Lado base (px) de uma estrutura não-base (Barracks, Gateway, etc.).
/// Bases (townhalls) usam `STRUCTURE_BASE_SIZE * 2` — âncoras visuais.
/// Ambos já incluem o "inflar 50%" em relação ao tamanho histórico
/// (6/12 px), para que estruturas fiquem mais legíveis.
const STRUCTURE_BASE_SIZE: f32 = 9.0;
const TOWNHALL_BASE_SIZE: f32 = 18.0;

/// Escala de tamanho em função do supply ocupado pela unidade (×10 — é
/// a unidade retornada por `balance_data::supply_cost_x10`). A fórmula
/// é `1.0 + (supply - 1) × 0.25`, clampada em 1.0 pra baixo:
///
/// | supply | fator |
/// |--------|-------|
/// |   1    | 1.00x |
/// |   2    | 1.25x |
/// |   3    | 1.50x |
/// |   4    | 1.75x |
/// |   5    | 2.00x |
/// |   6    | 2.25x |
///
/// Unidades de meio-supply (zergling = 0.5) e unidades desconhecidas
/// (supply_x10 == 0) caem no clamp inferior e ficam no tamanho base.
fn unit_scale_for_supply(supply_x10: u32) -> f32 {
    let supply = supply_x10 as f32 / 10.0;
    (1.0 + (supply - 1.0) * 0.25).max(1.0)
}

#[derive(Clone, Copy)]
pub(super) struct LiveEntity {
    /// Coordenadas em tile units, mas em `f32` pra acomodar a posição
    /// interpolada entre amostras esparsas de `unit_positions`.
    /// Estruturas usam `pos_x/pos_y as f32` direto do evento de
    /// nascimento (sempre integer aligned).
    pub x: f32,
    pub y: f32,
    pub category: EntityCategory,
    /// `true` para prédios de main-base (CC, OrbitalCommand,
    /// PlanetaryFortress, Nexus, Hatchery, Lair, Hive). Usado para
    /// desenhar esses prédios em tamanho maior no minimapa, já que
    /// servem de âncora visual pras bases dos jogadores.
    #[allow(dead_code)]
    pub is_base: bool,
    /// Lado do quadrado no minimapa (px), já com a escala por supply
    /// aplicada (ver `unit_scale_for_supply`). Pré-computado em
    /// `alive_entities_at` pra evitar lookup na tabela de balance data
    /// a cada frame.
    pub side: f32,
}

/// Detecta estruturas de main-base (townhalls). Inclui morphs zerg
/// (Lair, Hive) e terran (OrbitalCommand, PlanetaryFortress) pra que
/// a aparência visual se mantenha grande após o upgrade.
fn is_base_type(name: &str) -> bool {
    matches!(
        name,
        "CommandCenter"
            | "OrbitalCommand"
            | "PlanetaryFortress"
            | "Nexus"
            | "Hatchery"
            | "Lair"
            | "Hive"
    )
}

/// Lista as entidades vivas do jogador `p` no `until_loop` (inclusivo).
///
/// Premissa: `entity_events` está ordenado por `game_loop` (garantido
/// pelo parser e coberto por `entity_events_sorted_by_loop` em
/// `replay::tests`). Custo O(n) por chamada — aceitável para milhares
/// de eventos por replay e como esta função é chamada apenas uma vez
/// por frame da aba Timeline.
pub(super) fn alive_entities_at(
    p: &PlayerTimeline,
    until_loop: u32,
    base_build: u32,
) -> Vec<LiveEntity> {
    let mut alive: HashMap<i64, LiveEntity> = HashMap::new();
    for ev in &p.entity_events {
        if ev.game_loop > until_loop {
            break;
        }
        match ev.kind {
            EntityEventKind::ProductionFinished => {
                // Tumors são desenhadas implicitamente pela camada de
                // creep — pular aqui evita o quadrado de 9px de
                // estrutura por cima da própria mancha.
                if ev.entity_type.starts_with("CreepTumor") {
                    continue;
                }
                let is_base = is_base_type(&ev.entity_type);
                let side = match ev.category {
                    EntityCategory::Structure => {
                        if is_base { TOWNHALL_BASE_SIZE } else { STRUCTURE_BASE_SIZE }
                    }
                    // Workers e Unit: 1 supply × fator por supply.
                    // SCV/Drone/Probe são 1 supply → fator 1.0 → 4px.
                    _ => {
                        let cost = balance_data::supply_cost_x10(&ev.entity_type, base_build);
                        UNIT_BASE_SIZE * unit_scale_for_supply(cost)
                    }
                };
                alive.insert(
                    ev.tag,
                    LiveEntity {
                        x: ev.pos_x as f32,
                        y: ev.pos_y as f32,
                        category: ev.category,
                        is_base,
                        side,
                    },
                );
            }
            EntityEventKind::Died => {
                alive.remove(&ev.tag);
            }
            EntityEventKind::ProductionStarted | EntityEventKind::ProductionCancelled => {}
        }
    }
    // Sobrescreve a posição de nascimento com a posição interpolada
    // linearmente entre as duas amostras adjacentes de
    // `unit_positions`. Tags que nunca apareceram em `unit_positions`
    // (ex.: estruturas) ficam no ponto original.
    let positions = p.interpolated_positions(until_loop);
    for (tag, ent) in alive.iter_mut() {
        if let Some(&(x, y)) = positions.get(tag) {
            ent.x = x;
            ent.y = y;
        }
    }
    alive.into_values().collect()
}

// ── Structure attention ───────────────────────────────────────────────
//
// Percentual do tempo jogado (até `until_loop`) em que o viewport da
// câmera do jogador cobria ao menos uma estrutura própria viva. Derivado
// exclusivamente dos streams canônicos `camera_positions` e
// `entity_events` — sem novos campos persistentes.

/// Meia largura/altura do viewport em tiles, arredondadas para o teste de
/// overlap inteiro. As constantes-fonte são `CAMERA_WIDTH_TILES = 24.0`
/// e `CAMERA_HEIGHT_TILES = 14.0`.
const CAMERA_HALF_W_TILES: i32 = 12;
const CAMERA_HALF_H_TILES: i32 = 7;

/// Retorna `(attention_loops, elapsed_loops)` do jogador até
/// `until_loop` (inclusivo).
///
/// - `attention_loops`: soma das durações das amostras de câmera cujo
///   viewport (24×14 tiles, centrado em `(cam.x, cam.y)`) cobre ≥1
///   estrutura própria viva.
/// - `elapsed_loops`: soma total das durações dessas mesmas amostras.
///
/// Os dois valores são computados no mesmo sweep para que o caller
/// possa formatar como porcentagem (`att / tot`) sem divisões por zero
/// (retornamos `(0, 0)` quando não há nenhuma amostra de câmera).
pub(super) fn structure_attention_at(p: &PlayerTimeline, until_loop: u32) -> (u32, u32) {
    let end_idx = p
        .camera_positions
        .partition_point(|c| c.game_loop <= until_loop);
    if end_idx == 0 {
        return (0, 0);
    }
    let cams = &p.camera_positions[..end_idx];

    // Sweep combinado: iteramos estruturas em ordem de `game_loop` (os
    // `entity_events` já estão ordenados pelo parser) junto com as
    // amostras de câmera. Mantemos o conjunto de estruturas vivas
    // atualizado *antes* de avaliar cada amostra de câmera.
    let mut alive: HashMap<i64, (u8, u8)> = HashMap::new();
    let mut ev_iter = p.entity_events.iter().filter(|ev| {
        matches!(
            ev.kind,
            EntityEventKind::ProductionFinished | EntityEventKind::Died
        ) && ev.category == EntityCategory::Structure
            && !ev.entity_type.starts_with("CreepTumor")
    });
    let mut pending_ev = ev_iter.next();

    let mut attention_loops: u32 = 0;
    let mut elapsed_loops: u32 = 0;

    for (i, cam) in cams.iter().enumerate() {
        // Aplica todos os eventos com `game_loop <= cam.game_loop` antes
        // de avaliar a cobertura: uma estrutura que nasce no mesmo loop
        // da câmera já conta como alvo potencial; uma que morre no
        // mesmo loop já sai do conjunto.
        while let Some(ev) = pending_ev {
            if ev.game_loop > cam.game_loop {
                break;
            }
            match ev.kind {
                EntityEventKind::ProductionFinished => {
                    alive.insert(ev.tag, (ev.pos_x, ev.pos_y));
                }
                EntityEventKind::Died => {
                    alive.remove(&ev.tag);
                }
                _ => {}
            }
            pending_ev = ev_iter.next();
        }

        // Duração coberta pela amostra: até o próximo sample ou, na
        // última amostra, até `until_loop + 1` (inclusivo com o slider).
        let next_loop = if i + 1 < cams.len() {
            cams[i + 1].game_loop.min(until_loop + 1)
        } else {
            until_loop + 1
        };
        let dur = next_loop.saturating_sub(cam.game_loop);
        if dur == 0 {
            continue;
        }
        elapsed_loops += dur;

        let cx = cam.x as i32;
        let cy = cam.y as i32;
        let covers = alive.values().any(|&(sx, sy)| {
            (sx as i32 - cx).abs() <= CAMERA_HALF_W_TILES
                && (sy as i32 - cy).abs() <= CAMERA_HALF_H_TILES
        });
        if covers {
            attention_loops += dur;
        }
    }

    (attention_loops, elapsed_loops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{CameraPosition, EntityCategory, EntityEvent, EntityEventKind};

    fn empty_player() -> PlayerTimeline {
        PlayerTimeline {
            name: String::new(),
            clan: String::new(),
            race: String::new(),
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
            alive_count: HashMap::new(),
            worker_capacity: Vec::new(),
            worker_births: Vec::new(),
            army_capacity: Vec::new(),
            upgrade_cumulative: Vec::new(),
            creep_index: Vec::new(),
        }
    }

    fn ev_finished(tag: i64, loop_: u32, x: u8, y: u8, ty: &str) -> EntityEvent {
        EntityEvent {
            game_loop: loop_,
            seq: 0,
            kind: EntityEventKind::ProductionFinished,
            entity_type: ty.to_string(),
            category: EntityCategory::Structure,
            tag,
            pos_x: x,
            pos_y: y,
            creator_ability: None,
            creator_tag: None,
            killer_player_id: None,
        }
    }

    fn ev_died(tag: i64, loop_: u32, x: u8, y: u8, ty: &str) -> EntityEvent {
        EntityEvent {
            game_loop: loop_,
            seq: 0,
            kind: EntityEventKind::Died,
            entity_type: ty.to_string(),
            category: EntityCategory::Structure,
            tag,
            pos_x: x,
            pos_y: y,
            creator_ability: None,
            creator_tag: None,
            killer_player_id: None,
        }
    }

    fn cam(loop_: u32, x: u8, y: u8) -> CameraPosition {
        CameraPosition { game_loop: loop_, x, y }
    }

    #[test]
    fn empty_returns_zero_over_zero() {
        let p = empty_player();
        assert_eq!(structure_attention_at(&p, 1000), (0, 0));
    }

    #[test]
    fn camera_without_structures_is_fully_distracted() {
        let mut p = empty_player();
        p.camera_positions = vec![cam(0, 100, 100), cam(100, 200, 200)];
        // Sem nenhuma estrutura → nunca cobre → att=0, tot=201 (até loop 200 inclusive).
        let (att, tot) = structure_attention_at(&p, 200);
        assert_eq!(att, 0);
        assert_eq!(tot, 201);
    }

    #[test]
    fn alternating_camera_splits_attention() {
        let mut p = empty_player();
        // Uma estrutura em (50, 50), viva desde o início.
        p.entity_events = vec![ev_finished(1, 0, 50, 50, "Barracks")];
        // Câmera 1: em cima da estrutura (dist 0, dentro do viewport).
        // Câmera 2: longe (dist > 12 em x e > 7 em y).
        p.camera_positions = vec![cam(0, 50, 50), cam(100, 200, 200)];
        let (att, tot) = structure_attention_at(&p, 200);
        // Sample 0: duração 100 (loops 0..100), cobre → att += 100.
        // Sample 1: duração 101 (loops 100..=200), não cobre.
        assert_eq!(att, 100);
        assert_eq!(tot, 201);
    }

    #[test]
    fn viewport_edge_still_counts() {
        let mut p = empty_player();
        // Estrutura exatamente no limite do viewport (dx=12, dy=7).
        p.entity_events = vec![ev_finished(1, 0, 62, 57, "Barracks")];
        p.camera_positions = vec![cam(0, 50, 50)];
        let (att, tot) = structure_attention_at(&p, 10);
        assert_eq!(att, 11);
        assert_eq!(tot, 11);
    }

    #[test]
    fn structure_death_removes_coverage() {
        let mut p = empty_player();
        // Estrutura em (50, 50) nasce no loop 0 e morre no loop 50.
        p.entity_events = vec![
            ev_finished(1, 0, 50, 50, "Barracks"),
            ev_died(1, 50, 50, 50, "Barracks"),
        ];
        // Câmera em cima da estrutura o tempo todo, mas estrutura some no meio.
        p.camera_positions = vec![cam(0, 50, 50), cam(50, 50, 50)];
        let (att, tot) = structure_attention_at(&p, 100);
        // Sample 0 (loops 0..50): estrutura viva → cobre → att += 50.
        // Sample 1 (loops 50..=100, dur 51): estrutura morreu antes → não cobre.
        assert_eq!(att, 50);
        assert_eq!(tot, 101);
    }

    #[test]
    fn creep_tumor_is_ignored() {
        let mut p = empty_player();
        // Uma tumor bem no viewport — mas tumors não contam como "estrutura própria".
        p.entity_events = vec![ev_finished(1, 0, 50, 50, "CreepTumorBurrowed")];
        p.camera_positions = vec![cam(0, 50, 50)];
        let (att, tot) = structure_attention_at(&p, 10);
        assert_eq!(att, 0);
        assert_eq!(tot, 11);
    }
}
