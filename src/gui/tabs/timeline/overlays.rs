//! Overlays toggleáveis do minimapa: creep (camada de terreno) e
//! heatmap de câmera (tempo gasto olhando cada região).

use egui::{pos2, vec2, Color32, Rect};

use crate::balance_data;
use crate::replay::CreepEntry;
use crate::replay::PlayerTimeline;
use crate::replay_state::PlayableBounds;

use super::entities::alive_entities_at;
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

/// Resolução do grid do overlay de FOG (células por eixo). Mais alto
/// que o heatmap porque a borda da visão (interface entre visível e
/// nevoento) é o elemento visual dominante e granularidade baixa fica
/// "blocada" demais. 96 = ~9k células — barato dado o early-exit por
/// bounding box de cada entidade.
const FOG_GRID: usize = 96;

/// Alpha (0–255) do overlay escuro nas áreas sem visão. ~140 deixa o
/// mapa base e creep sob a névoa visíveis o bastante pra comparação,
/// mas escuro o suficiente pra leitura imediata "isso aqui está fora
/// da visão".
const FOG_ALPHA: u8 = 140;

/// Sight radius (em tiles) usado quando a balance data não tem entrada
/// para uma entidade — ex.: cooperativo, campanha, replays muito
/// antigos. Valor conservador típico de unidade ground.
const FOG_DEFAULT_SIGHT: f32 = 8.0;

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

/// Overlay de Fog of War da perspectiva de `player`: escurece as células
/// do grid que não estão dentro do `sight_radius` de nenhuma entidade
/// viva do jogador no instante `until_loop`.
///
/// Modelo simplificado — sem high-ground vision, sem detector/cloak.
/// Cada entidade contribui um disco de raio `sight_radius` (tiles do
/// jogo). Células marcam-se "visíveis" se o seu centro cai dentro de
/// pelo menos um disco; o resto recebe um retângulo translúcido escuro.
///
/// Custo: O(N · cells_per_entity), onde `cells_per_entity` é limitada
/// ao bounding box do disco no grid (não ao grid inteiro). Para um
/// late-game com ~150 unidades cada uma cobrindo ~5×5 cells, são ~4k
/// updates — barato comparado ao heatmap (~9k cells × N samples).
pub(super) fn draw_fog(
    painter: &egui::Painter,
    rect: Rect,
    player: &PlayerTimeline,
    until_loop: u32,
    bounds: PlayableBounds,
    base_build: u32,
) {
    let entities = alive_entities_at(player, until_loop, base_build);
    if entities.is_empty() {
        // Sem entidades vivas (pré-spawn ou jogador eliminado): tudo é
        // névoa. Pinta um único retângulo cobrindo o minimap inteiro —
        // mais barato que rasterizar o grid.
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(FOG_ALPHA));
        return;
    }

    let span_x = (bounds.max_x - bounds.min_x).max(1) as f32;
    let span_y = (bounds.max_y - bounds.min_y).max(1) as f32;

    let mut visible = vec![false; FOG_GRID * FOG_GRID];

    for e in &entities {
        let radius = balance_data::sight_radius(&e.entity_type, base_build)
            .unwrap_or(FOG_DEFAULT_SIGHT);
        if radius <= 0.0 {
            continue;
        }
        // Conversão tiles → unidades de grid em cada eixo. Mantemos os
        // dois ratios separados pra que mapas com aspect não-quadrado
        // continuem dando círculos bem cobertos no espaço de mundo.
        let r_gx = radius * (FOG_GRID as f32) / span_x;
        let r_gy = radius * (FOG_GRID as f32) / span_y;
        let center_gx = ((e.x - bounds.min_x as f32) / span_x) * FOG_GRID as f32;
        let center_gy = ((e.y - bounds.min_y as f32) / span_y) * FOG_GRID as f32;

        let gx_min = (center_gx - r_gx).floor().max(0.0) as usize;
        let gx_max = ((center_gx + r_gx).ceil() as i32)
            .clamp(0, FOG_GRID as i32 - 1) as usize;
        let gy_min = (center_gy - r_gy).floor().max(0.0) as usize;
        let gy_max = ((center_gy + r_gy).ceil() as i32)
            .clamp(0, FOG_GRID as i32 - 1) as usize;

        // Test em espaço normalizado de grid: divide o delta pelos
        // raios de cada eixo e usa unit-circle. Equivalente ao teste
        // em coords de mundo, sem precisar reconverter cell→tile.
        for gy in gy_min..=gy_max {
            for gx in gx_min..=gx_max {
                let dx = (gx as f32 + 0.5 - center_gx) / r_gx;
                let dy = (gy as f32 + 0.5 - center_gy) / r_gy;
                if dx * dx + dy * dy <= 1.0 {
                    visible[gy * FOG_GRID + gx] = true;
                }
            }
        }
    }

    let cell_w = rect.width() / FOG_GRID as f32;
    let cell_h = rect.height() / FOG_GRID as f32;
    let fill = Color32::from_black_alpha(FOG_ALPHA);

    for gy in 0..FOG_GRID {
        for gx in 0..FOG_GRID {
            if visible[gy * FOG_GRID + gx] {
                continue;
            }
            // Inverte Y igual ao heatmap: gy=0 = min_y do mundo (base) →
            // base da tela.
            let screen_gy = FOG_GRID - 1 - gy;
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
