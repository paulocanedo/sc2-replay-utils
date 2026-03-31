use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_line_segment_mut, draw_text_mut, text_size};

use crate::build_order::{format_time, BuildOrderEntry};

// ── Fonte embutida ────────────────────────────────────────────────────────────

static FONT_BYTES: &[u8] = include_bytes!("fonts/Ubuntu-L.ttf");

// ── Constantes de layout ──────────────────────────────────────────────────────

const FONT_SIZE: f32 = 26.0;
const TITLE_FONT_SIZE: f32 = 30.0;

const IMG_WIDTH: u32 = 1400;
const LEFT_MARGIN: u32 = 60;
const RIGHT_MARGIN: u32 = 40;

// Espaços verticais
const TITLE_TOP: u32 = 20;
const TITLE_H: u32 = 70;
const LABEL_GAP: u32 = 12; // espaço entre base do rótulo e topo do tick
const TICK_H: u32 = 20;
const AXIS_BELOW: u32 = 80; // espaço abaixo do eixo para rótulos de tempo

// ── Paleta ────────────────────────────────────────────────────────────────────

const BG: Rgba<u8> = Rgba([18, 18, 28, 255]);
const AXIS_COL: Rgba<u8> = Rgba([100, 120, 180, 255]);
const TICK_COL: Rgba<u8> = Rgba([180, 200, 255, 255]);
const LABEL_COL: Rgba<u8> = Rgba([240, 240, 255, 255]);
const TIME_COL: Rgba<u8> = Rgba([140, 160, 200, 255]);
const TITLE_COL: Rgba<u8> = Rgba([220, 180, 60, 255]);

// ── API pública ───────────────────────────────────────────────────────────────

/// Renderiza a build order de um jogador como imagem PNG de linha do tempo.
pub fn write_build_order_png(
    player_number: usize,
    name: &str,
    race: &str,
    entries: &[BuildOrderEntry],
    out_path: &Path,
) -> Result<(), String> {
    if entries.is_empty() {
        return Err("sem entradas para renderizar".to_string());
    }

    let font = FontRef::try_from_slice(FONT_BYTES)
        .map_err(|e| format!("fonte inválida: {:?}", e))?;
    let scale = PxScale::from(FONT_SIZE);
    let title_scale = PxScale::from(TITLE_FONT_SIZE);

    // Rótulo de cada evento: "Ação x3 (supply)"
    let labels: Vec<String> = entries
        .iter()
        .map(|e| {
            let action = if e.count > 1 {
                format!("{} x{}", e.action, e.count)
            } else {
                e.action.clone()
            };
            format!("{} ({})", action, e.supply)
        })
        .collect();

    // Largura máxima em pixels de um rótulo (vira altura ao ser rotacionado)
    let max_label_px = labels
        .iter()
        .map(|l| text_size(scale, &font, l).0)
        .max()
        .unwrap_or(100);

    let max_game_loop = entries.iter().map(|e| e.game_loop).max().unwrap_or(1);

    // ── Dimensões da imagem ───────────────────────────────────────────────────
    let char_h = FONT_SIZE.ceil() as u32 + 4; // altura do buffer de texto
    let label_area_h = max_label_px + LABEL_GAP + TICK_H;
    let img_height = TITLE_H + label_area_h + AXIS_BELOW;
    let axis_y = TITLE_H + label_area_h;

    let mut img = RgbaImage::from_pixel(IMG_WIDTH, img_height, BG);

    // ── Título ────────────────────────────────────────────────────────────────
    let title = if name.is_empty() {
        format!("Player {}", player_number)
    } else {
        format!("Player {} — {} ({})", player_number, name, race)
    };
    draw_text_mut(
        &mut img,
        TITLE_COL,
        LEFT_MARGIN as i32,
        TITLE_TOP as i32,
        title_scale,
        &font,
        &title,
    );

    // ── Linha do eixo ─────────────────────────────────────────────────────────
    let axis_width = (IMG_WIDTH - LEFT_MARGIN - RIGHT_MARGIN) as f32;
    draw_line_segment_mut(
        &mut img,
        (LEFT_MARGIN as f32, axis_y as f32),
        ((IMG_WIDTH - RIGHT_MARGIN) as f32, axis_y as f32),
        AXIS_COL,
    );

    // ── Eventos ───────────────────────────────────────────────────────────────
    for (label, entry) in labels.iter().zip(entries.iter()) {
        let x = LEFT_MARGIN as f32
            + (entry.game_loop as f32 / max_game_loop as f32) * axis_width;
        let xi = x as i32;

        // Tick vertical acima do eixo
        draw_line_segment_mut(
            &mut img,
            (x, (axis_y - TICK_H) as f32),
            (x, axis_y as f32),
            TICK_COL,
        );

        // Rende rótulo em buffer horizontal, depois rotaciona 90° CW (texto vai para cima)
        let label_w = text_size(scale, &font, label).0.max(1);
        let mut label_buf = RgbaImage::from_pixel(label_w, char_h.max(1), BG);
        draw_text_mut(&mut label_buf, LABEL_COL, 0, 0, scale, &font, label);
        let rotated = image::imageops::rotate270(&label_buf);
        // Após rotate90: width = char_h, height = label_w
        let paste_x = xi - (rotated.width() as i32 / 2);
        let paste_y =
            (axis_y - TICK_H - LABEL_GAP) as i32 - rotated.height() as i32;
        blit(&mut img, &rotated, paste_x, paste_y);
    }

    // ── Rótulos de tempo abaixo do eixo ──────────────────────────────────────
    let total_secs = max_game_loop / 16;
    let interval = nice_interval(total_secs);
    let time_w = text_size(scale, &font, "00:00").0 as i32;
    let mut t = 0u32;
    loop {
        let gl = (t * 16).min(max_game_loop);
        let x = LEFT_MARGIN as f32 + (gl as f32 / max_game_loop as f32) * axis_width;
        draw_line_segment_mut(
            &mut img,
            (x, axis_y as f32),
            (x, (axis_y + 5) as f32),
            AXIS_COL,
        );
        let time_str = format_time(gl);
        draw_text_mut(
            &mut img,
            TIME_COL,
            x as i32 - time_w / 2,
            (axis_y + 8) as i32,
            scale,
            &font,
            &time_str,
        );
        if t * 16 >= max_game_loop {
            break;
        }
        t += interval;
    }

    img.save(out_path).map_err(|e| format!("erro ao salvar PNG: {}", e))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Intervalo em segundos entre rótulos de tempo, baseado na duração total.
fn nice_interval(total_secs: u32) -> u32 {
    match total_secs {
        0..=90 => 15,
        91..=300 => 30,
        301..=600 => 60,
        _ => 120,
    }
}

/// Copia pixels de `src` sobre `dst` na posição (x, y), respeitando bordas.
fn blit(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    for (ox, oy, pixel) in src.enumerate_pixels() {
        let bx = x + ox as i32;
        let by = y + oy as i32;
        if bx >= 0 && by >= 0 && (bx as u32) < dst.width() && (by as u32) < dst.height() {
            dst.put_pixel(bx as u32, by as u32, *pixel);
        }
    }
}
