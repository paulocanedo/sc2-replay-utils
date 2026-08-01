// Janela modal de configurações.
//
// O conteúdo é dividido em abas (`SettingsTab`) — era uma lista única e
// longa demais para caber na janela sem rolagem interminável. A aba ativa
// mora no `AppState` (`settings_tab`), então fechar e reabrir a janela
// volta para onde o usuário estava.
//
// A função `show` recebe `&mut AppConfig` e devolve um SettingsOutcome
// indicando se o usuário clicou em "Salvar" ou "Restaurar padrões".
// O parent (AppState) usa isso para persistir, reiniciar o watcher e
// reaplicar o estilo.
//
// Changing the language in this window also sets
// `language_selected = true`, so the first-run language picker is not
// triggered again once the user has confirmed a choice here.

use egui::{Context, RichText, ScrollArea, Slider, Window};

use crate::config::AppConfig;
use crate::locale::{t, tf, Language};
use crate::tokens::{SPACE_M, SPACE_S, SPACE_XS};

#[derive(Default)]
pub struct SettingsOutcome {
    pub saved: bool,
    pub reset_defaults: bool,
    pub classify_now: bool,
    pub stop_classification: bool,
    /// Subir (ou reiniciar) o servidor do overlay agora, com os valores
    /// atuais de enable/porta, e persistir o config. Existe porque marcar
    /// o checkbox só vale depois de Salvar, e depois de um conflito de
    /// porta o usuário precisa de um retry explícito.
    pub overlay_apply: bool,
    /// Abrir a pasta de templates do overlay no gerenciador de arquivos.
    /// Age na hora, sem depender de Salvar.
    pub overlay_open_folder: bool,
    /// Reescrever `index.html`/`style.css` com os originais embutidos
    /// (guardando `.bak`). Também age na hora.
    pub overlay_restore_defaults: bool,
}

/// Estado do servidor do overlay, na forma que esta janela consegue
/// renderizar.
///
/// É um struct plano definido *aqui* de propósito: este módulo compila em
/// wasm e não pode nomear nada de `crate::overlay`, que é nativo-only. O
/// parent monta o valor a partir do handle real.
#[derive(Default, Clone)]
pub struct OverlayUiStatus {
    pub running: bool,
    pub bound_port: u16,
    pub url: String,
    pub error: Option<String>,
}

/// Abas da janela de configurações.
///
/// `Overlay` existe em todos os targets para manter o enum estável, mas
/// só aparece na barra em builds nativas e fora do first-run — ver
/// [`SettingsTab::visible`].
#[derive(Copy, Clone, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Nicknames,
    Overlay,
    Interface,
}

impl SettingsTab {
    const ALL: [SettingsTab; 4] = [
        SettingsTab::General,
        SettingsTab::Nicknames,
        SettingsTab::Overlay,
        SettingsTab::Interface,
    ];

    fn label(self, lang: Language) -> &'static str {
        let key = match self {
            SettingsTab::General => "settings.tab.general",
            SettingsTab::Nicknames => "settings.tab.nicknames",
            SettingsTab::Overlay => "settings.tab.overlay",
            SettingsTab::Interface => "settings.tab.interface",
        };
        t(key, lang)
    }

    /// Salt estável para o `ScrollArea`, de modo que cada aba guarde a
    /// própria posição de rolagem.
    fn id(self) -> &'static str {
        match self {
            SettingsTab::General => "general",
            SettingsTab::Nicknames => "nicknames",
            SettingsTab::Overlay => "overlay",
            SettingsTab::Interface => "interface",
        }
    }

    /// O overlay é nativo-only e fica escondido no first-run: o usuário
    /// ainda nem apontou a pasta de replays; oferecer um servidor HTTP
    /// nesse momento é prematuro.
    fn visible(self, force_initial: bool) -> bool {
        match self {
            SettingsTab::Overlay => !force_initial && !cfg!(target_arch = "wasm32"),
            _ => true,
        }
    }
}

/// Largura inicial da janela, derivada da tipografia para que fontes
/// grandes não espremam as descrições em colunas estreitas.
fn window_width(cfg: &AppConfig) -> f32 {
    (cfg.font_size * 40.0).clamp(520.0, 720.0)
}

