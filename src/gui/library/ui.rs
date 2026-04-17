//! Render egui da biblioteca + ação solicitada pelo usuário.

use std::path::{Path, PathBuf};

use egui::{Color32, Context, RichText, ScrollArea, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::tokens::SPACE_S;

use super::date::{matches_date_range, today_str};
use super::entry_row::*;
use super::filter::{DateRange, LibraryFilter, OutcomeFilter, SortOrder};
use super::hero::{self, HeroAction};
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

    // Header chrome (title + folder path + reload/pick/rename icons) and
    // the filter sidebar (search/chips/sort) live in app-level panels.
    // This function renders only: hero KPI strip, status, and the
    // virtualized entry list.

    // ── Hero KPI strip ───────────────────────────────────────────────
    if let Some(stats) = library.stats() {
        if stats.total_parsed > 0 {
            if let Some(ha) = hero::show(ui, stats, config) {
                match ha {
                    HeroAction::ClearFilters => {
                        filter.search.clear();
                        filter.race = None;
                        filter.outcome = OutcomeFilter::All;
                        let prev_range = filter.date_range;
                        filter.date_range = DateRange::All;
                        if prev_range != DateRange::All {
                            action = LibraryAction::SaveDateRange(DateRange::All);
                        }
                    }
                    HeroAction::FilterWins => {
                        filter.outcome = if filter.outcome == OutcomeFilter::Wins {
                            OutcomeFilter::All
                        } else {
                            OutcomeFilter::Wins
                        };
                    }
                    HeroAction::SortByDateDesc => {
                        filter.sort = SortOrder::Date;
                        filter.sort_ascending = false;
                    }
                    HeroAction::SetSearch(s) => {
                        filter.search = s;
                    }
                }
            }
            ui.add_space(SPACE_S);
        }
    }

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

/// Helper para a `app.rs` pedir repaint quando houver trabalho em andamento.
pub fn keep_alive(ctx: &Context, library: &ReplayLibrary) {
    if library.scanning {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    } else if library.pending_count() > 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}
