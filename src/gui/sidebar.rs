// Painel lateral direito com resumo da partida, cards de jogadores e detalhes.

use egui::{Color32, RichText, ScrollArea};

use crate::colors::{player_slot_color, user_fill, CARD_FILL, LABEL_DIM, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::locale::{t, tf, Language};
use crate::replay_state::{fmt_time, LoadedReplay};

pub fn sidebar_content(ui: &mut egui::Ui, loaded: &LoadedReplay, config: &AppConfig) {
    let lang = config.language;
    ui.add_space(8.0);

    ScrollArea::vertical().id_salt("sidebar_scroll").show(ui, |ui| {
        // ── Resumo ──────────────────────────────────────────────
        ui.heading(t("sidebar.summary", lang));
        ui.separator();
        ui.add_space(4.0);

        let matchup = build_matchup(&loaded.timeline.players);
        let duration = fmt_time(loaded.timeline.game_loops, loaded.timeline.loops_per_second);
        let date_display = format_date_short(&loaded.timeline.datetime, lang);

        egui::Frame::new()
            .fill(CARD_FILL)
            .stroke(egui::Stroke::new(0.5, Color32::from_gray(50)))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(&loaded.timeline.map)
                        .size(16.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(
                    RichText::new(format!("{matchup} \u{2022} {duration}"))
                        .color(Color32::from_gray(180)),
                );
                ui.label(
                    RichText::new(&date_display)
                        .small()
                        .color(LABEL_DIM),
                );
            });

        ui.add_space(12.0);

        // ── Jogadores ───────────────────────────────────────────
        ui.heading(t("sidebar.players", lang));
        ui.separator();
        ui.add_space(4.0);

        let last = loaded.timeline.players.len().saturating_sub(1);
        for (i, p) in loaded.timeline.players.iter().enumerate() {
            let is_user = config.is_user(&p.name);
            player_card(ui, p, i, is_user, lang);
            if i != last {
                ui.add_space(6.0);
            }
        }

        ui.add_space(12.0);

        // ── Detalhes ────────────────────────────────────────────
        ui.heading(t("sidebar.details", lang));
        ui.separator();
        ui.add_space(4.0);

        detail_row(ui, t("sidebar.detail.start", lang), &loaded.timeline.datetime);
        detail_row(
            ui,
            t("sidebar.detail.loops", lang),
            &loaded.timeline.game_loops.to_string(),
        );
        detail_row(
            ui,
            t("sidebar.detail.speed", lang),
            &tf(
                "sidebar.speed_value",
                lang,
                &[("value", &format!("{:.1}", loaded.timeline.loops_per_second))],
            ),
        );
    });
}

/// Renderiza o card de um jogador com borda lateral colorida (cor do slot).
fn player_card(
    ui: &mut egui::Ui,
    player: &crate::replay::PlayerTimeline,
    index: usize,
    is_user: bool,
    lang: Language,
) {
    let slot_color = player_slot_color(index);
    let fill = if is_user {
        user_fill(index)
    } else {
        CARD_FILL
    };

    let resp = egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(0.5, Color32::from_gray(50)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Linha 1: nome + chip "VOCÊ" (opcional) + MMR à direita.
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&player.name)
                        .size(16.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                if let Some(toon) = player.toon.as_ref() {
                    let handle = toon.handle();
                    if let Some(url) = toon.battlenet_url() {
                        // `on_hover_ui` adia a composição do tooltip —
                        // o handle/URL só vira WidgetText quando o
                        // usuário paira sobre o botão, evitando alocação
                        // por frame no hot path.
                        let resp = ui.small_button("🔗").on_hover_ui(|ui| {
                            ui.label(handle);
                            ui.label(url);
                        });
                        if resp.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    } else {
                        // Região desconhecida: mostra só o handle.
                        ui.label(
                            RichText::new(handle)
                                .small()
                                .color(Color32::from_gray(130)),
                        );
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // MMR primeiro (fica mais à direita).
                    match player.mmr {
                        Some(mmr) => {
                            ui.label(
                                RichText::new(mmr.to_string())
                                    .size(16.0)
                                    .strong()
                                    .color(Color32::from_gray(220)),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new("—")
                                    .size(16.0)
                                    .color(Color32::from_gray(100)),
                            );
                        }
                    }
                    if is_user {
                        ui.label(
                            RichText::new(t("sidebar.you_chip", lang))
                                .small()
                                .strong()
                                .color(USER_CHIP_FG)
                                .background_color(USER_CHIP_BG),
                        );
                    }
                });
            });

            // Linha 2: ícone raça + nome da raça.
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    race_icon(&player.race),
                    race_full_name(&player.race, lang),
                ))
                .color(Color32::from_gray(170)),
            );

            // Clan opcional abaixo da raça.
            if !player.clan.is_empty() {
                ui.label(
                    RichText::new(format!("    [{}]", player.clan))
                        .small()
                        .color(Color32::from_gray(130))
                        .italics(),
                );
            }
        });

    // Pinta borda lateral colorida sobre a borda do frame.
    let rect = resp.response.rect;
    let accent = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.left() + 3.0, rect.bottom()),
    );
    ui.painter().rect_filled(
        accent,
        egui::CornerRadius {
            nw: 6,
            sw: 6,
            ne: 0,
            se: 0,
        },
        slot_color,
    );
}

/// Emoji/ícone para a raça.
fn race_icon(race: &str) -> &'static str {
    match race.to_ascii_lowercase().chars().next() {
        Some('p') => "💎",
        Some('t') => "⚙",
        Some('z') => "🦷",
        _ => "❓",
    }
}

/// Display name for the race, honoring the UI language.
fn race_full_name<'a>(race: &'a str, lang: Language) -> &'a str {
    match race.to_ascii_lowercase().as_str() {
        "terr" | "terran" => t("race.terran", lang),
        "prot" | "protoss" => t("race.protoss", lang),
        "zerg" => t("race.zerg", lang),
        _ => race,
    }
}

/// Monta o matchup ("PvT", "ZvP", etc.) a partir dos jogadores.
fn build_matchup(players: &[crate::replay::PlayerTimeline]) -> String {
    if players.len() >= 2 {
        format!("{}v{}", race_letter(&players[0].race), race_letter(&players[1].race))
    } else {
        String::from("—")
    }
}

/// Letra inicial da raça (T/P/Z/R).
fn race_letter(race: &str) -> char {
    crate::utils::race_letter(race)
}

/// Formats "2026-04-10T17:46:40" → e.g. "10 apr 2026" / "10 abr 2026"
/// depending on language.
fn format_date_short(datetime: &str, lang: Language) -> String {
    let date_part = datetime.split('T').next().unwrap_or(datetime);
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() == 3 {
        let key = format!("month.{}", parts[1]);
        let month = t(&key, lang);
        let day = parts[2].trim_start_matches('0');
        format!("{day} {month} {}", parts[0])
    } else {
        date_part.to_string()
    }
}

/// Row de detalhe com label à esquerda, valor e chevron à direita.
fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .strong()
                .color(Color32::from_gray(190)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("›")
                    .color(Color32::from_gray(80)),
            );
            ui.label(
                RichText::new(value)
                    .color(Color32::from_gray(160)),
            );
        });
    });
    ui.separator();
}
