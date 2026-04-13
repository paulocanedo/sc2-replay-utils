// Aba Timeline — mini-mapa estilo SC2 com scrubbing por segundo.
//
// Reproduz uma versão simplificada do mini-mapa do jogo: um quadrado
// representando a área do mapa, um slider de 1 segundo de precisão para
// escolher o instante e pequenos quadrados marcando cada unidade viva
// daquele jogador na cor do slot (P1 vermelho / P2 azul). O cabeçalho
// mostra os indicadores rápidos do instante (supply, recursos, workers,
// army value) por jogador.
//
// Posições: cada unidade nasce em `EntityEvent.pos_x/pos_y` e, quando
// o parser captou amostras de movimento via `UnitPositionsEvent`,
// `alive_entities_at` sobrescreve com a última posição conhecida em
// `PlayerTimeline.unit_positions`. Estruturas raramente recebem
// amostras (o SC2 só amostra unidades móveis/visíveis), então
// permanecem no ponto de nascimento.

use std::collections::HashMap;

use egui::{
    pos2, vec2, Color32, ColorImage, Pos2, Rect, RichText, Sense, Slider, Stroke, TextureOptions,
    Ui,
};

use crate::colors::player_slot_color_bright;
use crate::config::AppConfig;
use crate::map_image::MapImage;
use crate::replay::{EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline};
use crate::replay_state::{fmt_time, LoadedReplay, PlayableBounds};

/// Tamanho do viewport da câmera do SC2 em tiles (zoom padrão).
const CAMERA_WIDTH_TILES: f32 = 24.0;
const CAMERA_HEIGHT_TILES: f32 = 14.0;

/// Largura fixa dos painéis laterais de stats dos jogadores.
const SIDE_PANEL_WIDTH: f32 = 110.0;

/// Resolução do grid de heatmap (células por eixo). Valores maiores
/// dão mais detalhe mas custam mais memória e iteração na renderização.
const HEATMAP_GRID: usize = 64;

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    _config: &AppConfig,
    current_second: &mut u32,
    show_heatmap: &mut bool,
) {
    let tl = &loaded.timeline;
    let max_s = tl.duration_seconds.max(1);
    if *current_second > max_s {
        *current_second = max_s;
    }
    let game_loop = (*current_second as f64 * tl.loops_per_second) as u32;

    transport_bar(ui, tl, current_second, game_loop, max_s, show_heatmap);
    ui.separator();

    // Layout: [P1 stats | minimap | P2 stats]
    // Pré-calcula o tamanho do minimapa usando toda a altura disponível
    // para que o ui.horizontal não comprima a altura ao conteúdo dos
    // painéis laterais.
    let spacing = ui.spacing().item_spacing.x;
    let avail = ui.available_size();
    let center_width = (avail.x - SIDE_PANEL_WIDTH * 2.0 - spacing * 2.0).max(100.0);
    let map_avail = vec2(center_width, avail.y);
    let aspect = map_aspect(loaded);
    let map_size = fit_aspect(map_avail, aspect);

    ui.horizontal(|ui| {
        // Força a altura do layout horizontal ao tamanho do mapa.
        ui.set_min_height(map_size.y);

        // P1 — painel esquerdo
        ui.vertical(|ui| {
            ui.set_width(SIDE_PANEL_WIDTH);
            if let Some(p) = loaded.timeline.players.get(0) {
                player_side_panel(ui, p, 0, game_loop);
            }
        });

        // Minimapa central — largura limitada para não empurrar o P2
        ui.vertical(|ui| {
            ui.set_width(map_size.x);
            minimap_with_size(ui, loaded, game_loop, map_size, *show_heatmap);
        });

        // P2 — painel direito
        ui.vertical(|ui| {
            ui.set_width(SIDE_PANEL_WIDTH);
            if let Some(p) = loaded.timeline.players.get(1) {
                player_side_panel(ui, p, 1, game_loop);
            }
        });
    });
}

// ── Transport bar ─────────────────────────────────────────────────────
//
// Slider de scrubbing em estilo "transport bar" de player de vídeo: o
// rail ocupa quase toda a largura disponível da aba para permitir
// arrasto granular (cada pixel ≈ 1s mesmo em partidas longas), e o
// tempo atual / tempo total fica compacto à esquerda. Sem rótulo
// "Instante:" pra reduzir ruído — o display de tempo já comunica.

