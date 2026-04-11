// Aba Timeline — mini-mapa estilo SC2 com scrubbing por segundo.
//
// Reproduz uma versão simplificada do mini-mapa do jogo: um quadrado
// representando a área do mapa, um slider de 1 segundo de precisão para
// escolher o instante e pequenos quadrados marcando cada unidade viva
// daquele jogador na cor do slot (P1 vermelho / P2 azul). O cabeçalho
// mostra os indicadores rápidos do instante (supply, recursos, workers,
// army value) por jogador.
//
// Limitação atual: o parser só guarda `pos_x`/`pos_y` nos eventos de
// nascimento (ProductionFinished) e morte (Died). Não há rastreamento de
// movimento — então cada unidade aparece na posição em que nasceu e some
// quando morre. Quando o parser passar a amostrar posições reais, basta
// trocar a fonte de dados em `alive_entities_at`; a UI continua a mesma.

use std::collections::HashMap;

use egui::{pos2, vec2, Color32, Pos2, Rect, RichText, Sense, Slider, Stroke, Ui};

use crate::colors::player_slot_color_bright;
use crate::config::AppConfig;
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline};
use crate::replay_state::{fmt_time, LoadedReplay};

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    _config: &AppConfig,
    current_second: &mut u32,
) {
    let tl = &loaded.timeline;
    let max_s = tl.duration_seconds.max(1);
    if *current_second > max_s {
        *current_second = max_s;
    }
    let game_loop = (*current_second as f64 * tl.loops_per_second) as u32;

    header_insights(ui, loaded, game_loop);
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Instante:");
        ui.add(
            Slider::new(current_second, 0..=max_s)
                .integer()
                .text("s"),
        );
        ui.monospace(fmt_time(game_loop, tl.loops_per_second));
        ui.weak(format!(
            "/ {}",
            fmt_time(tl.game_loops, tl.loops_per_second)
        ));
    });

    ui.add_space(6.0);
    minimap(ui, loaded, game_loop);
}

// ── Header de insights ─────────────────────────────────────────────────

/// Renderiza uma linha por jogador com supply, recursos, workers e army
/// value no instante `game_loop`. Tudo vem do `StatsSnapshot` mais
/// recente via `PlayerTimeline::stats_at` (binary search O(log n)).
fn header_insights(ui: &mut Ui, loaded: &LoadedReplay, game_loop: u32) {
    ui.add_space(4.0);
    for (i, p) in loaded.timeline.players.iter().enumerate() {
        let slot = player_slot_color_bright(i);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&p.name)
                    .strong()
                    .color(slot),
            );
            match p.stats_at(game_loop) {
                Some(s) => {
                    let army = s.army_value_minerals + s.army_value_vespene;
                    ui.separator();
                    ui.monospace(format!("Supply {}/{}", s.supply_used, s.supply_made));
                    ui.separator();
                    ui.monospace(format!("Min {}", s.minerals));
                    ui.separator();
                    ui.monospace(format!("Gas {}", s.vespene));
                    ui.separator();
                    ui.monospace(format!("Wks {}", s.workers));
                    ui.separator();
                    ui.monospace(format!("Army {}", army));
                }
                None => {
                    ui.weak("(sem stats neste instante)");
                }
            }
        });
    }
    ui.add_space(2.0);
}

// ── Mini-mapa ──────────────────────────────────────────────────────────

fn minimap(ui: &mut Ui, loaded: &LoadedReplay, game_loop: u32) {
    // Quadrado o maior possível centralizado horizontalmente, limitado
    // pelo espaço vertical disponível e por um teto pra não virar HUD.
    let avail = ui.available_size();
    let side = avail.x.min(avail.y).min(560.0).max(160.0);

    ui.vertical_centered(|ui| {
        let (rect, _resp) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
        let painter = ui.painter_at(rect);

        // Fundo do mapa (placeholder pra futura textura) + borda.
        painter.rect_filled(rect, 4.0, Color32::from_gray(22));
        painter.rect_stroke(rect, 4.0, Stroke::new(1.5, Color32::from_gray(90)));

        // Desenha cada jogador. Estruturas vão por cima das unidades
        // dentro do mesmo jogador (laço em duas passadas) pra ficarem
        // visíveis em aglomerados.
        for (i, p) in loaded.timeline.players.iter().enumerate() {
            let color = player_slot_color_bright(i);
            let entities = alive_entities_at(p, game_loop);

            for e in entities.iter().filter(|e| e.category != EntityCategory::Structure) {
                draw_unit(&painter, rect, e.x, e.y, 4.0, color, false);
            }
            for e in entities.iter().filter(|e| e.category == EntityCategory::Structure) {
                draw_unit(&painter, rect, e.x, e.y, 6.0, color, true);
            }
        }
    });
}

/// Mapeia coordenadas de mapa (u8 0..=255) para coordenadas de tela
/// dentro do retângulo do mini-mapa. Inverte Y porque no jogo Y cresce
/// para cima, mas na tela queremos topo = topo (igual ao mini-mapa
/// in-game).
fn to_screen(rect: Rect, x: u8, y: u8) -> Pos2 {
    let nx = x as f32 / 255.0;
    let ny = 1.0 - (y as f32 / 255.0);
    pos2(
        rect.left() + nx * rect.width(),
        rect.top() + ny * rect.height(),
    )
}

fn draw_unit(
    painter: &egui::Painter,
    rect: Rect,
    x: u8,
    y: u8,
    side: f32,
    color: Color32,
    structure: bool,
) {
    let center = to_screen(rect, x, y);
    let half = side * 0.5;
    let r = Rect::from_min_max(
        pos2(center.x - half, center.y - half),
        pos2(center.x + half, center.y + half),
    );
    painter.rect_filled(r, 0.0, color);
    if structure {
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE));
    }
}

// ── Reconstrução de entidades vivas ────────────────────────────────────

#[derive(Clone, Copy)]
struct LiveEntity {
    x: u8,
    y: u8,
    category: EntityCategory,
}

/// Lista as entidades vivas do jogador `p` no `until_loop` (inclusivo).
///
/// Premissa: `entity_events` está ordenado por `game_loop` (garantido
/// pelo parser e coberto por `entity_events_sorted_by_loop` em
/// `replay::tests`). Custo O(n) por chamada — aceitável para milhares
/// de eventos por replay e como esta função é chamada apenas uma vez
/// por frame da aba Timeline.
fn alive_entities_at(p: &PlayerTimeline, until_loop: u32) -> Vec<LiveEntity> {
    let mut alive: HashMap<i64, LiveEntity> = HashMap::new();
    for ev in &p.entity_events {
        if ev.game_loop > until_loop {
            break;
        }
        match ev.kind {
            EntityEventKind::ProductionFinished => {
                alive.insert(
                    ev.tag,
                    LiveEntity {
                        x: ev.pos_x,
                        y: ev.pos_y,
                        category: ev.category,
                    },
                );
            }
            EntityEventKind::Died => {
                alive.remove(&ev.tag);
            }
            EntityEventKind::ProductionStarted | EntityEventKind::ProductionCancelled => {}
        }
    }
    alive.into_values().collect()
}
