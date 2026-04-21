// Topbars de cada tela — Library, Rename e Analysis. O topbar de análise
// é o mais rico: back + mapa + matchup + popover de detalhes + chips de
// jogadores + atalhos (abrir/rename) + a tab bar logo abaixo.

// See `app/mod.rs` for why we use deprecated `Panel::show(ctx, ...)`.
#![allow(deprecated)]

use std::path::PathBuf;

use egui::{Color32, Panel, RichText};

use crate::colors::{
    player_slot_color, player_slot_color_bright, LABEL_DIM, LABEL_SOFT, SURFACE_ALT,
};
use crate::config::AppConfig;
use crate::locale::{t, tf, Language};
use crate::replay_state::{build_matchup, fmt_time, format_date_short, LoadedReplay};
use crate::tabs::Tab;
use crate::tokens::{
    size_body, size_caption, size_subtitle, SPACE_M, SPACE_S, SPACE_XS, TOPBAR_HEIGHT,
};
use crate::widgets::{icon_button, labeled_value, race_badge, you_chip_label, NameDensity};

use super::state::{AppState, Screen};

impl AppState {
    pub(super) fn show_library_topbar(&mut self, ctx: &egui::Context) {
        let lang = self.config.language;
        let mut reload_clicked = false;
        let mut pick_dir: Option<PathBuf> = None;
        let mut rename_clicked = false;
        let mut toggle_sidebar = false;
        let working_dir_display = self
            .library
            .working_dir
            .as_ref()
            .map(|d| d.display().to_string());
        Panel::top("library_topbar")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE_ALT)
                    .inner_margin(egui::Margin::symmetric(SPACE_M as i8, SPACE_S as i8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t("library.title", lang))
                            .size(size_subtitle(&self.config))
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(SPACE_M);
                    match working_dir_display.as_deref() {
                        Some(dir) => {
                            ui.label(
                                RichText::new(format!("📁 {dir}"))
                                    .monospace()
                                    .size(size_caption(&self.config))
                                    .color(LABEL_DIM),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new(t("library.dir_unset", lang))
                                    .italics()
                                    .size(size_caption(&self.config))
                                    .color(LABEL_DIM),
                            );
                        }
                    }

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if icon_button(ui, "📂", t("library.pick_dir_tooltip", lang))
                                .clicked()
                            {
                                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                    pick_dir = Some(p);
                                }
                            }
                            ui.add_space(SPACE_XS);
                            if icon_button(ui, "✏", t("library.rename_tooltip", lang))
                                .clicked()
                            {
                                rename_clicked = true;
                            }
                            ui.add_space(SPACE_XS);
                            if icon_button(ui, "↻", t("library.reload_tooltip", lang))
                                .clicked()
                            {
                                reload_clicked = true;
                            }
                            ui.add_space(SPACE_XS);
                            if icon_button(
                                ui,
                                "☰",
                                t("library.sidebar.toggle_tooltip", lang),
                            )
                            .clicked()
                            {
                                toggle_sidebar = true;
                            }
                        },
                    );
                });
            });
        if reload_clicked {
            self.refresh_library();
        }
        if let Some(p) = pick_dir {
            self.config.working_dir = Some(p);
            if let Err(e) = self.config.save() {
                self.set_toast(tf("toast.save_error", lang, &[("err", &e)]));
            }
            self.refresh_library();
        }
        if rename_clicked {
            self.rename_previews =
                crate::rename::generate_previews(&self.library, &self.rename_template);
            self.rename_status = None;
            self.screen = Screen::Rename;
        }
        if toggle_sidebar {
            self.library_sidebar_open = !self.library_sidebar_open;
        }
    }

    pub(super) fn show_rename_topbar(&mut self, ctx: &egui::Context) {
        let lang = self.config.language;
        Panel::top("rename_bar")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE_ALT)
                    .inner_margin(egui::Margin::symmetric(SPACE_M as i8, SPACE_S as i8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if icon_button(ui, "←", t("topbar.back_tooltip", lang)).clicked() {
                        self.screen = Screen::Library;
                    }
                    ui.add_space(SPACE_S);
                    ui.label(
                        RichText::new(t("rename_bar.title", lang))
                            .size(size_subtitle(&self.config))
                            .strong(),
                    );
                });
            });
    }

    pub(super) fn show_analysis_topbar(&mut self, ctx: &egui::Context) {
        let lang = self.config.language;
        let mut back_clicked = false;
        let mut open_clicked = false;
        let mut rename_clicked = false;
        if let Some(loaded) = self.loaded.as_ref() {
            let user_idx = self
                .loaded
                .as_ref()
                .and_then(|l| l.user_player_index(&self.config.user_nicknames));
            Panel::top("analysis_topbar")
                .frame(
                    egui::Frame::new()
                        .fill(SURFACE_ALT)
                        .inner_margin(egui::Margin::symmetric(SPACE_M as i8, SPACE_S as i8)),
                )
                .show(ctx, |ui| {
                    analysis_topbar(
                        ui,
                        loaded,
                        &self.config,
                        user_idx,
                        lang,
                        &mut back_clicked,
                        &mut open_clicked,
                        &mut rename_clicked,
                    );
                });
        }
        if back_clicked {
            self.screen = Screen::Library;
        }
        if open_clicked {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter(t("dialog.filter.sc2_replay", lang), &["SC2Replay"])
                .pick_file()
            {
                self.load_path(p);
            }
        }
        if rename_clicked {
            self.rename_previews =
                crate::rename::generate_previews(&self.library, &self.rename_template);
            self.rename_status = None;
            self.screen = Screen::Rename;
        }

        Panel::top("tabs").show(ctx, |ui| {
            ui.add_space(SPACE_S);
            ui.horizontal(|ui| {
                for tab in Tab::ALL {
                    ui.selectable_value(&mut self.active_tab, tab, tab.label(lang));
                }
            });
            ui.add_space(SPACE_XS);
        });
    }
}

