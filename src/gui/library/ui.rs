//! Render egui da biblioteca + ação solicitada pelo usuário.

use std::collections::HashSet;
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
    /// Persiste filtros que sobrevivem entre sessões (date range + race).
    /// Sempre carrega o snapshot completo, pra não perder uma mudança quando
    /// duas acontecem no mesmo frame (ex.: botão "limpar tudo").
    SaveLibraryFilters {
        date_range: DateRange,
        race: Option<char>,
    },
    /// Alterna a marcação de uma entrada na seleção múltipla (checkbox
    /// na coluna de seleção).
    ToggleSelected(PathBuf),
    /// Substitui a seleção múltipla pelos paths fornecidos (Ctrl+A
    /// sobre as entradas atualmente visíveis).
    SetSelected(Vec<PathBuf>),
    /// Limpa a seleção múltipla (Ctrl+Shift+A ou botão "Clear").
    ClearSelected,
    /// Pede para copiar os replays atualmente marcados — o app abre o
    /// diálogo de pasta e executa a cópia. Os paths vivem em
    /// `AppState.library_selected`.
    CopySelected,
}

/// Renderiza o hero (KPI strip clicável). Extraído da `show` principal
/// para que o `central.rs` consiga colocá-lo num `Panel::top` que ocupa
/// toda a largura restante depois do filtro lateral — assim o card de
/// detalhes (na direita) só rouba largura da lista, nunca do hero.
///
/// Devolve `LibraryAction::None` quando o usuário não interagiu, ou a
/// ação correspondente ao chip clicado (`SaveLibraryFilters` quando
/// limpa filtros e havia date range ou race ativos, etc.). Nada é
/// renderizado se a biblioteca ainda não tem stats ou está vazia.
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
                let prev_race = filter.race;
                filter.race = None;
                filter.outcome = OutcomeFilter::All;
                filter.opponent_name = None;
                filter.matchup_code = None;
                filter.map_name = None;
                filter.opening = None;
                let prev_range = filter.date_range;
                filter.date_range = DateRange::All;
                if prev_range != DateRange::All || prev_race.is_some() {
                    action = LibraryAction::SaveLibraryFilters {
                        date_range: DateRange::All,
                        race: None,
                    };
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
    selected_set: &HashSet<PathBuf>,
    save_template: &mut String,
    config: &AppConfig,
    filter: &mut LibraryFilter,
) -> LibraryAction {
    let mut action = LibraryAction::None;
    let lang = config.language;

    // Header chrome (title + folder path + reload/pick icons) and
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

    // ── Atalhos de teclado da seleção múltipla ───────────────────────
    // Ctrl+A: marca tudo que está visível pelo filtro atual.
    // Ctrl+Shift+A: limpa a seleção. Capturados aqui (e não em
    // app/mod.rs) porque dependem do `visible` filtrado, que só existe
    // dentro deste escopo.
    if !visible.is_empty()
        && ui.input(|i| {
            i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::A)
        })
    {
        let paths: Vec<PathBuf> = visible
            .iter()
            .map(|&i| library.entries[i].path.clone())
            .collect();
        action = LibraryAction::SetSelected(paths);
    }
    if !selected_set.is_empty()
        && ui.input(|i| {
            i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::A)
        })
    {
        action = LibraryAction::ClearSelected;
    }

    // ── Toolbar de seleção ───────────────────────────────────────────
    // Sempre que há entradas visíveis, mostra ao menos o botão "Select
    // all" pra dar descoberta visual à feature (atalho Ctrl+A é
    // espelhado no tooltip). Quando há alguma marcação, expande pra
    // mostrar contagem, Clear, campo de template, ajuda e o botão
    // "Salvar como…".
    if !visible.is_empty() {
        ui.add_space(SPACE_S);
        ui.horizontal_wrapped(|ui| {
            let select_all_resp = ui
                .button(t("library.selection.select_all_button", lang))
                .on_hover_text(t("library.selection.select_all_tooltip", lang));
            if select_all_resp.clicked() {
                let paths: Vec<PathBuf> = visible
                    .iter()
                    .map(|&i| library.entries[i].path.clone())
                    .collect();
                action = LibraryAction::SetSelected(paths);
            }

            if !selected_set.is_empty() {
                ui.separator();
                ui.label(
                    RichText::new(tf(
                        "library.selection.toolbar",
                        lang,
                        &[("count", &selected_set.len().to_string())],
                    ))
                    .strong(),
                );
                if ui
                    .button(t("library.selection.clear_button", lang))
                    .on_hover_text(t("library.selection.clear_tooltip", lang))
                    .clicked()
                {
                    action = LibraryAction::ClearSelected;
                }
                ui.separator();
                ui.label(t("library.selection.template_label", lang));
                ui.add(
                    egui::TextEdit::singleline(save_template)
                        .desired_width(280.0)
                        .font(egui::TextStyle::Monospace),
                );
                template_help_popup(ui, lang);
                if ui
                    .button(RichText::new(t("library.selection.save_button", lang)).strong())
                    .clicked()
                {
                    action = LibraryAction::CopySelected;
                }
            }
        });
        ui.add_space(SPACE_S);
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
                let is_checked = selected_set.contains(&entry.path);
                match entry_row(ui, entry, is_current, is_selected, is_checked, config, row_h) {
                    RowOutcome::None => {}
                    RowOutcome::Select => action = LibraryAction::Select(entry.path.clone()),
                    RowOutcome::Load => action = LibraryAction::Load(entry.path.clone()),
                    RowOutcome::ToggleSelected => {
                        action = LibraryAction::ToggleSelected(entry.path.clone());
                    }
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

/// Botão de ajuda com popup explicando as variáveis do template de
/// "salvar como…". As chaves `library.template.*` são definidas em
/// `data/locale/{en,pt-BR}/ui.txt`.
fn template_help_popup(ui: &mut Ui, lang: crate::locale::Language) {
    let resp = ui
        .button("?")
        .on_hover_text(t("library.selection.help_tooltip", lang));
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(280.0);
            ui.label(RichText::new(t("library.template.vars_header", lang)).strong());
            ui.add_space(SPACE_S);
            egui::Grid::new("library_save_template_vars")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    let vars: [(&str, &str); 8] = [
                        ("{datetime}", t("library.template.var.datetime", lang)),
                        ("{map}", t("library.template.var.map", lang)),
                        ("{p1}", t("library.template.var.p1", lang)),
                        ("{p2}", t("library.template.var.p2", lang)),
                        ("{r1}", t("library.template.var.r1", lang)),
                        ("{r2}", t("library.template.var.r2", lang)),
                        ("{loops}", t("library.template.var.loops", lang)),
                        ("{duration}", t("library.template.var.duration", lang)),
                    ];
                    for (var, desc) in vars {
                        ui.monospace(var);
                        ui.label(desc);
                        ui.end_row();
                    }
                });
            ui.add_space(SPACE_S);
            ui.small(t("library.template.note_special", lang));
            ui.small(t("library.template.note_ext", lang));
            ui.add_space(SPACE_S);
            ui.small(
                RichText::new(t("library.selection.help_fallback_note", lang))
                    .italics()
                    .color(Color32::from_gray(160)),
            );
        });
}

/// Helper para a `app.rs` pedir repaint quando houver trabalho em andamento.
pub fn keep_alive(ctx: &Context, library: &ReplayLibrary) {
    if library.scanning {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    } else if library.pending_count() > 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}
