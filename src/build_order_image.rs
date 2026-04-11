use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_line_segment_mut, draw_text_mut, text_size};

use crate::army_value_image::{P1_COLOR, P2_COLOR};
use crate::build_order::{format_time, BuildOrderEntry};
use crate::icons::{self, ICON_SIZE};

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
/// Faixa da escala de supply (linha + ticks + rótulos), logo abaixo do título
const SUPPLY_AREA: u32 = 44;
const SUPPLY_AXIS_GAP: u32 = 8;  // gap entre fundo do título e linha de supply
const SUPPLY_TICK_H: u32 = 6;    // comprimento do tick descendo da linha
const SUPPLY_LBL_GAP: u32 = 2;   // gap entre tick e rótulo
/// Pequeno espaço entre a escala de supply e a área de conteúdo acima do eixo
const AXIS_TOP: u32 = 8;
const TICK_H: u32 = 20;
const LABEL_GAP: u32 = 12; // espaço entre eixo+tick e topo do conteúdo
const TICK_H_UP: u32 = 20;   // tick subindo do eixo para upgrades/pesquisas
const LABEL_GAP_UP: u32 = 12; // espaço entre tick e base dos ícones de upgrade
/// Espaço reservado para os rótulos de tempo abaixo de todo o conteúdo
const TIME_AREA: u32 = 44;

// ── Paleta ────────────────────────────────────────────────────────────────────

const BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const AXIS_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);
const TICK_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);
const LABEL_COL: Rgba<u8> = Rgba([30, 30, 30, 255]);
const TIME_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);
const SUPPLY_AXIS_COL: Rgba<u8> = Rgba([160, 160, 160, 255]); // linha/ticks da escala de supply
const SUPPLY_LBL_COL: Rgba<u8> = Rgba([80, 80, 80, 255]);     // valores de supply

// ── API pública ───────────────────────────────────────────────────────────────

