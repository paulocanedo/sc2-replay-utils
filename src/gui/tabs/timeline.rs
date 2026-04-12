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
    transport_bar(ui, tl, current_second, game_loop, max_s);
    ui.separator();
    minimap(ui, loaded, game_loop);
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
) {
    ui.horizontal(|ui| {
        ui.monospace(format!(
            "{} / {}",
            fmt_time(game_loop, tl.loops_per_second),
            fmt_time(tl.game_loops, tl.loops_per_second),
        ));
        ui.add_space(12.0);
        // Toda a largura restante vira rail do slider. O `-12` é folga
        // pra evitar que o thumb encoste na borda direita.
        let slider_w = (ui.available_width() - 12.0).max(160.0);
        ui.spacing_mut().slider_width = slider_w;
        ui.add(
            Slider::new(current_second, 0..=max_s)
                .integer()
                .show_value(false),
        );
    });
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
                    // SC2 cap-in-game é 200 para todas as raças. O
                    // `food_made` cru pode ultrapassar (a Blizzard
                    // soma a contribuição de todos os depots/CCs sem
                    // clampar) — alinhamos com o que o jogo mostra.
                    let supply_cap = s.supply_made.min(200);
                    ui.separator();
                    ui.monospace(format!("Supply {}/{}", s.supply_used, supply_cap));
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
    // Ocupa todo o espaço restante da aba e preserva o aspect ratio
    // da imagem do minimap (que representa a playable area). Letterbox
    // no centro do canvas disponível.
    let avail = ui.available_size();
    let aspect = map_aspect(loaded);
    let rect_size = fit_aspect(avail, aspect);

    // Bounds da área onde unidades aparecem. Sem playable_bounds (só em
    // replays vazios) caímos em (0..255) — visual fica meio descalibrado
    // mas o app não trava.
    let bounds = loaded.playable_bounds.unwrap_or(PlayableBounds {
        min_x: 0,
        max_x: 255,
        min_y: 0,
        max_y: 255,
    });

    ui.vertical_centered(|ui| {
        let (rect, _resp) = ui.allocate_exact_size(rect_size, Sense::hover());
        let painter = ui.painter_at(rect);

        // Fundo do mapa: se temos a imagem rasterizada do mapa atual,
        // upload pra GPU (cacheado por nome do replay) e pinta como
        // background. Caso contrário fica o cinza placeholder.
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

        // Desenha cada jogador. Estruturas vão por cima das unidades
        // dentro do mesmo jogador (laço em duas passadas) pra ficarem
        // visíveis em aglomerados.
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
