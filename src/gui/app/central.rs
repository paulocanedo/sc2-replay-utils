// Roteamento do painel central: biblioteca ou aba de análise (Timeline
// / BuildOrder / Charts / Chat). Também consume a `LibraryAction`
// devolvida pela biblioteca após o render.

// See `app/mod.rs` for why we use deprecated `CentralPanel::show(ctx, ...)`.
// Note: only the outer `CentralPanel::show(ctx, ...)` is deprecated;
// the inner `Panel::left(...).show_inside(ui, ...)` for the filter
// sidebar is the correct API and is NOT deprecated.
#![allow(deprecated)]

use egui::{Color32, RichText};

#[cfg(not(target_arch = "wasm32"))]
use crate::library::{self, LibraryAction};
use crate::locale::{t, Language};
#[cfg(not(target_arch = "wasm32"))]
use crate::locale::tf;
use crate::tabs::{self, Tab};
use crate::tokens::{SPACE_M, SPACE_S, SPACE_XXL};

use super::state::{AppState, Screen};

impl AppState {
    /// Native: routes through Library / Analysis screens, returns the
    /// `LibraryAction` produced by the library UI for the caller to
    /// dispatch. Web: only the Analysis screen exists; returns `()`.
    #[cfg(not(target_arch = "wasm32"))]
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
                    // Hero (KPI strip) no topo — renderizado num
                    // `Panel::top` para ocupar 100% da largura à direita
                    // do filtro lateral. Reservar a faixa do topo ANTES
                    // do `Panel::right` faz com que o card de detalhes
                    // ocupe somente a área *abaixo* do hero, em vez de
                    // sentar ao lado e amputar a largura disponível
                    // para os KPIs.
                    let hero_action = egui::Panel::top("library_hero")
                        .resizable(false)
                        .frame(egui::Frame::new())
                        .show_inside(ui, |ui| {
                            library::show_hero(
                                ui,
                                &self.library,
                                &self.config,
                                &mut self.library_filter,
                            )
                        })
                        .inner;
                    if !matches!(hero_action, LibraryAction::None) {
                        library_action = hero_action;
                    }

                    // Card lateral de detalhes — renderizado ANTES da
                    // lista central para que `Panel::right` reserve sua
                    // coluna primeiro. Sob o hero, lado a lado com a
                    // lista. Sempre visível (placeholder quando vazio).
                    if let Some(action) = self.show_library_detail_card(ui) {
                        library_action = action;
                    }

                    let current = self.loaded.as_ref().map(|l| l.path.as_path());
                    let selected = self.library_selection.as_deref();
                    let central_action = library::show(
                        ui,
                        &self.library,
                        current,
                        selected,
                        &self.library_selected,
                        &mut self.library_save_template,
                        &self.config,
                        &mut self.library_filter,
                    );
                    if !matches!(central_action, LibraryAction::None) {
                        library_action = central_action;
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
                            &mut self.timeline_show_fog,
                            &mut self.timeline_fog_player,
                            &mut self.timeline_hovered_entity,
                        ),
                        Tab::BuildOrder => tabs::build_order::show(ui, loaded, &self.config),
                        Tab::Charts => tabs::charts::show(
                            ui,
                            loaded,
                            &self.config,
                            &mut self.charts_army_opts,
                            &mut self.charts_production_opts,
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
            }
        });
        library_action
    }

    #[cfg(not(target_arch = "wasm32"))]
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
            LibraryAction::SaveLibraryFilters { date_range, race } => {
                self.config.library_date_range = Some(date_range);
                self.config.library_race = race;
                if let Err(e) = self.config.save() {
                    self.set_toast(tf("toast.save_config_error", lang, &[("err", &e)]));
                }
            }
            LibraryAction::ToggleSelected(p) => {
                if !self.library_selected.remove(&p) {
                    self.library_selected.insert(p);
                }
            }
            LibraryAction::SetSelected(paths) => {
                self.library_selected = paths.into_iter().collect();
            }
            LibraryAction::ClearSelected => self.library_selected.clear(),
            LibraryAction::CopySelected => self.copy_selected_replays(),
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

/// Web central: renders the analysis tabs when a replay is loaded, or an
/// upload prompt otherwise. The library screen doesn't exist on web,
/// so there's no `LibraryAction` to return.
#[cfg(target_arch = "wasm32")]
impl AppState {
    pub(super) fn show_central(&mut self, ctx: &egui::Context) {
        let lang = self.config.language;
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

            match self.loaded.as_ref() {
                None => {
                    web_upload_prompt(ui, ctx, &self.pending_upload, lang);
                }
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
                        &mut self.timeline_show_fog,
                        &mut self.timeline_fog_player,
                        &mut self.timeline_hovered_entity,
                    ),
                    Tab::BuildOrder => tabs::build_order::show(ui, loaded, &self.config),
                    Tab::Charts => tabs::charts::show(
                        ui,
                        loaded,
                        &self.config,
                        &mut self.charts_army_opts,
                        &mut self.charts_production_opts,
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
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn web_upload_prompt(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    pending: &std::sync::Arc<std::sync::Mutex<Option<(String, Vec<u8>)>>>,
    lang: Language,
) {
    ui.add_space(SPACE_XXL * 2.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("📂").size(56.0));
        ui.add_space(SPACE_M);
        ui.label(RichText::new(t("empty.heading", lang)).heading());
        ui.add_space(SPACE_S);
        ui.label(RichText::new(t("empty.hint", lang)).italics());
        ui.add_space(SPACE_M);
        if ui
            .add(egui::Button::new(
                RichText::new("📂  Carregar replay").size(15.0),
            ))
            .clicked()
        {
            let pending = pending.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let dialog = rfd::AsyncFileDialog::new()
                    .add_filter("SC2Replay", &["SC2Replay"]);
                let Some(handle) = dialog.pick_file().await else {
                    return;
                };
                let file_name = handle.file_name();
                let bytes = handle.read().await;
                if let Ok(mut g) = pending.lock() {
                    *g = Some((file_name, bytes));
                }
                ctx.request_repaint();
            });
        }
    });
}
