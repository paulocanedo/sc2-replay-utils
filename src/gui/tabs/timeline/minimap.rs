//! Render do mini-mapa: orquestração + primitivas stateless de desenho
//! + helpers de coordenada.

use std::collections::HashMap;

use egui::{
    pos2, vec2, Color32, ColorImage, Image, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    TextStyle, TextureOptions, Ui,
};

use crate::colors::player_slot_color_bright;
use crate::locale::{localize, Language};
use crate::map_image::MapImage;
use crate::replay::{EntityCategory, EntityEventKind, ResourceKind, ResourceNode};
use crate::replay_state::{LoadedReplay, PlayableBounds};

use super::entities::{alive_entities_at, LiveEntity};
use super::overlays::{draw_fog, draw_heatmap};
use super::unit_column::{structure_icon, unit_icon};
use super::{CAMERA_HEIGHT_TILES, CAMERA_WIDTH_TILES};

/// Raio de detecção em pixels do minimap pra montar a lista do tooltip
/// de hover. Empírico: ~10 px cobre só a vizinhança imediata do cursor
/// (típico ~1-2 unidades pequenas), sem incluir grupos vizinhos.
const HOVER_TOOLTIP_RADIUS_PX: f32 = 10.0;

/// Janela em game loops durante a qual um marcador de morte/
/// cancelamento permanece visível depois do evento (~1s em Faster,
/// dado que o SC2 roda a 22.4 loops/s). Marcadores são flash — a
/// intenção é chamar atenção no momento sem poluir o minimapa.
const MARKER_DURATION_LOOPS: u32 = 23;

/// Lado do marcador (X/Ø) em pixels.
const MARKER_SIZE: f32 = 8.0;

/// Tamanho mínimo (lado em px) a partir do qual um ícone é sobreposto
/// ao quadrado colorido da entidade. Abaixo disso (workers pequenos, 4
/// px), a imagem vira pixel mush ilegível — só o fill sólido. 6 px já
/// resolve silhuetas razoáveis (unidades ≥2 supply e todas estruturas).
const MIN_ICON_SIZE_PX: f32 = 6.0;

/// Piso de tamanho (px) para estruturas escaladas pelo footprint real.
/// Em mapas 256×256 num minimap pequeno, um Pylon 2x2 cairia para ~3 px
/// e desapareceria atrás das unidades. Mantém pelo menos a leitura
/// "tem prédio aqui".
const MIN_STRUCTURE_PX: f32 = 5.0;

