// Aba Build Order — busca + filtros globais no topo, duas colunas
// lado a lado (uma por jogador) com cabeçalho estilo sidebar card
// (borda lateral colorida) + tabela scrollable. Legenda fixa na
// parte inferior, fora do scroll.

use egui::{Color32, Grid, Id, RichText, ScrollArea, TextEdit, Ui};

use crate::build_order::{classify_entry, EntryKind, EntryOutcome, PlayerBuildOrder};
use crate::colors::{player_slot_color, user_fill, CARD_FILL};
use crate::config::AppConfig;
use crate::locale::{self, t, tf};
use crate::replay_state::{fmt_time, LoadedReplay};
use crate::salt;
use crate::tabs::timeline::unit_column::{structure_icon, unit_icon};
use crate::tokens::SPACE_S;
use crate::widgets::{
    copy_icon_button, copy_labeled_button, player_identity, toggle_chip_bool, NameDensity,
};

/// Todas as categorias, na ordem de exibição da legenda / filtros.
const ALL_KINDS: [EntryKind; 6] = [
    EntryKind::Worker,
    EntryKind::Unit,
    EntryKind::Structure,
    EntryKind::Research,
    EntryKind::Upgrade,
    EntryKind::Inject,
];

/// Filtros globais de categoria. Pelo menos um deve estar ativo.
#[derive(Clone, Copy, Debug)]
struct BuildOrderFilter {
    show_workers: bool,
    show_units: bool,
    show_structures: bool,
    show_research: bool,
    show_upgrades: bool,
    show_injects: bool,
}

impl Default for BuildOrderFilter {
    fn default() -> Self {
        Self {
            show_workers: true,
            show_units: true,
            show_structures: true,
            show_research: true,
            show_upgrades: true,
            show_injects: true,
        }
    }
}

impl BuildOrderFilter {
    /// Retorna true se a categoria deve ser exibida.
    fn allows(&self, kind: EntryKind) -> bool {
        match kind {
            EntryKind::Worker => self.show_workers,
            EntryKind::Unit => self.show_units,
            EntryKind::Structure => self.show_structures,
            EntryKind::Research => self.show_research,
            EntryKind::Upgrade => self.show_upgrades,
            EntryKind::Inject => self.show_injects,
        }
    }

