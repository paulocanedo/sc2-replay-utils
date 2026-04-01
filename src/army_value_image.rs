use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_line_segment_mut, draw_text_mut, text_size};

use crate::army_value::{PlayerArmyValue, UpgradeKind};
use crate::build_order::format_time;
use crate::icons::{self, ICON_SIZE};

// ── Fonte embutida ────────────────────────────────────────────────────────────

static FONT_BYTES: &[u8] = include_bytes!("fonts/Ubuntu-L.ttf");

// ── Constantes de layout ──────────────────────────────────────────────────────

const FONT_SIZE: f32 = 22.0;
const TITLE_FONT_SIZE: f32 = 28.0;
const LABEL_FONT_SIZE: f32 = 18.0;

const IMG_WIDTH: u32 = 1400;
const LEFT_MARGIN: u32 = 70;
const RIGHT_MARGIN: u32 = 40;

const TITLE_TOP: u32 = 16;
const TITLE_H: u32 = 60;

/// Altura da área do gráfico de army value
const CHART_H: u32 = 340;
/// Altura da faixa de upgrades/pesquisas — usada tanto acima do gráfico (P2) quanto abaixo do eixo (P1)
const UPGRADE_LABEL_H: u32 = 160;
/// Espaço para rótulos de tempo na base
const TIME_AREA: u32 = 44;

// ── Paleta ────────────────────────────────────────────────────────────────────

const BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const AXIS_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);
const TIME_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);
const TITLE_COL: Rgba<u8> = Rgba([30, 30, 30, 255]);
const GRID_COL: Rgba<u8> = Rgba([228, 228, 228, 255]);
const Y_LABEL_COL: Rgba<u8> = Rgba([110, 110, 110, 255]);

/// Cor fixa da linha de army value do P1 (azul)
const P1_LINE: Rgba<u8> = Rgba([80, 150, 255, 255]);
/// Cor fixa da linha de army value do P2 (laranja)
const P2_LINE: Rgba<u8> = Rgba([255, 160, 60, 255]);

// ── API pública ───────────────────────────────────────────────────────────────