/// Altura fixa do corpo. Trocar de aba não deve fazer a janela pular de
/// tamanho, e o rodapé (Salvar) precisa continuar visível sem rolagem.
fn body_height(cfg: &AppConfig) -> f32 {
    (cfg.font_size * 24.0).clamp(300.0, 460.0)
}

pub fn show(
    ctx: &Context,
    open: &mut bool,
    tab: &mut SettingsTab,
    config: &mut AppConfig,
    nickname_buf: &mut String,
    nickname_suggestions: &[(String, u32)],
    force_initial: bool,
    overlay_status: &OverlayUiStatus,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::default();
    if !*open && !force_initial {
        return outcome;
    }
    let lang = config.language;
    // A aba de overlay é nativa-only; em wasm o parâmetro entra e não é
    // lido. Mantê-lo incondicional na assinatura é mais limpo que um
    // `#[cfg]` no parâmetro.
    #[cfg(target_arch = "wasm32")]
    let _ = overlay_status;

    // A aba lembrada pode não existir neste modo (Overlay some no
    // first-run) — cai para a primeira.
    if !tab.visible(force_initial) {
        *tab = SettingsTab::General;
    }

    let mut window = Window::new(t("settings.title", lang))
        .resizable(true)
        .default_width(window_width(config));
    if force_initial {
        // First-run mode: no X, no click-outside-to-close, centered.
        // Matches the language/disclaimer modal chrome in `modals.rs`.
        window = window
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]);
    } else {
        window = window.open(open);
    }
    window.show(ctx, |ui| {
        if force_initial {
            ui.vertical_centered(|ui| {
                ui.heading(t("settings.first_run.title", lang));
                ui.small(t("settings.first_run.subtitle", lang));
            });
            ui.add_space(SPACE_S);
        }

        tab_bar(ui, tab, lang, force_initial);
        ui.separator();

        let body_h = body_height(config);
        ScrollArea::vertical()
            .id_salt(("settings_body", tab.id()))
            .max_height(body_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_height(body_h);
                match *tab {
                    SettingsTab::General => general_tab(ui, config, &mut outcome),
                    SettingsTab::Nicknames => {
                        nicknames_tab(ui, config, nickname_buf, nickname_suggestions)
                    }
                    SettingsTab::Overlay => {
                        #[cfg(not(target_arch = "wasm32"))]
                        overlay_section(ui, config, overlay_status, &mut outcome);
                    }
                    SettingsTab::Interface => interface_tab(ui, config),
                }
            });

        ui.separator();
        footer(ui, config, force_initial, &mut outcome);
    });

    outcome
}

/// Barra de abas. Usa `selectable_value` igual à tab bar da tela de
/// análise (`app/topbar.rs`), para que "aba" tenha uma só aparência no app.
fn tab_bar(ui: &mut egui::Ui, tab: &mut SettingsTab, lang: Language, force_initial: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        for candidate in SettingsTab::ALL {
            if !candidate.visible(force_initial) {
                continue;
            }
            ui.selectable_value(tab, candidate, candidate.label(lang));
        }
    });
    ui.add_space(SPACE_XS);
}