    /// Quantidade de filtros ativos.
    fn active_count(&self) -> u32 {
        self.show_workers as u32
            + self.show_units as u32
            + self.show_structures as u32
            + self.show_research as u32
            + self.show_upgrades as u32
            + self.show_injects as u32
    }
}

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
    let lang = config.language;
    let Some(bo) = loaded.build_order.as_ref() else {
        ui.add_space(crate::tokens::SPACE_XXL);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t("build_order.unavailable", lang)).italics());
        });
        return;
    };

    let lps = bo.loops_per_second;
    let players = &bo.players;

    if players.is_empty() {
        ui.label(t("build_order.no_players", lang));
        return;
    }

    // ── Busca global + filtros (Workers / Unidades) ─────────────
    let search_id = Id::new("bo_search_query");
    let mut search: String = ui
        .ctx()
        .data(|d| d.get_temp::<String>(search_id))
        .unwrap_or_default();

    let filter_id = Id::new("bo_global_filter");
    let mut filter: BuildOrderFilter = ui
        .ctx()
        .data(|d| d.get_temp::<BuildOrderFilter>(filter_id))
        .unwrap_or_default();

    let icons_id = Id::new("bo_show_icons");
    let mut show_icons: bool = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(icons_id))
        .unwrap_or(false);

    // ── Campo de busca (lupa dentro do input) ────────────────────
    let resp = ui.add_sized(
        [ui.available_width(), 28.0],
        TextEdit::singleline(&mut search)
            .hint_text(t("build_order.search_placeholder", lang))
            .font(egui::TextStyle::Body),
    );
    if resp.changed() {
        ui.ctx()
            .data_mut(|d| d.insert_temp(search_id, search.clone()));
    }
    // Botão limpar sobreposto à direita quando há texto.
    if !search.is_empty() {
        let clear_rect = egui::Rect::from_min_size(
            egui::pos2(resp.rect.right() - 22.0, resp.rect.top() + 2.0),
            egui::vec2(20.0, resp.rect.height() - 4.0),
        );
        if ui.put(clear_rect, egui::Button::new("×").small().frame(false)).clicked() {
            search.clear();
            ui.ctx()
                .data_mut(|d| d.insert_temp(search_id, search.clone()));
        }
    }

    ui.add_space(2.0);

    // ── Filtros de categoria ────────────────────────────────────
    let mut filter_changed = false;
    let mut icons_changed = false;
    let prev_icons = show_icons;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        let prev = filter;
        toggle_chip_bool(ui, t("build_order.filter.workers", lang), &mut filter.show_workers, None);
        toggle_chip_bool(ui, t("build_order.filter.units", lang), &mut filter.show_units, None);
        toggle_chip_bool(ui, t("build_order.filter.structures", lang), &mut filter.show_structures, None);
        toggle_chip_bool(ui, t("build_order.filter.research", lang), &mut filter.show_research, None);
        toggle_chip_bool(ui, t("build_order.filter.upgrades", lang), &mut filter.show_upgrades, None);
        toggle_chip_bool(ui, t("build_order.filter.injects", lang), &mut filter.show_injects, None);

        if filter.active_count() == 0 {
            filter = prev;
        }

        filter_changed = filter.show_workers != prev.show_workers
            || filter.show_units != prev.show_units
            || filter.show_structures != prev.show_structures
            || filter.show_research != prev.show_research
            || filter.show_upgrades != prev.show_upgrades
            || filter.show_injects != prev.show_injects;

        ui.add_space(SPACE_S);
        ui.label(RichText::new("|").weak());
        ui.add_space(SPACE_S);
        toggle_chip_bool(ui, t("build_order.filter.icons", lang), &mut show_icons, None);
        icons_changed = show_icons != prev_icons;
    });
    if filter_changed {
        ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter));
    }
    if icons_changed {
        ui.ctx().data_mut(|d| d.insert_temp(icons_id, show_icons));
    }

    ui.add_space(SPACE_S);

    let query_lower = search.to_ascii_lowercase();

    // ── Colunas dos jogadores (área scrollable) ─────────────────
    let n = players.len().min(2).max(1);

    // Reserva espaço para a legenda fixa no rodapé.
    let available = ui.available_height() - 32.0;
    let layout_rect = ui.available_rect_before_wrap();
    let content_rect = egui::Rect::from_min_size(
        layout_rect.min,
        egui::vec2(layout_rect.width(), available.max(100.0)),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.columns(n, |cols| {
            for (i, player) in players.iter().take(n).enumerate() {
                let ui = &mut cols[i];
                let is_user = config.is_user(&player.name);
                player_column(
                    ui,
                    player,
                    i,
                    lps,
                    is_user,
                    &query_lower,
                    &filter,
                    show_icons,
                    config,
                    lang,
                );
            }
        });
    });

    // ── Modais SALT ─────────────────────────────────────────────
    for (i, player) in players.iter().take(n).enumerate() {
        let open_id = Id::new(format!("salt_open_{}", i));
        let data_id = Id::new(format!("salt_modal_{}", i));
        let mut is_open: bool = ui.ctx().data(|d| d.get_temp(open_id).unwrap_or(false));
        if is_open {
            let encoded: String = ui
                .ctx()
                .data(|d| d.get_temp::<String>(data_id).unwrap_or_default());
            egui::Window::new(tf(
                "build_order.salt.title",
                lang,
                &[("player", &player.name)],
            ))
                .open(&mut is_open)
                .resizable(true)
                .default_width(500.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(RichText::new(t("build_order.salt.desc", lang)).small());
                    ui.add_space(6.0);
                    let mut text = encoded.clone();
                    ui.add(
                        TextEdit::multiline(&mut text)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.add_space(4.0);
                    if copy_labeled_button(ui, t("build_order.salt.copy", lang)).clicked() {
                        ui.ctx().copy_text(encoded);
                    }
                });
            ui.ctx().data_mut(|d| d.insert_temp(open_id, is_open));
        }
    }

    // ── Legenda fixa no rodapé ──────────────────────────────────
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for kind in ALL_KINDS {
            legend_chip(ui, kind, lang);
        }
        // separador visual entre categorias e status
        ui.add_space(4.0);
        ui.label(RichText::new("|").weak());
        ui.add_space(4.0);
        legend_icon(
            ui,
            "⚡",
            Color32::from_rgb(180, 200, 255),
            t("build_order.legend.chrono", lang),
        );
        legend_icon(
            ui,
            "⊘",
            Color32::from_rgb(220, 180, 80),
            t("build_order.legend.cancelled", lang),
        );
        legend_icon(
            ui,
            "□",
            Color32::from_rgb(230, 90, 90),
            t("build_order.legend.destroyed", lang),
        );
    });
}