/// Renderiza o gráfico de valor de exército de ambos os jogadores em um único PNG.
///
/// Layout vertical:
///   TITLE_H        — título
///   UPGRADE_LABEL_H — upgrades/pesquisas do P2 (acima do gráfico, descendo)
///   CHART_H        — curvas de army value
///   axis_y         — eixo de tempo
///   UPGRADE_LABEL_H — upgrades/pesquisas do P1 (abaixo do eixo, descendo)
///   TIME_AREA      — rótulos MM:SS
pub fn write_army_value_png(
    p1: &PlayerArmyValue,
    p2: &PlayerArmyValue,
    map_name: &str,
    game_loops: u32,
    out_path: &Path,
) -> Result<(), String> {
    let font = FontRef::try_from_slice(FONT_BYTES)
        .map_err(|e| format!("fonte inválida: {:?}", e))?;
    let scale = PxScale::from(FONT_SIZE);
    let title_scale = PxScale::from(TITLE_FONT_SIZE);
    let label_scale = PxScale::from(LABEL_FONT_SIZE);

    // ── Dimensões ─────────────────────────────────────────────────────────────
    // P2 upgrades ficam na faixa [TITLE_H … TITLE_H + UPGRADE_LABEL_H)
    // P1 upgrades ficam na faixa [axis_y … axis_y + UPGRADE_LABEL_H)
    let chart_top = TITLE_H + UPGRADE_LABEL_H; // gráfico começa após a faixa do P2
    let axis_y = chart_top + CHART_H;           // linha horizontal do eixo de tempo
    let p2_label_top = TITLE_H;                 // início da faixa de upgrades do P2
    let p1_label_top = axis_y + 6;              // início da faixa de upgrades do P1
    let time_label_y = axis_y + UPGRADE_LABEL_H + 6; // rótulos de tempo ao final
    let img_height = TITLE_H + UPGRADE_LABEL_H + CHART_H + UPGRADE_LABEL_H + TIME_AREA;

    let axis_width = (IMG_WIDTH - LEFT_MARGIN - RIGHT_MARGIN) as f32;

    let mut img = RgbaImage::from_pixel(IMG_WIDTH, img_height, BG);

    // ── Escala X e Y ─────────────────────────────────────────────────────────
    let max_loop = game_loops.max(1);

    let max_army = p1
        .snapshots
        .iter()
        .chain(p2.snapshots.iter())
        .map(|s| s.army_total)
        .max()
        .unwrap_or(1)
        .max(1);

    // Arredonda max_army para o próximo múltiplo de 1000
    let max_army_ceil = ((max_army + 999) / 1000) * 1000;

    // ── Gridlines horizontais a cada 1000 unidades ───────────────────────────
    let mut v = 1000i32;
    while v <= max_army_ceil {
        let py = army_y(v, max_army_ceil, axis_y, CHART_H);
        draw_line_segment_mut(
            &mut img,
            (LEFT_MARGIN as f32, py),
            ((IMG_WIDTH - RIGHT_MARGIN) as f32, py),
            GRID_COL,
        );
        let label = format!("{}", v);
        draw_text_mut(
            &mut img,
            Y_LABEL_COL,
            2,
            (py as i32) - 9,
            PxScale::from(16.0),
            &font,
            &label,
        );
        v += 1000;
    }

    // ── Linhas verticais de upgrade + rótulos ────────────────────────────────
    // Desenhadas ANTES das curvas para ficarem abaixo delas.
    // Cada upgrade gera uma linha vertical que corta todo o gráfico (chart_top → label_bottom).
    draw_upgrade_verticals(
        &mut img, &font, label_scale,
        &p1.upgrade_events, &p1.race,
        chart_top, axis_y, p1_label_top,
        axis_width, max_loop,
        upgrade_vline_color_p1,
        upgrade_label_color_p1,
    );
    draw_upgrade_verticals(
        &mut img, &font, label_scale,
        &p2.upgrade_events, &p2.race,
        chart_top, axis_y, p2_label_top,
        axis_width, max_loop,
        upgrade_vline_color_p2,
        upgrade_label_color_p2,
    );

    // ── Curvas de army value (desenhadas sobre as linhas verticais) ───────────
    draw_army_curve(&mut img, p1, P1_LINE, axis_y, chart_top, axis_width, max_loop, max_army_ceil);
    draw_army_curve(&mut img, p2, P2_LINE, axis_y, chart_top, axis_width, max_loop, max_army_ceil);

    // ── Eixo horizontal de tempo ──────────────────────────────────────────────
    draw_line_segment_mut(
        &mut img,
        (LEFT_MARGIN as f32, axis_y as f32),
        ((IMG_WIDTH - RIGHT_MARGIN) as f32, axis_y as f32),
        AXIS_COL,
    );

    // ── Título ────────────────────────────────────────────────────────────────
    let p1_label = player_label(p1);
    let p2_label = player_label(p2);
    let title = format!("{} — {} vs {}", map_name, p1_label, p2_label);
    draw_text_mut(&mut img, TITLE_COL, LEFT_MARGIN as i32, TITLE_TOP as i32, title_scale, &font, &title);

    // Legendas coloridas (P1 e P2) à direita do título
    let title_w = text_size(title_scale, &font, &title).0 as i32;
    let legend_y = TITLE_TOP as i32;
    let sep = 18i32;
    let legend_x = LEFT_MARGIN as i32 + title_w + sep;
    draw_text_mut(&mut img, P1_LINE, legend_x, legend_y, scale, &font, &format!("■ {}", p1_label));
    let p1_w = text_size(scale, &font, &format!("■ {}", p1_label)).0 as i32;
    draw_text_mut(&mut img, P2_LINE, legend_x + p1_w + sep, legend_y, scale, &font, &format!("■ {}", p2_label));

    // ── Rótulos de tempo ──────────────────────────────────────────────────────
    let total_secs = max_loop / 16;
    let interval = nice_time_interval(total_secs);
    let time_w = text_size(scale, &font, "00:00").0 as i32;
    // Começa pelo primeiro intervalo — t=0 é omitido (irrelevante)
    let mut t = interval;
    loop {
        let gl = (t * 16).min(max_loop);
        let x = LEFT_MARGIN as f32 + (gl as f32 / max_loop as f32) * axis_width;

        draw_line_segment_mut(&mut img, (x, axis_y as f32), (x, (axis_y + 6) as f32), AXIS_COL);

        let time_str = format_time(gl);
        draw_text_mut(
            &mut img,
            TIME_COL,
            x as i32 - time_w / 2,
            time_label_y as i32,
            scale,
            &font,
            &time_str,
        );
        if gl >= max_loop { break; }
        t += interval;
    }

    img.save(out_path).map_err(|e| format!("erro ao salvar PNG: {}", e))
}

