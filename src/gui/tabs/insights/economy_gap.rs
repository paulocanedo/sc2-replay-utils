// Card de insight: gap de economia vs adversário.
//
// Compara o número de workers do POV contra o adversário em três
// checkpoints (4, 6, 8 min) — a janela onde a curva econômica define
// o resto da partida. Worker é o proxy mais robusto de "economia" na
// fase inicial (drives income, tech, army). Exige 1v1 (2 jogadores);
// em FFA/2v2 o conceito de "o adversário" não é único.

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay::{PlayerTimeline, StatsSnapshot};
use crate::replay_state::LoadedReplay;
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;

/// Checkpoints (minutos) em que a economia é amostrada.
const CHECKPOINT_MINUTES: [u32; 3] = [4, 6, 8];

/// Cada MULE ativo conta como N workers. 3.5 é o peso efetivo que a
/// comunidade competitiva usa: reflete o rendimento médio de um MULE
/// (que minera mais rápido que um SCV mas vive só 64s) e já desconta
/// distância/bounce no patch.
const MULE_WORKER_EQUIVALENT: f32 = 3.5;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;

    // 1v1 apenas. Em multi-player não há "adversário" canônico.
    if loaded.timeline.players.len() != 2 {
        return;
    }
    let Some(pov) = loaded.timeline.players.get(player_idx) else {
        return;
    };
    let opp_idx = if player_idx == 0 { 1 } else { 0 };
    let Some(opp) = loaded.timeline.players.get(opp_idx) else {
        return;
    };

    let lps = loaded.timeline.loops_per_second.max(0.0001);
    let game_end = loaded.timeline.game_loops;

    let rows: Vec<Row> = CHECKPOINT_MINUTES
        .iter()
        .map(|&min| {
            let target_loop = ((min as f64) * 60.0 * lps).round() as u32;
            if target_loop > game_end {
                return Row {
                    minute: min,
                    beyond_end: true,
                    pov_workers: 0,
                    opp_workers: 0,
                    pov_mules: 0,
                    opp_mules: 0,
                };
            }
            let pov_mules = active_count_at(pov, "MULE", target_loop);
            let opp_mules = active_count_at(opp, "MULE", target_loop);
            Row {
                minute: min,
                beyond_end: false,
                pov_workers: workers_at(pov, target_loop) + mule_bonus(pov_mules),
                opp_workers: workers_at(opp, target_loop) + mule_bonus(opp_mules),
                pov_mules,
                opp_mules,
            }
        })
        .collect();

    let any_mules = rows.iter().any(|r| r.pov_mules > 0 || r.opp_mules > 0);

    let title = t("insight.economy_gap.title", lang).to_string();
    let help_text = t("insight.economy_gap.help", lang).to_string();

    insight_card(ui, config, "economy_gap", &title, &help_text, |ui| {
        render_body(ui, config, &rows, any_mules);
    });
}

struct Row {
    minute: u32,
    beyond_end: bool,
    /// Workers efetivos (workers + peso × MULEs ativos).
    pov_workers: i32,
    /// Idem pro adversário.
    opp_workers: i32,
    pov_mules: i32,
    opp_mules: i32,
}

fn workers_at(player: &PlayerTimeline, target_loop: u32) -> i32 {
    // Último snapshot com game_loop ≤ target. Stats já vem ordenado
    // por loop no parser.
    let idx = match player
        .stats
        .binary_search_by_key(&target_loop, |s: &StatsSnapshot| s.game_loop)
    {
        Ok(i) => i,
        Err(0) => return 0,
        Err(i) => i - 1,
    };
    player.stats.get(idx).map(|s| s.workers).unwrap_or(0)
}

/// Bônus de worker-equivalente dado pelos MULEs ativos, arredondado
/// ao inteiro mais próximo pra somar ao worker count sem mexer no
/// tipo.
fn mule_bonus(mules: i32) -> i32 {
    (mules as f32 * MULE_WORKER_EQUIVALENT).round() as i32
}

/// Conta vivos de `entity_type` no instante `target_loop` usando o
/// índice cumulativo pré-construído em `PlayerTimeline.alive_count`.
fn active_count_at(player: &PlayerTimeline, entity_type: &str, target_loop: u32) -> i32 {
    let Some(series) = player.alive_count.get(entity_type) else {
        return 0;
    };
    let idx = match series.binary_search_by_key(&target_loop, |(gl, _)| *gl) {
        Ok(i) => i,
        Err(0) => return 0,
        Err(i) => i - 1,
    };
    series.get(idx).map(|(_, count)| *count).unwrap_or(0)
}

fn render_body(ui: &mut Ui, config: &AppConfig, rows: &[Row], any_mules: bool) {
    let lang = config.language;
    let size = size_subtitle(config);

    ui.label(
        RichText::new(t("insight.economy_gap.header", lang))
            .strong()
            .size(size * 0.95),
    );
    ui.add_space(SPACE_S);

    for r in rows {
        if r.beyond_end {
            ui.label(
                RichText::new(tf(
                    "insight.economy_gap.beyond_end_line",
                    lang,
                    &[("minute", &r.minute.to_string())],
                ))
                .italics()
                .color(Color32::from_gray(150)),
            );
            continue;
        }
        let delta = r.pov_workers - r.opp_workers;
        let (delta_color, delta_str) = delta_style(delta);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tf(
                    "insight.economy_gap.line",
                    lang,
                    &[
                        ("minute", &r.minute.to_string()),
                        ("pov", &format_worker_cell(r.pov_workers, r.pov_mules, lang)),
                        ("opp", &format_worker_cell(r.opp_workers, r.opp_mules, lang)),
                    ],
                ))
                .italics(),
            );
            ui.label(
                RichText::new(format!("({delta_str})"))
                    .italics()
                    .strong()
                    .color(delta_color),
            );
        });
    }

    if any_mules {
        ui.add_space(SPACE_S);
        ui.label(
            RichText::new(t("insight.economy_gap.mule_footnote", lang))
                .small()
                .italics()
                .color(Color32::from_gray(160)),
        );
    }

    ui.add_space(SPACE_M);
}

/// Formata `{effective} (+N MULE)` quando há mules ativos, só
/// `{effective}` caso contrário.
fn format_worker_cell(effective: i32, mules: i32, lang: crate::locale::Language) -> String {
    if mules <= 0 {
        return effective.to_string();
    }
    tf(
        "insight.economy_gap.cell_with_mules",
        lang,
        &[
            ("workers", &effective.to_string()),
            ("mules", &mules.to_string()),
        ],
    )
}

fn delta_style(delta: i32) -> (Color32, String) {
    if delta > 0 {
        (Color32::from_rgb(110, 190, 120), format!("+{delta}"))
    } else if delta < 0 {
        (Color32::from_rgb(220, 140, 80), delta.to_string())
    } else {
        (Color32::from_gray(160), "0".to_string())
    }
}