/// Renders the rich analysis top bar: back-to-library affordance, map
/// summary, per-player chips, details popover, rename/open shortcuts.
/// Fills what used to be the right-side `match_info` sidebar so the
/// tab content below can keep 100% of the viewport width.
fn analysis_topbar(
    ui: &mut egui::Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    user_idx: Option<usize>,
    lang: Language,
    back_clicked: &mut bool,
    open_clicked: &mut bool,
    rename_clicked: &mut bool,
) {
    let tl = &loaded.timeline;
    let matchup = build_matchup(&tl.players);
    let duration = fmt_time(tl.game_loops, tl.loops_per_second);
    let date_display = format_date_short(&tl.datetime, lang);

    ui.horizontal(|ui| {
        ui.set_min_height(TOPBAR_HEIGHT - (SPACE_S as f32) * 2.0);

        // ── Back + map summary (whole secondary line is the popover trigger) ──
        // `📚` is the same glyph the menu uses for "view library", so the
        // affordance reads consistently. A bare `←` glyph is missing from
        // egui's default fallback fonts and renders as ☐ on Windows.
        if icon_button(ui, "📚", t("topbar.back_tooltip", lang)).clicked() {
            *back_clicked = true;
        }
        ui.add_space(SPACE_S);

        ui.vertical(|ui| {
            ui.add_space(SPACE_XS);
            ui.label(
                RichText::new(&tl.map)
                    .size(size_subtitle(config))
                    .strong()
                    .color(Color32::WHITE),
            );
            // Whole secondary line acts as the "details" trigger — no
            // extra ⓘ glyph (which doesn't render on Windows). Hovering
            // gets a hint, click toggles the popover.
            let details_resp = ui
                .add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{matchup} \u{2022} {duration} \u{2022} {date_display}"
                        ))
                        .size(size_caption(config))
                        .monospace()
                        .color(LABEL_DIM)
                        .underline(),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_text(t("topbar.details_tooltip", lang));
            egui::Popup::from_toggle_button_response(&details_resp)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui: &mut egui::Ui| {
                    ui.set_min_width(220.0);
                    labeled_value(ui, t("topbar.detail.start", lang), &tl.datetime);
                    labeled_value(
                        ui,
                        t("topbar.detail.loops", lang),
                        &tl.game_loops.to_string(),
                    );
                    labeled_value(
                        ui,
                        t("topbar.detail.speed", lang),
                        &tf(
                            "topbar.speed_value",
                            lang,
                            &[("value", &format!("{:.1}", tl.loops_per_second))],
                        ),
                    );
                });
        });

        // ── Flex spacer + right cluster ─────────────────────────
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, "📂", t("topbar.open", lang)).clicked() {
                *open_clicked = true;
            }
            ui.add_space(SPACE_XS);
            // `✏` (BMP variant) renders with the default font; `✎` does not.
            if icon_button(ui, "✏", t("topbar.rename_tooltip", lang)).clicked() {
                *rename_clicked = true;
            }
            ui.add_space(SPACE_M);

            // Player chips flow right-to-left so P2 sits next to the
            // action buttons. We draw P2 first, then "vs", then P1.
            let players = &tl.players;
            if players.len() >= 2 {
                player_chip_topbar(ui, &players[1], 1, user_idx == Some(1), config, lang);
                ui.label(RichText::new(t("common.vs", lang)).color(LABEL_SOFT));
                player_chip_topbar(ui, &players[0], 0, user_idx == Some(0), config, lang);
            }
        });
    });
}