// ── Curva de army value ───────────────────────────────────────────────────────

fn draw_army_curve(
    img: &mut RgbaImage,
    player: &PlayerArmyValue,
    col: Rgba<u8>,
    axis_y: u32,
    chart_top: u32,
    axis_width: f32,
    max_loop: u32,
    max_army: i32,
) {
    // Ignora snapshots de t=0 (irrelevantes — exército ainda não existe)
    let snaps: Vec<_> = player.snapshots.iter().filter(|s| s.game_loop > 0).collect();
    if snaps.len() < 2 {
        return;
    }
    for w in snaps.windows(2) {
        let a = w[0];
        let b = w[1];

        let ax = LEFT_MARGIN as f32 + (a.game_loop as f32 / max_loop as f32) * axis_width;
        let bx = LEFT_MARGIN as f32 + (b.game_loop as f32 / max_loop as f32) * axis_width;
        let ay = army_y(a.army_total, max_army, axis_y, CHART_H).max(chart_top as f32);
        let by = army_y(b.army_total, max_army, axis_y, CHART_H).max(chart_top as f32);

        // 3 px de espessura com antialiasing (Wu)
        for dy in [-1i32, 0, 1] {
            draw_line_aa(img, ax, ay + dy as f32, bx, by + dy as f32, col);
        }
    }
}

// ── Linhas verticais de upgrade + rótulos ────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_upgrade_verticals<Fv, Fl>(
    img: &mut RgbaImage,
    font: &FontRef,
    label_scale: PxScale,
    events: &[crate::army_value::ArmyUpgradeEvent],
    race: &str,
    chart_top: u32,
    axis_y: u32,
    label_top: u32,    // posição Y onde o rótulo/ícone começa (desce a partir daqui)
    axis_width: f32,
    max_loop: u32,
    vline_color: Fv,
    label_color: Fl,
) where
    Fv: Fn(&crate::army_value::ArmyUpgradeEvent) -> Rgba<u8>,
    Fl: Fn(&crate::army_value::ArmyUpgradeEvent) -> Rgba<u8>,
{
    let char_h = (LABEL_FONT_SIZE.ceil() as u32) + 4;

    for ev in events {
        let x = LEFT_MARGIN as f32 + (ev.game_loop as f32 / max_loop as f32) * axis_width;
        let xi = x as i32;

        let lcol = label_color(ev);

        // Linha vertical através da área do gráfico (apenas para ataque / armadura), 2 px de largura
        if matches!(ev.kind, UpgradeKind::Attack | UpgradeKind::Armor) {
            let vcol = vline_color(ev);
            draw_line_segment_mut(img, (x,       chart_top as f32), (x,       axis_y as f32), vcol);
            draw_line_segment_mut(img, (x + 1.0, chart_top as f32), (x + 1.0, axis_y as f32), vcol);
        }

        // Ícone quando disponível; rótulo de texto como fallback — ambos descem a partir de label_top
        if let Some(icon) = icons::lookup(race, &ev.raw_name) {
            let icon_rgba = icon.to_rgba8();
            let paste_x = xi - (ICON_SIZE as i32 / 2);
            blit_alpha(img, &icon_rgba, paste_x, label_top as i32);
        } else {
            let label_w = text_size(label_scale, font, &ev.name).0.max(1);
            let mut buf = RgbaImage::from_pixel(label_w, char_h.max(1), BG);
            draw_text_mut(&mut buf, lcol, 0, 0, label_scale, font, &ev.name);
            let rotated = image::imageops::rotate90(&buf);
            let paste_x = xi - (rotated.width() as i32 / 2);
            blit(img, &rotated, paste_x, label_top as i32);
        }
    }
}

