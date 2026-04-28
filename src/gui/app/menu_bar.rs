// Hamburger menu — single ☰ button revealing a flat list of every app
// action (open / load / library / analysis / refresh / settings / about /
// quit). Lives inside the topbars (Library + Analysis), not in its own
// panel, so we don't burn a full row on six entries.

use egui::Ui;

use crate::locale::t;

use super::state::{AppState, Screen};

impl AppState {
    pub(super) fn show_menu_button(&mut self, ui: &mut Ui) {
        let lang = self.config.language;
        let ctx = ui.ctx().clone();
        let resp = ui
            .menu_button("\u{2630}", |ui| {
                if ui.button(t("menu.file.open", lang)).clicked() {
                    ui.close();
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter(t("dialog.filter.sc2_replay", lang), &["SC2Replay"])
                        .pick_file()
                    {
                        self.load_path(p);
                    }
                    #[cfg(target_arch = "wasm32")]
                    self.spawn_file_pick(&ctx);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button(t("menu.file.load_latest", lang)).clicked() {
                    ui.close();
                    self.try_load_latest();
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    if ui.button(t("menu.view.library", lang)).clicked() {
                        self.screen = Screen::Library;
                        ui.close();
                    }
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
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button(t("menu.view.refresh", lang)).clicked() {
                    self.refresh_library();
                    ui.close();
                }

                ui.separator();
                if ui.button(t("menu.view.settings", lang)).clicked() {
                    self.show_settings = true;
                    ui.close();
                }

                ui.separator();
                if ui.button(t("menu.help.about", lang)).clicked() {
                    ui.close();
                    self.show_about = true;
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    if ui.button(t("menu.file.quit", lang)).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            })
            .response;
        resp.on_hover_text(t("menu.tooltip", lang));
    }
}
