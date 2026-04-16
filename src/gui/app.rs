// AppState + impl eframe::App.
//
// A UI alterna entre duas telas mutuamente exclusivas:
// - `Screen::Library`: a biblioteca de replays ocupa toda a janela.
// - `Screen::Analysis`: replay bar + tab bar + central panel + painel
//   direito de jogadores ocupam toda a janela.
//
// Em ambas as telas há uma status bar inferior persistente exibindo o
// replay atualmente carregado, o estado do watcher e os toasts.
//
// On first launch (or whenever `config.language_selected` is false),
// a blocking modal prompts the user to pick a language before any
// other UI is reachable. Persisting the choice sets
// `language_selected = true`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Color32, Context, Panel, RichText};

use crate::colors::{
    player_slot_color, LABEL_DIM, LABEL_SOFT, SURFACE_ALT, USER_CHIP_BG, USER_CHIP_FG,
};
use crate::config::AppConfig;
use crate::library::{self, LibraryAction, ReplayLibrary};
use crate::locale::{t, tf, Language};
use crate::production_efficiency::EfficiencyTarget;
use crate::replay_state::{build_matchup, fmt_time, format_date_short, LoadedReplay};
use crate::tabs::{self, Tab};
use crate::tokens::{
    size_body, size_caption, size_subtitle, SPACE_M, SPACE_S, SPACE_XS, TOPBAR_HEIGHT,
};
use crate::ui_settings;
use crate::watcher::ReplayWatcher;
use crate::widgets::{icon_button, labeled_value};

const TOAST_TTL: Duration = Duration::from_secs(4);

/// Tela atualmente ativa. A transição é dirigida por intent do usuário,
/// não pelo estado de `loaded` — ao voltar para `Library`, o replay
/// carregado permanece na memória e o usuário pode reentrar na análise.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Library,
    Analysis,
    Rename,
}

pub struct AppState {
    pub config: AppConfig,
    pub loaded: Option<LoadedReplay>,
    pub load_error: Option<String>,
    pub active_tab: Tab,
    pub screen: Screen,
    pub show_settings: bool,
    pub nickname_input: String,
    pub watcher: Option<ReplayWatcher>,
    pub toast: Option<(String, Instant)>,
    pub library: ReplayLibrary,
    pub library_filter: library::LibraryFilter,
    /// Game loop selecionado no slider da aba Timeline (mini-mapa).
    /// Resetado a cada `load_path` para que troca de replay sempre
    /// comece em t=0.
    pub timeline_tab_loop: u32,
    /// Opções do plot principal de army (métrica, grouping, checkboxes).
    pub charts_army_opts: tabs::charts::ArmyChartOptions,
    /// Alvo do novo gráfico de eficiência de produção (workers x army).
    pub charts_efficiency_target: EfficiencyTarget,
    pub show_about: bool,
    pub timeline_show_heatmap: bool,
    pub timeline_show_creep: bool,
    pub timeline_show_map: bool,
    /// Template de renomeação em lote.
    pub rename_template: String,
    /// Previews gerados a partir do template + biblioteca.
    pub rename_previews: Vec<(std::path::PathBuf, String)>,
    /// Status da última operação de rename.
    pub rename_status: Option<String>,
    /// Carregamento do replay mais recente adiado até o scanner terminar.
    pub pending_load_latest: bool,
    /// Transient draft of the language picker (first-run modal). We keep
    /// the draft separate from `config.language` so cancelling the
    /// modal leaves the real config alone.
    pub language_draft: Language,
}

impl AppState {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        apply_style(&cc.egui_ctx, &config);

