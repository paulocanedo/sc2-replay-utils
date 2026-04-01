use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;

use crate::army_value_image::{P1_COLOR, P2_COLOR};
use crate::build_order::format_time;
use crate::supply_block::SupplyBlockEntry;

// ── Fonte embutida ────────────────────────────────────────────────────────────

static FONT_BYTES: &[u8] = include_bytes!("fonts/Ubuntu-L.ttf");

// ── Constantes de layout ──────────────────────────────────────────────────────

const FONT_SIZE: f32 = 26.0;
const TITLE_FONT_SIZE: f32 = 30.0;

const IMG_WIDTH: u32 = 1400;
const LEFT_MARGIN: u32 = 60;
const RIGHT_MARGIN: u32 = 40;

const TITLE_TOP: u32 = 20;
const TITLE_H: u32 = 70;
const BAR_GAP: u32 = 32; // inclui espaço para rótulos acima da barra (≥ FONT_SIZE + folga)
const BAR_H: u32 = 44;
const AXIS_BELOW: u32 = 65;

// ── Paleta ────────────────────────────────────────────────────────────────────

const BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const BAR_NORMAL: Rgba<u8> = Rgba([50, 70, 110, 255]);
const BAR_BLOCKED: Rgba<u8> = Rgba([200, 40, 40, 255]);
const TICK_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);
const TIME_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);
const SUPPLY_LABEL_COL: Rgba<u8> = Rgba([255, 255, 255, 255]);

// ── API pública ───────────────────────────────────────────────────────────────

/// Renderiza os supply blocks de um jogador como barra de linha do tempo em memória.
///
/// A barra inteira representa [0, game_loops]. Trechos em vermelho são supply blocks.
/// O título inclui o tempo total bloqueado.
pub fn render_supply_block(
    player_number: usize,
    name: &str,
    race: &str,
    entries: &[SupplyBlockEntry],
    game_loops: u32,
    loops_per_second: f64,
) -> Result<RgbaImage, String> {
    if game_loops == 0 {
        return Err("game_loops é 0, impossível renderizar".to_string());
    }

    let font = FontRef::try_from_slice(FONT_BYTES)
        .map_err(|e| format!("fonte inválida: {:?}", e))?;
    let scale = PxScale::from(FONT_SIZE);
    let title_scale = PxScale::from(TITLE_FONT_SIZE);

    let total_blocked: u32 = entries
        .iter()
        .map(|e| e.end_loop.saturating_sub(e.start_loop))
        .sum();

    let img_height = TITLE_H + BAR_GAP + BAR_H + AXIS_BELOW;
    let bar_y = TITLE_H + BAR_GAP;
    let axis_y = bar_y + BAR_H;

    let mut img = RgbaImage::from_pixel(IMG_WIDTH, img_height, BG);

    let axis_width = (IMG_WIDTH - LEFT_MARGIN - RIGHT_MARGIN) as f32;

    // ── Título ────────────────────────────────────────────────────────────────
    let player_label = if name.is_empty() {
        format!("Player {}", player_number)
    } else {
        format!("Player {} — {} ({})", player_number, name, race)
    };
    let title = format!(
        "{} | Supply Block total: {}",
        player_label,
        format_time(total_blocked, loops_per_second)
    );
    let title_color = if player_number == 1 { P1_COLOR } else { P2_COLOR };
    draw_text_mut(
        &mut img,
        title_color,
        LEFT_MARGIN as i32,
        TITLE_TOP as i32,
        title_scale,
        &font,
        &title,
    );

    // ── Barra de fundo (tempo normal) ─────────────────────────────────────────
    draw_filled_rect_mut(
        &mut img,
        Rect::at(LEFT_MARGIN as i32, bar_y as i32)
            .of_size(IMG_WIDTH - LEFT_MARGIN - RIGHT_MARGIN, BAR_H),
        BAR_NORMAL,
    );

    // ── Trechos vermelhos (supply block) + rótulo de supply ──────────────────
    let label_scale = PxScale::from(FONT_SIZE);
    let char_h = FONT_SIZE.ceil() as u32;
    let label_y = (bar_y + (BAR_H.saturating_sub(char_h)) / 2) as i32;

    for entry in entries {
        let x_start = LEFT_MARGIN as f32
            + (entry.start_loop as f32 / game_loops as f32) * axis_width;
        let x_end = LEFT_MARGIN as f32
            + (entry.end_loop as f32 / game_loops as f32) * axis_width;
        let w = ((x_end - x_start).ceil() as u32).max(2);
        draw_filled_rect_mut(
            &mut img,
            Rect::at(x_start as i32, bar_y as i32).of_size(w, BAR_H),
            BAR_BLOCKED,
        );

        // Rótulo do supply: dentro do trecho se couber, senão abaixo da barra
        let label = entry.supply.to_string();
        let label_w = text_size(label_scale, &font, &label).0;
        if label_w + 4 <= w {
            let label_x = x_start as i32 + (w as i32 - label_w as i32) / 2;
            draw_text_mut(&mut img, SUPPLY_LABEL_COL, label_x, label_y, label_scale, &font, &label);
        } else {
            // Não coube dentro: exibe acima da barra, centralizado no meio do bloco
            let mid_x = x_start as i32 + (w as i32 / 2) - (label_w as i32 / 2);
            let above_y = bar_y as i32 - char_h as i32 - 2;
            draw_text_mut(&mut img, TICK_COL, mid_x, above_y, label_scale, &font, &label);
        }
    }

    // ── Marcas e rótulos de tempo abaixo da barra ─────────────────────────────
    let time_w = text_size(scale, &font, "00:00").0 as i32;
    let mut t = 0u32;
    loop {
        let gl = (t as f64 * loops_per_second).round() as u32;
        if gl > game_loops { break; }
        let x = LEFT_MARGIN as f32 + (gl as f32 / game_loops as f32) * axis_width;
        draw_line_segment_mut(
            &mut img,
            (x, axis_y as f32),
            (x, (axis_y + 6) as f32),
            TICK_COL,
        );
        let time_str = format_time(gl, loops_per_second);
        draw_text_mut(
            &mut img,
            TIME_COL,
            x as i32 - time_w / 2,
            (axis_y + 10) as i32,
            scale,
            &font,
            &time_str,
        );
        t += 60;
    }

    Ok(img)
}

/// Salva os supply blocks de um jogador como barra de linha do tempo PNG.
pub fn write_supply_block_png(
    player_number: usize,
    name: &str,
    race: &str,
    entries: &[SupplyBlockEntry],
    game_loops: u32,
    loops_per_second: f64,
    out_path: &Path,
) -> Result<(), String> {
    let img = render_supply_block(player_number, name, race, entries, game_loops, loops_per_second)?;
    img.save(out_path).map_err(|e| format!("erro ao salvar PNG: {}", e))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