/// Rodapé fixo: Salvar (+ Restaurar padrões e caminho do arquivo fora do
/// first-run). Fica fora do `ScrollArea` para nunca sair de vista.
fn footer(
    ui: &mut egui::Ui,
    config: &mut AppConfig,
    force_initial: bool,
    outcome: &mut SettingsOutcome,
) {
    let lang = config.language;
    if force_initial {
        ui.vertical_centered(|ui| {
            if ui
                .add_sized([160.0, 32.0], egui::Button::new(t("settings.save", lang)))
                .clicked()
            {
                outcome.saved = true;
            }
        });
        return;
    }
    ui.horizontal(|ui| {
        if ui
            .add_sized([100.0, 28.0], egui::Button::new(t("settings.save", lang)))
            .clicked()
        {
            outcome.saved = true;
        }
        if ui.button(t("settings.reset_defaults", lang)).clicked() {
            *config = AppConfig::default();
            // Reset keeps the language_selected flag true so we
            // don't show the first-run modal again.
            config.language_selected = true;
            outcome.reset_defaults = true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = AppConfig::config_path() {
            ui.separator();
            ui.small(tf(
                "settings.file_path",
                lang,
                &[("path", &path.display().to_string())],
            ));
        }
    });
}

// ── Aba "Geral" — pastas, comportamento e classificação ──────────────

fn general_tab(ui: &mut egui::Ui, config: &mut AppConfig, outcome: &mut SettingsOutcome) {
    let lang = config.language;

    #[cfg(not(target_arch = "wasm32"))]
    {
        ui.heading(t("settings.section.folders", lang));
        working_dir_row(ui, config);
        ui.small(t("settings.working_dir.desc", lang));
        ui.add_space(SPACE_M);
    }

    ui.heading(t("settings.section.behavior", lang));
    ui.horizontal(|ui| {
        ui.label(t("settings.max_time.label", lang));
        ui.add(egui::DragValue::new(&mut config.default_max_time).speed(1.0));
    });

    ui.checkbox(
        &mut config.auto_load_latest,
        t("settings.auto_load_latest", lang),
    );

    ui.checkbox(&mut config.watch_replays, t("settings.watch_replays", lang));
    ui.add_enabled_ui(config.watch_replays, |ui| {
        ui.indent("auto_load_new", |ui| {
            ui.checkbox(
                &mut config.auto_load_on_new_replay,
                t("settings.auto_load_new", lang),
            );
        });
    });
    ui.add_space(SPACE_M);

    ui.heading(t("settings.section.classification", lang));
    ui.checkbox(
        &mut config.auto_classify_on_scan,
        t("settings.auto_classify_on_scan", lang),
    );
    ui.small(t("settings.auto_classify_on_scan.desc", lang));
    ui.add_space(SPACE_XS);
    ui.horizontal(|ui| {
        if ui.button(t("settings.classify_now", lang)).clicked() {
            outcome.classify_now = true;
        }
        if ui.button(t("settings.stop_classification", lang)).clicked() {
            outcome.stop_classification = true;
        }
    });
}

// ── Aba "Apelidos" ───────────────────────────────────────────────────
//
// Sem heading: o rótulo da aba já nomeia a seção. Os nicks viram chips
// que fluem em linha (`removable_chip`, o mesmo widget dos filtros da
// biblioteca) em vez de uma lista vertical com scroll próprio — cabem
// muito mais por linha e some um scroll aninhado.

fn nicknames_tab(
    ui: &mut egui::Ui,
    config: &mut AppConfig,
    nickname_buf: &mut String,
    nickname_suggestions: &[(String, u32)],
) {
    let lang = config.language;
    ui.small(t("settings.nicknames.desc", lang));
    ui.add_space(SPACE_S);

    let mut to_remove: Option<usize> = None;
    if config.user_nicknames.is_empty() {
        ui.label(RichText::new(t("settings.nicknames.empty", lang)).italics());
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SPACE_S, SPACE_S);
            for (i, nick) in config.user_nicknames.iter().enumerate() {
                if crate::widgets::removable_chip(ui, nick, config)
                    .on_hover_text(t("settings.nicknames.remove_tooltip", lang))
                    .clicked()
                {
                    to_remove = Some(i);
                }
            }
        });
    }
    if let Some(i) = to_remove {
        config.user_nicknames.remove(i);
    }

    ui.add_space(SPACE_S);
    ui.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(nickname_buf)
                .hint_text(t("settings.nicknames.add_placeholder", lang))
                .desired_width(200.0),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(t("settings.nicknames.add", lang)).clicked() || enter {
            let trimmed = nickname_buf.trim().to_string();
            if !trimmed.is_empty()
                && !config
                    .user_nicknames
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&trimmed))
            {
                config.user_nicknames.push(trimmed);
            }
            nickname_buf.clear();
        }
    });

    let suggestions: Vec<&str> = nickname_suggestions
        .iter()
        .filter(|(name, _)| !config.is_user(name))
        .take(3)
        .map(|(name, _)| name.as_str())
        .collect();
    if !suggestions.is_empty() {
        ui.add_space(SPACE_M);
        ui.small(t("settings.nicknames.suggested", lang));
        ui.horizontal_wrapped(|ui| {
            for name in suggestions {
                if ui
                    .small_button(format!("+ {name}"))
                    .on_hover_text(t("settings.nicknames.suggested.tooltip", lang))
                    .clicked()
                {
                    config.user_nicknames.push(name.to_string());
                }
            }
        });
    }
}