        let library_filter = library::LibraryFilter::from_config(&config);
        let language_draft = config.language;
        let mut me = Self {
            config,
            loaded: None,
            load_error: None,
            active_tab: Tab::Timeline,
            screen: Screen::Library,
            show_settings: false,
            nickname_input: String::new(),
            watcher: None,
            toast: None,
            library: ReplayLibrary::new(),
            library_filter,
            timeline_tab_loop: 0,
            charts_army_opts: tabs::charts::ArmyChartOptions::default(),
            charts_efficiency_target: EfficiencyTarget::Workers,
            show_about: false,
            timeline_show_heatmap: false,
            timeline_show_creep: true,
            timeline_show_map: true,
            rename_template: crate::rename::DEFAULT_TEMPLATE.to_string(),
            rename_previews: Vec::new(),
            rename_status: None,
            pending_load_latest: false,
            language_draft,
        };
        me.restart_watcher();
        me.refresh_library();
        if me.config.auto_load_latest {
            me.pending_load_latest = true;
        }
        me
    }

    /// Recarrega a biblioteca a partir do diretório de trabalho efetivo
    /// (persistido no config ou auto-detectado a partir do SC2).
    fn refresh_library(&mut self) {
        if let Some(dir) = self.config.effective_working_dir() {
            self.library.refresh(&dir);
        }
    }

    fn try_load_latest(&mut self) {
        // Se o scanner já rodou, usa o resultado dele (sem I/O extra).
        if let Some(p) = self.library.scan_latest.clone() {
            self.load_path(p);
            return;
        }
        let lang = self.config.language;
        let Some(dir) = self.config.effective_working_dir() else {
            self.set_toast(t("toast.no_working_dir", lang).to_string());
            return;
        };
        match crate::utils::find_latest_replay(&dir) {
            Some(p) => self.load_path(p),
            None => self.set_toast(tf(
                "toast.no_replays_found",
                lang,
                &[("dir", &dir.display().to_string())],
            )),
        }
    }

    fn load_path(&mut self, p: PathBuf) {
        let max_time = self.config.default_max_time;
        let lang = self.config.language;
        match LoadedReplay::load(&p, max_time) {
            Ok(r) => {
                self.loaded = Some(r);
                self.load_error = None;
                // Reset do scrubbing da aba Timeline: replay novo
                // sempre começa em t=0.
                self.timeline_tab_loop = 0;
                // Carregar com sucesso sempre transiciona para a Tela
                // Análise — é a única forma de chegar lá.
                self.screen = Screen::Analysis;
            }
            Err(e) => {
                self.load_error = Some(tf(
                    "error.load_failed",
                    lang,
                    &[("path", &p.display().to_string()), ("err", &e.to_string())],
                ));
            }
        }
    }

    fn restart_watcher(&mut self) {
        self.watcher = None;
        if !self.config.watch_replays {
            return;
        }
        let Some(dir) = self.config.effective_working_dir() else {
            return;
        };
        match ReplayWatcher::start(&dir) {
            Ok(w) => self.watcher = Some(w),
            Err(e) => eprintln!("watcher: falha ao observar {}: {}", dir.display(), e),
        }
    }

    fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn poll_watcher(&mut self, ctx: &Context) {
        let Some(w) = self.watcher.as_ref() else { return };
        if let Some(path) = w.poll_latest() {
            let lang = self.config.language;
            if self.config.auto_load_on_new_replay {
                self.load_path(path.clone());
                self.set_toast(tf(
                    "toast.new_replay_loaded",
                    lang,
                    &[("file", &file_name(&path))],
                ));
            } else {
                self.set_toast(tf(
                    "toast.new_replay_available",
                    lang,
                    &[("file", &file_name(&path))],
                ));
            }
            ctx.request_repaint();
        }
    }

    fn toast_visible(&self) -> Option<&str> {
        let (msg, t) = self.toast.as_ref()?;
        if t.elapsed() < TOAST_TTL {
            Some(msg.as_str())
        } else {
            None
        }
    }
}

impl eframe::App for AppState {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // -------- First-run language prompt (modal) --------
        // Renders before anything else and blocks interaction elsewhere
        // by simply not painting the rest of the UI when open.
        if !self.config.language_selected {
            language_prompt(&ctx, &mut self.language_draft, &mut self.config);
            // While the modal is open we still want a repaint so that
            // the language preview updates immediately.
            ctx.request_repaint();
            return;
        }

        let lang = self.config.language;