// ── Coordenada Y para um valor de army ───────────────────────────────────────

/// Converte um valor de army para coordenada Y (eixo positivo acima do axis_y).
fn army_y(value: i32, max_army: i32, axis_y: u32, chart_h: u32) -> f32 {
    let frac = (value.max(0) as f32 / max_army as f32).min(1.0);
    axis_y as f32 - frac * chart_h as f32
}

// ── Cores ─────────────────────────────────────────────────────────────────────

fn upgrade_vline_color_p1(ev: &crate::army_value::ArmyUpgradeEvent) -> Rgba<u8> {
    match ev.kind {
        UpgradeKind::Attack => Rgba([210, 165, 60, 255]),  // âmbar dourado
        UpgradeKind::Armor  => Rgba([90, 165, 225, 255]),  // azul aço
        UpgradeKind::Other  => Rgba([185, 185, 200, 255]), // cinza lavanda
    }
}

fn upgrade_vline_color_p2(ev: &crate::army_value::ArmyUpgradeEvent) -> Rgba<u8> {
    match ev.kind {
        UpgradeKind::Attack => Rgba([225, 140, 55, 255]),  // laranja queimado
        UpgradeKind::Armor  => Rgba([60, 195, 170, 255]),  // turquesa
        UpgradeKind::Other  => Rgba([190, 175, 160, 255]), // bege acinzentado
    }
}

fn upgrade_label_color_p1(ev: &crate::army_value::ArmyUpgradeEvent) -> Rgba<u8> {
    match ev.kind {
        UpgradeKind::Attack => Rgba([155, 105, 10, 255]),  // âmbar escuro
        UpgradeKind::Armor  => Rgba([20, 95, 165, 255]),   // azul escuro
        UpgradeKind::Other  => Rgba([90, 90, 110, 255]),   // cinza azulado
    }
}