// ── Aba "Interface" — idioma e aparência ─────────────────────────────

fn interface_tab(ui: &mut egui::Ui, config: &mut AppConfig) {
    let lang = config.language;

    ui.heading(t("settings.section.language", lang));
    ui.horizontal(|ui| {
        ui.label(t("settings.language.label", lang));
        egui::ComboBox::from_id_salt("lang_combo")
            .selected_text(config.language.label())
            .show_ui(ui, |ui| {
                for &lang_opt in Language::all() {
                    if ui
                        .selectable_value(&mut config.language, lang_opt, lang_opt.label())
                        .clicked()
                    {
                        // Changing the language here counts as
                        // an explicit selection — suppress the
                        // first-run modal forever after.
                        config.language_selected = true;
                    }
                }
            });
    });
    ui.add_space(SPACE_M);

    ui.heading(t("settings.section.appearance", lang));
    ui.horizontal(|ui| {
        ui.label(t("settings.font_size.label", lang));
        ui.add(Slider::new(&mut config.font_size, 8.0..=28.0).fixed_decimals(0));
    });
    ui.small(t("settings.font_size.desc", lang));
}

/// Conteúdo da aba "Overlay" (OBS).
///
/// Sem heading: o rótulo da aba já nomeia a seção.
///
/// O checkbox e a porta só escrevem no `config`; quem decide subir ou
/// derrubar o servidor é o parent, comparando o config com o servidor que
/// está **rodando** (`AppState::overlay_matches_config`). É por isso que o
/// `DragValue` da porta não rebinda a cada dígito digitado: nada aqui
/// dispara ação, só o Salvar e o botão de aplicar.
///
/// A linha de status + botão ficam fora do `add_enabled_ui`, senão
/// desmarcar o checkbox com o servidor no ar deixaria "Rodando" ao lado de
/// um botão cinza, sem caminho de saída.
#[cfg(not(target_arch = "wasm32"))]
fn overlay_section(
    ui: &mut egui::Ui,
    config: &mut AppConfig,
    status: &OverlayUiStatus,
    outcome: &mut SettingsOutcome,
) {
    let lang = config.language;
    ui.small(t("settings.overlay.desc", lang));
    ui.add_space(SPACE_S);
    // O recorte de ladder 1v1 é fixo (ver `library::overlay_snapshot::build`).
    // Não há checkbox aqui de propósito — o texto existe justamente para que
    // a ausência da opção seja uma decisão explicada, e não um bug aparente
    // ("liguei 'somente ladder' na biblioteca e o overlay não mudou").
    ui.small(
        egui::RichText::new(t("settings.overlay.ladder_note", lang))
            .italics()
            .color(crate::colors::LABEL_SOFT),
    );
    ui.add_space(SPACE_S);
    ui.checkbox(&mut config.overlay_enabled, t("settings.overlay.enable", lang));

    ui.add_enabled_ui(config.overlay_enabled, |ui| {
        ui.indent("overlay_opts", |ui| {
            ui.horizontal(|ui| {
                ui.label(t("settings.overlay.port.label", lang));
                ui.add(
                    egui::DragValue::new(&mut config.overlay_port)
                        .range(1024..=65535)
                        .speed(1.0),
                );
            });
            ui.small(t("settings.overlay.port.desc", lang));

            let url = format!("http://127.0.0.1:{}/", config.overlay_port);
            ui.horizontal(|ui| {
                ui.monospace(&url);
                if crate::widgets::copy_icon_button(
                    ui,
                    t("settings.overlay.copy_url.tooltip", lang),
                )
                .clicked()
                {
                    ui.ctx().copy_text(url.clone());
                }
            });

            ui.horizontal(|ui| {
                if ui
                    .add(crate::widgets::reveal_in_explorer_button_widget(
                        ui,
                        t("settings.overlay.open_folder", lang),
                    ))
                    .clicked()
                {
                    outcome.overlay_open_folder = true;
                }
                if ui
                    .button(t("settings.overlay.restore_defaults", lang))
                    .on_hover_text(t("settings.overlay.restore_defaults.tooltip", lang))
                    .clicked()
                {
                    outcome.overlay_restore_defaults = true;
                }
            });

            ui.small(t("settings.overlay.hint", lang));
        });
    });

    // Estado + ação ficam FORA do `add_enabled_ui`: com o checkbox
    // desmarcado e o servidor ainda no ar, o par "Rodando [botão cinza]"
    // era um beco sem saída visual. Aqui o botão sempre responde, e o
    // rótulo cobre os quatro estados possíveis.
    ui.add_space(SPACE_M);
    ui.horizontal(|ui| {
        if let Some(err) = &status.error {
            ui.colored_label(
                ui.visuals().error_fg_color,
                tf("settings.overlay.status.error", lang, &[("err", err)]),
            );
        } else if status.running {
            if status.bound_port != config.overlay_port {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t("settings.overlay.status.pending_restart", lang),
                );
            } else {
                ui.colored_label(
                    crate::colors::ACCENT_SUCCESS,
                    tf(
                        "settings.overlay.status.running",
                        lang,
                        &[("port", &status.bound_port.to_string())],
                    ),
                );
            }
        } else {
            ui.small(t("settings.overlay.status.stopped", lang));
        }

        // Um único botão que sempre reaplica o config; só o rótulo
        // muda. `apply_overlay_config` já derruba o servidor atual e
        // só sobe outro se `overlay_enabled` estiver ligado, então
        // "parar" e "iniciar" são a mesma chamada.
        let (label, tooltip) = match (config.overlay_enabled, status.running) {
            (true, true) => (
                "settings.overlay.restart",
                "settings.overlay.restart.tooltip",
            ),
            (true, false) => ("settings.overlay.start", "settings.overlay.start.tooltip"),
            (false, true) => ("settings.overlay.stop", "settings.overlay.stop.tooltip"),
            // Desligado e parado: não há o que aplicar.
            (false, false) => ("settings.overlay.start", "settings.overlay.start.tooltip"),
        };
        let enabled = config.overlay_enabled || status.running;
        if ui
            .add_enabled(enabled, egui::Button::new(t(label, lang)))
            .on_hover_text(t(tooltip, lang))
            .clicked()
        {
            outcome.overlay_apply = true;
        }
    });
}