/// One player chip rendered inside the analysis top bar. Compact card
/// with a slot-coloured left stripe · bold name · race letter · MMR ·
/// optional YOU badge. Sized for a single-line top bar so it collapses
/// gracefully on narrow windows.
fn player_chip_topbar(
    ui: &mut egui::Ui,
    player: &crate::replay::PlayerTimeline,
    idx: usize,
    is_user: bool,
    config: &AppConfig,
    lang: Language,
) {
    let slot_stripe = player_slot_color(idx);
    let name_color = player_slot_color_bright(idx);
    let mmr_text = match player.mmr {
        Some(v) => v.to_string(),
        None => "—".to_string(),
    };

    let frame = egui::Frame::new()
        .fill(Color32::from_gray(36))
        .inner_margin(egui::Margin::symmetric(SPACE_M as i8, SPACE_XS as i8))
        .corner_radius(crate::tokens::RADIUS_CHIP);

    let inner = frame.show(ui, |ui| {
        // `ui.horizontal` is the only API that fits-to-content cleanly,
        // but it inherits the parent placer's direction (egui 0.34
        // ui.rs:2623) — and our parent is right-to-left. So we add
        // widgets in REVERSE of the desired visual order.
        // Visual we want: race · name · YOU? · MMR
        // Code order:     MMR · YOU? · name · race
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_S;
            ui.label(
                RichText::new(mmr_text)
                    .size(size_body(config))
                    .monospace()
                    .strong()
                    .color(LABEL_SOFT),
            );
            if is_user {
                ui.label(you_chip_label(config, lang));
            }
            ui.label(
                RichText::new(&player.name)
                    .size(size_caption(config))
                    .strong()
                    .color(name_color),
            );
            race_badge(ui, &player.race, NameDensity::Compact, config);
        });
    });

    // Slot-coloured left stripe over the rounded corner.
    let rect = inner.response.rect;
    let stripe = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.left() + 3.0, rect.bottom()),
    );
    ui.painter().rect_filled(
        stripe,
        egui::CornerRadius {
            nw: crate::tokens::RADIUS_CHIP as u8,
            sw: crate::tokens::RADIUS_CHIP as u8,
            ne: 0,
            se: 0,
        },
        slot_stripe,
    );
}
