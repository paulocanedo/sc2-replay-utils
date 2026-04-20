// Card de insight: tempo ocioso de produção de workers até o minuto X.
//
// Recomputa `compute_idle_periods` com cutoff em `CUTOFF_MINUTES` para
// focar a análise na janela de macro early/mid game — depois disso a
// economia entra em regime e a métrica perde valor diagnóstico. Zerg
// fica N/A (o modelo de slot não se aplica à larva-bandwidth).

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::production_gap::{compute_idle_periods, ProductionGapEntry};
use crate::replay::PlayerTimeline;
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S, SPACE_XL};

use super::card::insight_card;
use super::util::human_duration;

/// Minuto de corte para o card. Fixo em 7 min — janela padrão onde o
/// macro ainda é o fator dominante e idle de worker produziu impacto
/// real na partida.
const CUTOFF_MINUTES: u32 = 7;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let lps = loaded.timeline.loops_per_second.max(0.0001);
    let cutoff_loop = ((CUTOFF_MINUTES as f64) * 60.0 * lps).round() as u32;
    // Clamp pela duração real: replays que terminam antes do cutoff
    // não devem contar o tempo pós-partida como ociosidade (jogo
    // acabou, nada pra produzir).
    let game_end = loaded.timeline.game_loops;
    let effective_end = cutoff_loop.min(game_end);
    let clamped = game_end < cutoff_loop;

    let title = tf(
        "insight.production_idle.title",
        lang,
        &[("minutes", &CUTOFF_MINUTES.to_string())],
    );
    let help_text = tf(
        "insight.production_idle.help",
        lang,
        &[("minutes", &CUTOFF_MINUTES.to_string())],
    );

    insight_card(ui, config, "production_idle", &title, &help_text, |ui| {
        let Some(player) = loaded.timeline.players.get(player_idx) else {
            ui.label(
                RichText::new(t("insight.production_idle.no_data", lang)).italics(),
            );
            return;
        };
        if is_zerg_race(&player.race) {
            ui.label(
                RichText::new(t("insight.production_idle.zerg_na", lang)).italics(),
            );
            ui.add_space(SPACE_M);
            return;
        }

        let (entries, total_idle_loops, efficiency_pct) = compute_idle_periods(
            &player.worker_births,
            &player.worker_capacity,
            effective_end,
        );
        render_body(
            ui,
            config,
            player,
            &entries,
            total_idle_loops,
            efficiency_pct,
            lps,
            clamped,
        );
    });
}

fn render_body(
    ui: &mut Ui,
    config: &AppConfig,
    _player: &PlayerTimeline,
    entries: &[ProductionGapEntry],
    total_idle_loops: u32,
    efficiency_pct: f64,
    lps: f64,
    clamped: bool,
) {
    let lang = config.language;
    let size = size_subtitle(config);

    let total_idle_secs = (total_idle_loops as f64 / lps).round() as u32;
    let eff_color = severity_color_efficiency(efficiency_pct);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("insight.production_idle.efficiency", lang)).size(size * 0.85),
            );
            ui.label(
                RichText::new(format!("{:.1}%", efficiency_pct))
                    .size(size * 1.4)
                    .strong()
                    .color(eff_color),
            );
        });
        ui.add_space(SPACE_XL);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("insight.production_idle.total_idle", lang)).size(size * 0.85),
            );
            ui.label(
                RichText::new(human_duration(total_idle_secs))
                    .size(size * 1.4)
                    .strong(),
            );
        });
    });

    ui.add_space(SPACE_S);

    // Pior gap = maior duração entre entries.
    let worst = entries
        .iter()
        .max_by_key(|e| e.end_loop.saturating_sub(e.start_loop));
    match worst {
        Some(e) => {
            let dur_secs = (e.end_loop.saturating_sub(e.start_loop) as f64 / lps).round() as u32;
            let start_secs = loop_to_secs(e.start_loop, lps) as u32;
            let mm = start_secs / 60;
            let ss = start_secs % 60;
            ui.label(
                RichText::new(tf(
                    "insight.production_idle.worst_line",
                    lang,
                    &[
                        ("duration", &human_duration(dur_secs)),
                        ("mm", &mm.to_string()),
                        ("ss", &format!("{ss:02}")),
                        ("slots", &e.idle_slots.to_string()),
                    ],
                ))
                .italics(),
            );
        }
        None => {
            ui.label(
                RichText::new(t("insight.production_idle.no_gaps", lang))
                    .italics()
                    .color(Color32::from_rgb(110, 190, 120)),
            );
        }
    }

    if clamped {
        ui.add_space(SPACE_S);
        ui.label(
            RichText::new(tf(
                "insight.production_idle.clamped_caveat",
                lang,
                &[("minutes", &CUTOFF_MINUTES.to_string())],
            ))
            .small()
            .italics()
            .color(Color32::from_gray(160)),
        );
    }

    ui.add_space(SPACE_M);
}

fn is_zerg_race(race: &str) -> bool {
    race.starts_with('Z') || race.starts_with('z')
}

fn severity_color_efficiency(pct: f64) -> Color32 {
    if pct >= 95.0 {
        Color32::from_rgb(110, 190, 120)
    } else if pct >= 85.0 {
        Color32::from_rgb(220, 190, 90)
    } else {
        Color32::from_rgb(220, 140, 80)
    }
}
