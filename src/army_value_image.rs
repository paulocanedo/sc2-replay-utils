use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_line_segment_mut, draw_text_mut, text_size};

use crate::army_value::{PlayerArmyValue, UpgradeKind};
use crate::build_order::format_time;

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
/// Altura da área de rótulos de upgrade abaixo do eixo (P2)
const LABEL_BELOW_H: u32 = 160;
/// Espaço para rótulos de tempo na base
const TIME_AREA: u32 = 44;

// ── Paleta ────────────────────────────────────────────────────────────────────

const BG: Rgba<u8> = Rgba([18, 18, 28, 255]);
const AXIS_COL: Rgba<u8> = Rgba([100, 120, 180, 255]);
const TIME_COL: Rgba<u8> = Rgba([140, 160, 200, 255]);
const TITLE_COL: Rgba<u8> = Rgba([220, 180, 60, 255]);
const GRID_COL: Rgba<u8> = Rgba([40, 52, 88, 255]);
const Y_LABEL_COL: Rgba<u8> = Rgba([110, 130, 170, 200]);

/// Cor fixa da linha de army value do P1 (azul)
const P1_LINE: Rgba<u8> = Rgba([80, 150, 255, 255]);
/// Cor fixa da linha de army value do P2 (laranja)
const P2_LINE: Rgba<u8> = Rgba([255, 160, 60, 255]);

// ── API pública ───────────────────────────────────────────────────────────────

/// Renderiza o gráfico de valor de exército de ambos os jogadores em um único PNG.
///
/// Layout vertical:
///   TITLE_H  — título
///   CHART_H  — curvas de army value + linhas verticais de upgrade + rótulos P1 (acima)
///   axis     — eixo de tempo
///   LABEL_BELOW_H — rótulos de upgrade P2 (abaixo)
///   TIME_AREA — rótulos MM:SS
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
    let img_height = TITLE_H + CHART_H + LABEL_BELOW_H + TIME_AREA;
    let axis_y = TITLE_H + CHART_H;           // linha horizontal do eixo de tempo
    let chart_top = TITLE_H;                  // topo da área do gráfico
    let label_bottom = axis_y + LABEL_BELOW_H; // base dos rótulos de upgrade P2

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
        &p1.upgrade_events,
        chart_top, label_bottom, axis_y,
        axis_width, max_loop,
        true,  // P1 → rótulo acima do eixo
        upgrade_vline_color_p1,
        upgrade_label_color_p1,
    );
    draw_upgrade_verticals(
        &mut img, &font, label_scale,
        &p2.upgrade_events,
        chart_top, label_bottom, axis_y,
        axis_width, max_loop,
        false, // P2 → rótulo abaixo do eixo
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
            (label_bottom + 6) as i32,
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

        // 3 px de espessura por deslocamento vertical
        for dy in [-1i32, 0, 1] {
            draw_line_segment_mut(img, (ax, ay + dy as f32), (bx, by + dy as f32), col);
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
    chart_top: u32,
    label_bottom: u32,
    axis_y: u32,
    axis_width: f32,
    max_loop: u32,
    above: bool,       // true = label acima do eixo (P1); false = abaixo (P2)
    vline_color: Fv,
    label_color: Fl,
) where
    Fv: Fn(&crate::army_value::ArmyUpgradeEvent) -> Rgba<u8>,
    Fl: Fn(&crate::army_value::ArmyUpgradeEvent) -> Rgba<u8>,
{
    let char_h = (LABEL_FONT_SIZE.ceil() as u32) + 4;
    let label_gap: u32 = 6;

    for ev in events {
        let x = LEFT_MARGIN as f32 + (ev.game_loop as f32 / max_loop as f32) * axis_width;
        let xi = x as i32;

        let lcol = label_color(ev);

        // Linha vertical contínua apenas para upgrades de ataque / defesa / escudo
        if matches!(ev.kind, UpgradeKind::Attack | UpgradeKind::Armor) {
            let vcol = vline_color(ev);
            draw_line_segment_mut(
                img,
                (x, chart_top as f32),
                (x, label_bottom as f32),
                vcol,
            );
        }

        // Rótulo rotacionado
        let label_w = text_size(label_scale, font, &ev.name).0.max(1);
        let mut buf = RgbaImage::from_pixel(label_w, char_h.max(1), BG);
        draw_text_mut(&mut buf, lcol, 0, 0, label_scale, font, &ev.name);

        if above {
            // Label rotacionada 270° — texto sobe a partir do eixo
            let rotated = image::imageops::rotate270(&buf);
            let paste_x = xi - (rotated.width() as i32 / 2);
            let paste_y = (axis_y - label_gap) as i32 - rotated.height() as i32;
            blit(img, &rotated, paste_x, paste_y);
        } else {
            // Label rotacionada 90° — texto desce a partir do eixo
            let rotated = image::imageops::rotate90(&buf);
            let paste_x = xi - (rotated.width() as i32 / 2);
            let paste_y = (axis_y + label_gap) as i32;
            blit(img, &rotated, paste_x, paste_y);
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
        UpgradeKind::Attack => Rgba([220, 160, 60, 60]),  // quente, semitransparente
        UpgradeKind::Armor  => Rgba([60, 180, 220, 60]),  // frio, semitransparente
        UpgradeKind::Other  => Rgba([140, 140, 160, 40]), // cinza, muito sutil
    }
}

fn upgrade_vline_color_p2(ev: &crate::army_value::ArmyUpgradeEvent) -> Rgba<u8> {
    match ev.kind {
        UpgradeKind::Attack => Rgba([255, 130, 30, 60]),  // laranja, semitransparente
        UpgradeKind::Armor  => Rgba([60, 200, 170, 60]),  // turquesa, semitransparente
        UpgradeKind::Other  => Rgba([160, 140, 120, 40]), // bege, muito sutil
    }
}

fn upgrade_label_color_p1(ev: &crate::army_value::ArmyUpgradeEvent) -> Rgba<u8> {
    match ev.kind {
        UpgradeKind::Attack => Rgba([255, 210, 120, 255]),
        UpgradeKind::Armor  => Rgba([120, 220, 255, 255]),
        UpgradeKind::Other  => Rgba([180, 180, 200, 200]),
    }
}

fn upgrade_label_color_p2(ev: &crate::army_value::ArmyUpgradeEvent) -> Rgba<u8> {
    match ev.kind {
        UpgradeKind::Attack => Rgba([255, 160, 60, 255]),
        UpgradeKind::Armor  => Rgba([80, 210, 185, 255]),
        UpgradeKind::Other  => Rgba([180, 160, 140, 200]),
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

fn blit(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    for (ox, oy, pixel) in src.enumerate_pixels() {
        let bx = x + ox as i32;
        let by = y + oy as i32;
        if bx >= 0 && by >= 0 && (bx as u32) < dst.width() && (by as u32) < dst.height() {
            dst.put_pixel(bx as u32, by as u32, *pixel);
        }
    }
}
