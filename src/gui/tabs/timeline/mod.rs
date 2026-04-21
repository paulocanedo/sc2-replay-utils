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
//
// Organização:
//   - `transport`  — slider + step buttons + hold-to-repeat.
//   - `side_panel` — painel lateral de stats por jogador.
//   - `minimap`    — orquestração + primitivas de render + coordenadas.
//   - `overlays`   — creep + heatmap de câmera (modos alternativos).
//   - `entities`   — `alive_entities_at`, `structure_attention_at`.

mod entities;
mod minimap;
mod overlays;
mod side_panel;
mod transport;

use egui::{Color32, TextStyle, Ui};

use crate::config::AppConfig;
use crate::locale::t;
use crate::replay_state::LoadedReplay;
use crate::tokens::SPACE_XS;
use crate::widgets::toggle_chip_bool;

/// Tamanho do viewport da câmera do SC2 em tiles (zoom padrão).
/// Compartilhado entre `minimap` (camera rect) e `overlays` (heatmap
/// footprint), por isso vive aqui no root do módulo.
pub(super) const CAMERA_WIDTH_TILES: f32 = 24.0;
pub(super) const CAMERA_HEIGHT_TILES: f32 = 14.0;

/// Número de caracteres monospace que cabem no painel lateral. A
/// largura real é derivada do glifo "M" da fonte monospace atual, então
/// escala com o `font_size_points` do usuário (HiDPI-aware).
/// Dimensionado para comportar a barra de supply com overlay "200/200"
/// mais ícones de recursos + barras inline de capacidade. 18 caracteres
/// dá ~216px com a fonte padrão — suficiente sem ficar largo demais.
const SIDE_PANEL_CHARS: f32 = 18.0;

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

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    current_loop: &mut u32,
    playing: &mut bool,
    playback_speed: &mut u8,
    show_heatmap: &mut bool,
    show_creep: &mut bool,
    show_map: &mut bool,
) {
    let lang = config.language;
    let tl = &loaded.timeline;
    let max_loop = tl.game_loops.max(1);
    if *current_loop > max_loop {
        *current_loop = max_loop;
    }
    // Avança o tempo antes de renderizar o frame; também pausa
    // automaticamente ao atingir o fim. Quando `*playing` é false,
    // `advance_playback` é no-op (exceto por resetar o acumulador
    // fracionário).
    let dt = ui.input(|i| i.unstable_dt);
    let ctx = ui.ctx().clone();
    transport::advance_playback(tl, current_loop, max_loop, playing, *playback_speed, dt, &ctx);
    if *playing {
        ctx.request_repaint();
    }
    let game_loop = *current_loop;
    let side_w = side_panel_width(ui);

    // Layout em painéis (estilo egui_demo `panels.rs`):
    // - Top: toggles de overlays (heatmap/creep/map)
    // - Bottom: indicador de tempo + botões de step + slider de scrubbing
    // - Left: stats do P1
    // - Right: stats do P2
    // - Central: minimapa
    egui::Panel::top("timeline_top")
        .resizable(false)
        .show_inside(ui, |ui| {
            ui.add_space(SPACE_XS);
            ui.horizontal(|ui| {
                toggle_chip_bool(ui, t("timeline.toggle.heatmap", lang), show_heatmap, None);
                toggle_chip_bool(ui, t("timeline.toggle.creep", lang), show_creep, None);
                toggle_chip_bool(ui, t("timeline.toggle.map", lang), show_map, None);
            });
            ui.add_space(SPACE_XS);
        });

    egui::Panel::bottom("timeline_bottom")
        .resizable(false)
        .show_inside(ui, |ui| {
            ui.add_space(2.0);
            transport::transport_slider(ui, tl, current_loop, max_loop, playing, playback_speed);
            ui.add_space(2.0);
        });

    egui::Panel::left("timeline_p1")
        .resizable(false)
        .exact_size(side_w)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("timeline_p1_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    side_panel::player_side_panel(ui, loaded, 0, game_loop, config);
                });
        });

    egui::Panel::right("timeline_p2")
        .resizable(false)
        .exact_size(side_w)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("timeline_p2_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    side_panel::player_side_panel(ui, loaded, 1, game_loop, config);
                });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let aspect = minimap::map_aspect(loaded);
        let map_size = minimap::fit_aspect(ui.available_size(), aspect);
        minimap::minimap_with_size(
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