fn transport_bar(
    ui: &mut Ui,
    tl: &ReplayTimeline,
    current_second: &mut u32,
    game_loop: u32,
    max_s: u32,
    show_heatmap: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.monospace(format!(
            "{} / {}",
            fmt_time(game_loop, tl.loops_per_second),
            fmt_time(tl.game_loops, tl.loops_per_second),
        ));
        ui.add_space(12.0);
        ui.toggle_value(show_heatmap, "Heatmap");
        ui.add_space(4.0);
        let slider_w = (ui.available_width() - 12.0).max(160.0);
        ui.spacing_mut().slider_width = slider_w;
        ui.add(
            Slider::new(current_second, 0..=max_s)
                .integer()
                .show_value(false),
        );
    });
}

// ── Painel lateral de stats ────────────────────────────────────────────

/// Renderiza stats de um jogador verticalmente num painel lateral.
fn player_side_panel(ui: &mut Ui, p: &PlayerTimeline, idx: usize, game_loop: u32) {
    let slot = player_slot_color_bright(idx);
    ui.add_space(4.0);
    ui.label(RichText::new(&p.name).strong().color(slot));
    ui.add_space(4.0);
    match p.stats_at(game_loop) {
        Some(s) => {
            let supply_cap = s.supply_made.min(200);
            let army = s.army_value_minerals + s.army_value_vespene;
            ui.monospace(format!("{}/{} supply", s.supply_used, supply_cap));
            ui.monospace(format!("{} min", s.minerals));
            ui.monospace(format!("{} gas", s.vespene));
            ui.monospace(format!("{} wks", s.workers));
            ui.monospace(format!("{} army", army));
        }
        None => {
            ui.weak("—");
        }
    }
}

// ── Mini-mapa ──────────────────────────────────────────────────────────

