// Aba Timeline — mini-mapa estilo SC2 com scrubbing por game loop.
//
// Reproduz uma versão simplificada do mini-mapa do jogo: um quadrado
// representando a área do mapa, um slider com precisão de game loop para
// escolher o instante e pequenos quadrados marcando cada unidade viva
// daquele jogador na cor do slot (P1 vermelho / P2 azul). O cabeçalho
// mostra os indicadores rápidos do instante (supply, recursos, workers,
// army value) por jogador.
//
// Posições: cada unidade nasce em `EntityEvent.pos_x/pos_y` e, quando
// o parser captou amostras de movimento via `UnitPositionsEvent`,
// `alive_entities_at` sobrescreve com a posição interpolada
// linearmente entre as duas amostras adjacentes em
// `PlayerTimeline.unit_positions`. A interpolação é necessária porque
// o SC2 emite as amostras esparsamente (~2-3 por unidade na vida
// inteira); sem ela as unidades pareceriam teleportar entre poucos
// pontos. Estruturas raramente recebem amostras (o SC2 só amostra
// unidades móveis/visíveis), então permanecem no ponto de nascimento.

use std::collections::HashMap;

use egui::{
    pos2, vec2, Color32, ColorImage, Pos2, Rect, RichText, Sense, Slider, Stroke, StrokeKind,
    TextStyle, TextureOptions, Ui,
};

use crate::balance_data;
use crate::colors::player_slot_color_bright;
use crate::config::AppConfig;
use crate::map_image::MapImage;
use crate::replay::{
    EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline, ResourceKind, ResourceNode,
};
use crate::replay::CreepEntry;
use crate::replay_state::{fmt_time, LoadedReplay, PlayableBounds};

/// Tamanho do viewport da câmera do SC2 em tiles (zoom padrão).
const CAMERA_WIDTH_TILES: f32 = 24.0;
const CAMERA_HEIGHT_TILES: f32 = 14.0;

/// Janela em game loops durante a qual um marcador de morte/
/// cancelamento permanece visível depois do evento (~1s em Faster,
/// dado que o SC2 roda a 22.4 loops/s). Marcadores são flash — a
/// intenção é chamar atenção no momento sem poluir o minimapa.
const MARKER_DURATION_LOOPS: u32 = 23;

/// Lado do marcador (X/Ø) em pixels.
const MARKER_SIZE: f32 = 8.0;

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

/// Raio aproximado (em tiles do mapa) da cobertura de creep gerada por
/// uma fonte (hatchery/lair/hive ou tumor) quando atingiu o tamanho
/// pleno. O valor real in-game varia (~10 para hatchery, ~10 para
/// tumor adulto) e o spread é progressivo, mas para o MVP usamos um
/// raio fixo — visualmente próximo o suficiente da cobertura real e
/// barato de renderizar com `circle_filled`.
const CREEP_RADIUS_TILES: f32 = 10.0;

/// Alpha (0–255) do círculo de creep. Translúcido o bastante para
/// múltiplas fontes próximas se sobreporem em mancha mais densa sem
/// saturar a cor do jogador, e transparente o suficiente para deixar
/// minerais/geysers visíveis abaixo.
const CREEP_ALPHA: u8 = 55;

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

/// Número de caracteres monospace que cabem no painel lateral. A
/// largura real é derivada do glifo "M" da fonte monospace atual, então
/// escala com o `font_size_points` do usuário (HiDPI-aware).
/// Dimensionado para o conteúdo mais largo, "200/200 supply" (14 ch),
/// com folga para padding interno.
const SIDE_PANEL_CHARS: f32 = 16.0;

/// Calcula a largura do painel lateral com base no tamanho atual da
/// fonte monospace + padding do frame do painel. Recomputado a cada
/// frame pra responder a mudanças no zoom/font size sem reload.
fn side_panel_width(ui: &Ui) -> f32 {
    let font_id = ui.style().text_styles[&TextStyle::Monospace].clone();
    // Mede um glifo "M" da monospace via `Painter::layout_no_wrap` —
    // a única API de medição que aceita `&self` em egui 0.34.
    let glyph_w = ui
        .painter()
        .layout_no_wrap("M".to_string(), font_id, Color32::WHITE)
        .rect
        .width();
    let frame_padding = ui.style().spacing.window_margin.sum().x;
    glyph_w * SIDE_PANEL_CHARS + frame_padding
}

