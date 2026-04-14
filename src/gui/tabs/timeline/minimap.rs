//! Render do mini-mapa: orquestração + primitivas stateless de desenho
//! + helpers de coordenada.

use egui::{
    pos2, vec2, Color32, ColorImage, Pos2, Rect, Sense, Stroke, StrokeKind, TextureOptions, Ui,
};

use crate::colors::player_slot_color_bright;
use crate::map_image::MapImage;
use crate::replay::{EntityCategory, EntityEventKind, ResourceKind, ResourceNode};
use crate::replay_state::{LoadedReplay, PlayableBounds};

use super::entities::alive_entities_at;
use super::overlays::{draw_creep, draw_heatmap};
use super::{CAMERA_HEIGHT_TILES, CAMERA_WIDTH_TILES};

/// Janela em game loops durante a qual um marcador de morte/
/// cancelamento permanece visível depois do evento (~1s em Faster,
/// dado que o SC2 roda a 22.4 loops/s). Marcadores são flash — a
/// intenção é chamar atenção no momento sem poluir o minimapa.
const MARKER_DURATION_LOOPS: u32 = 23;

/// Lado do marcador (X/Ø) em pixels.
const MARKER_SIZE: f32 = 8.0;

pub(super) fn minimap_with_size(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    game_loop: u32,
    rect_size: egui::Vec2,
    show_heatmap: bool,
    show_creep: bool,
    show_map: bool,
) {
    let bounds = loaded.playable_bounds.unwrap_or(PlayableBounds {
        min_x: 0,
        max_x: 255,
        min_y: 0,
        max_y: 255,
    });

    ui.vertical_centered(|ui| {
        let (rect, _resp) = ui.allocate_exact_size(rect_size, Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, Color32::from_gray(22));
        if show_map {
            if let Some(img) = loaded.map_image.as_ref() {
                let key = format!("map:{}", loaded.path.display());
                let texture = ui.ctx().load_texture(
                    key,
                    map_image_to_color_image(img),
                    TextureOptions::LINEAR,
                );
                painter.image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        painter.rect_stroke(rect, 4.0, Stroke::new(1.5, Color32::from_gray(90)), StrokeKind::Outside);

        if show_heatmap {
            // Heatmap: acumula posições da câmera até o instante atual
            // num grid e renderiza como overlay semi-transparente.
            for (i, p) in loaded.timeline.players.iter().enumerate() {
                let color = player_slot_color_bright(i);
                draw_heatmap(&painter, rect, p, game_loop, bounds, color);
            }
        } else {
            // Modo normal: creep → recursos → unidades → câmera. Creep
            // entra na base da pilha (logo acima do minimap) porque
            // representa terreno: minerais, estruturas e unidades
            // continuam visíveis por cima da mancha. Apenas Zerg tem
            // creep_index populado — chamadas para outras raças saem
            // imediatamente via short-circuit em `draw_creep`.
            if show_creep {
                for (i, p) in loaded.timeline.players.iter().enumerate() {
                    let color = player_slot_color_bright(i);
                    draw_creep(&painter, rect, p, game_loop, bounds, color);
                }
            }

            for r in &loaded.timeline.resources {
                draw_resource(&painter, rect, *r, bounds);
            }

            for (i, p) in loaded.timeline.players.iter().enumerate() {
                let color = player_slot_color_bright(i);
                let entities = alive_entities_at(p, game_loop, loaded.timeline.base_build);

                for e in entities.iter().filter(|e| e.category != EntityCategory::Structure) {
                    draw_unit(&painter, rect, e.x, e.y, bounds, e.side, color, false);
                }
                // Estruturas renderizadas por cima das unidades, com
                // borda branca para destacar. Bases (townhalls) usam
                // `TOWNHALL_BASE_SIZE` (2× uma estrutura normal) —
                // âncora visual das bases dos jogadores no minimapa.
                for e in entities.iter().filter(|e| e.category == EntityCategory::Structure) {
                    draw_unit(&painter, rect, e.x, e.y, bounds, e.side, color, true);
                }
            }

            // Marcadores de morte/cancelamento: X para unidade morta,
            // Ø para produção cancelada. Desenhados em cima das unidades
            // (pra chamar atenção) mas por baixo do retângulo de câmera.
            // Duração curta (MARKER_DURATION_LOOPS ≈ 1s) — flash visual.
            for (i, p) in loaded.timeline.players.iter().enumerate() {
                let color = player_slot_color_bright(i);
                for ev in &p.entity_events {
                    if ev.game_loop > game_loop {
                        break;
                    }
                    if game_loop - ev.game_loop > MARKER_DURATION_LOOPS {
                        continue;
                    }
                    // Tumors morrem o tempo todo (são plantadas em
                    // série e morrem ao plantar a filha) — mostrar um
                    // X pra cada morte poluiria o minimapa. A camada
                    // de creep já refleteo desaparecimento.
                    if ev.entity_type.starts_with("CreepTumor") {
                        continue;
                    }
                    match ev.kind {
                        EntityEventKind::Died => {
                            draw_death_marker(
                                &painter,
                                rect,
                                ev.pos_x as f32,
                                ev.pos_y as f32,
                                bounds,
                                color,
                            );
                        }
                        EntityEventKind::ProductionCancelled => {
                            draw_cancel_marker(
                                &painter,
                                rect,
                                ev.pos_x as f32,
                                ev.pos_y as f32,
                                bounds,
                                color,
                            );
                        }
                        _ => {}
                    }
                }
            }

            for (i, p) in loaded.timeline.players.iter().enumerate() {
                let color = player_slot_color_bright(i);
                if let Some(cam) = p.camera_at(game_loop) {
                    draw_camera_rect(&painter, rect, cam.x, cam.y, bounds, color);
                }
            }
        }
    });
}

/// Aspect ratio (largura/altura) do retângulo do minimap. Preferimos o
/// aspect do `Minimap.tga`, que representa a playable area do mapa
/// (o que queremos no rect). Fallback: aspect dos `playable_bounds`
/// observados; senão 1:1.
pub(super) fn map_aspect(loaded: &LoadedReplay) -> f32 {
    if let Some(img) = loaded.map_image.as_ref() {
        if img.width > 0 && img.height > 0 {
            return img.width as f32 / img.height as f32;
        }
    }
    if let Some(b) = loaded.playable_bounds {
        let w = b.max_x.saturating_sub(b.min_x) as f32;
        let h = b.max_y.saturating_sub(b.min_y) as f32;
        if w > 0.0 && h > 0.0 {
            return w / h;
        }
    }
    1.0
}

/// Encaixa um retângulo de aspect `aspect` (largura/altura) dentro de
/// `avail`, preservando proporção (letterbox). Pelo menos um dos eixos
/// fica grudado no `avail`.
pub(super) fn fit_aspect(avail: egui::Vec2, aspect: f32) -> egui::Vec2 {
    if avail.x <= 0.0 || avail.y <= 0.0 || aspect <= 0.0 {
        return vec2(0.0, 0.0);
    }
    let avail_aspect = avail.x / avail.y;
    if avail_aspect > aspect {
        // Espaço sobrando na horizontal: altura é o limite.
        vec2(avail.y * aspect, avail.y)
    } else {
        // Espaço sobrando na vertical: largura é o limite.
        vec2(avail.x, avail.x / aspect)
    }
}

/// Converte um `MapImage` (RGBA8 bruto) para o `ColorImage` que `egui`
/// consome ao criar uma textura. A criação real da `TextureHandle` é
/// cacheada pelo `ui.ctx().load_texture(key, ...)` — o callback abaixo
/// só é chamado de fato na primeira vez que a key aparece.
fn map_image_to_color_image(img: &MapImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied(
        [img.width as usize, img.height as usize],
        &img.rgba,
    )
}

/// Mapeia coordenadas de mapa (em células de tile) para coordenadas de
/// tela dentro do retângulo do mini-mapa, normalizando dentro dos
/// `playable_bounds` observados (que aproximam a área visível do
/// `Minimap.tga`). Inverte Y porque no jogo Y cresce para cima, mas na
/// tela queremos topo = topo.
pub(super) fn to_screen(rect: Rect, x: f32, y: f32, b: PlayableBounds) -> Pos2 {
    let span_x = (b.max_x - b.min_x).max(1) as f32;
    let span_y = (b.max_y - b.min_y).max(1) as f32;
    let nx = ((x - b.min_x as f32) / span_x).clamp(0.0, 1.0);
    let ny = 1.0 - ((y - b.min_y as f32) / span_y).clamp(0.0, 1.0);
    pos2(
        rect.left() + nx * rect.width(),
        rect.top() + ny * rect.height(),
    )
}

/// Cores estilo minimapa do SC2: minerais em azul-cyan claro, minerais
/// rich em amarelo-dourado, gás em verde vivo, rich vespene em violeta.
fn resource_color(kind: ResourceKind) -> Color32 {
    match kind {
        ResourceKind::Mineral => Color32::from_rgb(100, 180, 220),
        ResourceKind::RichMineral => Color32::from_rgb(235, 200, 80),
        ResourceKind::Vespene => Color32::from_rgb(60, 200, 110),
        ResourceKind::RichVespene => Color32::from_rgb(170, 90, 220),
    }
}

fn draw_resource(painter: &egui::Painter, rect: Rect, node: ResourceNode, bounds: PlayableBounds) {
    // Patches 6px, geysers 9px — proporcionais às estruturas não-base
    // (6px) e às bases (12px), dando destaque suficiente pra ler
    // bases/expansões sem afogar as unidades.
    let (side, filled) = match node.kind {
        ResourceKind::Mineral | ResourceKind::RichMineral => (6.0, true),
        ResourceKind::Vespene | ResourceKind::RichVespene => (9.0, true),
    };
    let center = to_screen(rect, node.x as f32, node.y as f32, bounds);
    let half = side * 0.5;
    let r = Rect::from_min_max(
        pos2(center.x - half, center.y - half),
        pos2(center.x + half, center.y + half),
    );
    let color = resource_color(node.kind);
    if filled {
        painter.rect_filled(r, 0.0, color);
    }
}

fn draw_unit(
    painter: &egui::Painter,
    rect: Rect,
    x: f32,
    y: f32,
    bounds: PlayableBounds,
    side: f32,
    color: Color32,
    structure: bool,
) {
    let center = to_screen(rect, x, y, bounds);
    let half = side * 0.5;
    let r = Rect::from_min_max(
        pos2(center.x - half, center.y - half),
        pos2(center.x + half, center.y + half),
    );
    painter.rect_filled(r, 0.0, color);
    if structure {
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
    }
}

fn draw_camera_rect(
    painter: &egui::Painter,
    rect: Rect,
    cx: u8,
    cy: u8,
    bounds: PlayableBounds,
    color: Color32,
) {
    let half_w = CAMERA_WIDTH_TILES / 2.0;
    let half_h = CAMERA_HEIGHT_TILES / 2.0;
    let cx_f = cx as f32;
    let cy_f = cy as f32;

    let top_left = to_screen(rect, cx_f - half_w, cy_f + half_h, bounds);
    let bottom_right = to_screen(rect, cx_f + half_w, cy_f - half_h, bounds);
    let cam_rect = Rect::from_min_max(top_left, bottom_right);

    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 25);
    painter.rect_filled(cam_rect, 0.0, fill);
    let stroke_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 140);
    painter.rect_stroke(cam_rect, 0.0, Stroke::new(1.5, stroke_color), StrokeKind::Outside);
}