/// Localized display name for an EntryKind.
fn entry_kind_full_name(kind: EntryKind, lang: locale::Language) -> &'static str {
    let key = match kind {
        EntryKind::Worker => "entrykind.worker",
        EntryKind::Unit => "entrykind.unit",
        EntryKind::Structure => "entrykind.structure",
        EntryKind::Research => "entrykind.research",
        EntryKind::Upgrade => "entrykind.upgrade",
        EntryKind::Inject => "entrykind.inject",
    };
    t(key, lang)
}

/// Chip compacto: [letra] nome
fn legend_chip(ui: &mut Ui, kind: EntryKind, lang: locale::Language) {
    let color = kind_color(kind);
    ui.label(
        RichText::new(format!(" {} ", kind.short_letter()))
            .monospace()
            .strong()
            .size(10.0)
            .color(Color32::BLACK)
            .background_color(color),
    );
    ui.label(RichText::new(entry_kind_full_name(kind, lang)).small().weak());
}

/// Ícone de status: símbolo colorido + label
fn legend_icon(ui: &mut Ui, icon: &str, color: Color32, label: &str) {
    ui.label(RichText::new(icon).strong().color(color).size(11.0));
    ui.label(RichText::new(label).small().weak());
}

fn player_column(
    ui: &mut Ui,
    player: &PlayerBuildOrder,
    index: usize,
    lps: f64,
    is_user: bool,
    query_lower: &str,
    filter: &BuildOrderFilter,
    show_icons: bool,
    config: &AppConfig,
    lang: locale::Language,
) {
    let slot_color = player_slot_color(index);
    let fill = if is_user { user_fill(index) } else { CARD_FILL };

    // ── Cabeçalho estilo sidebar card ───────────────────────────
    let header_resp = egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(0.5, Color32::from_gray(50)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_S;
                player_identity(
                    ui,
                    &player.name,
                    &player.race,
                    index,
                    is_user,
                    NameDensity::Normal,
                    config,
                    lang,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if copy_icon_button(ui, t("build_order.copy_tooltip", lang)).clicked() {
                        let text = format_clipboard_single(player, lps, lang);
                        ui.ctx().copy_text(text);
                    }
                    let salt_modal_id = Id::new(format!("salt_modal_{}", index));
                    if ui
                        .small_button("SALT")
                        .on_hover_text(t("build_order.salt_button_tooltip", lang))
                        .clicked()
                    {
                        let encoded = salt::encode(player, lps);
                        ui.ctx().data_mut(|d| d.insert_temp::<String>(salt_modal_id, encoded));
                        ui.ctx().data_mut(|d| d.insert_temp::<bool>(Id::new(format!("salt_open_{}", index)), true));
                    }
                });
            });
        });

    // Borda lateral colorida.
    let rect = header_resp.response.rect;
    let accent = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.left() + 3.0, rect.bottom()),
    );
    ui.painter().rect_filled(
        accent,
        egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 },
        slot_color,
    );

    ui.add_space(4.0);

    // ── Tabela scrollable ───────────────────────────────────────
    if player.entries.is_empty() {
        ui.label(RichText::new(t("build_order.empty_entries", lang)).italics());
        return;
    }

    let body_h = ui.text_style_height(&egui::TextStyle::Body);
    let icon_side = (body_h * 1.5).round();
    let row_min_h = if show_icons { body_h * 1.5 } else { 0.0 };

    ScrollArea::vertical()
        .id_salt(format!("bo_{}", index))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            Grid::new(format!("bo_grid_{}", index))
                .num_columns(4)
                .spacing([12.0, 2.0])
                .min_row_height(row_min_h)
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new(t("build_order.col.start", lang)).small().strong());
                    ui.label(RichText::new(t("build_order.col.end", lang)).small().strong());
                    ui.label(RichText::new(t("build_order.col.supply", lang)).small().strong());
                    ui.label(RichText::new(t("build_order.col.action", lang)).small().strong());
                    ui.end_row();

                    let mut rendered = 0usize;
                    for entry in &player.entries {
                        let kind = classify_entry(entry);

                        if !filter.allows(kind) {
                            continue;
                        }
                        let display_name = format_display_name(&entry.action, lang);
                        if !query_lower.is_empty()
                            && !entry.action.to_ascii_lowercase().contains(query_lower)
                            && !display_name.to_lowercase().contains(query_lower)
                        {
                            continue;
                        }

                        let outcome = entry.outcome;
                        let (outcome_tint, outcome_icon, outcome_tooltip): (
                            Option<Color32>,
                            Option<&str>,
                            Option<&str>,
                        ) = match outcome {
                            EntryOutcome::Completed => (None, None, None),
                            EntryOutcome::Cancelled => (
                                Some(Color32::from_rgb(220, 180, 80)),
                                Some("⊘"),
                                Some(t("build_order.outcome.cancelled", lang)),
                            ),
                            EntryOutcome::DestroyedInProgress => (
                                Some(Color32::from_rgb(230, 90, 90)),
                                Some("✕"),
                                Some(t("build_order.outcome.destroyed", lang)),
                            ),
                        };
                        let strike = outcome != EntryOutcome::Completed;

                        ui.monospace(fmt_time(entry.game_loop, lps));
                        let mut finish_rt =
                            RichText::new(fmt_time(entry.finish_loop, lps)).monospace();
                        if let Some(c) = outcome_tint {
                            finish_rt = finish_rt.color(c);
                        }
                        ui.label(finish_rt);
                        // Clampa `supply_made` ao cap visual de 200 (igual ao painel
                        // da Timeline e ao HUD do próprio SC2). O `supply_used` cru
                        // pode estourar transientemente durante morphs/mortes e fica
                        // sem clamp pra deixar o glitch real do tracker visível.
                        let supply_cap = entry.supply_made.min(200);
                        ui.monospace(format!("{:>3}/{:<3}", entry.supply, supply_cap));

                        let action_text = if entry.count > 1 {
                            format!("{} x{}", display_name, entry.count)
                        } else {
                            display_name.to_string()
                        };
                        ui.horizontal(|ui| {
                            if show_icons {
                                if let Some(sprite) = unit_icon(&entry.action)
                                    .or_else(|| structure_icon(&entry.action))
                                {
                                    ui.add(
                                        egui::Image::new(sprite)
                                            .fit_to_exact_size(egui::vec2(icon_side, icon_side)),
                                    );
                                } else {
                                    ui.add_space(icon_side);
                                }
                            }
                            if let (Some(icon), Some(tint)) = (outcome_icon, outcome_tint) {
                                ui.label(
                                    RichText::new(icon).monospace().strong().color(tint),
                                )
                                .on_hover_text(outcome_tooltip.unwrap_or(""));
                            }
                            let mut rt = RichText::new(action_text);
                            if strike {
                                rt = rt.strikethrough();
                            }
                            if let Some(c) = outcome_tint {
                                rt = rt.color(c);
                            }
                            let lbl = ui.label(rt);
                            if let Some(tt) = outcome_tooltip {
                                lbl.on_hover_text(tt);
                            }
                            if entry.chrono_boosts > 0 {
                                let chrono_text = if entry.chrono_boosts > 1 {
                                    format!("⚡×{}", entry.chrono_boosts)
                                } else {
                                    "⚡".to_string()
                                };
                                ui.label(
                                    RichText::new(chrono_text)
                                        .strong(),
                                )
                                .on_hover_text(tf(
                                    "build_order.chrono_tooltip",
                                    lang,
                                    &[("count", &entry.chrono_boosts.to_string())],
                                ));
                            }
                        });

                        ui.end_row();
                        rendered += 1;
                    }

                    if rendered == 0 {
                        ui.label(
                            RichText::new(t("build_order.no_match", lang))
                                .italics()
                                .color(Color32::from_gray(140)),
                        );
                        ui.end_row();
                    }
                });
        });
}

