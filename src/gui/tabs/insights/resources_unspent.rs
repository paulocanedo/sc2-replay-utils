// Card de insight: recursos flutuando (unspent).
//
// Média de minerals/vespene no banco ao longo da partida. Floats altos
// indicam que o jogador não está gastando rápido o bastante —
// produção idle, tech parada ou poucos produtores. Stats snapshots
// vêm em intervalos regulares (~10s) do tracker, então a média
// aritmética simples já é time-weighted.

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::LoadedReplay;
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S, SPACE_XL};

use super::card::insight_card;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let title = t("insight.resources_unspent.title", lang).to_string();
    let help_text = t("insight.resources_unspent.help", lang).to_string();

    insight_card(ui, config, "resources_unspent", &title, &help_text, |ui| {
        let Some(player) = loaded.timeline.players.get(player_idx) else {
            ui.label(
                RichText::new(t("insight.resources_unspent.no_data", lang)).italics(),
            );
            return;
        };
        if player.stats.is_empty() {
            ui.label(
                RichText::new(t("insight.resources_unspent.no_data", lang)).italics(),
            );
            return;
        }

        let n = player.stats.len() as f64;
        let sum_min: i64 = player.stats.iter().map(|s| s.minerals as i64).sum();
        let sum_gas: i64 = player.stats.iter().map(|s| s.vespene as i64).sum();
        let avg_min = (sum_min as f64 / n).round() as u32;
        let avg_gas = (sum_gas as f64 / n).round() as u32;
        let max_min = player.stats.iter().map(|s| s.minerals).max().unwrap_or(0).max(0) as u32;
        let max_gas = player.stats.iter().map(|s| s.vespene).max().unwrap_or(0).max(0) as u32;

        render_body(ui, config, avg_min, avg_gas, max_min, max_gas);
    });
}

fn render_body(
    ui: &mut Ui,
    config: &AppConfig,
    avg_min: u32,
    avg_gas: u32,
    max_min: u32,
    max_gas: u32,
) {
    let lang = config.language;
    let size = size_subtitle(config);

    let min_color = severity_color(avg_min);
    let gas_color = severity_color(avg_gas);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("insight.resources_unspent.avg_minerals", lang)).size(size * 0.85),
            );
            ui.label(
                RichText::new(avg_min.to_string())
                    .size(size * 1.4)
                    .strong()
                    .color(min_color),
            );
        });
        ui.add_space(SPACE_XL);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("insight.resources_unspent.avg_gas", lang)).size(size * 0.85),
            );
            ui.label(
                RichText::new(avg_gas.to_string())
                    .size(size * 1.4)
                    .strong()
                    .color(gas_color),
            );
        });
    });

    ui.add_space(SPACE_S);

    ui.label(
        RichText::new(tf(
            "insight.resources_unspent.peak_line",
            lang,
            &[
                ("max_min", &max_min.to_string()),
                ("max_gas", &max_gas.to_string()),
            ],
        ))
        .italics(),
    );

    ui.add_space(SPACE_M);
}

/// Thresholds heuristicos: <300 é gasto saudável, 300-600 cuidado,
/// 600+ está floatando demais. Naturalmente distorcido no late game
/// quando a economia satura — uma média alta num jogo curto é mais
/// alarmante que num jogo longo.
fn severity_color(avg: u32) -> Color32 {
    match avg {
        0..=299 => Color32::from_rgb(110, 190, 120),
        300..=599 => Color32::from_rgb(220, 190, 90),
        _ => Color32::from_rgb(220, 140, 80),
    }
}
