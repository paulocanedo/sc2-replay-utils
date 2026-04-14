//! Render egui da biblioteca + ação solicitada pelo usuário.

use std::path::{Path, PathBuf};

use egui::{Color32, Context, RichText, ScrollArea, Sense, Ui};

use crate::config::AppConfig;

use super::date::{matches_date_range, today_str};
use super::filter::{DateRange, LibraryFilter, OutcomeFilter, SortOrder};
use super::scanner::ReplayLibrary;
use super::types::{LibraryEntry, MetaState, ParsedMeta, PlayerMeta};

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

    // ── Header ───────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.heading("Biblioteca");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("↻").on_hover_text("Recarregar lista").clicked() {
                action = LibraryAction::Refresh;
            }
            if ui.small_button("🔎").on_hover_text("Zoom / configurações").clicked() {}
            if ui.small_button("✏").on_hover_text("Renomear replays em lote").clicked() {
                action = LibraryAction::OpenRename;
            }
            if ui
                .small_button("📂")
                .on_hover_text("Escolher diretório de trabalho")
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
            ui.small(RichText::new("Diretório não definido").italics());
        }
    }

    ui.add_space(4.0);

    // ── Barra de busca + contagem/sort ───────────────────────────────
    ui.horizontal(|ui| {
        ui.label("🔎");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut filter.search)
                .hint_text("Buscar jogador, mapa ou matchup…")
                .desired_width(ui.available_width() - 150.0),
        );
        if !filter.search.is_empty() && resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            filter.search.clear();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let total = library.entries.len();
            let sort_label = match filter.sort {
                SortOrder::Date => "Data",
                SortOrder::Duration => "Duração",
                SortOrder::Mmr => "MMR",
                SortOrder::Map => "Mapa",
            };
            let arrow = if filter.sort_ascending { "↑" } else { "↓" };
            egui::ComboBox::from_id_salt("library_sort")
                .selected_text(format!("{total} replays {arrow}"))
                .width(120.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut filter.sort, SortOrder::Date, "Data");
                    ui.selectable_value(&mut filter.sort, SortOrder::Duration, "Duração");
                    ui.selectable_value(&mut filter.sort, SortOrder::Mmr, "MMR");
                    ui.selectable_value(&mut filter.sort, SortOrder::Map, "Mapa");
                    ui.separator();
                    let asc_label = if filter.sort_ascending { "▸ Crescente" } else { "  Crescente" };
                    let desc_label = if !filter.sort_ascending { "▸ Decrescente" } else { "  Decrescente" };
                    if ui.selectable_label(filter.sort_ascending, asc_label).clicked() {
                        filter.sort_ascending = true;
                    }
                    if ui.selectable_label(!filter.sort_ascending, desc_label).clicked() {
                        filter.sort_ascending = false;
                    }
                });
            let _ = sort_label; // utilizado no ComboBox acima
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
        if chip(ui, "Todos", todos_active, None).clicked() {
            filter.race = None;
            filter.outcome = OutcomeFilter::All;
            filter.date_range = DateRange::All;
        }

        ui.add_space(4.0);

        for (label, letter, color) in [
            ("Terran", 'T', RACE_COLOR_TERRAN),
            ("Protoss", 'P', RACE_COLOR_PROTOSS),
            ("Zerg", 'Z', RACE_COLOR_ZERG),
        ] {
            let selected = filter.race == Some(letter);
            let resp = chip(ui, label, selected, Some(color));
            if resp.clicked() && has_nicknames {
                filter.race = if selected { None } else { Some(letter) };
            }
            if !has_nicknames {
                resp.on_hover_text("Configure seus nicknames para filtrar por raça");
            }
        }

        ui.add_space(4.0);

        let wins_selected = filter.outcome == OutcomeFilter::Wins;
        let resp = chip(ui, "Vitórias", wins_selected, Some(Color32::from_rgb(80, 180, 80)));
        if resp.clicked() && has_nicknames {
            filter.outcome = if wins_selected { OutcomeFilter::All } else { OutcomeFilter::Wins };
        }
        if !has_nicknames {
            resp.on_hover_text("Configure seus nicknames para filtrar por resultado");
        }

        let losses_selected = filter.outcome == OutcomeFilter::Losses;
        let resp = chip(ui, "Derrotas", losses_selected, Some(Color32::from_rgb(180, 80, 80)));
        if resp.clicked() && has_nicknames {
            filter.outcome = if losses_selected { OutcomeFilter::All } else { OutcomeFilter::Losses };
        }
        if !has_nicknames {
            resp.on_hover_text("Configure seus nicknames para filtrar por resultado");
        }

        ui.add_space(4.0);

        let prev_date_range = filter.date_range;
        let date_label = match filter.date_range {
            DateRange::All => "Sempre",
            DateRange::Today => "Hoje",
            DateRange::ThisWeek => "Semana",
            DateRange::ThisMonth => "Mês",
        };
        let date_active = filter.date_range != DateRange::All;
        let date_text_color = if date_active { Color32::WHITE } else { Color32::from_gray(160) };
        egui::ComboBox::from_id_salt("date_range_chip")
            .selected_text(RichText::new(format!("{date_label} ▾")).color(date_text_color).small())
            .width(80.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter.date_range, DateRange::All, "Sempre");
                ui.selectable_value(&mut filter.date_range, DateRange::Today, "Hoje");
                ui.selectable_value(&mut filter.date_range, DateRange::ThisWeek, "Esta semana");
                ui.selectable_value(&mut filter.date_range, DateRange::ThisMonth, "Este mês");
            });
        if filter.date_range != prev_date_range {
            action = LibraryAction::SaveDateRange(filter.date_range);
        }
    });

    ui.add_space(2.0);

    // ── Status ───────────────────────────────────────────────────────
    if library.scanning {
        ui.small(
            RichText::new(format!("🔍 varrendo pasta… {} encontrados", library.entries.len()))
                .italics(),
        );
    } else {
        let pending = library.pending_count();
        if pending > 0 {
            ui.small(format!("🔄 {pending}/{} lendo metadados…", library.entries.len()));
        }
    }

    ui.separator();

    if library.entries.is_empty() && library.working_dir.is_none() {
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Defina um 'Diretório de trabalho' (botão 📂 acima ou em Configurações) para listar seus replays aqui.",
            )
            .italics(),
        );
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
            RichText::new("Nenhum replay corresponde ao filtro.")
                .italics()
                .color(Color32::from_gray(160)),
        );
        return action;
    }

    if any_filter_active {
        ui.small(
            RichText::new(format!("🔎 {shown}/{} correspondem ao filtro", library.entries.len()))
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

// ── Helpers de filtro/sort ────────────────────────────────────────────

fn find_user_player<'a>(meta: &'a ParsedMeta, config: &AppConfig) -> Option<&'a PlayerMeta> {
    if config.user_nicknames.is_empty() {
        return None;
    }
    meta.players.iter().find(|p| {
        config
            .user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&p.name))
    })
}

