// Roteamento do painel central: biblioteca, aba de análise (Timeline /
// BuildOrder / Charts / Chat) ou tela de rename. Também consume a
// `LibraryAction` devolvida pela biblioteca após o render.

// See `app/mod.rs` for why we use deprecated `CentralPanel::show(ctx, ...)`.
// Note: only the outer `CentralPanel::show(ctx, ...)` is deprecated;
// the inner `Panel::left(...).show_inside(ui, ...)` for the filter
// sidebar is the correct API and is NOT deprecated.
#![allow(deprecated)]

use egui::{Color32, RichText};

use crate::library::{self, LibraryAction};
use crate::locale::{t, tf, Language};
use crate::tabs::{self, Tab};
use crate::tokens::{SPACE_M, SPACE_S, SPACE_XXL};

use super::state::{AppState, Screen};

impl AppState {
    pub(super) fn show_central(&mut self, ctx: &egui::Context) -> LibraryAction {
        let lang = self.config.language;
        let mut library_action = LibraryAction::None;
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = self.load_error.clone() {
                egui::Frame::new()
                    .fill(Color32::from_rgb(60, 20, 20))
                    .stroke(egui::Stroke::new(1.0, Color32::LIGHT_RED))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(err).color(Color32::LIGHT_RED));
                            if ui.small_button("×").clicked() {
                                self.load_error = None;
                            }
                        });
                    });
                ui.add_space(8.0);
            }

            match self.screen {
                Screen::Library => {
                    if self.library_sidebar_open {
                        let mut side_action = LibraryAction::None;
                        egui::Panel::left("library_filters")
                            .resizable(false)
                            .exact_size(260.0)
                            .show_inside(ui, |ui| {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    side_action = library::show_sidebar(
                                        ui,
                                        &mut self.library_filter,
                                        self.library.stats(),
                                        &self.config,
                                    );
                                });
                            });
                        if !matches!(side_action, LibraryAction::None) {
                            library_action = side_action;
                        }
                    }
                    let current = self.loaded.as_ref().map(|l| l.path.as_path());
                    let selected = self.library_selection.as_deref();
                    let central_action = library::show(
                        ui,
                        &self.library,
                        current,
                        selected,
                        &self.config,
                        &mut self.library_filter,
                    );
                    if !matches!(central_action, LibraryAction::None) {
                        library_action = central_action;
                    }

                    // Card lateral de detalhes da seleção. Renderizado
                    // *depois* da lista para ocupar a coluna direita do
                    // CentralPanel via `Panel::right`. Sem seleção, o
                    // card colapsa e a lista fica com 100% da largura.
                    if self.library_selection.is_some() {
                        if let Some(action) = self.show_library_detail_card(ui) {
                            library_action = action;
                        }
                    }
                }
                Screen::Analysis => match self.loaded.as_ref() {
                    None => empty_state(ui, lang),
                    Some(loaded) => match self.active_tab {
                        Tab::Timeline => tabs::timeline::show(
                            ui,
                            loaded,
                            &self.config,
                            &mut self.timeline_tab_loop,
                            &mut self.timeline_playing,
                            &mut self.timeline_playback_speed,
                            &mut self.timeline_show_heatmap,
                            &mut self.timeline_show_creep,
                            &mut self.timeline_show_map,
                        ),
                        Tab::BuildOrder => tabs::build_order::show(ui, loaded, &self.config),
                        Tab::Charts => tabs::charts::show(
                            ui,
                            loaded,
                            &self.config,
                            &mut self.charts_army_opts,
                        ),
                        Tab::Chat => tabs::chat::show(ui, loaded, &self.config),
                        Tab::Insights => {
                            if let Some(target) = tabs::insights::show(
                                ui,
                                loaded,
                                &self.config,
                                &mut self.insights_pov,
                            ) {
                                self.timeline_tab_loop = target;
                                self.active_tab = Tab::Timeline;
                            }
                        }
                    },
                },
                Screen::Rename => {
                    crate::rename::show(
                        ui,
                        &self.library,
                        &self.config,
                        &mut self.rename_template,
                        &mut self.rename_previews,
                        &mut self.rename_status,
                    );
                }
            }
        });
        library_action
    }

    pub(super) fn handle_library_action(&mut self, action: LibraryAction) {
        let lang = self.config.language;
        match action {
            LibraryAction::None => {}
            LibraryAction::Load(p) => self.load_path(p),
            LibraryAction::Select(p) => self.set_library_selection(Some(p)),
            LibraryAction::ClearSelection => self.set_library_selection(None),
            LibraryAction::Refresh => self.refresh_library(),
            LibraryAction::PickWorkingDir(p) => {
                self.config.working_dir = Some(p);
                if let Err(e) = self.config.save() {
                    self.set_toast(tf("toast.save_error", lang, &[("err", &e)]));
                }
                self.refresh_library();
            }
            LibraryAction::SaveDateRange(range) => {
                self.config.library_date_range = range;
                if let Err(e) = self.config.save() {
                    self.set_toast(tf("toast.save_config_error", lang, &[("err", &e)]));
                }
            }
            LibraryAction::OpenRename => {
                self.rename_previews =
                    crate::rename::generate_previews(&self.library, &self.rename_template);
                self.rename_status = None;
                self.screen = Screen::Rename;
            }
        }
    }
}

fn empty_state(ui: &mut egui::Ui, lang: Language) {
    ui.add_space(SPACE_XXL * 2.5);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("🎮").size(56.0));
        ui.add_space(SPACE_M);
        ui.label(RichText::new(t("empty.heading", lang)).heading());
        ui.add_space(SPACE_S);
        ui.label(RichText::new(t("empty.hint", lang)).italics());
    });
}
