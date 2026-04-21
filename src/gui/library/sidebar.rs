//! Left filter sidebar. Owns search + race chips + outcome + date range +
//! sort + clear-all + insights (matchup winrate list, top maps). Mutates
//! the caller's `LibraryFilter` in place and returns a `LibraryAction`
//! (only non-`None` for `SaveDateRange`, which the app persists).

use egui::{Align, Color32, ComboBox, CornerRadius, Layout, Rect, RichText, Sense, Ui};

use crate::colors::{
    ACCENT_DANGER, ACCENT_SUCCESS, LABEL_DIM, LABEL_DIMMER, LABEL_STRONG, RACE_PROTOSS,
    RACE_TERRAN, RACE_ZERG,
};
use crate::config::AppConfig;
use crate::locale::t;
use crate::tokens::{SPACE_M, SPACE_S, SPACE_XS, size_caption};
use crate::widgets::chip;

use super::filter::{DateRange, LibraryFilter, OutcomeFilter, SortOrder};
use super::stats::LibraryStats;
use super::ui::LibraryAction;

pub fn show(
    ui: &mut Ui,
    filter: &mut LibraryFilter,
    stats: Option<&LibraryStats>,
    config: &AppConfig,
) -> LibraryAction {
    let lang = config.language;
    let mut action = LibraryAction::None;
    let has_nicknames = !config.user_nicknames.is_empty();

    section_header(ui, t("library.sidebar.search", lang), config);
    ui.horizontal(|ui| {
        ui.label("🔎");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut filter.search)
                .hint_text(t("library.search_placeholder", lang))
                .desired_width(ui.available_width()),
        );
        if !filter.search.is_empty() && resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            filter.search.clear();
        }
    });

    ui.add_space(SPACE_M);

    section_header(ui, t("library.sidebar.race", lang), config);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        for (label, letter, color) in [
            (t("race.terran", lang), 'T', RACE_TERRAN),
            (t("race.protoss", lang), 'P', RACE_PROTOSS),
            (t("race.zerg", lang), 'Z', RACE_ZERG),
        ] {
            let selected = filter.race == Some(letter);
            let resp = chip(ui, label, selected, Some(color));
            if resp.clicked() && has_nicknames {
                filter.race = if selected { None } else { Some(letter) };
            }
            if !has_nicknames {
                resp.on_hover_text(t("library.filter.nicknames_race_tooltip", lang));
            }
        }
    });

    ui.add_space(SPACE_M);

    section_header(ui, t("library.sidebar.outcome", lang), config);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;

        let wins_selected = filter.outcome == OutcomeFilter::Wins;
        let resp = chip(
            ui,
            t("library.filter.wins", lang),
            wins_selected,
            Some(ACCENT_SUCCESS),
        );
        if resp.clicked() && has_nicknames {
            filter.outcome = if wins_selected {
                OutcomeFilter::All
            } else {
                OutcomeFilter::Wins
            };
        }
        if !has_nicknames {
            resp.on_hover_text(t("library.filter.nicknames_outcome_tooltip", lang));
        }

        let losses_selected = filter.outcome == OutcomeFilter::Losses;
        let resp = chip(
            ui,
            t("library.filter.losses", lang),
            losses_selected,
            Some(ACCENT_DANGER),
        );
        if resp.clicked() && has_nicknames {
            filter.outcome = if losses_selected {
                OutcomeFilter::All
            } else {
                OutcomeFilter::Losses
            };
        }
        if !has_nicknames {
            resp.on_hover_text(t("library.filter.nicknames_outcome_tooltip", lang));
        }
    });

    ui.add_space(SPACE_M);

    section_header(ui, t("library.sidebar.date_range", lang), config);
    let prev_date_range = filter.date_range;
    let date_label = date_range_label(filter.date_range, config);
    ComboBox::from_id_salt("sidebar_date_range")
        .selected_text(date_label)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut filter.date_range,
                DateRange::All,
                t("library.date.always_full", lang),
            );
            ui.selectable_value(
                &mut filter.date_range,
                DateRange::Today,
                t("library.date.today_full", lang),
            );
            ui.selectable_value(
                &mut filter.date_range,
                DateRange::ThisWeek,
                t("library.date.this_week", lang),
            );
            ui.selectable_value(
                &mut filter.date_range,
                DateRange::ThisMonth,
                t("library.date.this_month", lang),
            );
        });
    if filter.date_range != prev_date_range {
        action = LibraryAction::SaveDateRange(filter.date_range);
    }

    ui.add_space(SPACE_M);

    section_header(ui, t("library.sidebar.sort", lang), config);
    ComboBox::from_id_salt("sidebar_sort")
        .selected_text(sort_label(filter.sort, config))
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut filter.sort,
                SortOrder::Date,
                t("library.sort.date", lang),
            );
            ui.selectable_value(
                &mut filter.sort,
                SortOrder::Duration,
                t("library.sort.duration", lang),
            );
            ui.selectable_value(
                &mut filter.sort,
                SortOrder::Mmr,
                t("library.sort.mmr", lang),
            );
            ui.selectable_value(
                &mut filter.sort,
                SortOrder::Map,
                t("library.sort.map", lang),
            );
        });
    ui.horizontal(|ui| {
        if ui
            .selectable_label(filter.sort_ascending, t("library.sidebar.ascending", lang))
            .clicked()
        {
            filter.sort_ascending = true;
        }
        if ui
            .selectable_label(!filter.sort_ascending, t("library.sidebar.descending", lang))
            .clicked()
        {
            filter.sort_ascending = false;
        }
    });

    ui.add_space(SPACE_M);

    let any_filter = !filter.search.is_empty()
        || filter.race.is_some()
        || filter.outcome != OutcomeFilter::All
        || filter.date_range != DateRange::All
        || filter.opponent_name.is_some()
        || filter.matchup_code.is_some()
        || filter.map_name.is_some()
        || filter.opening.is_some();
    ui.add_enabled_ui(any_filter, |ui| {
        if ui.button(t("library.filter.clear_all", lang)).clicked() {
            filter.search.clear();
            filter.race = None;
            filter.outcome = OutcomeFilter::All;
            filter.opponent_name = None;
            filter.matchup_code = None;
            filter.map_name = None;
            filter.opening = None;
            let prev = filter.date_range;
            filter.date_range = DateRange::All;
            if prev != DateRange::All {
                action = LibraryAction::SaveDateRange(DateRange::All);
            }
        }
    });

    if let Some(stats) = stats {
        if has_nicknames && !stats.matchup_winrates.is_empty() {
            ui.add_space(SPACE_M);
            ui.separator();
            ui.add_space(SPACE_M);
            show_matchup_insights(ui, stats, config);
        }
        if !stats.top_maps.is_empty() {
            ui.add_space(SPACE_M);
            show_top_maps(ui, stats, config);
        }
    }

    action
}

