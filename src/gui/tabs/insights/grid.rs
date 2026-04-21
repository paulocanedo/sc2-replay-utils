// Responsive grid layout for the Insights tab.
//
// Cards are laid out in rows of N columns, where N is derived from the
// available width. Every card in a row is forced to the same height
// (the tallest card in that row), so adjacent cards align visually.
//
// Implementation:
//   1. Sizing pass — every card is rendered into a
//      `UiBuilder::sizing_pass()` child Ui that collects layout metrics
//      but paints nothing. Cards that skip silently (race-locked,
//      mirror-only, etc.) produce ~0 height and are filtered out, so
//      they don't leave empty slots in the grid.
//   2. Visible pass — the surviving cards are chunked into rows of
//      `cols`. For each row, the row height is broadcast to `card.rs`
//      via a `ctx.data` slot (`MIN_INNER_H_KEY`) and each card is
//      re-rendered with `ui.set_min_height` applied inside its Frame
//      body, so adjacent cards end up visually aligned.

use egui::{Align, Id, Layout, Rect, Ui, UiBuilder, vec2};

use crate::config::AppConfig;
use crate::replay_state::LoadedReplay;
use crate::tokens::{
    CARD_INNER_MY, INSIGHT_CARD_MIN_W, INSIGHT_COL_GAP, INSIGHT_MAX_COLS, SPACE_M,
};

use super::card::MIN_INNER_H_KEY;
use super::{
    army_prod_by_battle, army_trades, base_timings, chrono_distribution, economy_gap,
    inject_efficiency, production_idle, resources_unspent, supply_block,
    tech_timings, turning_point, worker_potential,
};

// Threshold below which a card is considered "not rendered".
const SKIP_HEIGHT_EPS: f32 = 4.0;

// Uniform signature for every card. The void-returning cards are
// wrapped to always return `None`; `turning_point::show` already
// matches and is used directly.
type CardFn = fn(&mut Ui, &LoadedReplay, &AppConfig, usize) -> Option<u32>;

fn wrap_worker_potential(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    worker_potential::show(ui, l, c, i);
    None
}
fn wrap_supply_block(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    supply_block::show(ui, l, c, i);
    None
}
fn wrap_production_idle(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    production_idle::show(ui, l, c, i);
    None
}
fn wrap_resources_unspent(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    resources_unspent::show(ui, l, c, i);
    None
}
fn wrap_economy_gap(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    economy_gap::show(ui, l, c, i);
    None
}
fn wrap_base_timings(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    base_timings::show(ui, l, c, i);
    None
}
fn wrap_tech_timings(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    tech_timings::show(ui, l, c, i);
    None
}
fn wrap_chrono_distribution(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    chrono_distribution::show(ui, l, c, i);
    None
}
fn wrap_inject_efficiency(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    inject_efficiency::show(ui, l, c, i);
    None
}
fn wrap_army_trades(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    army_trades::show(ui, l, c, i);
    None
}
fn wrap_army_prod_by_battle(ui: &mut Ui, l: &LoadedReplay, c: &AppConfig, i: usize) -> Option<u32> {
    army_prod_by_battle::show(ui, l, c, i);
    None
}

const CARDS: &[CardFn] = &[
    wrap_worker_potential,
    wrap_supply_block,
    wrap_production_idle,
    wrap_resources_unspent,
    wrap_economy_gap,
    wrap_base_timings,
    wrap_tech_timings,
    wrap_chrono_distribution,
    wrap_inject_efficiency,
    wrap_army_trades,
    wrap_army_prod_by_battle,
    turning_point::show,
];

pub fn render_masonry(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    selected: usize,
) -> Option<u32> {
    let available = ui.available_rect_before_wrap();
    let total_w = available.width();
    let origin = available.min;

    let cols = (((total_w + INSIGHT_COL_GAP) / (INSIGHT_CARD_MIN_W + INSIGHT_COL_GAP))
        .floor() as usize)
        .clamp(1, INSIGHT_MAX_COLS);
    let col_w =
        ((total_w - INSIGHT_COL_GAP * (cols.saturating_sub(1)) as f32) / cols as f32).max(1.0);

    let min_inner_id = Id::new(MIN_INNER_H_KEY);
    // Defensive: make sure no stale hint leaks in from a previous frame.
    ui.ctx()
        .data_mut(|d| d.insert_temp::<f32>(min_inner_id, 0.0));

    // ── Pass 1: sizing for every card ────────────────────────────
    // Measure natural heights at the column width that will actually
    // be used, then drop cards that rendered empty so they don't leave
    // gaps in the grid.
    let mut rendered: Vec<(CardFn, f32)> = Vec::with_capacity(CARDS.len());
    for card_fn in CARDS {
        let probe_rect = Rect::from_min_size(origin, vec2(col_w, f32::INFINITY));
        let res = ui.scope_builder(
            UiBuilder::new()
                .max_rect(probe_rect)
                .sizing_pass()
                .invisible()
                .layout(Layout::top_down_justified(Align::Min)),
            |ui| {
                card_fn(ui, loaded, config, selected);
                ui.min_rect().height()
            },
        );
        if res.inner > SKIP_HEIGHT_EPS {
            rendered.push((*card_fn, res.inner));
        }
    }

    let mut seek_request: Option<u32> = None;
    let mut cursor_y: f32 = 0.0;

    for chunk in rendered.chunks(cols) {
        // Tallest card in this row drives the shared height.
        let row_h = chunk
            .iter()
            .map(|(_, h)| *h)
            .fold(0.0_f32, f32::max);

        // ── Pass 2: visible render with equalized height ─────────
        // The hint is the min inner height for the Ui *inside* the Frame.
        // `row_h` was measured from the outer child Ui and therefore
        // includes the Frame's vertical inner margin on both sides plus
        // the trailing SPACE_M outside the Frame — strip those out so
        // the rendered Frame ends up the same height as the tallest
        // natural card (not taller), preserving the SPACE_M row gap.
        let frame_inner_hint = (row_h - 2.0 * CARD_INNER_MY as f32 - SPACE_M).max(0.0);
        ui.ctx()
            .data_mut(|d| d.insert_temp::<f32>(min_inner_id, frame_inner_hint));

        for (i_in_row, (card_fn, _)) in chunk.iter().enumerate() {
            let col_rect = Rect::from_min_size(
                egui::pos2(
                    origin.x + (col_w + INSIGHT_COL_GAP) * i_in_row as f32,
                    origin.y + cursor_y,
                ),
                vec2(col_w, f32::INFINITY),
            );
            let res = ui.scope_builder(
                UiBuilder::new()
                    .max_rect(col_rect)
                    .layout(Layout::top_down_justified(Align::Min)),
                |ui| card_fn(ui, loaded, config, selected),
            );
            seek_request = seek_request.or(res.inner);
        }

        // Clear the hint so it doesn't leak to unrelated widgets
        // (e.g. help popups) painted later in the frame.
        ui.ctx()
            .data_mut(|d| d.insert_temp::<f32>(min_inner_id, 0.0));

        cursor_y += row_h + SPACE_M;
    }

    ui.allocate_space(vec2(total_w, cursor_y));

    seek_request
}
