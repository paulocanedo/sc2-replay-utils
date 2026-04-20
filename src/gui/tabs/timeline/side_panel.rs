//! Painel lateral — stats de um jogador no instante de scrubbing.
//!
//! Organizado em blocos separados por dividers hairline:
//! 1. Identidade — nome + chip da raça.
//! 2. Supply — barra de progresso com destaque quando blocked.
//! 3. Economia — minerals/gas (com ícones desenhados) + rates,
//!    workers com barra de capacidade.
//! 4. Army — valor total + split mineral/gas + pips de attack/armor.
//! 5. Eficiência — building focus, idle time e supply block count.

use egui::{
    epaint::Shape, pos2, vec2, Align, Color32, Layout, ProgressBar, Rect, RichText, Sense, Stroke,
    Ui,
};

use crate::colors::{
    player_slot_color_bright, race_color, ACCENT_DANGER, ACCENT_WARNING, LABEL_DIM, LABEL_SOFT,
};
use crate::config::AppConfig;
use crate::locale::{tf, Language};
use crate::production_gap::PlayerProductionGap;
use crate::replay::{PlayerTimeline, StatsSnapshot};
use crate::replay_state::LoadedReplay;
use crate::supply_block::SupplyBlockEntry;
use crate::tokens::{size_body, size_caption, size_subtitle, SPACE_S, SPACE_XS};
use crate::utils::race_letter;
use crate::widgets::chip;

use super::entities::structure_attention_at;

/// Cor do diamante de minerals. Azul-claro próximo do cristal in-game.
const MINERAL_COLOR: Color32 = Color32::from_rgb(100, 180, 230);
/// Cor do círculo de gas. Verde-claro próximo do geyser Vespene.
const GAS_COLOR: Color32 = Color32::from_rgb(90, 200, 150);

/// Renderiza o painel lateral de um jogador. Faz lookup de todos os
/// dados derivados (`production`, `supply_blocks`) diretamente no
/// `LoadedReplay` pra evitar um fan-out de argumentos.
pub(super) fn player_side_panel(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    idx: usize,
    game_loop: u32,
    cfg: &AppConfig,
) {
    let Some(p) = loaded.timeline.players.get(idx) else {
        return;
    };
    let production: Option<&PlayerProductionGap> =
        loaded.production.as_ref().and_then(|r| r.players.get(idx));
    let supply_blocks: &[SupplyBlockEntry] = loaded
        .supply_blocks_per_player
        .get(idx)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let loops_per_second = loaded.timeline.loops_per_second;
    let lang = cfg.language;
    let slot_color = player_slot_color_bright(idx);

    ui.add_space(SPACE_S);
    header(ui, p, slot_color, cfg);
    ui.add_space(SPACE_XS);
    ui.separator();

    match p.stats_at(game_loop) {
        Some(s) => {
            ui.add_space(SPACE_XS);
            supply_bar(ui, s, slot_color);
            ui.add_space(SPACE_S);
            economy_block(ui, p, s, game_loop, cfg);
            ui.add_space(SPACE_S);
            ui.separator();
            ui.add_space(SPACE_XS);
            army_block(ui, p, s, game_loop, slot_color, cfg);
        }
        None => {
            ui.add_space(SPACE_S);
            ui.weak("—");
        }
    }

    ui.add_space(SPACE_XS);
    ui.separator();
    ui.add_space(SPACE_XS);
    efficiency_block(ui, p, game_loop, production, supply_blocks, loops_per_second, lang);
}

// ── Header ─────────────────────────────────────────────────────────────

fn header(ui: &mut Ui, p: &PlayerTimeline, slot_color: Color32, cfg: &AppConfig) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&p.name)
                .size(size_subtitle(cfg))
                .strong()
                .color(slot_color),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let letter = race_letter(&p.race);
            chip(ui, &letter.to_string(), true, Some(race_color(&p.race)));
        });
    });
}

// ── Supply bar ─────────────────────────────────────────────────────────

fn supply_bar(ui: &mut Ui, s: &StatsSnapshot, slot_color: Color32) {
    let cap = s.supply_made.min(200);
    let frac = if cap > 0 {
        (s.supply_used as f32 / cap as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let blocked = s.supply_made > 0 && s.supply_used >= s.supply_made;
    let bar_color = if blocked { ACCENT_DANGER } else { slot_color };
    let label = if blocked {
        format!("⚠ {}/{}", s.supply_used, cap)
    } else {
        format!("{}/{}", s.supply_used, cap)
    };
    ui.add(
        ProgressBar::new(frac)
            .fill(bar_color)
            .text(RichText::new(label).small().strong()),
    );
}

// ── Economy block ──────────────────────────────────────────────────────

fn economy_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    s: &StatsSnapshot,
    game_loop: u32,
    cfg: &AppConfig,
) {
    resource_row(ui, MINERAL_COLOR, s.minerals, s.minerals_rate, true, cfg);
    resource_row(ui, GAS_COLOR, s.vespene, s.vespene_rate, false, cfg);
    worker_row(ui, p, s, game_loop);
}

fn resource_row(
    ui: &mut Ui,
    icon_color: Color32,
    value: i32,
    rate: i32,
    is_mineral: bool,
    cfg: &AppConfig,
) {
    ui.horizontal(|ui| {
        let size = size_body(cfg);
        let (resp, painter) = ui.allocate_painter(vec2(size, size), Sense::hover());
        if is_mineral {
            paint_mineral_icon(&painter, resp.rect, icon_color);
        } else {
            paint_gas_icon(&painter, resp.rect, icon_color);
        }
        ui.monospace(value.to_string());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("+{}/m", rate))
                    .size(size_caption(cfg))
                    .color(LABEL_DIM),
            );
        });
    });
}