fn minimap_with_size(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    game_loop: u32,
    rect_size: egui::Vec2,
    show_heatmap: bool,
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
        painter.rect_stroke(rect, 4.0, Stroke::new(1.5, Color32::from_gray(90)));

        if show_heatmap {
            // Heatmap: acumula posições da câmera até o instante atual
            // num grid e renderiza como overlay semi-transparente.
            for (i, p) in loaded.timeline.players.iter().enumerate() {
                let color = player_slot_color_bright(i);
                draw_heatmap(&painter, rect, p, game_loop, bounds, color);
            }
        } else {
            // Modo normal: unidades + câmera.
            for (i, p) in loaded.timeline.players.iter().enumerate() {
                let color = player_slot_color_bright(i);
                let entities = alive_entities_at(p, game_loop);

                for e in entities.iter().filter(|e| e.category != EntityCategory::Structure) {
                    draw_unit(&painter, rect, e.x, e.y, bounds, 4.0, color, false);
                }
                for e in entities.iter().filter(|e| e.category == EntityCategory::Structure) {
                    draw_unit(&painter, rect, e.x, e.y, bounds, 6.0, color, true);
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
fn map_aspect(loaded: &LoadedReplay) -> f32 {
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
fn fit_aspect(avail: egui::Vec2, aspect: f32) -> egui::Vec2 {
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
fn to_screen(rect: Rect, x: u8, y: u8, b: PlayableBounds) -> Pos2 {
    let span_x = (b.max_x - b.min_x).max(1) as f32;
    let span_y = (b.max_y - b.min_y).max(1) as f32;
    let nx = (x.saturating_sub(b.min_x) as f32 / span_x).clamp(0.0, 1.0);
    let ny = 1.0 - (y.saturating_sub(b.min_y) as f32 / span_y).clamp(0.0, 1.0);
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
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE));
    }
}

/// Variante de `to_screen` que aceita coordenadas `f32` para sub-tile
/// precision (necessária para as bordas do retângulo da câmera).
fn to_screen_f32(rect: Rect, x: f32, y: f32, b: PlayableBounds) -> Pos2 {
    let span_x = (b.max_x - b.min_x).max(1) as f32;
    let span_y = (b.max_y - b.min_y).max(1) as f32;
    let nx = ((x - b.min_x as f32) / span_x).clamp(0.0, 1.0);
    let ny = 1.0 - ((y - b.min_y as f32) / span_y).clamp(0.0, 1.0);
    pos2(
        rect.left() + nx * rect.width(),
        rect.top() + ny * rect.height(),
    )
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

    let top_left = to_screen_f32(rect, cx_f - half_w, cy_f + half_h, bounds);
    let bottom_right = to_screen_f32(rect, cx_f + half_w, cy_f - half_h, bounds);
    let cam_rect = Rect::from_min_max(top_left, bottom_right);

    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 25);
    painter.rect_filled(cam_rect, 0.0, fill);
    let stroke_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 140);
    painter.rect_stroke(cam_rect, 0.0, Stroke::new(1.5, stroke_color));
}

// ── Heatmap de câmera ─────────────────────────────────────────────────

/// Renderiza um heatmap de tempo de câmera do jogador sobre o minimapa.
///
/// Para cada posição de câmera, preenche a área inteira do viewport
/// (~24×14 tiles) no grid, ponderada pela duração (game loops) que a
/// câmera permaneceu naquela posição. Isso produz um mapa de calor que
/// realça mais onde o jogador olhou por mais tempo e com uma área de
/// influência proporcional ao campo de visão real do jogo.
fn draw_heatmap(
    painter: &egui::Painter,
    rect: Rect,
    player: &PlayerTimeline,
    until_loop: u32,
    bounds: PlayableBounds,
    color: Color32,
) {
    let span_x = (bounds.max_x - bounds.min_x).max(1) as f32;
    let span_y = (bounds.max_y - bounds.min_y).max(1) as f32;

    // Tamanho do viewport da câmera em células do grid.
    let vp_gx = ((CAMERA_WIDTH_TILES / span_x) * HEATMAP_GRID as f32).ceil() as usize;
    let vp_gy = ((CAMERA_HEIGHT_TILES / span_y) * HEATMAP_GRID as f32).ceil() as usize;
    let half_vp_gx = vp_gx / 2;
    let half_vp_gy = vp_gy / 2;

    let mut grid = vec![0.0f32; HEATMAP_GRID * HEATMAP_GRID];

    // Índice do último sample relevante (para calcular duração).
    let end_idx = player
        .camera_positions
        .partition_point(|c| c.game_loop <= until_loop);
    let samples = &player.camera_positions[..end_idx];

    for (i, cam) in samples.iter().enumerate() {
        // Duração: delta até o próximo sample (ou até until_loop para o último).
        let next_loop = if i + 1 < samples.len() {
            samples[i + 1].game_loop.min(until_loop)
        } else {
            until_loop
        };
        let duration = next_loop.saturating_sub(cam.game_loop) as f32;
        if duration <= 0.0 {
            continue;
        }

        // Centro da câmera em coordenadas de grid.
        let nx = ((cam.x as f32 - bounds.min_x as f32) / span_x).clamp(0.0, 0.999);
        let ny = ((cam.y as f32 - bounds.min_y as f32) / span_y).clamp(0.0, 0.999);
        let center_gx = (nx * HEATMAP_GRID as f32) as usize;
        let center_gy = (ny * HEATMAP_GRID as f32) as usize;

        // Preenche toda a área do viewport no grid.
        let gy_min = center_gy.saturating_sub(half_vp_gy);
        let gy_max = (center_gy + half_vp_gy).min(HEATMAP_GRID - 1);
        let gx_min = center_gx.saturating_sub(half_vp_gx);
        let gx_max = (center_gx + half_vp_gx).min(HEATMAP_GRID - 1);

        for gy in gy_min..=gy_max {
            for gx in gx_min..=gx_max {
                grid[gy * HEATMAP_GRID + gx] += duration;
            }
        }
    }

    let max_val = grid.iter().copied().fold(0.0f32, f32::max);
    if max_val <= 0.0 {
        return;
    }

    let cell_w = rect.width() / HEATMAP_GRID as f32;
    let cell_h = rect.height() / HEATMAP_GRID as f32;

    for gy in 0..HEATMAP_GRID {
        for gx in 0..HEATMAP_GRID {
            let val = grid[gy * HEATMAP_GRID + gx];
            if val <= 0.0 {
                continue;
            }
            // Curva cúbica: zonas pouco visitadas ficam quase
            // invisíveis, zonas densas mantêm realce forte.
            let ratio = val / max_val;
            let intensity = ratio * ratio * ratio;
            let alpha = (intensity * 220.0) as u8;
            let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha);
            // Y invertido: gy=0 é min_y do jogo (base do mapa) → base da tela.
            let screen_gy = HEATMAP_GRID - 1 - gy;
            let cell_rect = Rect::from_min_size(
                pos2(
                    rect.left() + gx as f32 * cell_w,
                    rect.top() + screen_gy as f32 * cell_h,
                ),
                vec2(cell_w, cell_h),
            );
            painter.rect_filled(cell_rect, 0.0, fill);
        }
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
    // Sobrescreve a posição de nascimento com a última amostra de
    // movimento conhecida. Tags que nunca apareceram em
    // `unit_positions` (ex.: estruturas) ficam no ponto original.
    let positions = p.last_known_positions(until_loop);
    for (tag, ent) in alive.iter_mut() {
        if let Some(&(x, y)) = positions.get(tag) {
            ent.x = x;
            ent.y = y;
        }
    }
    alive.into_values().collect()
}