/// Resolução do grid de heatmap (células por eixo). Valores maiores
/// dão mais detalhe mas custam mais memória e iteração na renderização.
const HEATMAP_GRID: usize = 64;

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    _config: &AppConfig,
    current_loop: &mut u32,
    show_heatmap: &mut bool,
    show_creep: &mut bool,
    show_map: &mut bool,
) {
    let tl = &loaded.timeline;
    let max_loop = tl.game_loops.max(1);
    if *current_loop > max_loop {
        *current_loop = max_loop;
    }
    let game_loop = *current_loop;
    let side_w = side_panel_width(ui);

    // Layout em painéis (estilo egui_demo `panels.rs`):
    // - Top: indicador de tempo + toggle de heatmap
    // - Bottom: botões de step + slider de scrubbing
    // - Left: stats do P1
    // - Right: stats do P2
    // - Central: minimapa
    egui::Panel::top("timeline_top")
        .resizable(false)
        .show_inside(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.monospace(format!(
                    "{} / {}",
                    fmt_time(*current_loop, tl.loops_per_second),
                    fmt_time(tl.game_loops, tl.loops_per_second),
                ));
                ui.add_space(12.0);
                ui.toggle_value(show_heatmap, "Heatmap");
                ui.toggle_value(show_creep, "Creep");
                ui.toggle_value(show_map, "Map");
            });
            ui.add_space(2.0);
        });

    egui::Panel::bottom("timeline_bottom")
        .resizable(false)
        .show_inside(ui, |ui| {
            ui.add_space(2.0);
            transport_slider(ui, tl, current_loop, max_loop);
            ui.add_space(2.0);
        });

    egui::Panel::left("timeline_p1")
        .resizable(false)
        .exact_size(side_w)
        .show_inside(ui, |ui| {
            if let Some(p) = loaded.timeline.players.get(0) {
                player_side_panel(ui, p, 0, game_loop);
            }
        });

    egui::Panel::right("timeline_p2")
        .resizable(false)
        .exact_size(side_w)
        .show_inside(ui, |ui| {
            if let Some(p) = loaded.timeline.players.get(1) {
                player_side_panel(ui, p, 1, game_loop);
            }
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let aspect = map_aspect(loaded);
        let map_size = fit_aspect(ui.available_size(), aspect);
        minimap_with_size(
            ui,
            loaded,
            game_loop,
            map_size,
            *show_heatmap,
            *show_creep,
            *show_map,
        );
    });
}

// ── Transport bar ─────────────────────────────────────────────────────
//
// Slider de scrubbing em estilo "transport bar" de player de vídeo: o
// rail ocupa quase toda a largura disponível do bottom panel para
// permitir arrasto granular. Botões de step permitem avançar/retroceder
// 1 game loop (◂/▸) ou 1 segundo (|◂/▸|), com hold-to-repeat.

/// Delay antes de iniciar o repeat ao manter um botão pressionado.
const HOLD_INITIAL_DELAY: f32 = 0.30;
/// Intervalo entre steps durante hold-to-repeat (~15 steps/s).
const HOLD_REPEAT_INTERVAL: f32 = 0.066;

fn transport_slider(
    ui: &mut Ui,
    tl: &ReplayTimeline,
    current_loop: &mut u32,
    max_loop: u32,
) {
    let one_second = tl.loops_per_second.round() as i64;

    ui.horizontal(|ui| {
        step_button(ui, "|◂", current_loop, -one_second, max_loop);
        step_button(ui, "◂", current_loop, -1, max_loop);
        step_button(ui, "▸", current_loop, 1, max_loop);
        step_button(ui, "▸|", current_loop, one_second, max_loop);
        ui.add_space(4.0);
        let slider_w = (ui.available_width() - 12.0).max(160.0);
        ui.spacing_mut().slider_width = slider_w;
        ui.add(
            Slider::new(current_loop, 0..=max_loop)
                .integer()
                .show_value(false),
        );
    });
}

