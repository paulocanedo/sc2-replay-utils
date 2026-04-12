// Aba Build Order — busca + filtros globais no topo, duas colunas
// lado a lado (uma por jogador) com cabeçalho estilo sidebar card
// (borda lateral colorida) + tabela scrollable. Legenda fixa na
// parte inferior, fora do scroll.

use egui::{Color32, Grid, Id, RichText, ScrollArea, TextEdit, Ui};

use crate::build_order::{classify_entry, EntryKind, EntryOutcome, PlayerBuildOrder};
use crate::colors::{player_slot_color, user_fill, CARD_FILL, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::locale;
use crate::replay_state::{fmt_time, LoadedReplay};
use crate::salt;

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
    let Some(bo) = loaded.build_order.as_ref() else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Build Order não disponível para este replay.").italics());
        });
        return;
    };

    let lps = bo.loops_per_second;
    let players = &bo.players;

    if players.is_empty() {
        ui.label("Nenhum jogador encontrado.");
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

    // ── Campo de busca (lupa dentro do input) ────────────────────
    let resp = ui.add_sized(
        [ui.available_width(), 28.0],
        TextEdit::singleline(&mut search)
            .hint_text("🔎  buscar ação... (ex: Marine, Stimpack)")
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
    ui.horizontal_wrapped(|ui| {
        let prev = filter;
        ui.checkbox(&mut filter.show_workers, "Workers");
        ui.checkbox(&mut filter.show_units, "Unidades");
        ui.checkbox(&mut filter.show_structures, "Estruturas");
        ui.checkbox(&mut filter.show_research, "Pesquisa");
        ui.checkbox(&mut filter.show_upgrades, "Upgrades");
        ui.checkbox(&mut filter.show_injects, "Injects");

        if filter.active_count() == 0 {
            filter = prev;
        }

        filter_changed = filter.show_workers != prev.show_workers
            || filter.show_units != prev.show_units
            || filter.show_structures != prev.show_structures
            || filter.show_research != prev.show_research
            || filter.show_upgrades != prev.show_upgrades
            || filter.show_injects != prev.show_injects;
    });
    if filter_changed {
        ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter));
    }

    ui.add_space(4.0);

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
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.columns(n, |cols| {
            for (i, player) in players.iter().take(n).enumerate() {
                let ui = &mut cols[i];
                let is_user = config.is_user(&player.name);
                player_column(ui, player, i, lps, is_user, &query_lower, &filter, config.language);
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
            egui::Window::new(format!("SALT Encoding — {}", player.name))
                .open(&mut is_open)
                .resizable(true)
                .default_width(500.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(RichText::new("Copie a string abaixo para importar em ferramentas como Spawning Tool, Embot Advanced ou SC2 Scrapbook.").small());
                    ui.add_space(6.0);
                    let mut text = encoded.clone();
                    ui.add(
                        TextEdit::multiline(&mut text)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.add_space(4.0);
                    if ui.button("📋 Copiar para área de transferência").clicked() {
                        ui.ctx().copy_text(encoded);
                    }
                });
            ui.ctx().data_mut(|d| d.insert_temp(open_id, is_open));
        }
    }

    // ── Legenda fixa no rodapé ──────────────────────────────────
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("legenda:").small().italics());
        for kind in ALL_KINDS {
            legend_chip(ui, kind);
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new("⚡")
                .strong(),
        )
        .on_hover_text("Chrono Boost (Protoss)");
        ui.small("chrono");
        ui.add_space(6.0);
        ui.label(
            RichText::new("⊘")
                .monospace()
                .strong()
                .color(Color32::from_rgb(220, 180, 80)),
        )
        .on_hover_text("cancelado pelo jogador");
        ui.small("cancelado");
        ui.add_space(6.0);
        ui.label(
            RichText::new("✕")
                .monospace()
                .strong()
                .color(Color32::from_rgb(230, 90, 90)),
        )
        .on_hover_text("destruído durante a construção");
        ui.small("destruído");
    });
}

