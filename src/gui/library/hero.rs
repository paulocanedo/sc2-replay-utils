//! Hero KPI strip rendered at the top of the Library central panel.
//!
//! Reads a pre-computed `LibraryStats` snapshot (from `ReplayLibrary::stats()`)
//! and renders 2-5 clickable cards. Each click yields a `HeroAction` that the
//! caller applies to `LibraryFilter`/sort — the hero itself is stateless.
//!
//! When `config.user_nicknames` is empty, only Total + Top Map render.

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, Ui};

use crate::colors::{
    ACCENT_DANGER, ACCENT_SUCCESS, ACCENT_WARNING, FOCUS_RING, LABEL_DIM, LABEL_DIMMER, LABEL_SOFT,
    LABEL_STRONG, race_color,
};
use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::tokens::{CARD_INNER_MY, RADIUS_CARD, SPACE_M, size_caption, size_title};
use crate::widgets::card;

use super::filter::DateRange;
use super::stats::{LibraryStats, MMR_TREND_WINDOW};

pub enum HeroAction {
    ClearFilters,
    FilterWins,
    SortByDateDesc,
    SetSearch(String),
}

pub fn show(
    ui: &mut Ui,
    stats: &LibraryStats,
    config: &AppConfig,
    date_range: DateRange,
) -> Option<HeroAction> {
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

        // TOTAL's subtitle mirrors the active date-range filter. Without
        // it the card has no third line and sits shorter than the others
        // (WINRATE/MMR/…), breaking the strip's vertical rhythm. Reusing
        // the existing sidebar date labels keeps terminology consistent.
        let total_sub = date_range_label(date_range, lang).to_lowercase();
        if kpi_card(
            ui,
            width,
            card_h,
            content_h,
            t("library.hero.total", lang),
            &stats.total_parsed.to_string(),
            Some(&total_sub),
            None,
            None,
            None,
            Some(FOCUS_RING),
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
            let wr_accent = winrate_color(stats.winrate_global);
            if kpi_card(
                ui,
                width,
                card_h,
                content_h,
                t("library.hero.winrate", lang),
                &wr_label,
                Some(&wr_sub),
                wr_accent,
                None,
                None,
                wr_accent,
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
            //
            // The raw delta ("+74") is hard to read without its window.
            // Compose a self-explanatory subtitle like "+74 · last 7 games"
            // and tint it green/red so the direction reads at a glance.
            let window_s = MMR_TREND_WINDOW.to_string();
            let (trend_text, trend_color): (Option<String>, Option<Color32>) =
                match stats.mmr_trend_delta {
                    Some(d) if d > 0 => (
                        Some(tf(
                            "library.hero.mmr_trend_up",
                            lang,
                            &[("delta", &d.to_string()), ("window", &window_s)],
                        )),
                        Some(ACCENT_SUCCESS),
                    ),
                    Some(d) if d < 0 => (
                        Some(tf(
                            "library.hero.mmr_trend_down",
                            lang,
                            &[("delta", &d.to_string()), ("window", &window_s)],
                        )),
                        Some(ACCENT_DANGER),
                    ),
                    Some(_) => (
                        Some(tf(
                            "library.hero.mmr_trend_flat",
                            lang,
                            &[("window", &window_s)],
                        )),
                        None,
                    ),
                    None => (None, None),
                };
            let trend_tooltip = tf(
                "library.hero.mmr_trend_tooltip",
                lang,
                &[("window", &window_s)],
            );
            if kpi_card(
                ui,
                width,
                card_h,
                content_h,
                t("library.hero.mmr", lang),
                &mmr_label,
                trend_text.as_deref(),
                None,
                trend_color,
                Some(&trend_tooltip),
                trend_color,
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
            // Accent stripe = opponent race colour. `best_matchup` is
            // "XvY" where X is the user's race and Y is the opponent —
            // tint by the one that defines the matchup's identity.
            let bm_accent = stats
                .best_matchup
                .as_ref()
                .and_then(|c| c.chars().last())
                .map(|c| race_color(&c.to_string()));
            if kpi_card(
                ui,
                width,
                card_h,
                content_h,
                t("library.hero.best_matchup", lang),
                &bm_code,
                bm_sub.as_deref(),
                None,
                None,
                None,
                bm_accent,
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
            None,
            None,
            Some(ACCENT_WARNING),
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
    subtitle_color: Option<Color32>,
    tooltip: Option<&str>,
    accent: Option<Color32>,
    config: &AppConfig,
) -> bool {
    let resp = ui
        .allocate_ui_with_layout(
            egui::vec2(width, card_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                card(ui, accent, |ui| {
                    // Lock the vertical flow to `content_h` so a card with
                    // no subtitle (TOTAL) doesn't shrink relative to a
                    // card with one (WINRATE/MMR/…). Keeps the strip on
                    // a single visual baseline.
                    //
                    // Labels use `.truncate()` so long values (e.g. the
                    // map name "10000 Feet LE" in a narrow card) don't
                    // wrap to a second line and make that one card taller
                    // than the others.
                    ui.set_min_height(content_h);
                    ui.add(
                        egui::Label::new(
                            RichText::new(title)
                                .size(size_caption(config))
                                .color(LABEL_DIMMER)
                                .strong(),
                        )
                        .truncate(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(value)
                                .size(size_title(config))
                                .color(value_color.unwrap_or(LABEL_STRONG))
                                .strong(),
                        )
                        .truncate(),
                    );
                    if let Some(sub) = subtitle {
                        ui.add(
                            egui::Label::new(
                                RichText::new(sub)
                                    .size(size_caption(config))
                                    .color(subtitle_color.unwrap_or(LABEL_DIM)),
                            )
                            .truncate(),
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
    let resp = if let Some(tip) = tooltip {
        resp.on_hover_text(tip)
    } else {
        resp
    };
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

fn date_range_label(range: DateRange, lang: crate::locale::Language) -> &'static str {
    match range {
        DateRange::All => t("library.date.always_full", lang),
        DateRange::Today => t("library.date.today_full", lang),
        DateRange::ThisWeek => t("library.date.this_week", lang),
        DateRange::ThisMonth => t("library.date.this_month", lang),
    }
}