fn section_header(ui: &mut Ui, label: &str, config: &AppConfig) {
    ui.label(
        RichText::new(label)
            .size(size_caption(config))
            .strong()
            .color(LABEL_DIMMER),
    );
    ui.add_space(2.0);
}

fn show_matchup_insights(ui: &mut Ui, stats: &LibraryStats, config: &AppConfig) {
    let lang = config.language;
    section_header(ui, t("library.sidebar.insights_matchup", lang), config);
    let size = size_caption(config);
    for m in &stats.matchup_winrates {
        ui.horizontal(|ui| {
            matchup_code_colored(ui, &m.code, size);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let wr = m.winrate() * 100.0;
                ui.label(
                    RichText::new(format!("{:.0}% · {}g", wr, m.n))
                        .size(size)
                        .color(winrate_color(wr)),
                );
            });
        });
        let bar_w = ui.available_width();
        winrate_bar(ui, bar_w, m.wins, m.n);
        ui.add_space(SPACE_XS);
    }
}

fn matchup_code_colored(ui: &mut Ui, code: &str, size: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for ch in code.chars() {
            let color = match ch {
                'T' => RACE_TERRAN,
                'P' => RACE_PROTOSS,
                'Z' => RACE_ZERG,
                'v' => LABEL_DIM,
                _ => LABEL_STRONG,
            };
            ui.label(RichText::new(ch).size(size).strong().color(color));
        }
    });
}

fn winrate_bar(ui: &mut Ui, width: f32, wins: usize, n: usize) {
    let h = 5.0;
    let r: u8 = 2;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), Sense::hover());
    let full = CornerRadius::same(r);
    ui.painter().rect_filled(rect, full, Color32::from_gray(40));
    if n == 0 {
        return;
    }
    let ratio = wins as f32 / n as f32;
    let split = rect.left() + width * ratio;
    let full_win = ratio >= 1.0;
    let full_loss = ratio <= 0.0;
    if ratio > 0.0 {
        let win_rect = Rect::from_min_max(rect.left_top(), egui::pos2(split, rect.bottom()));
        let radius = if full_win {
            full
        } else {
            CornerRadius { nw: r, sw: r, ne: 0, se: 0 }
        };
        ui.painter().rect_filled(win_rect, radius, ACCENT_SUCCESS);
    }
    if ratio < 1.0 {
        let loss_rect = Rect::from_min_max(egui::pos2(split, rect.top()), rect.right_bottom());
        let radius = if full_loss {
            full
        } else {
            CornerRadius { nw: 0, sw: 0, ne: r, se: r }
        };
        ui.painter().rect_filled(loss_rect, radius, ACCENT_DANGER);
    }
}

fn show_top_maps(ui: &mut Ui, stats: &LibraryStats, config: &AppConfig) {
    let lang = config.language;
    section_header(ui, t("library.sidebar.insights_maps", lang), config);
    for (map, n) in stats.top_maps.iter().take(3) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(truncate(map, 22))
                    .size(size_caption(config))
                    .color(LABEL_STRONG),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{n} g"))
                        .size(size_caption(config))
                        .color(LABEL_DIM),
                );
            });
        });
    }
}

fn winrate_color(wr: f32) -> Color32 {
    if wr >= 55.0 {
        ACCENT_SUCCESS
    } else if wr >= 45.0 {
        LABEL_STRONG
    } else {
        ACCENT_DANGER
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

fn date_range_label(r: DateRange, config: &AppConfig) -> String {
    let lang = config.language;
    match r {
        DateRange::All => t("library.date.always_full", lang).to_string(),
        DateRange::Today => t("library.date.today_full", lang).to_string(),
        DateRange::ThisWeek => t("library.date.this_week", lang).to_string(),
        DateRange::ThisMonth => t("library.date.this_month", lang).to_string(),
    }
}

fn sort_label(sort: SortOrder, config: &AppConfig) -> String {
    let lang = config.language;
    match sort {
        SortOrder::Date => t("library.sort.date", lang).to_string(),
        SortOrder::Duration => t("library.sort.duration", lang).to_string(),
        SortOrder::Mmr => t("library.sort.mmr", lang).to_string(),
        SortOrder::Map => t("library.sort.map", lang).to_string(),
    }
}