/// Pequeno chip colorido exibindo uma categoria na legenda: fundo
/// com a cor da categoria, letra em negrito e, ao lado, o nome
/// completo em texto neutro pra leitura.
fn legend_chip(ui: &mut Ui, kind: EntryKind) {
    let color = kind_color(kind);
    ui.label(
        RichText::new(format!(" {} ", kind.short_letter()))
            .monospace()
            .strong()
            .color(Color32::BLACK)
            .background_color(color),
    )
    .on_hover_text(kind.full_name());
    ui.small(kind.full_name());
    ui.add_space(4.0);
}

fn player_column(
    ui: &mut Ui,
    player: &PlayerBuildOrder,
    index: usize,
    lps: f64,
    is_user: bool,
    query_lower: &str,
    filter: &BuildOrderFilter,
    lang: locale::Language,
) {
    let slot_color = player_slot_color(index);
    let fill = if is_user { user_fill(index) } else { CARD_FILL };

    // ── Cabeçalho estilo sidebar card ───────────────────────────
    let race_letter = race_initial(&player.race);
    let header_resp = egui::Frame::none()
        .fill(fill)
        .stroke(egui::Stroke::new(0.5, Color32::from_gray(50)))
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(14.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("({race_letter}) {}", player.name))
                        .size(15.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                if is_user {
                    ui.label(
                        RichText::new(" VOCÊ ")
                            .small()
                            .strong()
                            .color(USER_CHIP_FG)
                            .background_color(USER_CHIP_BG),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("📋").on_hover_text("Copiar build order para a área de transferência").clicked() {
                        let text = format_clipboard_single(player, lps, lang);
                        ui.ctx().copy_text(text);
                    }
                    let salt_modal_id = Id::new(format!("salt_modal_{}", index));
                    if ui.small_button("SALT").on_hover_text("Gerar SALT Encoding").clicked() {
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
        egui::Rounding { nw: 6.0, sw: 6.0, ne: 0.0, se: 0.0 },
        slot_color,
    );

    ui.add_space(4.0);

    // ── Tabela scrollable ───────────────────────────────────────
    if player.entries.is_empty() {
        ui.label(RichText::new("(nenhuma entrada)").italics());
        return;
    }

    ScrollArea::vertical()
        .id_salt(format!("bo_{}", index))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            Grid::new(format!("bo_grid_{}", index))
                .num_columns(5)
                .spacing([12.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("início").small().strong());
                    ui.label(RichText::new("fim").small().strong());
                    ui.label(RichText::new("supply").small().strong());
                    ui.label(RichText::new("ação").small().strong());
                    ui.label(RichText::new("tipo").small().strong());
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
                                Some("cancelado pelo jogador"),
                            ),
                            EntryOutcome::DestroyedInProgress => (
                                Some(Color32::from_rgb(230, 90, 90)),
                                Some("✕"),
                                Some("destruído durante a construção"),
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
                        ui.monospace(format!("{:>3}/{:<3}", entry.supply, entry.supply_made));

                        let action_text = if entry.count > 1 {
                            format!("{} x{}", display_name, entry.count)
                        } else {
                            display_name.to_string()
                        };
                        ui.horizontal(|ui| {
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
                                .on_hover_text(format!(
                                    "Chrono Boost ×{}",
                                    entry.chrono_boosts,
                                ));
                            }
                        });

                        // tipo (última coluna)
                        let color = kind_color(kind);
                        ui.label(
                            RichText::new(kind.short_letter())
                                .monospace()
                                .strong()
                                .color(color),
                        )
                        .on_hover_text(kind.full_name());

                        ui.end_row();
                        rendered += 1;
                    }

                    if rendered == 0 {
                        ui.label(
                            RichText::new("(nada corresponde aos filtros)")
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
    out.push_str("tipo  início  fim     supply  ação\n");
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
            EntryOutcome::Cancelled => " [cancelado]",
            EntryOutcome::DestroyedInProgress => " [destruído]",
        };
        out.push_str(&format!(
            "{}     {:>5}  {:>5}  {:>3}/{:<3}  {}{}\n",
            kind.short_letter(),
            fmt_time(entry.game_loop, lps),
            fmt_time(entry.finish_loop, lps),
            entry.supply,
            entry.supply_made,
            action_text,
            outcome_mark,
        ));
    }
    out
}
