// Card de insight: piores trades de army.
//
// Agrupa as mortes em engajamentos (clusters contíguos por proximidade
// temporal) e destaca os 3 piores do ponto de vista do POV — onde ele
// perdeu mais recurso do que destruiu. Só units/workers contam; prédios
// são cobertos pelo card Key Losses.

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use crate::loss_analysis::{cluster_engagements, player_kills, player_losses, Engagement};

use super::card::insight_card;

/// Gap (em segundos) que separa engajamentos. Mortes dentro dessa
/// janela entram no mesmo cluster — suficiente pra cobrir um combate
/// típico de SC2 sem fundir fights separados.
const GAP_SECS: u32 = 15;

/// Valor mínimo total (lost + killed) em recursos pra um engajamento
/// ser considerado. Filtra escaramuças pequenas (1-2 unidades).
const MIN_TOTAL_VALUE: u32 = 300;

/// Quantos engajamentos mostrar.
const TOP_N: usize = 3;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let lps = loaded.timeline.loops_per_second.max(0.0001);
    let gap_loops = (GAP_SECS as f64 * lps).round() as u32;

    let losses = player_losses(&loaded.timeline, player_idx);
    let kills = player_kills(&loaded.timeline, player_idx);
    let mut engagements = cluster_engagements(&losses, &kills, gap_loops);
    engagements.retain(|e| e.total_value() >= MIN_TOTAL_VALUE);
    // Pior trade = net mais negativo (perdi muito mais do que matei).
    engagements.sort_by(|a, b| a.net_trade().cmp(&b.net_trade()));
    engagements.truncate(TOP_N);

    let title = t("insight.army_trades.title", lang).to_string();
    let help_text = t("insight.army_trades.help", lang).to_string();

    insight_card(ui, config, "army_trades", &title, &help_text, |ui| {
        render_body(ui, config, &engagements, lps);
    });
}

fn render_body(ui: &mut Ui, config: &AppConfig, engagements: &[Engagement], lps: f64) {
    let lang = config.language;
    let size = size_subtitle(config);

    if engagements.is_empty() {
        ui.label(
            RichText::new(t("insight.army_trades.no_trades", lang)).italics(),
        );
        ui.add_space(SPACE_M);
        return;
    }

    ui.label(
        RichText::new(t("insight.army_trades.header", lang))
            .strong()
            .size(size * 0.95),
    );
    ui.add_space(SPACE_S);

    for e in engagements {
        let start_secs = loop_to_secs(e.start_loop, lps) as u32;
        let mm = start_secs / 60;
        let ss = start_secs % 60;
        let net = e.net_trade();
        let net_color = net_color(net);
        let net_text = format_signed(net);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "• {}",
                    tf(
                        "insight.army_trades.line",
                        lang,
                        &[
                            ("mm", &mm.to_string()),
                            ("ss", &format!("{ss:02}")),
                            ("lost", &e.lost_value.to_string()),
                            ("killed", &e.killed_value.to_string()),
                        ],
                    )
                ))
                .italics(),
            );
            ui.label(
                RichText::new(format!("({net_text})"))
                    .italics()
                    .strong()
                    .color(net_color),
            );
        });
    }

    ui.add_space(SPACE_M);
}

fn format_signed(net: i64) -> String {
    if net >= 0 {
        format!("+{net}")
    } else {
        net.to_string()
    }
}

fn net_color(net: i64) -> Color32 {
    if net >= 0 {
        Color32::from_rgb(110, 190, 120)
    } else if net >= -500 {
        Color32::from_rgb(220, 190, 90)
    } else {
        Color32::from_rgb(220, 140, 80)
    }
}
