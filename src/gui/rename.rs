// Tela de renomear replays em lote.
//
// O usuário define um template com variáveis como {datetime}, {map}, {p1}, etc.
// A UI exibe um preview dos novos nomes e permite copiar os arquivos renomeados
// para um diretório de destino escolhido via diálogo.

use std::fs;
use std::path::{Path, PathBuf};

use egui::{RichText, ScrollArea, Ui};

use crate::config::AppConfig;
use crate::library::{MetaState, ParsedMeta, ReplayLibrary};
use crate::locale::{t, tf, Language};
use crate::utils::{race_letter, sanitize};

/// Template padrão — mesmo formato usado pelo antigo CLI.
pub const DEFAULT_TEMPLATE: &str = "{datetime}_{map}-{p1}({r1})_vs_{p2}({r2})_{loops}";

// ── Template engine ─────────────────────────────────────────────────────────

/// Expande o template usando os metadados de um replay.
/// Retorna `None` se o replay tiver menos de 2 jogadores.
pub fn expand_template(template: &str, meta: &ParsedMeta) -> Option<String> {
    if meta.players.len() < 2 {
        return None;
    }

    let datetime_compact = {
        let raw = meta.datetime.replace(['-', ':', 'T'], "");
        if raw.len() >= 12 { raw[..12].to_string() } else { raw }
    };

    let duration = format!(
        "{:02}m{:02}s",
        meta.duration_seconds / 60,
        meta.duration_seconds % 60,
    );

    let result = template
        .replace("{datetime}", &datetime_compact)
        .replace("{map}", &sanitize(&meta.map))
        .replace("{p1}", &sanitize(&meta.players[0].name))
        .replace("{p2}", &sanitize(&meta.players[1].name))
        .replace("{r1}", &race_letter(&meta.players[0].race).to_string())
        .replace("{r2}", &race_letter(&meta.players[1].race).to_string())
        .replace("{loops}", &meta.game_loops.to_string())
        .replace("{duration}", &duration);

    Some(format!("{result}.SC2Replay"))
}

// ── Preview / Execução ──────────────────────────────────────────────────────

/// Gera a lista de previews a partir dos entries da biblioteca.
pub fn generate_previews(library: &ReplayLibrary, template: &str) -> Vec<(PathBuf, String)> {
    library
        .entries
        .iter()
        .filter_map(|entry| match &entry.meta {
            MetaState::Parsed(meta) => {
                let new_name = expand_template(template, meta)?;
                Some((entry.path.clone(), new_name))
            }
            _ => None,
        })
        .collect()
}

/// Copia os arquivos renomeados para o diretório de destino.
/// Retorna o número de cópias bem-sucedidas ou um erro.
fn execute_rename(
    previews: &[(PathBuf, String)],
    dest_dir: &Path,
    lang: Language,
) -> Result<usize, String> {
    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir).map_err(|e| {
            tf(
                "rename.status.mkdir_err",
                lang,
                &[("err", &e.to_string())],
            )
        })?;
    }

    let mut ok = 0usize;
    let mut errors = Vec::new();

    for (src, new_name) in previews {
        let dest = dest_dir.join(new_name);
        match fs::copy(src, &dest) {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("{}: {e}", src.display())),
        }
    }

    if errors.is_empty() {
        Ok(ok)
    } else {
        let header = tf(
            "rename.status.partial",
            lang,
            &[
                ("ok", &ok.to_string()),
                ("errors", &errors.len().to_string()),
            ],
        );
        Err(format!("{header}\n{}", errors.join("\n")))
    }
}

// ── UI ──────────────────────────────────────────────────────────────────────

pub fn show(
    ui: &mut Ui,
    library: &ReplayLibrary,
    config: &AppConfig,
    template: &mut String,
    previews: &mut Vec<(PathBuf, String)>,
    status: &mut Option<String>,
) {
    let lang = config.language;
    ui.add_space(8.0);
    ui.heading(t("rename.heading", lang));
    ui.add_space(4.0);

    // ── Template editor ─────────────────────────────────────────────────
    ui.label(RichText::new(t("rename.template_label", lang)).strong());
    let resp = ui.add(
        egui::TextEdit::singleline(template)
            .desired_width(f32::INFINITY)
            .font(egui::TextStyle::Monospace),
    );
    if resp.changed() {
        *previews = generate_previews(library, template);
    }

    ui.add_space(6.0);

    // ── Explicação das variáveis ────────────────────────────────────────
    egui::CollapsingHeader::new(RichText::new(t("rename.vars_header", lang)).italics())
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("template_vars")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    let vars: [(&str, &str); 8] = [
                        ("{datetime}", t("rename.var.datetime", lang)),
                        ("{map}", t("rename.var.map", lang)),
                        ("{p1}", t("rename.var.p1", lang)),
                        ("{p2}", t("rename.var.p2", lang)),
                        ("{r1}", t("rename.var.r1", lang)),
                        ("{r2}", t("rename.var.r2", lang)),
                        ("{loops}", t("rename.var.loops", lang)),
                        ("{duration}", t("rename.var.duration", lang)),
                    ];
                    for (var, desc) in vars {
                        ui.monospace(var);
                        ui.label(desc);
                        ui.end_row();
                    }
                });
            ui.add_space(2.0);
            ui.small(t("rename.note_special", lang));
            ui.small(t("rename.note_ext", lang));
        });

    ui.add_space(8.0);
    ui.separator();

    // ── Preview ─────────────────────────────────────────────────────────
    let total_library = library.entries.len();
    let total_previews = previews.len();
    let skipped = total_library.saturating_sub(total_previews);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tf(
                "rename.total",
                lang,
                &[("count", &total_previews.to_string())],
            ))
            .strong(),
        );
        if skipped > 0 {
            ui.label(
                RichText::new(tf(
                    "rename.skipped",
                    lang,
                    &[("count", &skipped.to_string())],
                ))
                .small()
                .color(egui::Color32::from_gray(140)),
            );
        }
    });

    ui.add_space(4.0);

    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 60.0)
        .show_rows(ui, row_height, total_previews, |ui, range| {
            for i in range {
                let (src, new_name) = &previews[i];
                let orig = src
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.monospace(
                        RichText::new(&orig)
                            .color(egui::Color32::from_gray(140))
                            .small(),
                    );
                    ui.label(RichText::new(" → ").color(egui::Color32::from_gray(100)));
                    ui.monospace(RichText::new(new_name).small());
                });
            }
        });

    // ── Botão Renomear + Status ─────────────────────────────────────────
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let enabled = !previews.is_empty();
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(RichText::new(t("rename.execute", lang)).strong()),
            )
            .on_hover_text(t("rename.execute_tooltip", lang))
            .clicked()
        {
            if let Some(dest) = rfd::FileDialog::new().pick_folder() {
                match execute_rename(previews, &dest, lang) {
                    Ok(n) => {
                        *status = Some(tf(
                            "rename.status.ok",
                            lang,
                            &[
                                ("count", &n.to_string()),
                                ("dir", &dest.display().to_string()),
                            ],
                        ));
                    }
                    Err(e) => {
                        *status = Some(e);
                    }
                }
            }
        }

        if let Some(msg) = status.as_ref() {
            ui.separator();
            // Error messages contain the localized "errors" marker
            // (e.g. "erros" in pt-BR). Use it to pick the color.
            let error_marker = t("rename.status.error_marker", lang);
            let is_error = !error_marker.is_empty() && msg.contains(error_marker);
            ui.label(RichText::new(msg).small().color(if is_error {
                egui::Color32::LIGHT_RED
            } else {
                egui::Color32::LIGHT_GREEN
            }));
        }
    });
}