fn worker_row(ui: &mut Ui, p: &PlayerTimeline, s: &StatsSnapshot, game_loop: u32) {
    let cap = p.worker_capacity_at(game_loop);
    ui.horizontal(|ui| {
        ui.monospace(format!("👷 {}/{}", s.workers, cap.max(s.workers)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let frac = if cap > 0 {
                (s.workers as f32 / cap as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            draw_mini_bar(ui, 42.0, 4.0, frac, LABEL_SOFT);
        });
    });
}

// ── Army block ─────────────────────────────────────────────────────────

fn army_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    s: &StatsSnapshot,
    game_loop: u32,
    slot_color: Color32,
    cfg: &AppConfig,
) {
    let total = s.army_value_minerals + s.army_value_vespene;
    ui.label(
        RichText::new(format!("⚔ {total}"))
            .size(size_subtitle(cfg))
            .strong()
            .color(slot_color),
    );
    ui.label(
        RichText::new(format!(
            "m {} · g {}",
            s.army_value_minerals, s.army_value_vespene
        ))
        .size(size_caption(cfg))
        .color(LABEL_DIM),
    );

    let atk = p.attack_level_at(game_loop);
    let arm = p.armor_level_at(game_loop);
    if atk > 0 || arm > 0 {
        ui.add_space(SPACE_XS);
        ui.horizontal(|ui| {
            if atk > 0 {
                chip(ui, &format!("⚔+{atk}"), true, Some(slot_color));
            }
            if arm > 0 {
                chip(ui, &format!("🛡+{arm}"), true, Some(slot_color));
            }
        });
    }
}

// ── Efficiency block ───────────────────────────────────────────────────

fn efficiency_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    game_loop: u32,
    production: Option<&PlayerProductionGap>,
    supply_blocks: &[SupplyBlockEntry],
    loops_per_second: f64,
    lang: Language,
) {
    // Building focus — retained metric with inline mini-bar.
    let (att, tot) = structure_attention_at(p, game_loop);
    ui.horizontal(|ui| {
        if tot == 0 {
            ui.label(
                RichText::new(tf("timeline.stats.bldg_focus_none", lang, &[]))
                    .small()
                    .color(LABEL_DIM),
            );
        } else {
            let pct = att as f32 * 100.0 / tot as f32;
            ui.label(
                RichText::new(format!("🏢 {pct:.0}%"))
                    .small()
                    .color(LABEL_SOFT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                draw_mini_bar(ui, 32.0, 3.0, pct / 100.0, LABEL_DIM);
            });
        }
    });

    // Idle production time — Terran/Protoss only; Zerg uses larva model
    // and the detector returns empty for them.
    if let Some(prod) = production
        && !prod.is_zerg
        && prod.total_idle_loops > 0
    {
        let secs = (prod.total_idle_loops as f64 / loops_per_second).round() as u32;
        ui.label(
            RichText::new(tf(
                "timeline.stats.idle",
                lang,
                &[("secs", &secs.to_string())],
            ))
            .small()
            .color(LABEL_DIM),
        );
    }

    // Supply block count — warning colour, only when > 0.
    if !supply_blocks.is_empty() {
        let total_loops: u32 = supply_blocks
            .iter()
            .map(|b| b.end_loop.saturating_sub(b.start_loop))
            .sum();
        let secs = (total_loops as f64 / loops_per_second).round() as u32;
        ui.label(
            RichText::new(tf(
                "timeline.stats.blocks",
                lang,
                &[
                    ("count", &supply_blocks.len().to_string()),
                    ("secs", &secs.to_string()),
                ],
            ))
            .small()
            .color(ACCENT_WARNING),
        );
    }
}

// ── Drawing primitives ─────────────────────────────────────────────────

fn paint_mineral_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.4;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x, c.y - r),
            pos2(c.x + r * 0.75, c.y),
            pos2(c.x, c.y + r),
            pos2(c.x - r * 0.75, c.y),
        ],
        color,
        Stroke::NONE,
    ));
}

fn paint_gas_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.32;
    painter.circle_filled(c, r, color);
}

/// Barra fina usada como inline indicator (worker cap, building focus).
/// Track em cinza escuro, fill em `color`.
fn draw_mini_bar(ui: &mut Ui, width: f32, height: f32, frac: f32, color: Color32) {
    let (resp, painter) = ui.allocate_painter(vec2(width, height), Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 1.0, Color32::from_gray(50));
    let fill_w = rect.width() * frac.clamp(0.0, 1.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, vec2(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 1.0, color);
    }
}