/// Marcador de morte: dois segmentos diagonais formando um "X" centrado
/// na posição do evento. Desenhado na cor do slot do jogador que perdeu
/// a unidade (quem morreu, não quem matou).
fn draw_death_marker(
    painter: &egui::Painter,
    rect: Rect,
    x: f32,
    y: f32,
    bounds: PlayableBounds,
    color: Color32,
) {
    let center = to_screen(rect, x, y, bounds);
    let half = MARKER_SIZE * 0.5;
    let stroke = Stroke::new(1.8, color);
    painter.line_segment(
        [
            pos2(center.x - half, center.y - half),
            pos2(center.x + half, center.y + half),
        ],
        stroke,
    );
    painter.line_segment(
        [
            pos2(center.x - half, center.y + half),
            pos2(center.x + half, center.y - half),
        ],
        stroke,
    );
}

/// Marcador de cancelamento: zero cortado (Ø) — um círculo com um
/// segmento diagonal por cima. Usado quando o jogador cancela a
/// construção de um prédio ou o treino de uma unidade.
fn draw_cancel_marker(
    painter: &egui::Painter,
    rect: Rect,
    x: f32,
    y: f32,
    bounds: PlayableBounds,
    color: Color32,
) {
    let center = to_screen(rect, x, y, bounds);
    let half = MARKER_SIZE * 0.5;
    let stroke = Stroke::new(1.5, color);
    painter.circle_stroke(center, half, stroke);
    painter.line_segment(
        [
            pos2(center.x - half, center.y + half),
            pos2(center.x + half, center.y - half),
        ],
        stroke,
    );
}
