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

use egui::{Color32, Context, Panel, RichText, ScrollArea};

use crate::colors::{player_slot_color, user_fill, CARD_FILL, LABEL_DIM, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::library::{self, LibraryAction, ReplayLibrary};
use crate::locale::{t, tf, Language};
use crate::production_efficiency::EfficiencyTarget;
use crate::replay_state::{fmt_time, LoadedReplay};
use crate::tabs::{self, Tab};
use crate::ui_settings;
use crate::watcher::ReplayWatcher;

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

        // -------- Barra de navegação Rename --------
        if self.screen == Screen::Rename {
            Panel::top("rename_bar").show_inside(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("📚 {}", t("menu.view.library", lang)))
                        .on_hover_text(t("replay_bar.back_tooltip", lang))
                        .clicked()
                    {
                        self.screen = Screen::Library;
                    }
                    ui.separator();
                    ui.label(RichText::new(t("rename_bar.title", lang)).strong());
                });
                ui.add_space(4.0);
            });
        }

        // -------- Replay bar + Tab bar (apenas Tela Análise) --------
        if self.screen == Screen::Analysis {
            Panel::top("replay_bar").show_inside(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("📚 {}", t("menu.view.library", lang)))
                        .on_hover_text(t("replay_bar.back_tooltip", lang))
                        .clicked()
                    {
                        self.screen = Screen::Library;
                    }
                    ui.separator();
                    if let Some(loaded) = self.loaded.as_ref() {
                        ui.label("📼");
                        ui.monospace(loaded.file_name());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t("replay_bar.open", lang)).clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter(t("dialog.filter.sc2_replay", lang), &["SC2Replay"])
                                .pick_file()
                            {
                                self.load_path(p);
                            }
                        }
                    });
                });
                ui.add_space(4.0);
            });

            Panel::top("tabs").show_inside(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for tab in Tab::ALL {
                        ui.selectable_value(&mut self.active_tab, tab, tab.label(lang));
                    }
                });
                ui.add_space(2.0);
            });
        }

        // -------- Status bar inferior (sempre visível) --------
        // Snapshot dos campos antes do closure para evitar conflitos de
        // borrow (toast_visible empresta self inteiro).
        let loaded_snapshot = self.loaded.as_ref().map(|l| {
            (
                l.file_name(),
                l.timeline.map.clone(),
                fmt_time(l.timeline.game_loops, l.timeline.loops_per_second),
                l.timeline.datetime.clone(),
            )
        });
        let watcher_dir = self
            .watcher
            .as_ref()
            .map(|w| w.watched_dir().to_path_buf());
        let toast_msg = self.toast_visible().map(|s| s.to_string());

        Panel::bottom("status_bar").show_inside(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                match &loaded_snapshot {
                    Some((file, map, time, dt)) => {
                        ui.label("📼");
                        ui.monospace(file);
                        ui.separator();
                        ui.small(map);
                        ui.separator();
                        ui.small(time);
                        ui.separator();
                        ui.small(dt);
                    }
                    None => {
                        ui.label(
                            RichText::new(t("app.status.no_replay", lang))
                                .italics()
                                .small(),
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
                            .inner_margin(egui::Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).color(Color32::LIGHT_GREEN).small());
                            });
                    }
                });
            });
            ui.add_space(2.0);
        });

        // -------- SidePanel direita: Jogadores/Partida (apenas Análise) --------
        if self.screen == Screen::Analysis && let Some(loaded) = self.loaded.as_ref() {
            let config = &self.config;
            Panel::right("match_info")
                .resizable(true)
                .default_size(280.0)
                .size_range(240.0..=360.0)
                .show_inside(ui, |ui| {
                    sidebar_content(ui, loaded, config);
                });
        }

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

fn sidebar_content(ui: &mut egui::Ui, loaded: &LoadedReplay, config: &AppConfig) {
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

/// Letra inicial da raça (T/P/Z/R).
fn race_letter(race: &str) -> char {
    crate::utils::race_letter(race)
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

fn empty_state(ui: &mut egui::Ui, lang: Language) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("🎮").size(56.0));
        ui.add_space(8.0);
        ui.label(RichText::new(t("empty.heading", lang)).heading());
        ui.add_space(4.0);
        ui.label(RichText::new(t("empty.hint", lang)).italics());
    });
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
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
