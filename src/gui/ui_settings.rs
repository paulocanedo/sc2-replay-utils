// Janela modal de configurações.
//
// A função `show` recebe `&mut AppConfig` e devolve um SettingsOutcome
// indicando se o usuário clicou em "Salvar" ou "Restaurar padrões".
// O parent (AppState) usa isso para persistir, reiniciar o watcher e
// reaplicar o estilo.

use egui::{Context, RichText, ScrollArea, Slider, Window};

use crate::config::AppConfig;

#[derive(Default)]
pub struct SettingsOutcome {
    pub saved: bool,
    pub reset_defaults: bool,
}

pub fn show(
    ctx: &Context,
    open: &mut bool,
    config: &mut AppConfig,
    nickname_buf: &mut String,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::default();
    if !*open {
        return outcome;
    }

    Window::new("Configurações")
        .open(open)
        .resizable(true)
        .default_width(520.0)
        .show(ctx, |ui| {
            ui.heading("Pastas");
            working_dir_row(ui, config);
            ui.small(
                "Pasta onde o app procura, lista e observa seus replays. Se vazio, o app usa o diretório do SC2 detectado automaticamente.",
            );
            ui.add_space(4.0);

            path_row(ui, "Pasta de saída", &mut config.output_dir);
            ui.small("Destino padrão para exports (CSVs, PNGs, YAMLs).");

            ui.separator();
            ui.heading("Nicknames do usuário");
            ui.small("Replays com estes nicks serão destacados como 'Você' na UI.");

            // Lista atual de nicks
            let mut to_remove: Option<usize> = None;
            ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    if config.user_nicknames.is_empty() {
                        ui.label(RichText::new("(nenhum nickname cadastrado)").italics());
                    }
                    for (i, nick) in config.user_nicknames.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("×").on_hover_text("Remover").clicked() {
                                to_remove = Some(i);
                            }
                            ui.monospace(nick);
                        });
                    }
                });
            if let Some(i) = to_remove {
                config.user_nicknames.remove(i);
            }

            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(nickname_buf)
                        .hint_text("Adicionar nickname…")
                        .desired_width(200.0),
                );
                let enter =
                    resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Adicionar").clicked() || enter {
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

            ui.separator();
            ui.heading("Comportamento");

            ui.horizontal(|ui| {
                ui.label("max_time padrão (s, 0 = sem limite):");
                ui.add(egui::DragValue::new(&mut config.default_max_time).speed(1.0));
            });

            ui.checkbox(&mut config.auto_load_latest, "Carregar replay mais recente ao abrir");

            ui.checkbox(&mut config.watch_replays, "Observar pasta do SC2 (file watcher)");
            ui.add_enabled_ui(config.watch_replays, |ui| {
                ui.indent("auto_load_new", |ui| {
                    ui.checkbox(
                        &mut config.auto_load_on_new_replay,
                        "Carregar automaticamente quando surgir novo replay",
                    );
                });
            });

            ui.separator();
            ui.heading("Aparência");
            ui.checkbox(&mut config.dark_mode, "Tema escuro");

            ui.horizontal(|ui| {
                ui.label("Escala da fonte:");
                ui.add(Slider::new(&mut config.font_scale, 0.8..=1.5).fixed_decimals(2));
            });
            ui.small("HiDPI é detectado automaticamente pelo sistema — este slider só afeta o tamanho do texto.");

            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_sized([100.0, 28.0], egui::Button::new("Salvar"))
                    .clicked()
                {
                    outcome.saved = true;
                }
                if ui.button("Restaurar padrões").clicked() {
                    *config = AppConfig::default();
                    outcome.reset_defaults = true;
                }
                if let Some(path) = AppConfig::config_path() {
                    ui.separator();
                    ui.small(format!("Arquivo: {}", path.display()));
                }
            });
        });

    outcome
}

/// Linha especial do diretório de trabalho. Mostra o caminho efetivo
/// (persistido ou auto-detectado) e oferece um botão "Detectar SC2"
/// que preenche `working_dir` com o diretório padrão do SC2, para que
/// o usuário possa persistir esse valor clicando em "Salvar".
fn working_dir_row(ui: &mut egui::Ui, config: &mut crate::config::AppConfig) {
    let detected = crate::utils::sc2_default_dir();
    ui.horizontal(|ui| {
        ui.label("Diretório de trabalho:");
        match config.working_dir.as_ref() {
            Some(p) => {
                ui.monospace(p.display().to_string());
            }
            None => match detected.as_ref() {
                Some(p) => {
                    ui.monospace(p.display().to_string());
                    ui.small(
                        RichText::new("(auto: SC2 detectado)")
                            .italics()
                            .color(egui::Color32::from_gray(160)),
                    );
                }
                None => {
                    ui.monospace("(não definido)");
                }
            },
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        if ui.button("Escolher…").clicked() {
            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                config.working_dir = Some(p);
            }
        }
        let detect_enabled = detected.is_some();
        if ui
            .add_enabled(detect_enabled, egui::Button::new("Detectar SC2"))
            .on_hover_text(match detected.as_ref() {
                Some(p) => format!("Usar {}", p.display()),
                None => "Não foi possível detectar o diretório do SC2".to_string(),
            })
            .clicked()
        {
            config.working_dir = detected.clone();
        }
        if ui.button("Limpar").clicked() {
            config.working_dir = None;
        }
    });
}

fn path_row(ui: &mut egui::Ui, label: &str, path: &mut Option<std::path::PathBuf>) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        let text = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(não definido)".into());
        ui.monospace(text);
    });
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        if ui.button("Escolher…").clicked() {
            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                *path = Some(p);
            }
        }
        if ui.button("Limpar").clicked() {
            *path = None;
        }
    });
}
