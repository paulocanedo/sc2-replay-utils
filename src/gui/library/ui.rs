//! Render egui da biblioteca + ação solicitada pelo usuário.

use std::path::{Path, PathBuf};

use egui::{Color32, Context, RichText, ScrollArea, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::tokens::SPACE_S;

use super::date::today_str;
use super::entry_row::*;
use super::filter::{DateRange, LibraryFilter, OutcomeFilter, SortOrder, matches_filter};
use super::hero::{self, HeroAction};
use super::scanner::ReplayLibrary;
use crate::widgets::removable_chip;

/// Ação solicitada pelo usuário ao interagir com o painel.
pub enum LibraryAction {
    None,
    Load(PathBuf),
    /// Clique simples — seleciona a entrada (alimenta o card lateral)
    /// sem disparar o parse pesado do `Load`.
    Select(PathBuf),
    /// Limpa a seleção atual (botão × no card de detalhes).
    ClearSelection,
    Refresh,
    PickWorkingDir(PathBuf),
    OpenRename,
    SaveDateRange(DateRange),
}

/// Renderiza o hero (KPI strip clicável). Extraído da `show` principal
/// para que o `central.rs` consiga colocá-lo num `Panel::top` que ocupa
/// toda a largura restante depois do filtro lateral — assim o card de
/// detalhes (na direita) só rouba largura da lista, nunca do hero.
///
/// Devolve `LibraryAction::None` quando o usuário não interagiu, ou a
/// ação correspondente ao chip clicado (`SaveDateRange` quando limpa
/// filtros e havia date range ativo, etc.). Nada é renderizado se a
/// biblioteca ainda não tem stats ou está vazia.
pub fn show_hero(
    ui: &mut Ui,
    library: &ReplayLibrary,
    config: &AppConfig,
    filter: &mut LibraryFilter,
) -> LibraryAction {
    let mut action = LibraryAction::None;
    let Some(stats) = library.stats() else { return action };
    if stats.total_parsed == 0 {
        return action;
    }
    if let Some(ha) = hero::show(ui, stats, config, filter.date_range) {
        match ha {
            HeroAction::ClearFilters => {
                filter.search.clear();
                filter.race = None;
                filter.outcome = OutcomeFilter::All;
                filter.opponent_name = None;
                filter.matchup_code = None;
                filter.map_name = None;
                filter.opening = None;
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
    action
}

pub fn show(
    ui: &mut Ui,
    library: &ReplayLibrary,
    current_path: Option<&Path>,
    selected_path: Option<&Path>,
    config: &AppConfig,
    filter: &mut LibraryFilter,
) -> LibraryAction {
    let mut action = LibraryAction::None;
    let lang = config.language;

    // Header chrome (title + folder path + reload/pick/rename icons) and
    // the filter sidebar (search/chips/sort) live in app-level panels.
    // The hero KPI strip is now rendered by `show_hero` from `central.rs`
    // (so it can span the full width above the detail card). This
    // function renders only: status, related-filter chips, and the
    // virtualized entry list.

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

    // ── Chips de "relacionados" ──────────────────────────────────────
    // Cada chip é cancelável; clicar limpa apenas aquele campo. Ficam
    // acima do status "X de Y" para dar contexto imediato do filtro
    // ativo vindo do menu de contexto.
    let has_related = filter.opponent_name.is_some()
        || filter.matchup_code.is_some()
        || filter.map_name.is_some()
        || filter.opening.is_some();
    if has_related {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_S;
            if let Some(name) = filter.opponent_name.clone() {
                let label = tf(
                    "library.related.chip.vs_opponent",
                    lang,
                    &[("name", &name)],
                );
                if removable_chip(ui, &label, config).clicked() {
                    filter.opponent_name = None;
                }
            }
            if let Some(code) = filter.matchup_code.clone() {
                let label = tf("library.related.chip.matchup", lang, &[("code", &code)]);
                if removable_chip(ui, &label, config).clicked() {
                    filter.matchup_code = None;
                }
            }
            if let Some(map) = filter.map_name.clone() {
                let label = tf("library.related.chip.map", lang, &[("map", &map)]);
                if removable_chip(ui, &label, config).clicked() {
                    filter.map_name = None;
                }
            }
            if let Some(op) = filter.opening.clone() {
                let label = tf("library.related.chip.opening", lang, &[("opening", &op)]);
                if removable_chip(ui, &label, config).clicked() {
                    filter.opening = None;
                }
            }
        });
        ui.add_space(SPACE_S);
    }

    // ── Filtragem ────────────────────────────────────────────────────
    let any_filter_active = !filter.search.trim().is_empty()
        || filter.race.is_some()
        || filter.outcome != OutcomeFilter::All
        || filter.date_range != DateRange::All
        || filter.opponent_name.is_some()
        || filter.matchup_code.is_some()
        || filter.map_name.is_some()
        || filter.opening.is_some();

    let today = today_str();

    let mut visible: Vec<usize> = library
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| matches_filter(e, filter, config, &today))
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
    // `max_height` is a belt-and-suspenders bound: with `auto_shrink=false`
    // the ScrollArea tries to fill all available space, and historically we
    // saw it grow past the bottom `Panel::bottom` strip on the first frame
    // (before `PanelState` caches the status bar height). Capping at the
    // current `ui.available_height()` guarantees the list never paints on
    // top of the status bar, regardless of what egui's panel sizer does
    // that frame.
    let row_h = row_height(ui);
    let max_scroll_h = ui.available_height().max(0.0);
    ScrollArea::vertical()
        .id_salt("library_list")
        .auto_shrink([false, false])
        .max_height(max_scroll_h)
        .show_rows(ui, row_h, shown, |ui, row_range| {
            for virtual_idx in row_range {
                let idx = visible[virtual_idx];
                let entry = &library.entries[idx];
                let is_current = current_path.map_or(false, |cp| cp == entry.path);
                let is_selected = selected_path.map_or(false, |sp| sp == entry.path);
                match entry_row(ui, entry, is_current, is_selected, config, row_h) {
                    RowOutcome::None => {}
                    RowOutcome::Select => action = LibraryAction::Select(entry.path.clone()),
                    RowOutcome::Load => action = LibraryAction::Load(entry.path.clone()),
                    RowOutcome::ApplyRelated(RelatedFilter::Opponent(n)) => {
                        filter.opponent_name = Some(n);
                    }
                    RowOutcome::ApplyRelated(RelatedFilter::Matchup(c)) => {
                        filter.matchup_code = Some(c);
                    }
                    RowOutcome::ApplyRelated(RelatedFilter::Map(m)) => {
                        filter.map_name = Some(m);
                    }
                    RowOutcome::ApplyRelated(RelatedFilter::Opening(o)) => {
                        filter.opening = Some(o);
                    }
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
