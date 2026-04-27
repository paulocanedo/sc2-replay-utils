// Barra de status inferior persistente em todas as telas. Mostra o
// replay atualmente carregado (ou estado da biblioteca), indica se o
// watcher está ativo e renderiza toasts transitórios.

// See `app/mod.rs` for why we use deprecated `Panel::show(ctx, ...)`.
#![allow(deprecated)]

use egui::{Color32, Panel, RichText};

use crate::colors::LABEL_DIM;
use crate::locale::{t, tf};
use crate::tokens::{SPACE_M, SPACE_XS, STATUSBAR_HEIGHT};

use super::state::{AppState, Screen};

impl AppState {
    pub(super) fn show_status_bar(&mut self, ctx: &egui::Context) {
        let lang = self.config.language;

        // Snapshot dos campos antes do closure para evitar conflitos de
        // borrow (toast_visible empresta self inteiro).
        let loaded_file = self.loaded.as_ref().map(|l| l.file_name());
        #[cfg(not(target_arch = "wasm32"))]
        let watcher_dir = self
            .watcher
            .as_ref()
            .map(|w| w.watched_dir().to_path_buf());
        let toast_msg = self.toast_visible().map(|s| s.to_string());
        let screen = self.screen;
        #[cfg(not(target_arch = "wasm32"))]
        let library_total = self.library.entries.len();
        #[cfg(not(target_arch = "wasm32"))]
        let library_pending = self.library.pending_count();
        #[cfg(not(target_arch = "wasm32"))]
        let library_scanning = self.library.scanning;

        // `exact_size` pins the reserved height so that `Panel::bottom` always
        // carves out the same strip on every frame. Without it egui falls
        // back to `spacing.interact_size.y + margin` on the first frame (no
        // `PanelState` cached yet), which means the very first render can
        // under-reserve and let the virtualized replay list below it paint
        // over the bar. `STATUSBAR_HEIGHT` is the design-token value the
        // bar is drawn at (≈22 px, covers `SPACE_XS*2 + small text line`).
        //
        // `.show(ctx, ...)` (not `show_inside`) is load-bearing: it calls
        // `pass_state.allocate_bottom_panel(rect)` so the subsequent
        // `CentralPanel::show(ctx, ...)` sees the shrunken `available_rect`.
        // With `show_inside` on a top-level ui, only the parent ui's cursor
        // is updated — the ctx-level allocation is skipped, and in some
        // configurations the CentralPanel ends up using a rect that still
        // includes the bottom strip, letting its ScrollArea paint over us.
        Panel::bottom("status_bar")
            .resizable(false)
            .exact_size(STATUSBAR_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(18))
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ctx, |ui| {
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
                                RichText::new(t("app.status.no_replay", lang)).italics(),
                            );
                        }
                    },
                    #[cfg(not(target_arch = "wasm32"))]
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
                        ui.label(RichText::new(msg).color(LABEL_DIM));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
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
                                ui.label(RichText::new(msg).color(Color32::LIGHT_GREEN));
                            });
                    }
                });
            });
            ui.add_space(SPACE_XS);
        });
    }
}