fn find_user_index(meta: &ParsedMeta, config: &AppConfig) -> Option<usize> {
    if config.user_nicknames.is_empty() {
        return None;
    }
    meta.players.iter().position(|p| {
        config
            .user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&p.name))
    })
}

fn matchup_code(meta: &ParsedMeta, config: &AppConfig) -> String {
    if meta.players.len() != 2 {
        return String::new();
    }
    let ui = find_user_index(meta, config);
    let (first, second) = match ui {
        Some(0) => (0, 1),
        Some(1) => (1, 0),
        _ => (0, 1),
    };
    format!(
        "{}v{}",
        race_letter(&meta.players[first].race),
        race_letter(&meta.players[second].race)
    )
}

fn get_duration(entry: &LibraryEntry) -> u32 {
    match &entry.meta {
        MetaState::Parsed(m) => m.duration_seconds,
        _ => 0,
    }
}

fn get_user_mmr(entry: &LibraryEntry, config: &AppConfig) -> i32 {
    match &entry.meta {
        MetaState::Parsed(m) => find_user_player(m, config)
            .and_then(|p| p.mmr)
            .unwrap_or(0),
        _ => 0,
    }
}

fn get_map(entry: &LibraryEntry) -> &str {
    match &entry.meta {
        MetaState::Parsed(m) => &m.map,
        _ => "",
    }
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

    let icon = if accent.is_some() {
        if selected {
            format!("■ {label}")
        } else {
            format!("□ {label}")
        }
    } else {
        label.to_string()
    };

    ui.add(
        egui::Button::new(RichText::new(icon).color(text_color).small())
            .fill(fill)
            .corner_radius(12.0),
    )
}