/// Formata o nome de exibição de uma entrada. Para injects, parseia o
/// formato `InjectLarva@Type@X_Y` e exibe como
/// "Inject Larva @ Hatchery (45, 67)". Para tudo o mais, delega ao
/// `locale::localize`.
fn format_display_name(action: &str, lang: locale::Language) -> String {
    if let Some(rest) = action.strip_prefix("InjectLarva@") {
        // rest = "Hatchery@45_67"
        let inject_label = locale::localize("InjectLarva", lang);
        if let Some((target_type, coords)) = rest.split_once('@') {
            let target_label = locale::localize(target_type, lang);
            let coords_display = coords.replace('_', ", ");
            format!("{inject_label} @ {target_label} ({coords_display})")
        } else {
            let target_label = locale::localize(rest, lang);
            format!("{inject_label} @ {target_label}")
        }
    } else {
        locale::localize(action, lang).to_string()
    }
}

/// Cor característica de cada categoria. Escolhidas pra serem
/// distinguíveis mesmo em scan rápido e não colidirem com as cores
/// de slot (P1 vermelho / P2 azul) usadas na borda do card.
fn kind_color(kind: EntryKind) -> Color32 {
    match kind {
        EntryKind::Worker => Color32::from_rgb(120, 200, 140),   // verde suave
        EntryKind::Unit => Color32::from_gray(200),              // cinza claro
        EntryKind::Structure => Color32::from_rgb(230, 170, 80), // laranja
        EntryKind::Research => Color32::from_rgb(180, 140, 230), // roxo
        EntryKind::Upgrade => Color32::from_rgb(240, 210, 120),  // amarelo/dourado
        EntryKind::Inject => Color32::from_rgb(100, 200, 220),   // ciano/teal
    }
}

