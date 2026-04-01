use std::path::Path;

use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_line_segment_mut;

use crate::army_value::PlayerArmyValue;
use crate::army_value_image::render_army_value;
use crate::build_order::PlayerBuildOrder;
use crate::build_order_image::render_build_order;
use crate::supply_block::SupplyBlockEntry;
use crate::supply_block_image::render_supply_block;

const BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const SEPARATOR_COL: Rgba<u8> = Rgba([200, 200, 200, 255]);
/// Padding branco acima e abaixo de cada linha separadora
const SEP_PAD: u32 = 8;
/// Espessura da linha separadora
const SEP_LINE: u32 = 2;
/// Altura total do bloco separador entre seções
const SEP_H: u32 = SEP_PAD + SEP_LINE + SEP_PAD;

/// Gera um PNG combinando army value, build orders e supply blocks dos dois jogadores.
///
/// Layout lado a lado (otimizado para telas 16:9):
///   Coluna esquerda: Army Value (ambos os jogadores)
///   Coluna direita:  P1 Build Order + P1 Supply Block
///                    ── separador ──
///                    P2 Build Order + P2 Supply Block
pub fn write_all_png(
    army_p1: &PlayerArmyValue,
    army_p2: &PlayerArmyValue,
    map_name: &str,
    game_loops: u32,
    loops_per_second: f64,
    bo_p1: &PlayerBuildOrder,
    bo_p2: &PlayerBuildOrder,
    sb_p1: &[SupplyBlockEntry],
    sb_p2: &[SupplyBlockEntry],
    out_path: &Path,
) -> Result<(), String> {
    let army_img = render_army_value(army_p1, army_p2, map_name, game_loops, loops_per_second)?;

    let bo1_img = render_build_order(1, &bo_p1.name, &bo_p1.race, bo_p1.mmr, &bo_p1.entries, loops_per_second)
        .unwrap_or_else(|_| RgbaImage::from_pixel(army_img.width(), 0, BG));

    let sb1_img = render_supply_block(1, &army_p1.name, &army_p1.race, army_p1.mmr, sb_p1, game_loops, loops_per_second)
        .unwrap_or_else(|_| RgbaImage::from_pixel(army_img.width(), 0, BG));

    let bo2_img = render_build_order(2, &bo_p2.name, &bo_p2.race, bo_p2.mmr, &bo_p2.entries, loops_per_second)
        .unwrap_or_else(|_| RgbaImage::from_pixel(army_img.width(), 0, BG));

    let sb2_img = render_supply_block(2, &army_p2.name, &army_p2.race, army_p2.mmr, sb_p2, game_loops, loops_per_second)
        .unwrap_or_else(|_| RgbaImage::from_pixel(army_img.width(), 0, BG));

    let left_w = army_img.width();
    let right_w = bo1_img.width();

    let right_h = bo1_img.height() + sb1_img.height()
        + SEP_H
        + bo2_img.height() + sb2_img.height();

    let total_w = left_w + right_w;
    let total_h = army_img.height().max(right_h);

    let mut canvas = RgbaImage::from_pixel(total_w, total_h, BG);

    // Coluna esquerda: army value
    blit_at(&mut canvas, &army_img, 0, 0);

    // Separador vertical entre colunas
    draw_vertical_separator(&mut canvas, left_w, total_h);

    // Coluna direita: P1 BO + P1 SB + separador + P2 BO + P2 SB
    let mut y: u32 = 0;

    blit_at(&mut canvas, &bo1_img, left_w, y);
    y += bo1_img.height();

    blit_at(&mut canvas, &sb1_img, left_w, y);
    y += sb1_img.height();

    draw_separator(&mut canvas, y, left_w, total_w);
    y += SEP_H;

    blit_at(&mut canvas, &bo2_img, left_w, y);
    y += bo2_img.height();

    blit_at(&mut canvas, &sb2_img, left_w, y);
    let _ = y;

    canvas.save(out_path).map_err(|e| format!("erro ao salvar PNG: {}", e))
}

fn blit_at(dst: &mut RgbaImage, src: &RgbaImage, x_offset: u32, y_offset: u32) {
    if src.height() == 0 {
        return;
    }
    for (x, y, pixel) in src.enumerate_pixels() {
        let dx = x_offset + x;
        let dy = y_offset + y;
        if dx < dst.width() && dy < dst.height() {
            dst.put_pixel(dx, dy, *pixel);
        }
    }
}

fn draw_separator(img: &mut RgbaImage, y_offset: u32, x_start: u32, width: u32) {
    let line_y = y_offset + SEP_PAD;
    for dy in 0..SEP_LINE {
        draw_line_segment_mut(
            img,
            (x_start as f32, (line_y + dy) as f32),
            (width as f32, (line_y + dy) as f32),
            SEPARATOR_COL,
        );
    }
}

fn draw_vertical_separator(img: &mut RgbaImage, x_offset: u32, height: u32) {
    for dx in 0..SEP_LINE {
        draw_line_segment_mut(
            img,
            ((x_offset + dx) as f32, 0.0),
            ((x_offset + dx) as f32, height as f32),
            SEPARATOR_COL,
        );
    }
}
