// AppState + impl eframe::App.
//
// A UI alterna entre duas telas mutuamente exclusivas:
// - `Screen::Library`: a biblioteca de replays ocupa toda a janela.
// - `Screen::Analysis`: replay bar + tab bar + central panel + painel
//   direito de jogadores ocupam toda a janela.
//
// Em ambas as telas há uma status bar inferior persistente exibindo o
// replay atualmente carregado, o estado do watcher e os toasts.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Color32, Context, RichText, ScrollArea, SidePanel, TopBottomPanel};

use crate::colors::{player_slot_color, USER_CHIP_BG, USER_CHIP_FG, USER_FILL};
use crate::config::AppConfig;
use crate::library::{self, LibraryAction, ReplayLibrary};
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
    pub library_filter: String,
    /// Segundo selecionado no slider da aba Timeline (mini-mapa).
    /// Resetado a cada `load_path` para que troca de replay sempre
    /// comece em t=0.
    pub timeline_tab_second: u32,
    /// Flags de exibição do gráfico de army value.
    pub charts_show_army: bool,
    pub charts_show_workers: bool,
    pub show_about: bool,
}

impl AppState {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        apply_style(&cc.egui_ctx, &config);

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
            library_filter: String::new(),
            timeline_tab_second: 0,
            charts_show_army: true,
            charts_show_workers: false,
            show_about: false,
        };
        me.restart_watcher();
        me.refresh_library();
        if me.config.auto_load_latest {
            me.try_load_latest();
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
        let Some(dir) = self.config.effective_working_dir() else {
            self.set_toast("Diretório de trabalho não definido (veja Configurações).");
            return;
        };
        match crate::utils::find_latest_replay(&dir) {
            Some(p) => self.load_path(p),
            None => self.set_toast(format!("Nenhum replay encontrado em {}", dir.display())),
        }
    }

    fn load_path(&mut self, p: PathBuf) {
        let max_time = self.config.default_max_time;
        match LoadedReplay::load(&p, max_time) {
            Ok(r) => {
                self.loaded = Some(r);
                self.load_error = None;
                // Reset do scrubbing da aba Timeline: replay novo
                // sempre começa em t=0.
                self.timeline_tab_second = 0;
                // Carregar com sucesso sempre transiciona para a Tela
                // Análise — é a única forma de chegar lá.
                self.screen = Screen::Analysis;
            }
            Err(e) => {
                self.load_error = Some(format!("Erro ao carregar {}: {}", p.display(), e));
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
            if self.config.auto_load_on_new_replay {
                self.load_path(path.clone());
                self.set_toast(format!("Novo replay carregado: {}", file_name(&path)));
            } else {
                self.set_toast(format!(
                    "Novo replay disponível: {} — Arquivo → Carregar mais recente",
                    file_name(&path)
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
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Polling do watcher ANTES de qualquer painel.
        self.poll_watcher(ctx);
        // Drena resultados do worker da biblioteca.
        if self.library.poll() {
            ctx.request_repaint();
        }

        // Guarda: Tela Análise exige replay carregado. Se por qualquer
        // motivo o estado divergir, força fallback para a biblioteca.
        if self.screen == Screen::Analysis && self.loaded.is_none() {
            self.screen = Screen::Library;
        }

        // -------- Menu bar (sempre) --------
        TopBottomPanel::top("menubar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Arquivo", |ui| {
                    if ui.button("Abrir replay…").clicked() {
                        ui.close_menu();
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("SC2 Replay", &["SC2Replay"])
                            .pick_file()
                        {
                            self.load_path(p);
                        }
                    }
                    if ui.button("Carregar mais recente").clicked() {
                        ui.close_menu();
                        self.try_load_latest();
                    }
                    ui.separator();
                    if ui.button("Sair").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Exibir", |ui| {
                    if ui.button("Biblioteca").clicked() {
                        self.screen = Screen::Library;
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.loaded.is_some(), egui::Button::new("Análise")).clicked() {
                        self.screen = Screen::Analysis;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Configurações…").clicked() {
                        self.show_settings = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Ajuda", |ui| {
                    if ui.button("Sobre").clicked() {
                        ui.close_menu();
                        self.show_about = true;
                    }
                });
            });
        });

        // -------- Replay bar + Tab bar (apenas Tela Análise) --------
        if self.screen == Screen::Analysis {
            TopBottomPanel::top("replay_bar").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("📚 Biblioteca")
                        .on_hover_text("Voltar para a biblioteca de replays")
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
                        if ui.button("Trocar…").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter("SC2 Replay", &["SC2Replay"])
                                .pick_file()
                            {
                                self.load_path(p);
                            }
                        }
                    });
                });
                ui.add_space(4.0);
            });

            TopBottomPanel::top("tabs").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for tab in Tab::ALL {
                        ui.selectable_value(&mut self.active_tab, tab, tab.label());
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

        TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
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
                            RichText::new("(nenhum replay carregado)")
                                .italics()
                                .small(),
                        );
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(dir) = &watcher_dir {
                        ui.label("👁")
                            .on_hover_text(format!("Observando: {}", dir.display()));
                    }
                    if let Some(msg) = toast_msg {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(28, 60, 28))
                            .stroke(egui::Stroke::new(1.0, Color32::LIGHT_GREEN))
                            .inner_margin(egui::Margin::symmetric(8.0, 2.0))
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
            SidePanel::right("match_info")
                .resizable(true)
                .default_width(280.0)
                .width_range(240.0..=360.0)
                .show(ctx, |ui| {
                    sidebar_content(ui, loaded, config);
                });
        }

        // -------- Central --------
        let mut library_action = LibraryAction::None;
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = self.load_error.clone() {
                egui::Frame::none()
                    .fill(Color32::from_rgb(60, 20, 20))
                    .stroke(egui::Stroke::new(1.0, Color32::LIGHT_RED))
                    .inner_margin(egui::Margin::same(8.0))
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
                    None => empty_state(ui),
                    Some(loaded) => match self.active_tab {
                        Tab::Timeline => tabs::timeline::show(
                            ui,
                            loaded,
                            &self.config,
                            &mut self.timeline_tab_second,
                        ),
                        Tab::BuildOrder => tabs::build_order::show(ui, loaded, &self.config),
                        Tab::Charts => tabs::charts::show(ui, loaded, &self.config, &mut self.charts_show_army, &mut self.charts_show_workers),
                        Tab::Chat => tabs::chat::show(ui, loaded, &self.config),
                    },
                },
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
                    self.set_toast(format!("Erro ao salvar: {e}"));
                }
                self.refresh_library();
            }
        }

        // Mantém repaint enquanto a biblioteca estiver parseando em background.
        library::keep_alive(ctx, &self.library);

        // -------- Settings window --------
        let prev_effective_dir = self.config.effective_working_dir();
        let outcome = ui_settings::show(
            ctx,
            &mut self.show_settings,
            &mut self.config,
            &mut self.nickname_input,
        );
        if outcome.saved {
            match self.config.save() {
                Ok(()) => self.set_toast("Configurações salvas."),
                Err(e) => self.set_toast(format!("Erro ao salvar: {e}")),
            }
            apply_style(ctx, &self.config);
            self.restart_watcher();
            if self.config.effective_working_dir() != prev_effective_dir {
                self.refresh_library();
            }
        } else if outcome.reset_defaults {
            apply_style(ctx, &self.config);
            if self.config.effective_working_dir() != prev_effective_dir {
                self.refresh_library();
            }
        }

        // -------- About window --------
        if self.show_about {
            egui::Window::new("Sobre")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.heading("sc2-replay-utils");
                        ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                        ui.add_space(12.0);
                        ui.label("Ferramenta de análise de replays de StarCraft II");
                        ui.add_space(12.0);
                        ui.label(RichText::new("Autor").strong());
                        ui.label("Paulo Canedo");
                        ui.add_space(12.0);
                        ui.label(RichText::new("Tecnologias").strong());
                        ui.label("Rust · egui · s2protocol");
                        ui.add_space(16.0);
                        if ui.button("Fechar").clicked() {
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

fn sidebar_content(ui: &mut egui::Ui, loaded: &LoadedReplay, config: &AppConfig) {
    ui.add_space(8.0);
    ui.heading("Jogadores");
    ui.separator();
    ui.add_space(4.0);

    ScrollArea::vertical().id_salt("sidebar_players").show(ui, |ui| {
        // Card externo unificado que contém os sub-cards dos 2 jogadores.
        // A ideia visual é "um card maior agrupando dois menores" — o
        // container dá o recorte do grupo, os filhos diferenciam os
        // jogadores sem precisar de borda completa.
        egui::Frame::none()
            .fill(Color32::from_gray(22))
            .stroke(egui::Stroke::new(0.5, Color32::from_gray(60)))
            .rounding(8.0)
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let last = loaded.timeline.players.len().saturating_sub(1);
                for (i, p) in loaded.timeline.players.iter().enumerate() {
                    let is_user = config.is_user(&p.name);
                    player_card(ui, p, i, is_user);
                    if i != last {
                        ui.add_space(8.0);
                    }
                }
            });

        ui.add_space(8.0);
        ui.separator();
        ui.heading("Partida");
        ui.add_space(2.0);
        ui.small(format!("🗺  {}", loaded.timeline.map));
        ui.small(format!("🗓  {}", loaded.timeline.datetime));
        ui.small(format!(
            "⏱  {} ({} loops)",
            fmt_time(loaded.timeline.game_loops, loaded.timeline.loops_per_second),
            loaded.timeline.game_loops
        ));
        ui.small(format!("⚙  {:.1} loops/s", loaded.timeline.loops_per_second));
    });
}