/// Renderiza a build order de um jogador como imagem de linha do tempo em memória.
///
/// Layout vertical (de cima para baixo):
///   TITLE_H           — título
///   SUPPLY_AREA       — escala de supply (linha + ticks + rótulos de supply)
///   AXIS_TOP          — respiro entre escala de supply e área de upgrades
///   upgrade_content_h — ícones de upgrades/estruturas (acima do eixo)
///   LABEL_GAP_UP      — espaço entre ícones e topo do tick
///   TICK_H_UP         — ticks subindo até o eixo
///   axis_y            — linha horizontal do eixo X
///   TICK_H            — ticks descendo do eixo
///   LABEL_GAP         — espaço entre ticks e ícones/rótulos
///   units_content_h   — ícones de unidades (ou rótulos rotacionados)
///   TIME_AREA         — rótulos de tempo MM:SS
pub fn render_build_order(
    player_number: usize,
    name: &str,
    race: &str,
    mmr: Option<i32>,
    entries: &[BuildOrderEntry],
    loops_per_second: f64,
) -> Result<RgbaImage, String> {
    if entries.is_empty() {
        return Err("sem entradas para renderizar".to_string());
    }

    let entries_filtered: Vec<BuildOrderEntry> = entries
        .iter()
        .filter(|e| !is_worker(&e.action))
        .cloned()
        .collect();
    let entries = entries_filtered.as_slice();

    let font = FontRef::try_from_slice(FONT_BYTES)
        .map_err(|e| format!("fonte inválida: {:?}", e))?;
    let scale = PxScale::from(FONT_SIZE);
    let title_scale = PxScale::from(TITLE_FONT_SIZE);

    // Pré-carrega ícones (None = usar rótulo de texto como fallback)
    let entry_icons: Vec<Option<image::DynamicImage>> = entries
        .iter()
        .map(|e| icons::lookup(race, &e.action))
        .collect();

    // Rótulos de texto — usados apenas quando não há ícone
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

    // ── Pré-passe: xi de cada entrada + detecção de colisão/empilhamento ────────
    let max_game_loop = entries.iter().map(|e| e.game_loop).max().unwrap_or(1);
    let axis_width = (IMG_WIDTH - LEFT_MARGIN - RIGHT_MARGIN) as f32;

    // Xi pixel de cada entrada (mesma fórmula usada na renderização)
    let xis: Vec<i32> = entries.iter()
        .map(|e| (LEFT_MARGIN as f32 + (e.game_loop as f32 / max_game_loop as f32) * axis_width) as i32)
        .collect();

    // Listas (orig_idx, xi) por região, somente entradas com ícone, ordenadas por xi
    // Acima do eixo: upgrades/pesquisas E estruturas
    let mut upgrade_icon_xis: Vec<(usize, i32)> = entry_icons.iter().zip(entries.iter())
        .enumerate()
        .filter(|(_, (icon, e))| icon.is_some() && (e.is_upgrade || e.is_structure))
        .map(|(i, _)| (i, xis[i]))
        .collect();
    upgrade_icon_xis.sort_by_key(|&(_, xi)| xi);

    // Abaixo do eixo: apenas unidades
    let mut units_icon_xis: Vec<(usize, i32)> = entry_icons.iter().zip(entries.iter())
        .enumerate()
        .filter(|(_, (icon, e))| icon.is_some() && !e.is_upgrade && !e.is_structure)
        .map(|(i, _)| (i, xis[i]))
        .collect();
    units_icon_xis.sort_by_key(|&(_, xi)| xi);

    // Algoritmo guloso: atribui slot a cada ícone sem colisão
    let upgrade_slots = assign_slots(&upgrade_icon_xis);
    let units_slots   = assign_slots(&units_icon_xis);

    // Mapas orig_idx → slot para uso na renderização
    let mut upgrade_slot_for = vec![0usize; entries.len()];
    for (pos, &(orig, _)) in upgrade_icon_xis.iter().enumerate() {
        upgrade_slot_for[orig] = upgrade_slots[pos];
    }
    let mut units_slot_for = vec![0usize; entries.len()];
    for (pos, &(orig, _)) in units_icon_xis.iter().enumerate() {
        units_slot_for[orig] = units_slots[pos];
    }

    // Profundidade máxima de pilha por região
    let max_depth_upgrades = upgrade_slots.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    let max_depth_units    = units_slots.iter().copied().max().map(|m| m + 1).unwrap_or(0);

    // Altura máxima do conteúdo: separada para a região acima (upgrades+estruturas) e abaixo (unidades)
    let upgrade_content_h = {
        let max_txt = labels.iter().zip(entry_icons.iter()).zip(entries.iter())
            .filter(|((_, icon), e)| icon.is_none() && (e.is_upgrade || e.is_structure))
            .map(|((l, _), _)| text_size(scale, &font, l).0)
            .max().unwrap_or(0);
        let icon_h = (max_depth_upgrades as u32) * ICON_SIZE;
        max_txt.max(icon_h)
    };

    let units_content_h = {
        let max_txt = labels.iter().zip(entry_icons.iter()).zip(entries.iter())
            .filter(|((_, icon), e)| icon.is_none() && !e.is_upgrade && !e.is_structure)
            .map(|((l, _), _)| text_size(scale, &font, l).0)
            .max().unwrap_or(0);
        let icon_h = (max_depth_units as u32) * ICON_SIZE;
        max_txt.max(icon_h)
    };

    // ── Dimensões da imagem ───────────────────────────────────────────────────
    let char_h = FONT_SIZE.ceil() as u32 + 4;
    let above_axis_reserved = if upgrade_content_h > 0 {
        upgrade_content_h + LABEL_GAP_UP + TICK_H_UP
    } else {
        0
    };
    // axis_y agora inclui SUPPLY_AREA logo após o título
    let axis_y = TITLE_H + SUPPLY_AREA + AXIS_TOP + above_axis_reserved;
    let content_top = axis_y + TICK_H + LABEL_GAP;
    let time_top = content_top + units_content_h + 8;
    let img_height = time_top + TIME_AREA;

    let mut img = RgbaImage::from_pixel(IMG_WIDTH, img_height, BG);

    // ── Título ────────────────────────────────────────────────────────────────
    let title = if name.is_empty() {
        format!("Player {}", player_number)
    } else {
        match mmr {
            Some(m) => format!("Player {} — {} ({}) · {}", player_number, name, race, m),
            None    => format!("Player {} — {} ({})", player_number, name, race),
        }
    };
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

    // ── Escala de supply (topo do gráfico) ───────────────────────────────────
    // Linha horizontal da escala de supply
    let supply_axis_y = TITLE_H + SUPPLY_AXIS_GAP;
    let supply_label_y = (supply_axis_y + SUPPLY_TICK_H + SUPPLY_LBL_GAP) as i32;
    draw_line_segment_mut(
        &mut img,
        (LEFT_MARGIN as f32, supply_axis_y as f32),
        ((IMG_WIDTH - RIGHT_MARGIN) as f32, supply_axis_y as f32),
        SUPPLY_AXIS_COL,
    );

    // Faixa de supply: do valor inicial até o máximo atingido na partida.
    // Para cada unidade N nessa faixa, interpola o game_loop em que supply
    // primeiro atingiu N e desenha um tick — maior em múltiplos de 5,
    // menor nos demais — e um rótulo a cada 10 unidades.
    let s_min = entries.first().map(|e| e.supply).unwrap_or(0);
    let s_max = entries.iter().map(|e| e.supply).max().unwrap_or(0);
    let mut last_label_right: i32 = i32::MIN;

    'tick: for n in s_min..=s_max {
        // Percorre os entries procurando onde supply atinge n pela 1ª vez.
        // Quando supply passa de prev_s para entry.supply com prev_s < n ≤ entry.supply,
        // interpola linearmente o game_loop correspondente.
        let mut prev_gl = entries[0].game_loop;
        let mut prev_s  = entries[0].supply;
        for entry in entries {
            if entry.supply >= n {
                let gl = if prev_s >= n {
                    // Primeiro entry já tem supply ≥ n (n = s_min)
                    entry.game_loop
                } else {
                    let frac = (n as f32 - prev_s as f32)
                        / (entry.supply as f32 - prev_s as f32);
                    (prev_gl as f32 + frac * (entry.game_loop as f32 - prev_gl as f32))
                        .round() as u32
                };
                let x  = LEFT_MARGIN as f32 + (gl as f32 / max_game_loop as f32) * axis_width;
                let xi = x as i32;

                // Tick: maior em múltiplos de 5, menor nos demais
                let tick_h = if n % 5 == 0 { SUPPLY_TICK_H } else { SUPPLY_TICK_H / 2 };
                draw_line_segment_mut(
                    &mut img,
                    (x, supply_axis_y as f32),
                    (x, (supply_axis_y + tick_h) as f32),
                    SUPPLY_AXIS_COL,
                );

                // Rótulo a cada 10 unidades, evitando sobreposição
                if n % 10 == 0 {
                    let label   = n.to_string();
                    let label_w = text_size(scale, &font, &label).0 as i32;
                    let label_x = xi - label_w / 2;
                    if label_x >= last_label_right + 4 {
                        draw_text_mut(
                            &mut img,
                            SUPPLY_LBL_COL,
                            label_x,
                            supply_label_y,
                            scale,
                            &font,
                            &label,
                        );
                        last_label_right = label_x + label_w;
                    }
                }
                continue 'tick;
            }
            prev_gl = entry.game_loop;
            prev_s  = entry.supply;
        }
    }

    // ── Linha do eixo ─────────────────────────────────────────────────────────
    draw_line_segment_mut(
        &mut img,
        (LEFT_MARGIN as f32, axis_y as f32),
        ((IMG_WIDTH - RIGHT_MARGIN) as f32, axis_y as f32),
        AXIS_COL,
    );

    // ── Eventos ───────────────────────────────────────────────────────────────
    for (entry_index, ((icon_opt, label), entry)) in entry_icons
        .iter()
        .zip(labels.iter())
        .zip(entries.iter())
        .enumerate()
    {
        let x = LEFT_MARGIN as f32
            + (entry.game_loop as f32 / max_game_loop as f32) * axis_width;
        let xi = x as i32;

        if entry.is_upgrade || entry.is_structure {
            // Tick vertical subindo do eixo (upgrades, pesquisas e estruturas ficam acima)
            draw_line_segment_mut(
                &mut img,
                (x, axis_y as f32),
                (x, (axis_y - TICK_H_UP) as f32),
                TICK_COL,
            );

            if let Some(icon) = icon_opt {
                // Ícone centralizado horizontalmente acima do tick, empilhado se colidir
                let icon_rgba = icon.to_rgba8();
                let paste_x = xi - (ICON_SIZE as i32 / 2);
                let slot = upgrade_slot_for[entry_index];
                let paste_y = axis_y as i32 - TICK_H_UP as i32 - LABEL_GAP_UP as i32 - (slot as i32 + 1) * ICON_SIZE as i32;
                blit_alpha(&mut img, &icon_rgba, paste_x, paste_y);
            } else {
                // Rótulo rotacionado 270° (texto sobe a partir do eixo)
                let label_w = text_size(scale, &font, label).0.max(1);
                let mut label_buf = RgbaImage::from_pixel(label_w, char_h.max(1), BG);
                draw_text_mut(&mut label_buf, LABEL_COL, 0, 0, scale, &font, label);
                let rotated = image::imageops::rotate270(&label_buf);
                let paste_x = xi - (rotated.width() as i32 / 2);
                let paste_y = axis_y as i32 - TICK_H_UP as i32 - LABEL_GAP_UP as i32 - rotated.height() as i32;
                blit(&mut img, &rotated, paste_x, paste_y);
            }
        } else {
            // Tick vertical descendo do eixo (apenas unidades ficam abaixo)
            draw_line_segment_mut(
                &mut img,
                (x, axis_y as f32),
                (x, (axis_y + TICK_H) as f32),
                TICK_COL,
            );

            if let Some(icon) = icon_opt {
                // Ícone centralizado horizontalmente abaixo do tick, empilhado se colidir
                let icon_rgba = icon.to_rgba8();
                let paste_x = xi - (ICON_SIZE as i32 / 2);
                let slot = units_slot_for[entry_index];
                let paste_y = content_top as i32 + slot as i32 * ICON_SIZE as i32;
                blit_alpha(&mut img, &icon_rgba, paste_x, paste_y);
            } else {
                // Rótulo em buffer horizontal, rotacionado 90° (texto desce a partir do eixo)
                let label_w = text_size(scale, &font, label).0.max(1);
                let mut label_buf = RgbaImage::from_pixel(label_w, char_h.max(1), BG);
                draw_text_mut(&mut label_buf, LABEL_COL, 0, 0, scale, &font, label);
                let rotated = image::imageops::rotate90(&label_buf);
                let paste_x = xi - (rotated.width() as i32 / 2);
                let paste_y = content_top as i32;
                blit(&mut img, &rotated, paste_x, paste_y);
            }
        }
    }

    // ── Rótulos de tempo abaixo do conteúdo ──────────────────────────────────
    let time_w = text_size(scale, &font, "00:00").0 as i32;
    let mut t = 0u32;
    loop {
        let gl = (t as f64 * loops_per_second).round() as u32;
        if gl > max_game_loop { break; }
        let x = LEFT_MARGIN as f32 + (gl as f32 / max_game_loop as f32) * axis_width;
        draw_line_segment_mut(
            &mut img,
            (x, axis_y as f32),
            (x, (axis_y + 5) as f32),
            AXIS_COL,
        );
        let time_str = format_time(gl, loops_per_second);
        draw_text_mut(
            &mut img,
            TIME_COL,
            x as i32 - time_w / 2,
            time_top as i32,
            scale,
            &font,
            &time_str,
        );
        t += 60;
    }

    Ok(img)
}

