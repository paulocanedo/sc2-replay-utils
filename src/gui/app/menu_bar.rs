// Barra de menu no topo da janela (sempre visível). Em um único lugar
// expõe abrir/carregar replay, alternar tela, abrir settings e about.

// See `app/mod.rs` for why we use deprecated `Panel::show(ctx, ...)`.
#![allow(deprecated)]

use egui::{Context, Panel};

use crate::locale::t;

use super::state::{AppState, Screen};

impl AppState {
    pub(super) fn show_menu_bar(&mut self, ctx: &Context) {
        let lang = self.config.language;
        Panel::top("menubar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(t("menu.file", lang), |ui| {
                    if ui.button(t("menu.file.open", lang)).clicked() {
                        ui.close();
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter(t("dialog.filter.sc2_replay", lang), &["SC2Replay"])
                            .pick_file()
                        {
                            self.load_path(p);
                        }
                    }
                    if ui.button(t("menu.file.load_latest", lang)).clicked() {
                        ui.close();
                        self.try_load_latest();
                    }
                    ui.separator();
                    if ui.button(t("menu.file.quit", lang)).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button(t("menu.view", lang), |ui| {
                    if ui.button(t("menu.view.library", lang)).clicked() {
                        self.screen = Screen::Library;
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.loaded.is_some(),
                            egui::Button::new(t("menu.view.analysis", lang)),
                        )
                        .clicked()
                    {
                        self.screen = Screen::Analysis;
                        ui.close();
                    }
                    if ui.button(t("menu.view.rename", lang)).clicked() {
                        self.rename_previews = crate::rename::generate_previews(
                            &self.library,
                            &self.rename_template,
                        );
                        self.screen = Screen::Rename;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t("menu.view.settings", lang)).clicked() {
                        self.show_settings = true;
                        ui.close();
                    }
                });
                ui.menu_button(t("menu.help", lang), |ui| {
                    if ui.button(t("menu.help.about", lang)).clicked() {
                        ui.close();
                        self.show_about = true;
                    }
                });
            });
        });
    }
}