fn race_initial(race: &str) -> char {
    match race.to_ascii_lowercase().chars().next() {
        Some('t') => 'T',
        Some('p') => 'P',
        Some('z') => 'Z',
        Some('r') => 'R',
        _ => '?',
    }
}

/// Formata build order de um jogador como texto para clipboard.
fn format_clipboard_single(player: &PlayerBuildOrder, lps: f64, lang: locale::Language) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "=== ({}) {} ===\n",
        race_initial(&player.race),
        player.name,
    ));
    out.push_str(t("build_order.clipboard.header", lang));
    out.push('\n');
    for entry in &player.entries {
        let kind = classify_entry(entry);
        let display = format_display_name(&entry.action, lang);
        let action_text = if entry.count > 1 {
            format!("{} x{}", display, entry.count)
        } else {
            display
        };
        let outcome_mark = match entry.outcome {
            EntryOutcome::Completed => "",
            EntryOutcome::Cancelled => t("build_order.clipboard.cancelled_mark", lang),
            EntryOutcome::DestroyedInProgress => t("build_order.clipboard.destroyed_mark", lang),
        };
        out.push_str(&format!(
            "{}     {:>5}  {:>5}  {:>3}/{:<3}  {}{}\n",
            kind.short_letter(),
            fmt_time(entry.game_loop, lps),
            fmt_time(entry.finish_loop, lps),
            entry.supply,
            entry.supply_made.min(200),
            action_text,
            outcome_mark,
        ));
    }
    out
}