/// Botão de step com hold-to-repeat. Um clique aplica `delta` uma vez;
/// manter pressionado repete após um delay inicial.
fn step_button(ui: &mut Ui, label: &str, current_loop: &mut u32, delta: i64, max_loop: u32) {
    let btn = ui.button(label);
    if btn.clicked() {
        apply_delta(current_loop, delta, max_loop);
    }
    if btn.is_pointer_button_down_on() {
        ui.ctx().request_repaint();
        let held = btn.interact_pointer_pos().map_or(0.0, |_| {
            ui.input(|i| i.pointer.press_start_time().map_or(0.0, |t| i.time - t))
        });
        if held > HOLD_INITIAL_DELAY as f64 {
            let dt = ui.input(|i| i.unstable_dt);
            // Accumulate fractional steps via the response's ID-based memory.
            let accum = ui.memory_mut(|mem| {
                let a = mem.data.get_temp_mut_or_default::<f32>(btn.id);
                *a += dt;
                *a
            });
            if accum >= HOLD_REPEAT_INTERVAL {
                let steps = (accum / HOLD_REPEAT_INTERVAL) as i64;
                apply_delta(current_loop, delta * steps, max_loop);
                ui.memory_mut(|mem| {
                    let a = mem.data.get_temp_mut_or_default::<f32>(btn.id);
                    *a -= steps as f32 * HOLD_REPEAT_INTERVAL;
                });
            }
        }
    } else {
        // Reset accumulator when button is released.
        ui.memory_mut(|mem| mem.data.remove::<f32>(btn.id));
    }
}

fn apply_delta(current_loop: &mut u32, delta: i64, max_loop: u32) {
    *current_loop = (*current_loop as i64 + delta).clamp(0, max_loop as i64) as u32;
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
    let (att, tot) = structure_attention_at(p, game_loop);
    let txt = if tot == 0 {
        "— bldg focus".to_string()
    } else {
        let pct = att as f32 * 100.0 / tot as f32;
        format!("{:.0}% bldg focus", pct)
    };
    ui.monospace(txt);
}

// ── Mini-mapa ──────────────────────────────────────────────────────────

fn minimap_with_size(
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
fn to_screen(rect: Rect, x: f32, y: f32, b: PlayableBounds) -> Pos2 {
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

// ── Camada de creep ───────────────────────────────────────────────────

/// Desenha a camada de creep do jogador como círculos translúcidos
/// centrados em cada fonte (hatchery/lair/hive/tumor) viva no instante
/// `until_loop`. Usa o índice `creep_index` (pré-computado em
/// `finalize.rs`) e binary-search para parar cedo no range de "já
/// nasceu", cabendo no orçamento por-frame mesmo em late-game.
fn draw_creep(
    painter: &egui::Painter,
    rect: Rect,
    player: &PlayerTimeline,
    until_loop: u32,
    bounds: PlayableBounds,
    color: Color32,
) {
    if player.creep_index.is_empty() {
        return;
    }
    let span_x = (bounds.max_x - bounds.min_x).max(1) as f32;
    let radius_px = CREEP_RADIUS_TILES * (rect.width() / span_x);
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), CREEP_ALPHA);

    // O(log n) para achar o fim do range "já nasceu". O loop linear
    // dentro filtra `died_loop`, que é u32::MAX para fontes ainda vivas.
    let end = player
        .creep_index
        .partition_point(|c: &CreepEntry| c.born_loop <= until_loop);
    for src in &player.creep_index[..end] {
        if src.died_loop <= until_loop {
            continue;
        }
        let center = to_screen(rect, src.x as f32, src.y as f32, bounds);
        painter.circle_filled(center, radius_px, fill);
    }
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
    /// Coordenadas em tile units, mas em `f32` pra acomodar a posição
    /// interpolada entre amostras esparsas de `unit_positions`.
    /// Estruturas usam `pos_x/pos_y as f32` direto do evento de
    /// nascimento (sempre integer aligned).
    x: f32,
    y: f32,
    category: EntityCategory,
    /// `true` para prédios de main-base (CC, OrbitalCommand,
    /// PlanetaryFortress, Nexus, Hatchery, Lair, Hive). Usado para
    /// desenhar esses prédios em tamanho maior no minimapa, já que
    /// servem de âncora visual pras bases dos jogadores.
    is_base: bool,
    /// Lado do quadrado no minimapa (px), já com a escala por supply
    /// aplicada (ver `unit_scale_for_supply`). Pré-computado em
    /// `alive_entities_at` pra evitar lookup na tabela de balance data
    /// a cada frame.
    side: f32,
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
fn alive_entities_at(p: &PlayerTimeline, until_loop: u32, base_build: u32) -> Vec<LiveEntity> {
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
fn structure_attention_at(p: &PlayerTimeline, until_loop: u32) -> (u32, u32) {
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