fn upgrade_label_color_p2(ev: &crate::army_value::ArmyUpgradeEvent) -> Rgba<u8> {
    match ev.kind {
        UpgradeKind::Attack => Rgba([175, 90, 10, 255]),   // laranja escuro
        UpgradeKind::Armor  => Rgba([10, 135, 115, 255]),  // teal escuro
        UpgradeKind::Other  => Rgba([100, 88, 76, 255]),   // marrom acinzentado
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn player_label(p: &PlayerArmyValue) -> String {
    if p.name.is_empty() {
        "Player".to_string()
    } else {
        format!("{} ({})", p.name, p.race)
    }
}

fn nice_time_interval(total_secs: u32) -> u32 {
    match total_secs {
        0..=90   => 15,
        91..=300 => 30,
        301..=600 => 60,
        _ => 120,
    }
}

// ── Antialiasing (algoritmo de Xiaolin Wu) ────────────────────────────────────

/// Linha antialiasada usando o algoritmo de Wu.
/// Usa composição alfa sobre o conteúdo já presente no buffer.
fn draw_line_aa(img: &mut RgbaImage, x0: f32, y0: f32, x1: f32, y1: f32, col: Rgba<u8>) {
    let steep = (y1 - y0).abs() > (x1 - x0).abs();

    let (mut x0, mut y0, mut x1, mut y1) = if steep {
        (y0, x0, y1, x1)
    } else {
        (x0, y0, x1, y1)
    };
    if x0 > x1 {
        std::mem::swap(&mut x0, &mut x1);
        std::mem::swap(&mut y0, &mut y1);
    }

    let dx = x1 - x0;
    let dy = y1 - y0;
    let gradient = if dx.abs() < f32::EPSILON { 1.0 } else { dy / dx };

    // Primeiro endpoint
    let xend = x0.round();
    let yend = y0 + gradient * (xend - x0);
    let xgap = 1.0 - (x0 + 0.5).fract();
    let xpxl1 = xend as i32;
    let ypxl1 = yend.floor() as i32;
    plot_aa(img, xpxl1, ypxl1,     (1.0 - yend.fract()) * xgap, col, steep);
    plot_aa(img, xpxl1, ypxl1 + 1,        yend.fract()  * xgap, col, steep);
    let mut intery = yend + gradient;

    // Segundo endpoint
    let xend = x1.round();
    let yend = y1 + gradient * (xend - x1);
    let xgap = (x1 + 0.5).fract();
    let xpxl2 = xend as i32;
    let ypxl2 = yend.floor() as i32;
    plot_aa(img, xpxl2, ypxl2,     (1.0 - yend.fract()) * xgap, col, steep);
    plot_aa(img, xpxl2, ypxl2 + 1,        yend.fract()  * xgap, col, steep);

    // Loop principal
    for x in (xpxl1 + 1)..xpxl2 {
        plot_aa(img, x, intery.floor() as i32,     1.0 - intery.fract(), col, steep);
        plot_aa(img, x, intery.floor() as i32 + 1,       intery.fract(), col, steep);
        intery += gradient;
    }
}

/// Plota um pixel com cobertura parcial (passo interno do Wu).
/// `swap_xy` inverte os eixos quando a linha é íngreme.
fn plot_aa(img: &mut RgbaImage, x: i32, y: i32, coverage: f32, col: Rgba<u8>, swap_xy: bool) {
    let (px, py) = if swap_xy { (y, x) } else { (x, y) };
    if px < 0 || py < 0 || px as u32 >= img.width() || py as u32 >= img.height() {
        return;
    }
    let a = (coverage * col[3] as f32 / 255.0).clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let dst = *img.get_pixel(px as u32, py as u32);
    let r = (a * col[0] as f32 + (1.0 - a) * dst[0] as f32) as u8;
    let g = (a * col[1] as f32 + (1.0 - a) * dst[1] as f32) as u8;
    let b = (a * col[2] as f32 + (1.0 - a) * dst[2] as f32) as u8;
    img.put_pixel(px as u32, py as u32, Rgba([r, g, b, 255]));
}

fn blit(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    for (ox, oy, pixel) in src.enumerate_pixels() {
        let bx = x + ox as i32;
        let by = y + oy as i32;
        if bx >= 0 && by >= 0 && (bx as u32) < dst.width() && (by as u32) < dst.height() {
            dst.put_pixel(bx as u32, by as u32, *pixel);
        }
    }
}

fn blit_alpha(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    for (ox, oy, pixel) in src.enumerate_pixels() {
        let bx = x + ox as i32;
        let by = y + oy as i32;
        if bx >= 0 && by >= 0 && (bx as u32) < dst.width() && (by as u32) < dst.height() {
            let a = pixel.0[3] as f32 / 255.0;
            if a == 0.0 {
                continue;
            }
            let bg = dst.get_pixel(bx as u32, by as u32);
            let r = (a * pixel.0[0] as f32 + (1.0 - a) * bg.0[0] as f32) as u8;
            let g = (a * pixel.0[1] as f32 + (1.0 - a) * bg.0[1] as f32) as u8;
            let b = (a * pixel.0[2] as f32 + (1.0 - a) * bg.0[2] as f32) as u8;
            dst.put_pixel(bx as u32, by as u32, Rgba([r, g, b, 255]));
        }
    }
}
