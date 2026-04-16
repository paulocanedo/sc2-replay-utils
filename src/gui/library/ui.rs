//! Render egui da biblioteca + ação solicitada pelo usuário.

use std::path::{Path, PathBuf};

use egui::{Color32, Context, RichText, ScrollArea, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};

use super::date::{matches_date_range, today_str};
use super::entry_row::*;
use super::filter::{DateRange, LibraryFilter, OutcomeFilter, SortOrder};
use super::scanner::ReplayLibrary;
use super::types::MetaState;

/// Ação solicitada pelo usuário ao interagir com o painel.
pub enum LibraryAction {
    None,
    Load(PathBuf),
    Refresh,
    PickWorkingDir(PathBuf),
    OpenRename,
    SaveDateRange(DateRange),
}

pub fn show(
    ui: &mut Ui,
    library: &ReplayLibrary,
    current_path: Option<&Path>,
    config: &AppConfig,
    filter: &mut LibraryFilter,
) -> LibraryAction {
    let mut action = LibraryAction::None;
    let lang = config.language;

    // ── Header ───────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.heading(t("library.title", lang));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("↻")
                .on_hover_text(t("library.reload_tooltip", lang))
                .clicked()
            {
                action = LibraryAction::Refresh;
            }
            if ui
                .small_button("🔎")
                .on_hover_text(t("library.zoom_tooltip", lang))
                .clicked()
            {}
            if ui
                .small_button("✏")
                .on_hover_text(t("library.rename_tooltip", lang))
                .clicked()
            {
                action = LibraryAction::OpenRename;
            }
            if ui
                .small_button("📂")
                .on_hover_text(t("library.pick_dir_tooltip", lang))
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    action = LibraryAction::PickWorkingDir(p);
                }
            }
        });
    });

    match library.working_dir.as_ref() {
        Some(dir) => {
            ui.small(
                RichText::new(format!("📁 {}", dir.display()))
                    .color(Color32::from_gray(120)),
            );
        }
        None => {
            ui.small(RichText::new(t("library.dir_unset", lang)).italics());
        }
    }

    ui.add_space(4.0);

    // ── Barra de busca + contagem/sort ───────────────────────────────
    ui.horizontal(|ui| {
        ui.label("🔎");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut filter.search)
                .hint_text(t("library.search_placeholder", lang))
                .desired_width(ui.available_width() - 150.0),
        );
        if !filter.search.is_empty() && resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            filter.search.clear();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let total = library.entries.len();
            egui::ComboBox::from_id_salt("library_sort")
                .selected_text(tf(
                    "library.sort.total_count",
                    lang,
                    &[("total", &total.to_string())],
                ))
                .width(120.0)
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
                    ui.separator();
                    let asc_label = if filter.sort_ascending {
                        t("library.sort.ascending_marked", lang)
                    } else {
                        t("library.sort.ascending_unmarked", lang)
                    };
                    let desc_label = if !filter.sort_ascending {
                        t("library.sort.descending_marked", lang)
                    } else {
                        t("library.sort.descending_unmarked", lang)
                    };
                    if ui.selectable_label(filter.sort_ascending, asc_label).clicked() {
                        filter.sort_ascending = true;
                    }
                    if ui
                        .selectable_label(!filter.sort_ascending, desc_label)
                        .clicked()
                    {
                        filter.sort_ascending = false;
                    }
                });
        });
    });

    ui.add_space(2.0);

    // ── Chips de filtro rápido ────────────────────────────────────────
    let has_nicknames = !config.user_nicknames.is_empty();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        let todos_active = filter.race.is_none()
            && filter.outcome == OutcomeFilter::All
            && filter.date_range == DateRange::All;
        if chip(ui, t("library.filter.all", lang), todos_active, None).clicked() {
            filter.race = None;
            filter.outcome = OutcomeFilter::All;
            filter.date_range = DateRange::All;
        }

        ui.add_space(4.0);

        for (label, letter, color) in [
            (t("race.terran", lang), 'T', RACE_COLOR_TERRAN),
            (t("race.protoss", lang), 'P', RACE_COLOR_PROTOSS),
            (t("race.zerg", lang), 'Z', RACE_COLOR_ZERG),
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

        ui.add_space(4.0);

        let wins_selected = filter.outcome == OutcomeFilter::Wins;
        let resp = chip(
            ui,
            t("library.filter.wins", lang),
            wins_selected,
            Some(Color32::from_rgb(80, 180, 80)),
        );
        if resp.clicked() && has_nicknames {
            filter.outcome = if wins_selected { OutcomeFilter::All } else { OutcomeFilter::Wins };
        }
        if !has_nicknames {
            resp.on_hover_text(t("library.filter.nicknames_outcome_tooltip", lang));
        }

        let losses_selected = filter.outcome == OutcomeFilter::Losses;
        let resp = chip(
            ui,
            t("library.filter.losses", lang),
            losses_selected,
            Some(Color32::from_rgb(180, 80, 80)),
        );
        if resp.clicked() && has_nicknames {
            filter.outcome = if losses_selected { OutcomeFilter::All } else { OutcomeFilter::Losses };
        }
        if !has_nicknames {
            resp.on_hover_text(t("library.filter.nicknames_outcome_tooltip", lang));
        }

        ui.add_space(4.0);

        let prev_date_range = filter.date_range;
        let date_label = match filter.date_range {
            DateRange::All => t("library.date.always", lang),
            DateRange::Today => t("library.date.today", lang),
            DateRange::ThisWeek => t("library.date.week", lang),
            DateRange::ThisMonth => t("library.date.month", lang),
        };
        let date_active = filter.date_range != DateRange::All;
        let date_text_color = if date_active { Color32::WHITE } else { Color32::from_gray(160) };
        egui::ComboBox::from_id_salt("date_range_chip")
            .selected_text(RichText::new(date_label).color(date_text_color).small())
            .width(80.0)
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
    });

    ui.add_space(2.0);

    // ── Status ───────────────────────────────────────────────────────
    if library.scanning {
        ui.small(
            RichText::new(tf(
                "library.scanning",
                lang,
                &[("found", &library.entries.len().to_string())],
            ))
            .italics(),
        );
    } else {
        let pending = library.pending_count();
        if pending > 0 {
            ui.small(tf(
                "library.parsing",
                lang,
                &[
                    ("pending", &pending.to_string()),
                    ("total", &library.entries.len().to_string()),
                ],
            ));
        }
    }

    ui.separator();

    if library.entries.is_empty() && library.working_dir.is_none() {
        ui.add_space(12.0);
        ui.label(RichText::new(t("library.setup_hint", lang)).italics());
        return action;
    }

    // ── Filtragem ────────────────────────────────────────────────────
    let needle = filter.search.trim().to_ascii_lowercase();
    let search_active = !needle.is_empty();
    let any_filter_active = search_active
        || filter.race.is_some()
        || filter.outcome != OutcomeFilter::All
        || filter.date_range != DateRange::All;

    let today = today_str();

    let mut visible: Vec<usize> = library
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| match &e.meta {
            MetaState::Parsed(meta) => {
                if search_active {
                    let name_match = meta
                        .players
                        .iter()
                        .any(|p| p.name.to_ascii_lowercase().contains(&needle));
                    let map_match = meta.map.to_ascii_lowercase().contains(&needle);
                    let mc = matchup_code(meta, config);
                    let matchup_match = mc.to_ascii_lowercase().contains(&needle);
                    if !(name_match || map_match || matchup_match) {
                        return false;
                    }
                }
                if let Some(race_ch) = filter.race {
                    let user = find_user_player(meta, config);
                    let matches = user
                        .map_or(false, |p| race_letter(&p.race) == race_ch);
                    if !matches {
                        return false;
                    }
                }
                match filter.outcome {
                    OutcomeFilter::All => {}
                    OutcomeFilter::Wins => {
                        let won = find_user_player(meta, config)
                            .map_or(false, |p| p.result == "Win");
                        if !won {
                            return false;
                        }
                    }
                    OutcomeFilter::Losses => {
                        let lost = find_user_player(meta, config)
                            .map_or(false, |p| p.result == "Loss");
                        if !lost {
                            return false;
                        }
                    }
                }
                if !matches_date_range(&meta.datetime, filter.date_range, &today) {
                    return false;
                }
                true
            }
            _ => !any_filter_active,
        })
        .map(|(i, _)| i)
        .collect();

    // ── Ordenação ────────────────────────────────────────────────────
    match filter.sort {
        SortOrder::Date => {
            // Já ordenado por mtime no entries vec. Se ascendente, inverter.
            if filter.sort_ascending {
                visible.reverse();
            }
        }
        SortOrder::Duration => {
            visible.sort_by(|&a, &b| {
                let da = get_duration(&library.entries[a]);
                let db = get_duration(&library.entries[b]);
                if filter.sort_ascending { da.cmp(&db) } else { db.cmp(&da) }
            });
        }
        SortOrder::Mmr => {
            visible.sort_by(|&a, &b| {
                let ma = get_user_mmr(&library.entries[a], config);
                let mb = get_user_mmr(&library.entries[b], config);
                if filter.sort_ascending { ma.cmp(&mb) } else { mb.cmp(&ma) }
            });
        }
        SortOrder::Map => {
            visible.sort_by(|&a, &b| {
                let ma = get_map(&library.entries[a]);
                let mb = get_map(&library.entries[b]);
                if filter.sort_ascending { ma.cmp(mb) } else { mb.cmp(ma) }
            });
        }
    }

    let shown = visible.len();

    if any_filter_active && shown == 0 {
        ui.add_space(8.0);
        ui.label(
            RichText::new(t("library.no_match", lang))
                .italics()
                .color(Color32::from_gray(160)),
        );
        return action;
    }

    if any_filter_active {
        ui.small(
            RichText::new(tf(
                "library.filter_status",
                lang,
                &[
                    ("shown", &shown.to_string()),
                    ("total", &library.entries.len().to_string()),
                ],
            ))
            .color(Color32::from_gray(140)),
        );
    }

    // ── Lista virtualizada ───────────────────────────────────────────
    let row_h = row_height(ui);
    ScrollArea::vertical()
        .id_salt("library_list")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, shown, |ui, row_range| {
            for virtual_idx in row_range {
                let idx = visible[virtual_idx];
                let entry = &library.entries[idx];
                let is_current = current_path.map_or(false, |cp| cp == entry.path);
                if entry_row(ui, entry, is_current, config, row_h) {
                    action = LibraryAction::Load(entry.path.clone());
                }
            }
        });

    action
}

// ── UI components ────────────────────────────────────────────────────

fn chip(ui: &mut Ui, label: &str, selected: bool, accent: Option<Color32>) -> egui::Response {
    let fill = if selected {
        accent.map_or(Color32::from_rgb(55, 75, 55), |c| {
            Color32::from_rgb(
                (c.r() as u16 / 3) as u8 + 20,
                (c.g() as u16 / 3) as u8 + 20,
                (c.b() as u16 / 3) as u8 + 20,
            )
        })
    } else {
        Color32::from_gray(40)
    };
    let text_color = if selected {
        Color32::WHITE
    } else {
        Color32::from_gray(160)
    };

    let icon = label.to_string();

    ui.add(
        egui::Button::new(RichText::new(icon).color(text_color).small())
            .fill(fill)
            .corner_radius(12.0),
    )
}

/// Helper para a `app.rs` pedir repaint quando houver trabalho em andamento.
pub fn keep_alive(ctx: &Context, library: &ReplayLibrary) {
    if library.scanning {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    } else if library.pending_count() > 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}
