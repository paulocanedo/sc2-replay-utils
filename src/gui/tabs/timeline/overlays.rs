//! Overlays toggleáveis do minimapa: creep (camada de terreno) e
//! heatmap de câmera (tempo gasto olhando cada região).

use egui::{pos2, vec2, Color32, Rect};

use crate::replay::CreepEntry;
use crate::replay::PlayerTimeline;
use crate::replay_state::PlayableBounds;

use super::minimap::to_screen;
use super::{CAMERA_HEIGHT_TILES, CAMERA_WIDTH_TILES};

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

/// Resolução do grid de heatmap (células por eixo). Valores maiores
/// dão mais detalhe mas custam mais memória e iteração na renderização.
const HEATMAP_GRID: usize = 64;

/// Desenha a camada de creep do jogador como círculos translúcidos
/// centrados em cada fonte (hatchery/lair/hive/tumor) viva no instante
/// `until_loop`. Usa o índice `creep_index` (pré-computado em
/// `finalize.rs`) e binary-search para parar cedo no range de "já
/// nasceu", cabendo no orçamento por-frame mesmo em late-game.
pub(super) fn draw_creep(
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

/// Renderiza um heatmap de tempo de câmera do jogador sobre o minimapa.
///
/// Para cada posição de câmera, preenche a área inteira do viewport
/// (~24×14 tiles) no grid, ponderada pela duração (game loops) que a
/// câmera permaneceu naquela posição. Isso produz um mapa de calor que
/// realça mais onde o jogador olhou por mais tempo e com uma área de
/// influência proporcional ao campo de visão real do jogo.
pub(super) fn draw_heatmap(
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
