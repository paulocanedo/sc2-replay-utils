// Card de insight: potencial de produção de workers até o minuto X.
//
// Calcula quantos workers o jogador poderia ter em X minutos respeitando
// timing de bases, saturação e chronos gastos em probes (Protoss) ou
// regra de 80% das larvas (Zerg). Mostra real vs potencial + gap.

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::LoadedReplay;
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S, SPACE_XL};
use crate::worker_potential::{self, LimitingFactor, WorkerPotential};

use super::card::insight_card;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let minutes = config.insight_worker_minutes.max(1);
    // Game-clock minute → loops. `loops_per_second` depende da
    // game speed (22.4 em Faster, 16 em Normal, etc.) — usar constante
    // 960 quebrava todo replay competitivo.
    let until_loop =
        ((minutes as f64) * 60.0 * loaded.timeline.loops_per_second).round() as u32;

    // Chrono budget: soma `chrono_boosts` de entradas Probe (só Protoss
    // terá valor > 0). `BuildOrderEntry.chrono_boosts` já vem com a
    // estimativa do build_order, que respeita o uso real do jogador.
    let chrono_probe_count = loaded
        .build_order
        .as_ref()
        .and_then(|bo| bo.players.get(player_idx))
        .map(|p| {
            p.entries
                .iter()
                .filter(|e| e.action == "Probe")
                .map(|e| e.chrono_boosts as u32)
                .sum::<u32>()
        })
        .unwrap_or(0);

    let wp = worker_potential::compute_worker_potential(
        &loaded.timeline,
        player_idx,
        until_loop,
        chrono_probe_count,
    );

    let title = tf(
        "insight.worker_potential.title",
        lang,
        &[("minutes", &minutes.to_string())],
    );
    let help_text = t("insight.worker_potential.help", lang).to_string();

    insight_card(
        ui,
        config,
        "worker_potential",
        &title,
        &help_text,
        |ui| render_body(ui, config, wp, minutes),
    );
}

fn render_body(ui: &mut Ui, config: &AppConfig, wp: WorkerPotential, minutes: u32) {
    let lang = config.language;
    let size = size_subtitle(config);

    // Linha principal: Real — Potencial — Gap.
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("insight.worker_potential.produced", lang))
                    .size(size * 0.85),
            );
            ui.label(RichText::new(wp.produced.to_string()).size(size * 1.4).strong());
        });
        ui.add_space(SPACE_XL);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("insight.worker_potential.potential", lang))
                    .size(size * 0.85),
            );
            ui.label(RichText::new(wp.potential.to_string()).size(size * 1.4).strong());
        });
        ui.add_space(SPACE_XL);

        let gap = wp.potential.saturating_sub(wp.produced);
        let gap_color = match gap {
            0..=2 => Color32::from_rgb(110, 190, 120),   // verde
            3..=7 => Color32::from_rgb(220, 190, 90),    // amarelo
            _ => Color32::from_rgb(220, 140, 80),        // laranja
        };
        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("insight.worker_potential.gap_label", lang))
                    .size(size * 0.85),
            );
            ui.label(
                RichText::new(format!("-{gap}"))
                    .size(size * 1.4)
                    .strong()
                    .color(gap_color),
            );
        });
    });

    ui.add_space(SPACE_S);

    let key = match wp.limited_by {
        LimitingFactor::Bases => "insight.worker_potential.limited_by_bases",
        LimitingFactor::ChronoBudget => "insight.worker_potential.limited_by_chrono_budget",
        LimitingFactor::LarvaSupply => "insight.worker_potential.limited_by_larva",
        LimitingFactor::NoLimit => "insight.worker_potential.limited_by_none",
    };
    ui.label(RichText::new(t(key, lang)).italics());

    if wp.clamped {
        ui.add_space(SPACE_S);
        ui.label(
            RichText::new(tf(
                "insight.worker_potential.clamped_caveat",
                lang,
                &[("minutes", &minutes.to_string())],
            ))
            .small()
            .italics()
            .color(Color32::from_gray(160)),
        );
    }

    ui.add_space(SPACE_M);
}