/// Altura de cada linha da lista virtualizada.
fn row_height(ui: &Ui) -> f32 {
    use egui::TextStyle;
    let body = ui.text_style_height(&TextStyle::Body);
    let small = ui.text_style_height(&TextStyle::Small);
    let gap = ui.spacing().item_spacing.y;
    body + small * 2.0 + gap * 2.0 + FRAME_CHROME_V
}

const FRAME_CHROME_V: f32 = 13.0;

// Cores de raça — distintas das cores de slot P1/P2 (vermelho/azul)
// para que "raça" e "jogador" nunca se confundam visualmente.
const RACE_COLOR_TERRAN: Color32 = Color32::from_rgb(90, 130, 180);   // azul aço
const RACE_COLOR_PROTOSS: Color32 = Color32::from_rgb(120, 180, 100); // verde dourado
const RACE_COLOR_ZERG: Color32 = Color32::from_rgb(160, 80, 150);     // roxo magenta

/// Cor da borda esquerda baseada na raça.
fn race_border_color(race: &str) -> Color32 {
    match race_letter(race) {
        'T' => RACE_COLOR_TERRAN,
        'P' => RACE_COLOR_PROTOSS,
        'Z' => RACE_COLOR_ZERG,
        _ => Color32::from_gray(100),
    }
}

