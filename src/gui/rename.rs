// Tela de renomear replays em lote.
//
// O usuário define um template com variáveis como {datetime}, {map}, {p1}, etc.
// A UI exibe um preview dos novos nomes e permite copiar os arquivos renomeados
// para um diretório de destino escolhido via diálogo.

use std::fs;
use std::path::{Path, PathBuf};

use egui::{RichText, ScrollArea, Ui};

use crate::library::{MetaState, ParsedMeta, ReplayLibrary};
use crate::utils::{race_letter, sanitize};

/// Template padrão — mesmo formato usado pelo antigo CLI.
pub const DEFAULT_TEMPLATE: &str = "{datetime}_{map}-{p1}({r1})_vs_{p2}({r2})_{loops}";

// ── Template engine ─────────────────────────────────────────────────────────

/// Expande o template usando os metadados de um replay.
/// Retorna `None` se o replay tiver menos de 2 jogadores.
fn expand_template(template: &str, meta: &ParsedMeta) -> Option<String> {
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
fn execute_rename(previews: &[(PathBuf, String)], dest_dir: &Path) -> Result<usize, String> {
    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Erro ao criar diretório: {e}"))?;
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
        Err(format!(
            "{ok} copiados, {} erros:\n{}",
            errors.len(),
            errors.join("\n")
        ))
    }
}

// ── UI ──────────────────────────────────────────────────────────────────────

pub fn show(
    ui: &mut Ui,
    library: &ReplayLibrary,
    template: &mut String,
    previews: &mut Vec<(PathBuf, String)>,
    status: &mut Option<String>,
) {
    ui.add_space(8.0);
    ui.heading("Renomear Replays em Lote");
    ui.add_space(4.0);

    // ── Template editor ─────────────────────────────────────────────────
    ui.label(RichText::new("Template de nome:").strong());
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
    egui::CollapsingHeader::new(RichText::new("Variáveis disponíveis").italics())
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("template_vars")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    let vars = [
                        ("{datetime}", "Data/hora compacta (YYYYMMDDHHMM)"),
                        ("{map}", "Nome do mapa (sanitizado)"),
                        ("{p1}", "Nome do jogador 1 (sanitizado)"),
                        ("{p2}", "Nome do jogador 2 (sanitizado)"),
                        ("{r1}", "Raça do jogador 1 (T, P, Z ou R)"),
                        ("{r2}", "Raça do jogador 2 (T, P, Z ou R)"),
                        ("{loops}", "Total de game loops"),
                        ("{duration}", "Duração da partida (MMmSSs)"),
                    ];
                    for (var, desc) in vars {
                        ui.monospace(var);
                        ui.label(desc);
                        ui.end_row();
                    }
                });
            ui.add_space(2.0);
            ui.small("Caracteres especiais em nomes de jogadores e mapas são substituídos por '_'.");
            ui.small("A extensão .SC2Replay é adicionada automaticamente.");
        });

    ui.add_space(8.0);
    ui.separator();

    // ── Preview ─────────────────────────────────────────────────────────
    let total_library = library.entries.len();
    let total_previews = previews.len();
    let skipped = total_library.saturating_sub(total_previews);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{total_previews} replays serão renomeados"))
                .strong(),
        );
        if skipped > 0 {
            ui.label(
                RichText::new(format!("({skipped} ignorados — pendentes ou não-1v1)"))
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
            .add_enabled(enabled, egui::Button::new(
                RichText::new("Renomear Tudo…").strong(),
            ))
            .on_hover_text("Escolha a pasta de destino para copiar os replays renomeados")
            .clicked()
        {
            if let Some(dest) = rfd::FileDialog::new().pick_folder() {
                match execute_rename(previews, &dest) {
                    Ok(n) => {
                        *status = Some(format!(
                            "{n} replays copiados para {}",
                            dest.display()
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
            ui.label(RichText::new(msg).small().color(
                if msg.contains("erros") {
                    egui::Color32::LIGHT_RED
                } else {
                    egui::Color32::LIGHT_GREEN
                },
            ));
        }
    });
}