/// Linha especial do diretório de trabalho. Mostra o caminho efetivo
/// (persistido ou auto-detectado) e oferece um botão "Detectar SC2"
/// que preenche `working_dir` com o diretório padrão do SC2, para que
/// o usuário possa persistir esse valor clicando em "Salvar".
#[cfg(not(target_arch = "wasm32"))]
fn working_dir_row(ui: &mut egui::Ui, config: &mut crate::config::AppConfig) {
    let lang = config.language;
    let detected = crate::utils::sc2_default_dir();
    ui.horizontal(|ui| {
        ui.label(t("settings.working_dir.label", lang));
        match config.working_dir.as_ref() {
            Some(p) => {
                ui.monospace(p.display().to_string());
            }
            None => match detected.as_ref() {
                Some(p) => {
                    ui.monospace(p.display().to_string());
                    ui.small(
                        RichText::new(t("settings.working_dir.auto", lang))
                            .italics()
                            .color(egui::Color32::from_gray(160)),
                    );
                }
                None => {
                    ui.monospace(t("settings.working_dir.unset", lang));
                }
            },
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        if ui.button(t("settings.working_dir.choose", lang)).clicked() {
            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                config.working_dir = Some(p);
            }
        }
        let detect_enabled = detected.is_some();
        if ui
            .add_enabled(
                detect_enabled,
                egui::Button::new(t("settings.working_dir.detect", lang)),
            )
            .on_hover_text(match detected.as_ref() {
                Some(p) => tf(
                    "settings.working_dir.detect_ok_tooltip",
                    lang,
                    &[("dir", &p.display().to_string())],
                ),
                None => t("settings.working_dir.detect_fail_tooltip", lang).to_string(),
            })
            .clicked()
        {
            config.working_dir = detected.clone();
        }
        if ui.button(t("settings.working_dir.clear", lang)).clicked() {
            config.working_dir = None;
        }
    });
}