/// Renderiza o card de um jogador na sidebar. A identidade visual
/// primária é a cor do slot do SC2 (P1 = vermelho, P2 = azul) aplicada
/// na borda. A indicação "Você" é secundária: pequeno chip + leve tom
/// esverdeado no fill, sem sequestrar a borda colorida do slot.
fn player_card(
    ui: &mut egui::Ui,
    player: &crate::replay::PlayerTimeline,
    index: usize,
    is_user: bool,
) {
    // Cor do slot do jogador (padrão SC2): sempre presente, prioridade
    // sobre o destaque do usuário.
    let slot_color = player_slot_color(index);

    // Fill base neutro; se é o usuário, ganha um tom esverdeado sutil
    // (sem virar "verde chapado") só para indicar "sou eu".
    let fill = if is_user {
        USER_FILL
    } else {
        Color32::from_gray(34)
    };
    // Borda: sempre a cor do slot. Fica um pouco mais grossa para
    // reforçar a identidade visual do jogador dentro do card unificado.
    let stroke = egui::Stroke::new(1.8, slot_color);

    egui::Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            // Trava a largura no máximo disponível do painel.
            ui.set_width(ui.available_width());

            // Linha 1: nome grande + chip "Você" (discreto) à direita.
            ui.horizontal(|ui| {
                let name = RichText::new(&player.name)
                    .size(16.0)
                    .strong()
                    .color(Color32::WHITE);
                ui.label(name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_user {
                        // Chip "Você" discreto — tom esverdeado escuro,
                        // texto claro, sem competir visualmente com a
                        // borda do slot.
                        ui.label(
                            RichText::new(" Você ")
                                .small()
                                .strong()
                                .color(USER_CHIP_FG)
                                .background_color(USER_CHIP_BG),
                        );
                    }
                });
            });

            // Clan opcional (entre nome e linha de raça).
            if !player.clan.is_empty() {
                ui.small(
                    RichText::new(format!("[{}]", player.clan))
                        .color(Color32::from_gray(150))
                        .italics(),
                );
            }

            ui.add_space(6.0);

            // Linha 2: raça (texto simples, sem badge colorido) + MMR.
            // A cor "tag" do card é a cor do slot — não competimos com ela.
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        race_letter_char(&player.race),
                        race_full_name(&player.race)
                    ))
                    .color(Color32::from_gray(210)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match player.mmr {
                        Some(mmr) => {
                            ui.label(
                                RichText::new(format!("MMR {mmr}"))
                                    .monospace()
                                    .strong(),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new("MMR —")
                                    .monospace()
                                    .color(Color32::from_gray(140)),
                            );
                        }
                    }
                });
            });
        });
}

/// Letra inicial da raça (T/P/Z/R) sem envolver badge colorido.
fn race_letter_char(race: &str) -> char {
    match race.to_ascii_lowercase().chars().next() {
        Some('t') => 'T',
        Some('p') => 'P',
        Some('z') => 'Z',
        Some('r') => 'R',
        _ => '?',
    }
}

/// Normaliza o nome da raça para exibição.
fn race_full_name(race: &str) -> &str {
    match race.to_ascii_lowercase().as_str() {
        "terr" | "terran" => "Terran",
        "prot" | "protoss" => "Protoss",
        "zerg" => "Zerg",
        _ => race,
    }
}

fn empty_state(ui: &mut egui::Ui) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("🎮").size(56.0));
        ui.add_space(8.0);
        ui.label(
            RichText::new("Nenhum replay carregado")
                .heading(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Use Arquivo → Abrir replay… ou habilite o file watcher em Configurações para auto-carregar replays novos.",
            )
            .italics(),
        );
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
    let mut style = (*ctx.style()).clone();
    for (_, font_id) in style.text_styles.iter_mut() {
        font_id.size *= config.font_scale.clamp(0.5, 2.0);
    }
    ctx.set_style(style);
}
