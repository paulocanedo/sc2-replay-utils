// Barra de status inferior persistente em todas as telas. Mostra o
// replay atualmente carregado (ou estado da biblioteca), indica se o
// watcher está ativo e renderiza toasts transitórios.

use egui::{Color32, Panel, RichText};

use crate::colors::LABEL_DIM;
use crate::locale::{t, tf};
use crate::tokens::{SPACE_M, SPACE_XS};

use super::state::{AppState, Screen};

impl AppState {
    pub(super) fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;

        // Snapshot dos campos antes do closure para evitar conflitos de
        // borrow (toast_visible empresta self inteiro).
        let loaded_file = self.loaded.as_ref().map(|l| l.file_name());
        let watcher_dir = self
            .watcher
            .as_ref()
            .map(|w| w.watched_dir().to_path_buf());
        let toast_msg = self.toast_visible().map(|s| s.to_string());
        let screen = self.screen;
        let library_total = self.library.entries.len();
        let library_pending = self.library.pending_count();
        let library_scanning = self.library.scanning;

        Panel::bottom("status_bar").show_inside(ui, |ui| {
            ui.add_space(SPACE_XS);
            ui.horizontal(|ui| {
                match screen {
                    Screen::Analysis => match &loaded_file {
                        Some(file) => {
                            ui.label("📼");
                            ui.monospace(file);
                        }
                        None => {
                            ui.label(
                                RichText::new(t("app.status.no_replay", lang))
                                    .italics()
                                    .small(),
                            );
                        }
                    },
                    Screen::Library => {
                        let msg = if library_scanning {
                            tf(
                                "status.library.scanning",
                                lang,
                                &[("found", &library_total.to_string())],
                            )
                        } else if library_pending > 0 {
                            tf(
                                "status.library.parsing",
                                lang,
                                &[
                                    ("pending", &library_pending.to_string()),
                                    ("total", &library_total.to_string()),
                                ],
                            )
                        } else {
                            tf(
                                "status.library.count",
                                lang,
                                &[("total", &library_total.to_string())],
                            )
                        };
                        ui.small(RichText::new(msg).color(LABEL_DIM));
                    }
                    Screen::Rename => {
                        ui.small(
                            RichText::new(t("rename_bar.title", lang))
                                .italics()
                                .color(LABEL_DIM),
                        );
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(dir) = &watcher_dir {
                        ui.label("👁").on_hover_text(tf(
                            "app.status.watching",
                            lang,
                            &[("dir", &dir.display().to_string())],
                        ));
                    }
                    if let Some(msg) = toast_msg {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(28, 60, 28))
                            .stroke(egui::Stroke::new(1.0, Color32::LIGHT_GREEN))
                            .inner_margin(egui::Margin::symmetric(SPACE_M as i8, SPACE_XS as i8))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).color(Color32::LIGHT_GREEN).small());
                            });
                    }
                });
            });
            ui.add_space(SPACE_XS);
        });
    }
}