/// Salva a build order de um jogador como imagem PNG de linha do tempo.
pub fn write_build_order_png(
    player_number: usize,
    name: &str,
    race: &str,
    mmr: Option<i32>,
    entries: &[BuildOrderEntry],
    loops_per_second: f64,
    out_path: &Path,
) -> Result<(), String> {
    let img = render_build_order(player_number, name, race, mmr, entries, loops_per_second)?;
    img.save(out_path).map_err(|e| format!("erro ao salvar PNG: {}", e))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn blit(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    for (ox, oy, pixel) in src.enumerate_pixels() {
        let bx = x + ox as i32;
        let by = y + oy as i32;
        if bx >= 0 && by >= 0 && (bx as u32) < dst.width() && (by as u32) < dst.height() {
            dst.put_pixel(bx as u32, by as u32, *pixel);
        }
    }
}

/// Algoritmo guloso de alocação de slots sem colisão para ícones de `ICON_SIZE` px.
///
/// `xis`: pares `(orig_idx, xi)` já ordenados por `xi` crescente.
/// Retorna `Vec` onde o índice `list_pos` corresponde ao slot atribuído àquele par.
///
/// Dois ícones colidem quando `|xi_a - xi_b| < ICON_SIZE`, pois cada ícone ocupa
/// `[xi - ICON_SIZE/2, xi + ICON_SIZE/2]`.
fn assign_slots(xis: &[(usize, i32)]) -> Vec<usize> {
    let mut slots: Vec<i32> = Vec::new(); // slots[s] = último xi colocado no slot s
    let mut result = Vec::with_capacity(xis.len());
    for &(_, xi) in xis {
        let slot = slots
            .iter()
            .position(|&last| xi - last >= ICON_SIZE as i32)
            .unwrap_or_else(|| {
                slots.push(i32::MIN);
                slots.len() - 1
            });
        slots[slot] = xi;
        result.push(slot);
    }
    result
}

fn is_worker(action: &str) -> bool {
    matches!(action, "SCV" | "Probe" | "Drone")
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
