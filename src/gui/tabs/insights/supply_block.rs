// Card de insight: supply blocks.
//
// Expõe o tempo total bloqueado (severidade) e as maiores janelas de
// supply block (impacto real). Blocos curtos (<3s) são filtrados da
// lista como ruído — são microblocks que o detector captura quando uma
// produção passou brevemente do supply cap e logo liberou. O total
// continua somando todos os blocos para preservar o sinal agregado.

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::supply_block::SupplyBlockEntry;
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;
use super::util::human_duration;

/// Threshold em segundos para um bloco entrar na lista de "impacto
/// real". Abaixo disso o bloco é microscópico e confundiria a leitura.
const SIGNIFICANT_SECS: u32 = 3;

/// Quantos blocos mostrar no máximo na lista de impacto.
const TOP_N: usize = 3;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let lps = loaded.timeline.loops_per_second.max(0.0001);

    let blocks = loaded
        .supply_blocks_per_player
        .get(player_idx)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);

    let total_loops: u32 = blocks
        .iter()
        .map(|b| b.end_loop.saturating_sub(b.start_loop))
        .sum();
    let total_secs = (total_loops as f64 / lps).round() as u32;

    // Top blocos significativos, ordenados por duração desc.
    let mut significant: Vec<&SupplyBlockEntry> = blocks
        .iter()
        .filter(|b| {
            let dur_secs = (b.end_loop.saturating_sub(b.start_loop) as f64 / lps).round() as u32;
            dur_secs >= SIGNIFICANT_SECS
        })
        .collect();
    significant.sort_by(|a, b| {
        let da = a.end_loop.saturating_sub(a.start_loop);
        let db = b.end_loop.saturating_sub(b.start_loop);
        db.cmp(&da)
    });
    significant.truncate(TOP_N);

    // Materializa (duration_secs, mm:ss, supply) — evita passar
    // referências ao player pelo closure do card.
    let top: Vec<(u32, u32, u32, i32)> = significant
        .iter()
        .map(|b| {
            let dur_secs = (b.end_loop.saturating_sub(b.start_loop) as f64 / lps).round() as u32;
            let start_secs = loop_to_secs(b.start_loop, lps) as u32;
            (dur_secs, start_secs / 60, start_secs % 60, b.supply)
        })
        .collect();

    let title = t("insight.supply_block.title", lang).to_string();
    let help_text = t("insight.supply_block.help", lang).to_string();

    insight_card(ui, config, "supply_block", &title, &help_text, |ui| {
        render_body(ui, config, total_secs, &top);
    });
}

fn render_body(
    ui: &mut Ui,
    config: &AppConfig,
    total_secs: u32,
    top: &[(u32, u32, u32, i32)],
) {
    let lang = config.language;
    let size = size_subtitle(config);

    let color = severity_color_total(total_secs);

    ui.vertical(|ui| {
        ui.label(RichText::new(t("insight.supply_block.total_label", lang)).size(size * 0.85));
        ui.label(
            RichText::new(human_duration(total_secs))
                .size(size * 1.4)
                .strong()
                .color(color),
        );
    });

    ui.add_space(SPACE_M);

    if top.is_empty() {
        ui.label(
            RichText::new(t("insight.supply_block.no_significant", lang))
                .italics()
                .color(Color32::from_rgb(110, 190, 120)),
        );
        ui.add_space(SPACE_M);
        return;
    }

    ui.label(
        RichText::new(t("insight.supply_block.top_header", lang))
            .strong()
            .size(size * 0.95),
    );
    ui.add_space(SPACE_S);

    for (dur_secs, mm, ss, supply) in top {
        ui.label(
            RichText::new(format!(
                "• {}",
                tf(
                    "insight.supply_block.block_line",
                    lang,
                    &[
                        ("duration", &human_duration(*dur_secs)),
                        ("mm", &mm.to_string()),
                        ("ss", &format!("{ss:02}")),
                        ("supply", &supply.to_string()),
                    ],
                )
            ))
            .italics(),
        );
    }

    ui.add_space(SPACE_M);
}

/// Thresholds heuristicos calibrados para ladder médio: <5s é ruído
/// agregado, 5-14s começa a custar unidades, >=15s é um problema de
/// macro real.
fn severity_color_total(total_secs: u32) -> Color32 {
    match total_secs {
        0..=4 => Color32::from_rgb(110, 190, 120),
        5..=14 => Color32::from_rgb(220, 190, 90),
        _ => Color32::from_rgb(220, 140, 80),
    }
}
