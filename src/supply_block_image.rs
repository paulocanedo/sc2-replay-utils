use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;

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
const BAR_GAP: u32 = 16;
const BAR_H: u32 = 44;
const AXIS_BELOW: u32 = 65;

// ── Paleta ────────────────────────────────────────────────────────────────────

const BG: Rgba<u8> = Rgba([18, 18, 28, 255]);
const BAR_NORMAL: Rgba<u8> = Rgba([50, 70, 110, 255]);
const BAR_BLOCKED: Rgba<u8> = Rgba([200, 40, 40, 255]);
const TICK_COL: Rgba<u8> = Rgba([100, 120, 180, 255]);
const TIME_COL: Rgba<u8> = Rgba([140, 160, 200, 255]);
const TITLE_COL: Rgba<u8> = Rgba([220, 180, 60, 255]);

// ── API pública ───────────────────────────────────────────────────────────────

/// Renderiza os supply blocks de um jogador como barra de linha do tempo PNG.
///
/// A barra inteira representa [0, game_loops]. Trechos em vermelho são supply blocks.
/// O título inclui o tempo total bloqueado.
pub fn write_supply_block_png(
    player_number: usize,
    name: &str,
    race: &str,
    entries: &[SupplyBlockEntry],
    game_loops: u32,
    out_path: &Path,
) -> Result<(), String> {
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
        format_time(total_blocked)
    );
    draw_text_mut(
        &mut img,
        TITLE_COL,
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

    // ── Trechos vermelhos (supply block) ──────────────────────────────────────
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
    }

    // ── Marcas e rótulos de tempo abaixo da barra ─────────────────────────────
    let total_secs = game_loops / 16;
    let interval = nice_interval(total_secs);
    let time_w = text_size(scale, &font, "00:00").0 as i32;
    let mut t = 0u32;
    loop {
        let gl = (t * 16).min(game_loops);
        let x = LEFT_MARGIN as f32 + (gl as f32 / game_loops as f32) * axis_width;
        draw_line_segment_mut(
            &mut img,
            (x, axis_y as f32),
            (x, (axis_y + 6) as f32),
            TICK_COL,
        );
        let time_str = format_time(gl);
        draw_text_mut(
            &mut img,
            TIME_COL,
            x as i32 - time_w / 2,
            (axis_y + 10) as i32,
            scale,
            &font,
            &time_str,
        );
        if t * 16 >= game_loops {
            break;
        }
        t += interval;
    }

    img.save(out_path).map_err(|e| format!("erro ao salvar PNG: {}", e))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn nice_interval(total_secs: u32) -> u32 {
    match total_secs {
        0..=90 => 15,
        91..=300 => 30,
        301..=600 => 60,
        _ => 120,
    }
}
