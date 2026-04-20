// Helper de análise de perdas/engajamentos — consumer puro de
// `ReplayTimeline`. Usado por consumidores de GUI (cards
// `key_losses`, `army_trades`) e por extractors não-UI
// (`army_production_by_battle`).
//
// Filtra `entity_events` do stream canônico por
// `kind = Died && killer_player_id != self` — isso exclui morphs
// (killer=None) e auto-deaths. Resolve custo via
// `balance_data::resource_cost` pra trabalhar em "valor de recursos"
// em vez de contar unidades (um Battlecruiser conta diferente de um
// Zergling).

use crate::balance_data::resource_cost;
use crate::replay::{EntityCategory, EntityEventKind, ReplayTimeline};

pub struct DeathEvent {
    pub game_loop: u32,
    pub entity_type: String,
    pub category: EntityCategory,
    pub minerals: u32,
    pub vespene: u32,
}

impl DeathEvent {
    pub fn total_value(&self) -> u32 {
        self.minerals + self.vespene
    }
}

/// Unidades/estruturas que o jogador `player_idx` perdeu para o
/// adversário. Exclui morphs (killer None) e auto-deaths
/// (killer == self). Ordenado por `game_loop`.
pub fn player_losses(timeline: &ReplayTimeline, player_idx: usize) -> Vec<DeathEvent> {
    let Some(player) = timeline.players.get(player_idx) else {
        return Vec::new();
    };
    let self_id = Some(player.player_id);
    player
        .entity_events
        .iter()
        .filter(|e| {
            e.kind == EntityEventKind::Died
                && e.killer_player_id.is_some()
                && e.killer_player_id != self_id
        })
        .map(|e| {
            let (m, v) = resource_cost(&e.entity_type, timeline.base_build);
            DeathEvent {
                game_loop: e.game_loop,
                entity_type: e.entity_type.clone(),
                category: e.category,
                minerals: m,
                vespene: v,
            }
        })
        .collect()
}

/// Unidades inimigas que o jogador `player_idx` matou. Varre os
/// `entity_events` dos outros players procurando `killer_player_id`
/// que bate com o `player_id` do POV. Ordenado por `game_loop`.
pub fn player_kills(timeline: &ReplayTimeline, player_idx: usize) -> Vec<DeathEvent> {
    let Some(player) = timeline.players.get(player_idx) else {
        return Vec::new();
    };
    let my_id = player.player_id;
    let mut out = Vec::new();
    for (idx, other) in timeline.players.iter().enumerate() {
        if idx == player_idx {
            continue;
        }
        for e in &other.entity_events {
            if e.kind != EntityEventKind::Died {
                continue;
            }
            if e.killer_player_id != Some(my_id) {
                continue;
            }
            let (m, v) = resource_cost(&e.entity_type, timeline.base_build);
            out.push(DeathEvent {
                game_loop: e.game_loop,
                entity_type: e.entity_type.clone(),
                category: e.category,
                minerals: m,
                vespene: v,
            });
        }
    }
    out.sort_by_key(|d| d.game_loop);
    out
}

/// Engajamento agrupado: todas as mortes que ocorrem em sequência
/// com gap menor que `gap_loops` entre eventos consecutivos. Agrupa
/// perdas do POV + kills do POV numa única janela.
pub struct Engagement {
    pub start_loop: u32,
    pub end_loop: u32,
    pub lost_value: u32,
    pub killed_value: u32,
}

impl Engagement {
    /// Trade líquido do ponto de vista do POV — negativo quando ele
    /// perdeu mais recurso do que destruiu.
    pub fn net_trade(&self) -> i64 {
        self.killed_value as i64 - self.lost_value as i64
    }

    pub fn total_value(&self) -> u32 {
        self.lost_value + self.killed_value
    }
}

/// Agrupa perdas + kills em engajamentos contíguos. Gap em loops:
/// eventos consecutivos dentro de `gap_loops` entram no mesmo
/// engajamento. Ignora `Structure` — prédios caídos não representam
/// trade de army (cobertos pelo card Key Losses).
pub fn cluster_engagements(
    losses: &[DeathEvent],
    kills: &[DeathEvent],
    gap_loops: u32,
) -> Vec<Engagement> {
    enum Side {
        Loss,
        Kill,
    }
    let mut events: Vec<(u32, &DeathEvent, Side)> = Vec::new();
    for d in losses {
        if d.category == EntityCategory::Structure {
            continue;
        }
        events.push((d.game_loop, d, Side::Loss));
    }
    for d in kills {
        if d.category == EntityCategory::Structure {
            continue;
        }
        events.push((d.game_loop, d, Side::Kill));
    }
    events.sort_by_key(|(gl, _, _)| *gl);

    let mut out: Vec<Engagement> = Vec::new();
    let mut current: Option<Engagement> = None;
    let mut last_loop: u32 = 0;

    for (gl, d, side) in events {
        let start_new = match &current {
            None => true,
            Some(_) => gl.saturating_sub(last_loop) > gap_loops,
        };
        if start_new {
            if let Some(e) = current.take() {
                out.push(e);
            }
            current = Some(Engagement {
                start_loop: gl,
                end_loop: gl,
                lost_value: 0,
                killed_value: 0,
            });
        }
        let eng = current.as_mut().expect("current is Some");
        eng.end_loop = gl;
        match side {
            Side::Loss => eng.lost_value += d.total_value(),
            Side::Kill => eng.killed_value += d.total_value(),
        }
        last_loop = gl;
    }
    if let Some(e) = current {
        out.push(e);
    }
    out
}