fn entry_row(
    ui: &mut Ui,
    entry: &LibraryEntry,
    is_current: bool,
    config: &AppConfig,
    row_h: f32,
) -> bool {
    let loadable = entry.meta.is_loadable();
    let fill = if is_current {
        Color32::from_rgb(24, 48, 24)
    } else if matches!(entry.meta, MetaState::Unsupported(_)) {
        Color32::from_gray(22)
    } else {
        Color32::from_gray(28)
    };
    let stroke = if is_current {
        egui::Stroke::new(1.5, Color32::LIGHT_GREEN)
    } else if matches!(entry.meta, MetaState::Unsupported(_)) {
        egui::Stroke::new(0.5, Color32::from_gray(50))
    } else {
        egui::Stroke::new(0.5, Color32::from_gray(60))
    };

    let content_h = (row_h - FRAME_CHROME_V).max(0.0);

    let inner = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(content_h);

            match &entry.meta {
                MetaState::Parsed(meta) => {
                    let user_idx = find_user_index(meta, config);
                    let mc = matchup_code(meta, config);

                    // Player names label: "Player1 vs Player2"
                    let vs_label = if meta.players.len() == 2 {
                        format!("{} vs {}", meta.players[0].name, meta.players[1].name)
                    } else {
                        meta.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" vs ")
                    };

                    let dur = format!(
                        "{:02}:{:02}",
                        meta.duration_seconds / 60,
                        meta.duration_seconds % 60
                    );

                    let mmrs: Vec<String> = meta
                        .players
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let v = match p.mmr {
                                Some(v) => v.to_string(),
                                None => "—".into(),
                            };
                            if user_idx == Some(i) {
                                format!("{v}")
                            } else {
                                v
                            }
                        })
                        .collect();

                    let (short_date, time_part) = split_datetime(&meta.datetime);

                    ui.horizontal(|ui| {
                        // ── Coluna esquerda ──
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&vs_label)
                                    .strong()
                                    .color(if is_current {
                                        Color32::LIGHT_GREEN
                                    } else {
                                        Color32::WHITE
                                    }),
                            );
                            ui.small(
                                RichText::new(format!("🗺 {} • ⏱ {dur} • {short_date}", meta.map))
                                    .color(Color32::from_gray(140)),
                            );
                            let mmr_user = user_idx.and_then(|i| meta.players[i].mmr);
                            let mmr_text = format!("MMR {}", mmrs.join(" / "));
                            if mmr_user.is_some() {
                                ui.small(
                                    RichText::new(mmr_text)
                                        .color(Color32::from_gray(140))
                                        .strong(),
                                );
                            } else {
                                ui.small(
                                    RichText::new(mmr_text).color(Color32::from_gray(140)),
                                );
                            }
                        });

                        // ── Coluna direita ──
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // Botão "abrir"
                                let btn = ui.add(
                                    egui::Button::new(
                                        RichText::new("abrir").color(Color32::from_gray(180)),
                                    )
                                    .fill(Color32::from_gray(45))
                                    .corner_radius(4.0),
                                );
                                if btn.clicked() {
                                    // Handled below via inner.response
                                }

                                ui.add_space(8.0);

                                ui.vertical(|ui| {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Min),
                                        |ui| {
                                            ui.label(
                                                RichText::new(&mc)
                                                    .strong()
                                                    .size(ui.text_style_height(&egui::TextStyle::Body) * 1.1)
                                                    .color(Color32::from_gray(200)),
                                            );
                                        },
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Min),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!("{short_date} "))
                                                    .small()
                                                    .color(Color32::from_gray(100)),
                                            );
                                            ui.label(
                                                RichText::new(&time_part)
                                                    .small()
                                                    .strong()
                                                    .color(Color32::from_gray(200)),
                                            );
                                        },
                                    );
                                });
                            },
                        );
                    });
                }
                MetaState::Pending => {
                    ui.label(RichText::new(&entry.filename).monospace());
                    ui.small(RichText::new("lendo metadados…").italics());
                }
                MetaState::Unsupported(reason) => {
                    ui.label(
                        RichText::new(&entry.filename)
                            .monospace()
                            .color(Color32::from_gray(140)),
                    );
                    ui.small(
                        RichText::new(format!("⚠ não suportado: {reason}"))
                            .color(Color32::from_rgb(210, 170, 60))
                            .italics(),
                    );
                }
                MetaState::Failed(err) => {
                    ui.label(RichText::new(&entry.filename).monospace());
                    ui.small(
                        RichText::new(format!("falha: {err}"))
                            .color(Color32::LIGHT_RED)
                            .italics(),
                    );
                }
            }
        });

    // Pinta a borda esquerda colorida por raça (sobre o frame já renderizado).
    if let MetaState::Parsed(meta) = &entry.meta {
        let user_idx = find_user_index(meta, config).unwrap_or(0);
        let border_color = race_border_color(&meta.players[user_idx].race);
        let rect = inner.response.rect;
        let border_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 3.5, rect.bottom()),
        );
        ui.painter().rect_filled(border_rect, 4.0, border_color);
    }

    loadable && inner.response.interact(Sense::click()).clicked()
}

fn race_letter(race: &str) -> char {
    crate::utils::race_letter(race)
}

fn split_datetime(dt: &str) -> (String, String) {
    // "2025-12-18T06:44:53" → ("2025-12-18", "06:44")
    if dt.len() >= 16 {
        let date = dt[..10].to_string();
        let time = dt[11..16].to_string();
        (date, time)
    } else if dt.len() >= 10 {
        (dt[..10].to_string(), String::new())
    } else {
        (dt.to_string(), String::new())
    }
}

/// Helper para a `app.rs` pedir repaint quando houver trabalho em andamento.
pub fn keep_alive(ctx: &Context, library: &ReplayLibrary) {
    if library.scanning {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    } else if library.pending_count() > 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}
