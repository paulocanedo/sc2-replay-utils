//! Hero KPI strip rendered at the top of the Library central panel.
//!
//! Reads a pre-computed `LibraryStats` snapshot (from `ReplayLibrary::stats()`)
//! and renders 2-5 clickable cards. Each click yields a `HeroAction` that the
//! caller applies to `LibraryFilter`/sort — the hero itself is stateless.
//!
//! When `config.user_nicknames` is empty, only Total + Top Map render.

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, Ui};

use crate::colors::{ACCENT_DANGER, ACCENT_SUCCESS, LABEL_DIM, LABEL_DIMMER, LABEL_SOFT, LABEL_STRONG};
use crate::config::AppConfig;
use crate::locale::t;
use crate::tokens::{CARD_INNER_MY, RADIUS_CARD, SPACE_M, size_caption, size_title};
use crate::widgets::card;

use super::stats::LibraryStats;

pub enum HeroAction {
    ClearFilters,
    FilterWins,
    SortByDateDesc,
    SetSearch(String),
}

pub fn show(ui: &mut Ui, stats: &LibraryStats, config: &AppConfig) -> Option<HeroAction> {
    let lang = config.language;
    let has_nicknames = !config.user_nicknames.is_empty();
    let mut action: Option<HeroAction> = None;

    // Cards stay on a single row with top-aligned, equal-height frames.
    // `ui.horizontal` defaults to vertical-center alignment, which made
    // each card jump to a different baseline when their heights diverged.
    // `horizontal_top` pins every child to the same top Y instead.
    let row_gap = ui.spacing().item_spacing.y;
    let content_h = size_caption(config)
        + row_gap
        + size_title(config)
        + row_gap
        + size_caption(config);
    // Frame inner_margin (top + bottom) + outer stroke (1px).
    let card_h = content_h + CARD_INNER_MY as f32 * 2.0 + 2.0;

    ui.horizontal_top(|ui| {
        let n_cards: usize = if has_nicknames { 5 } else { 2 };
        let gap = SPACE_M;
        let avail = ui.available_width();
        let width = ((avail - gap * (n_cards as f32 - 1.0)) / n_cards as f32).max(120.0);

        if kpi_card(
            ui,
            width,
            card_h,
            content_h,
            t("library.hero.total", lang),
            &stats.total_parsed.to_string(),
            None,
            None,
            config,
        ) {
            action = Some(HeroAction::ClearFilters);
        }

        if has_nicknames {
            let wr_label = match stats.winrate_global {
                Some(wr) => format!("{:.1}%", wr * 100.0),
                None => "—".into(),
            };
            let wr_sub = format!(
                "{} {} · {} {}",
                stats.wins,
                t("library.hero.wins_suffix", lang),
                stats.losses,
                t("library.hero.losses_suffix", lang),
            );
            if kpi_card(
                ui,
                width,
                card_h,
                content_h,
                t("library.hero.winrate", lang),
                &wr_label,
                Some(&wr_sub),
                winrate_color(stats.winrate_global),
                config,
            ) {
                action = Some(HeroAction::FilterWins);
            }

            let mmr_label = match stats.mmr_latest {
                Some(m) => m.to_string(),
                None => "—".into(),
            };
            // Use ASCII-safe prefixes — unicode arrows (↑ ↓ →) are missing
            // from egui's default fallback font on Windows and render as □.
            let trend_text: Option<String> = match stats.mmr_trend_delta {
                Some(d) if d > 0 => Some(format!("+{d}")),
                Some(d) if d < 0 => Some(d.to_string()),
                Some(_) => Some("0".into()),
                None => None,
            };
            if kpi_card(
                ui,
                width,
                card_h,
                content_h,
                t("library.hero.mmr", lang),
                &mmr_label,
                trend_text.as_deref(),
                None,
                config,
            ) {
                action = Some(HeroAction::SortByDateDesc);
            }

            let bm_code = stats.best_matchup.clone().unwrap_or_else(|| "—".into());
            let bm_sub: Option<String> = stats.best_matchup.as_ref().and_then(|code| {
                stats
                    .matchup_winrates
                    .iter()
                    .find(|m| &m.code == code)
                    .map(|m| format!("{:.0}% · {} g", m.winrate() * 100.0, m.n))
            });
            if kpi_card(
                ui,
                width,
                card_h,
                content_h,
                t("library.hero.best_matchup", lang),
                &bm_code,
                bm_sub.as_deref(),
                None,
                config,
            ) && let Some(code) = stats.best_matchup.clone()
            {
                action = Some(HeroAction::SetSearch(code));
            }
        }

        let (top_map_label, top_map_sub) = match stats.top_maps.first() {
            Some((m, n)) => (m.clone(), Some(format!("{n} g"))),
            None => ("—".into(), None),
        };
        if kpi_card(
            ui,
            width,
            card_h,
            content_h,
            t("library.hero.top_map", lang),
            &top_map_label,
            top_map_sub.as_deref(),
            None,
            config,
        ) && let Some((m, _)) = stats.top_maps.first()
        {
            action = Some(HeroAction::SetSearch(m.clone()));
        }
    });

    action
}

#[allow(clippy::too_many_arguments)]
fn kpi_card(
    ui: &mut Ui,
    width: f32,
    card_h: f32,
    content_h: f32,
    title: &str,
    value: &str,
    subtitle: Option<&str>,
    value_color: Option<Color32>,
    config: &AppConfig,
) -> bool {
    let resp = ui
        .allocate_ui_with_layout(
            egui::vec2(width, card_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                card(ui, None, |ui| {
                    // Lock the vertical flow to `content_h` so a card with
                    // no subtitle (TOTAL) doesn't shrink relative to a
                    // card with one (WINRATE/MMR/…). Keeps the strip on
                    // a single visual baseline.
                    ui.set_min_height(content_h);
                    ui.label(
                        RichText::new(title)
                            .size(size_caption(config))
                            .color(LABEL_DIMMER)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(value)
                            .size(size_title(config))
                            .color(value_color.unwrap_or(LABEL_STRONG))
                            .strong(),
                    );
                    if let Some(sub) = subtitle {
                        ui.label(
                            RichText::new(sub)
                                .size(size_caption(config))
                                .color(LABEL_DIM),
                        );
                    }
                })
                .response
            },
        )
        .inner;

    let resp = resp.interact(Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            RADIUS_CARD,
            Stroke::new(1.0, LABEL_SOFT),
            StrokeKind::Outside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn winrate_color(winrate: Option<f32>) -> Option<Color32> {
    winrate.map(|wr| {
        if wr >= 0.55 {
            ACCENT_SUCCESS
        } else if wr >= 0.45 {
            LABEL_STRONG
        } else {
            ACCENT_DANGER
        }
    })
}
