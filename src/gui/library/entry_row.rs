// Renderização de uma entrada individual da biblioteca + helpers de
// metadados usados para filtragem, ordenação e exibição.

use egui::{Color32, RichText, Sense, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};

use super::types::{LibraryEntry, MetaState, ParsedMeta, PlayerMeta};

// ── Helpers de filtro/sort ────────────────────────────────────────────

pub(super) fn find_user_player<'a>(meta: &'a ParsedMeta, config: &AppConfig) -> Option<&'a PlayerMeta> {
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

pub(super) fn find_user_index(meta: &ParsedMeta, config: &AppConfig) -> Option<usize> {
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

pub(super) fn matchup_code(meta: &ParsedMeta, config: &AppConfig) -> String {
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

pub(super) fn get_duration(entry: &LibraryEntry) -> u32 {
    match &entry.meta {
        MetaState::Parsed(m) => m.duration_seconds,
        _ => 0,
    }
}

pub(super) fn get_user_mmr(entry: &LibraryEntry, config: &AppConfig) -> i32 {
    match &entry.meta {
        MetaState::Parsed(m) => find_user_player(m, config)
            .and_then(|p| p.mmr)
            .unwrap_or(0),
        _ => 0,
    }
}

pub(super) fn get_map(entry: &LibraryEntry) -> &str {
    match &entry.meta {
        MetaState::Parsed(m) => &m.map,
        _ => "",
    }
}

// ── UI components ────────────────────────────────────────────────────

/// Altura de cada linha da lista virtualizada.
pub(super) fn row_height(ui: &Ui) -> f32 {
    use egui::TextStyle;
    let body = ui.text_style_height(&TextStyle::Body);
    let small = ui.text_style_height(&TextStyle::Small);
    let gap = ui.spacing().item_spacing.y;
    body + small * 2.0 + gap * 2.0 + FRAME_CHROME_V
}

const FRAME_CHROME_V: f32 = 13.0;

// Cores de raça — distintas das cores de slot P1/P2 (vermelho/azul)
// para que "raça" e "jogador" nunca se confundam visualmente.
pub(super) const RACE_COLOR_TERRAN: Color32 = Color32::from_rgb(90, 130, 180);   // azul aço
pub(super) const RACE_COLOR_PROTOSS: Color32 = Color32::from_rgb(120, 180, 100); // verde dourado
pub(super) const RACE_COLOR_ZERG: Color32 = Color32::from_rgb(160, 80, 150);     // roxo magenta

/// Cor da borda esquerda baseada na raça.
fn race_border_color(race: &str) -> Color32 {
    match race_letter(race) {
        'T' => RACE_COLOR_TERRAN,
        'P' => RACE_COLOR_PROTOSS,
        'Z' => RACE_COLOR_ZERG,
        _ => Color32::from_gray(100),
    }
}

pub(super) fn entry_row(
    ui: &mut Ui,
    entry: &LibraryEntry,
    is_current: bool,
    config: &AppConfig,
    row_h: f32,
) -> bool {
    let lang = config.language;
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
                                        RichText::new(t("library.entry.open", lang))
                                            .color(Color32::from_gray(180)),
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
                    ui.small(RichText::new(t("library.entry.parsing", lang)).italics());
                }
                MetaState::Unsupported(reason) => {
                    ui.label(
                        RichText::new(&entry.filename)
                            .monospace()
                            .color(Color32::from_gray(140)),
                    );
                    ui.small(
                        RichText::new(tf(
                            "library.entry.unsupported",
                            lang,
                            &[("reason", reason)],
                        ))
                        .color(Color32::from_rgb(210, 170, 60))
                        .italics(),
                    );
                }
                MetaState::Failed(err) => {
                    ui.label(RichText::new(&entry.filename).monospace());
                    ui.small(
                        RichText::new(tf("library.entry.failed", lang, &[("err", err)]))
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

pub(super) fn race_letter(race: &str) -> char {
    crate::utils::race_letter(race)
}

pub(super) fn split_datetime(dt: &str) -> (String, String) {
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