pub(super) fn minimap_with_size(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    game_loop: u32,
    rect_size: egui::Vec2,
    show_heatmap: bool,
    show_creep: bool,
    show_map: bool,
    show_fog: bool,
    fog_player: usize,
    hovered_entity: Option<&(usize, String)>,
    lang: Language,
) {
    let bounds = loaded.playable_bounds.unwrap_or(PlayableBounds {
        min_x: 0,
        max_x: 255,
        min_y: 0,
        max_y: 255,
    });

    // Cache O(n) por jogador: as passes de unidades, estruturas,
    // highlight e tooltip leem o mesmo Vec — evita varrer
    // `entity_events` 3-4 vezes por frame.
    let entities_per_player: Vec<Vec<LiveEntity>> = loaded
        .timeline
        .players
        .iter()
        .map(|p| alive_entities_at(p, game_loop, loaded.timeline.base_build))
        .collect();

    ui.vertical_centered(|ui| {
        let (rect, resp) = ui.allocate_exact_size(rect_size, Sense::hover());
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
            // Modo normal: start locations → recursos → unidades →
            // estruturas → câmera. Start locations renderizam por baixo
            // de tudo (são marcadores estáticos, viram "background"
            // quando as unidades começam a poluir o mapa).
            for sl in &loaded.start_locations {
                draw_start_location(&painter, rect, sl.x, sl.y, bounds);
            }

            // CreepTumors entram no pass de estruturas; `show_creep`
            // toggle apenas os ícones de tumor — late-game Zerg pode
            // acumular dezenas, então mantemos a opção de esconder.
            for r in &loaded.timeline.resources {
                draw_resource(&painter, rect, *r, bounds);
            }

            // Pré-computa px/tile uma vez por frame; usado por toda
            // estrutura com footprint conhecido pra escalar do espaço
            // de tile (vindo da balance data) pro espaço de tela.
            let ppt = pixels_per_tile(rect, bounds);

            for (i, entities) in entities_per_player.iter().enumerate() {
                let color = player_slot_color_bright(i);
                for e in entities.iter().filter(|e| e.category != EntityCategory::Structure) {
                    let icon = unit_icon(&e.entity_type);
                    draw_unit(ui, &painter, rect, e.x, e.y, bounds, (e.side, e.side), color, false, icon);
                }
                // Estruturas renderizadas por cima das unidades, com
                // borda branca para destacar. Estruturas com footprint
                // conhecido na balance data (Nexus 5x5, Pylon 2x2,
                // Gateway 3x3, Hatchery 6x5, etc.) são escaladas pelo
                // tamanho real em tiles — bases ficam naturalmente
                // grandes sem precisar de heurística de townhall. As
                // sem footprint (variantes raras) caem no `e.side`
                // legado (9/18 px).
                for e in entities.iter().filter(|e| {
                    e.category == EntityCategory::Structure
                        && (show_creep || e.entity_type != "CreepTumor")
                }) {
                    let icon = structure_icon(&e.entity_type);
                    let size = structure_size_px(e, ppt);
                    draw_unit(ui, &painter, rect, e.x, e.y, bounds, size, color, true, icon);
                }
            }

            // Halo de highlight: quando o cursor está sobre um chip do
            // `unit_column`, realça as instâncias daquele tipo do mesmo
            // jogador. Short-circuit em frames sem hover — overhead zero.
            if let Some((hov_slot, hov_type)) = hovered_entity {
                if let Some(entities) = entities_per_player.get(*hov_slot) {
                    let color = player_slot_color_bright(*hov_slot);
                    for e in entities.iter().filter(|e| &e.entity_type == hov_type) {
                        draw_highlight_halo(&painter, rect, e.x, e.y, bounds, e.side, color);
                    }
                }
            }

            // Fog of War: escurece áreas fora do alcance de visão das
            // entidades do `fog_player`. Roda depois das unidades pra
            // esconder as inimigas em zonas sem visão; antes dos
            // marcadores e da câmera pra que esses indicadores de
            // análise continuem sempre visíveis.
            if show_fog {
                let slot = fog_player.min(loaded.timeline.players.len().saturating_sub(1));
                if let Some(p) = loaded.timeline.players.get(slot) {
                    draw_fog(&painter, rect, p, game_loop, bounds, loaded.timeline.base_build);
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
                    // série e morrem ao plantar a filha) — um X pra
                    // cada morte poluiria o minimapa.
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

        // Tooltip "o que está perto do cursor": só roda quando o mouse
        // está sobre o minimap. A coleta é O(total_alive_units) com
        // early reject por bounding box quadrado — barato. Heatmap mode
        // não tem entidades visíveis, então pular.
        if !show_heatmap {
            if let Some(hover_pos) = resp.hover_pos() {
                let (cx, cy) = to_world(rect, hover_pos, bounds);
                let ppt = pixels_per_tile(rect, bounds);
                let tol_tiles = if ppt > 0.0 {
                    HOVER_TOOLTIP_RADIUS_PX / ppt
                } else {
                    0.0
                };
                let mut entries = nearby_grouped(&entities_per_player, cx, cy, tol_tiles);
                if !show_creep {
                    entries.retain(|(_, ty, _)| ty != "CreepTumor");
                }
                if !entries.is_empty() {
                    resp.on_hover_ui_at_pointer(|ui| {
                        render_nearby_tooltip(ui, &entries, loaded, lang);
                    });
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

/// Inversa de `to_screen` — converte um ponto da tela (cursor do mouse)
/// para coordenadas de tile no espaço do mapa. Usado pra resolver
/// "quem está perto do cursor?" no tooltip de hover do minimap.
fn to_world(rect: Rect, screen: Pos2, b: PlayableBounds) -> (f32, f32) {
    let span_x = (b.max_x - b.min_x).max(1) as f32;
    let span_y = (b.max_y - b.min_y).max(1) as f32;
    let w = rect.width().max(1.0);
    let h = rect.height().max(1.0);
    let nx = ((screen.x - rect.left()) / w).clamp(0.0, 1.0);
    let ny = ((screen.y - rect.top()) / h).clamp(0.0, 1.0);
    let world_x = b.min_x as f32 + nx * span_x;
    let world_y = b.min_y as f32 + (1.0 - ny) * span_y;
    (world_x, world_y)
}

/// Conversão isotrópica px→tile no minimap atual. Como o aspect do
/// rect espelha o aspect dos `playable_bounds` (via `fit_aspect` +
/// `map_aspect`), a média dos dois ratios é praticamente o mesmo
/// número — média elimina ruído de arredondamento.
fn pixels_per_tile(rect: Rect, b: PlayableBounds) -> f32 {
    let span_x = (b.max_x - b.min_x).max(1) as f32;
    let span_y = (b.max_y - b.min_y).max(1) as f32;
    (rect.width() / span_x + rect.height() / span_y) * 0.5
}

/// Cores estilo minimapa do SC2: minerais em azul-cyan claro, minerais
/// rich em amarelo-dourado, gás em verde vivo, rich vespene em violeta.
fn resource_color(kind: ResourceKind) -> Color32 {
    match kind {
        ResourceKind::Mineral => Color32::from_rgb(70, 215, 230),
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
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    x: f32,
    y: f32,
    bounds: PlayableBounds,
    size_px: (f32, f32),
    color: Color32,
    structure: bool,
    icon: Option<egui::ImageSource<'static>>,
) {
    let (w, h) = size_px;
    let center = to_screen(rect, x, y, bounds);
    let half_w = w * 0.5;
    let half_h = h * 0.5;
    let r = Rect::from_min_max(
        pos2(center.x - half_w, center.y - half_h),
        pos2(center.x + half_w, center.y + half_h),
    );
    // Estruturas: ícone PNG renderiza por completo (a arte cheia da
    // Blizzard tem cor própria, então não precisa de fundo) e a
    // identificação do jogador vem só do contorno na cor do slot.
    // Unidades: fill colorido como sempre — workers de 4 px ficam só
    // com o quadrado sólido, e o icon (quando ≥ 6 px) sobrepõe usando
    // o fundo como anel de identificação.
    if !structure {
        painter.rect_filled(r, 0.0, color);
    }
    let min_side = w.min(h);
    if min_side >= MIN_ICON_SIZE_PX {
        if let Some(icon) = icon {
            let inset = (min_side * 0.1).max(0.5);
            let icon_r = Rect::from_min_max(
                pos2(r.min.x + inset, r.min.y + inset),
                pos2(r.max.x - inset, r.max.y - inset),
            );
            Image::new(icon).paint_at(ui, icon_r);
        }
    }
    if structure {
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, color), StrokeKind::Outside);
    }
}

/// Tamanho em px de uma estrutura no minimap.
///
/// Quando a balance data tem o footprint, usa `(w, h) × pixels_per_tile`
/// — bate com o tamanho que o jogo desenha. Aplica um piso de
/// `MIN_STRUCTURE_PX` em cada eixo pra evitar que prédios pequenos
/// (Pylon 2x2, SupplyDepot 2x2) desapareçam em mapas grandes ou
/// minimapas pequenos. Sem footprint (variantes não cobertas pela
/// versão de balance data), cai no `e.side` legado (9/18 px).
fn structure_size_px(e: &LiveEntity, pixels_per_tile: f32) -> (f32, f32) {
    if let Some((fw, fh)) = e.footprint_tiles {
        let w = (fw as f32 * pixels_per_tile).max(MIN_STRUCTURE_PX);
        let h = (fh as f32 * pixels_per_tile).max(MIN_STRUCTURE_PX);
        (w, h)
    } else {
        (e.side, e.side)
    }
}

/// Coleta entidades dentro de `tol_tiles` (raio em tile units) do ponto
/// `(cx, cy)` em coordenadas de mapa, agrupando por (slot, canonical
/// type). Retorna entries ordenadas: slot asc → count desc → tipo
/// alfabético. Ignora distância pra ordenar — agrupar por tipo já é o
/// foco do tooltip.
fn nearby_grouped(
    entities_per_player: &[Vec<LiveEntity>],
    cx: f32,
    cy: f32,
    tol_tiles: f32,
) -> Vec<(usize, String, i32)> {
    let tol_sq = tol_tiles * tol_tiles;
    let mut acc: HashMap<(usize, String), i32> = HashMap::new();
    for (slot, entities) in entities_per_player.iter().enumerate() {
        for e in entities {
            let dx = e.x - cx;
            let dy = e.y - cy;
            if dx * dx + dy * dy <= tol_sq {
                *acc.entry((slot, e.entity_type.clone())).or_insert(0) += 1;
            }
        }
    }
    let mut entries: Vec<(usize, String, i32)> =
        acc.into_iter().map(|((s, t), c)| (s, t, c)).collect();
    entries.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.1.cmp(&b.1))
    });
    entries
}

/// Tooltip de "o que está perto do cursor": header por jogador (cor do
/// slot) e linhas `[ícone] N× Nome`. Reusa `unit_icon` /
/// `structure_icon` do `unit_column` pra consistência visual com a
/// coluna lateral.
fn render_nearby_tooltip(
    ui: &mut Ui,
    entries: &[(usize, String, i32)],
    loaded: &LoadedReplay,
    lang: Language,
) {
    let icon_side = ui.text_style_height(&TextStyle::Body) * 1.4;
    let mut current_slot: Option<usize> = None;
    for (slot, ty, count) in entries {
        if current_slot != Some(*slot) {
            if current_slot.is_some() {
                ui.add_space(2.0);
            }
            let name = loaded
                .timeline
                .players
                .get(*slot)
                .map(|p| p.name.as_str())
                .unwrap_or("");
            ui.label(
                RichText::new(name)
                    .color(player_slot_color_bright(*slot))
                    .strong(),
            );
            current_slot = Some(*slot);
        }
        ui.horizontal(|ui| {
            if let Some(icon) = unit_icon(ty).or_else(|| structure_icon(ty)) {
                ui.add(Image::new(icon).fit_to_exact_size(vec2(icon_side, icon_side)));
            } else {
                ui.add_space(icon_side);
            }
            ui.label(format!("{}× {}", count, localize(ty, lang)));
        });
    }
}

/// Halo do highlight de hover: anel quadrado na cor brilhante do slot,
/// envolvendo a entidade com ~3 px de folga pra ler bem mesmo sobre
/// outras unidades coladas. Usado pelo pass de hover do `unit_column`.
fn draw_highlight_halo(
    painter: &egui::Painter,
    rect: Rect,
    x: f32,
    y: f32,
    bounds: PlayableBounds,
    side: f32,
    color: Color32,
) {
    let center = to_screen(rect, x, y, bounds);
    let half = side * 0.5 + 3.0;
    let r = Rect::from_min_max(
        pos2(center.x - half, center.y - half),
        pos2(center.x + half, center.y + half),
    );
    painter.rect_stroke(r, 0.0, Stroke::new(2.0, color), StrokeKind::Outside);
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

/// Marcador de start location: anel branco translúcido com um ponto
/// central da mesma cor. Renderizado por baixo de todo o resto do
/// minimap pra ficar quase invisível quando há muita coisa acontecendo,
/// mas claro o suficiente em `game_loop=0` (antes de qualquer unidade
/// aparecer) pra deixar visível onde cada lado vai spawnar. Cor neutra
/// (sem associação a slot) porque o mapeamento slot→spawn só fica
/// definido quando a primeira townhall é construída.
fn draw_start_location(painter: &egui::Painter, rect: Rect, x: f32, y: f32, bounds: PlayableBounds) {
    let center = to_screen(rect, x, y, bounds);
    let radius = 7.0;
    let ring = Color32::from_rgba_unmultiplied(255, 255, 255, 110);
    let fill = Color32::from_rgba_unmultiplied(255, 255, 255, 40);
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius, Stroke::new(1.4, ring));
    painter.circle_filled(center, 1.6, ring);
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