        // Polling do watcher ANTES de qualquer painel.
        self.poll_watcher(&ctx);
        // Drena resultados do worker da biblioteca.
        if self.library.poll() {
            ctx.request_repaint();
        }
        // Carrega o replay mais recente quando o scanner terminar.
        if self.pending_load_latest && !self.library.scanning {
            self.pending_load_latest = false;
            if let Some(path) = self.library.scan_latest.clone() {
                self.load_path(path);
            }
        }

        // Guarda: Tela Análise exige replay carregado. Se por qualquer
        // motivo o estado divergir, força fallback para a biblioteca.
        if self.screen == Screen::Analysis && self.loaded.is_none() {
            self.screen = Screen::Library;
        }

        // -------- Menu bar (sempre) --------
        Panel::top("menubar").show_inside(ui, |ui| {
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
                        self.rename_previews = crate::rename::generate_previews(&self.library, &self.rename_template);
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

        // -------- Topbar Library --------
        if self.screen == Screen::Library {
            let mut reload_clicked = false;
            let mut pick_dir: Option<PathBuf> = None;
            let mut rename_clicked = false;
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
                .show_inside(ui, |ui| {
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
        }

        // -------- Barra de navegação Rename --------
        if self.screen == Screen::Rename {
            Panel::top("rename_bar")
                .frame(
                    egui::Frame::new()
                        .fill(SURFACE_ALT)
                        .inner_margin(egui::Margin::symmetric(SPACE_M as i8, SPACE_S as i8)),
                )
                .show_inside(ui, |ui| {
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

        // -------- Topbar + Tab bar (apenas Tela Análise) --------
        if self.screen == Screen::Analysis {
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
                    .show_inside(ui, |ui| {
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

            Panel::top("tabs").show_inside(ui, |ui| {
                ui.add_space(SPACE_S);
                ui.horizontal(|ui| {
                    for tab in Tab::ALL {
                        ui.selectable_value(&mut self.active_tab, tab, tab.label(lang));
                    }
                });
                ui.add_space(SPACE_XS);
            });
        }

        // -------- Status bar inferior (sempre visível) --------
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

        // -------- Central --------
        let mut library_action = LibraryAction::None;
        egui::CentralPanel::default().show_inside(ui, |ui| {
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
                    let current = self.loaded.as_ref().map(|l| l.path.as_path());
                    library_action = library::show(
                        ui,
                        &self.library,
                        current,
                        &self.config,
                        &mut self.library_filter,
                    );
                }
                Screen::Analysis => match self.loaded.as_ref() {
                    None => empty_state(ui, lang),
                    Some(loaded) => match self.active_tab {
                        Tab::Timeline => tabs::timeline::show(
                            ui,
                            loaded,
                            &self.config,
                            &mut self.timeline_tab_loop,
                            &mut self.timeline_show_heatmap,
                            &mut self.timeline_show_creep,
                            &mut self.timeline_show_map,
                        ),
                        Tab::BuildOrder => tabs::build_order::show(ui, loaded, &self.config),
                        Tab::Charts => tabs::charts::show(ui, loaded, &self.config, &mut self.charts_army_opts, &mut self.charts_efficiency_target),
                        Tab::Chat => tabs::chat::show(ui, loaded, &self.config),
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

        // Processa ação pedida pela biblioteca (somente válida se a Tela
        // Biblioteca foi renderizada neste frame).
        match library_action {
            LibraryAction::None => {}
            LibraryAction::Load(p) => self.load_path(p),
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
                self.rename_previews = crate::rename::generate_previews(&self.library, &self.rename_template);
                self.rename_status = None;
                self.screen = Screen::Rename;
            }
        }

        // Mantém repaint enquanto a biblioteca estiver parseando em background.
        library::keep_alive(&ctx, &self.library);

        // -------- Settings window --------
        let prev_effective_dir = self.config.effective_working_dir();
        let outcome = ui_settings::show(
            &ctx,
            &mut self.show_settings,
            &mut self.config,
            &mut self.nickname_input,
        );
        if outcome.saved {
            match self.config.save() {
                Ok(()) => self.set_toast(t("toast.settings_saved", lang).to_string()),
                Err(e) => self.set_toast(tf("toast.save_error", lang, &[("err", &e)])),
            }
            apply_style(&ctx, &self.config);
            self.restart_watcher();
            if self.config.effective_working_dir() != prev_effective_dir {
                self.refresh_library();
            }
        } else if outcome.reset_defaults {
            apply_style(&ctx, &self.config);
            if self.config.effective_working_dir() != prev_effective_dir {
                self.refresh_library();
            }
        }

        // -------- About window --------
        if self.show_about {
            egui::Window::new(t("about.title", lang))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.heading(t("app.title", lang));
                        ui.label(tf(
                            "about.version",
                            lang,
                            &[("version", env!("CARGO_PKG_VERSION"))],
                        ));
                        ui.add_space(12.0);
                        ui.label(t("about.description", lang));
                        ui.add_space(12.0);
                        ui.label(RichText::new(t("about.author_label", lang)).strong());
                        ui.label(t("about.author_name", lang));
                        ui.add_space(12.0);
                        ui.label(RichText::new(t("about.tech_label", lang)).strong());
                        ui.label(t("about.tech_value", lang));
                        ui.add_space(16.0);
                        if ui.button(t("about.close", lang)).clicked() {
                            self.show_about = false;
                        }
                        ui.add_space(4.0);
                    });
                });
        }

        // Mantém o ciclo de polling do watcher vivo mesmo sem input.
        if self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.library.save_cache();
        let _ = self.config.save();
    }
}

/// First-run modal that forces the user to pick a UI language. Uses a
/// bilingual title/description so it's intelligible regardless of the
/// default. Once confirmed, `config.language_selected` is set and the
/// rest of the app becomes reachable.
fn language_prompt(ctx: &Context, draft: &mut Language, config: &mut AppConfig) {
    egui::Window::new(t("language_prompt.title", *draft))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(t("language_prompt.description", *draft));
                ui.add_space(12.0);
                for &lang in Language::all() {
                    ui.radio_value(draft, lang, lang.label());
                }
                ui.add_space(16.0);
                if ui
                    .add_sized(
                        [160.0, 32.0],
                        egui::Button::new(
                            RichText::new(t("language_prompt.confirm", *draft)).strong(),
                        ),
                    )
                    .clicked()
                {
                    config.language = *draft;
                    config.language_selected = true;
                    let _ = config.save();
                }
                ui.add_space(4.0);
            });
        });
}

fn empty_state(ui: &mut egui::Ui, lang: Language) {
    ui.add_space(crate::tokens::SPACE_XXL * 2.5);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("🎮").size(56.0));
        ui.add_space(SPACE_M);
        ui.label(RichText::new(t("empty.heading", lang)).heading());
        ui.add_space(SPACE_S);
        ui.label(RichText::new(t("empty.hint", lang)).italics());
    });
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
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
    let slot = player_slot_color(idx);
    let race_letter = crate::utils::race_letter(&player.race);
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
                    .strong()
                    .color(LABEL_SOFT),
            );
            if is_user {
                ui.label(
                    RichText::new(t("common.you_chip", lang))
                        .size(size_caption(config))
                        .strong()
                        .color(USER_CHIP_FG)
                        .background_color(USER_CHIP_BG),
                );
            }
            ui.label(
                RichText::new(&player.name)
                    .size(size_caption(config))
                    .strong()
                    .color(slot),
            );
            ui.label(
                RichText::new(format!("({race_letter})"))
                    .size(size_caption(config))
                    .strong()
                    .color(slot),
            );
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
        slot,
    );
}

pub fn apply_style(ctx: &Context, config: &AppConfig) {
    ctx.set_visuals(if config.dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    });
    let mut style = (*ctx.global_style()).clone();
    let base = config.font_size.clamp(8.0, 28.0);
    for (text_style, font_id) in style.text_styles.iter_mut() {
        font_id.size = match text_style {
            egui::TextStyle::Small => (base * 0.72).round(),
            egui::TextStyle::Heading => (base * 1.43).round(),
            _ => base,
        };
    }
    ctx.set_global_style(style);
}
