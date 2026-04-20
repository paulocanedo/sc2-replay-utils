// Card de insight: timings de expansão (mirror-only).
//
// Compara quando cada jogador terminou sua 2ª, 3ª e 4ª base contra o
// adversário. Só renderiza em mirror match (1v1 mesma raça) — fora
// disso as economias de raças diferentes têm curvas de expansão
// distintas (Zerg expande muito mais cedo por design), e a comparação
// vira ruído em vez de sinal.

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay::{EntityEventKind, PlayerTimeline};
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;
use super::util::{format_signed_time, mirror_match};

/// Quais bases comparar. A 1ª é o main (spawn), não tem timing
/// relevante; começamos da 2ª (expansion natural).
const BASE_ORDINALS: [usize; 3] = [2, 3, 4];

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let Some((opp_idx, race)) = mirror_match(loaded, player_idx) else {
        return;
    };
    let Some(pov) = loaded.timeline.players.get(player_idx) else {
        return;
    };
    let Some(opp) = loaded.timeline.players.get(opp_idx) else {
        return;
    };
    let townhall = match race {
        'T' => "CommandCenter",
        'P' => "Nexus",
        'Z' => "Hatchery",
        _ => return,
    };
    let lps = loaded.timeline.loops_per_second.max(0.0001);

    let rows: Vec<Row> = BASE_ORDINALS
        .iter()
        .map(|&n| Row {
            ordinal: n,
            pov_loop: nth_townhall_loop(pov, townhall, n),
            opp_loop: nth_townhall_loop(opp, townhall, n),
        })
        .collect();

    let title = t("insight.base_timings.title", lang).to_string();
    let help_text = t("insight.base_timings.help", lang).to_string();

    insight_card(ui, config, "base_timings", &title, &help_text, |ui| {
        render_body(ui, config, &rows, lps);
    });
}

struct Row {
    ordinal: usize,
    pov_loop: Option<u32>,
    opp_loop: Option<u32>,
}

/// Acha o loop do N-ésimo `ProductionFinished` do tipo townhall (new
/// construction). Morphs (Lair/Hive/Orbital/PF) não contam porque
/// emitem eventos do novo tipo.
fn nth_townhall_loop(player: &PlayerTimeline, townhall: &str, n: usize) -> Option<u32> {
    let mut count = 0;
    for e in &player.entity_events {
        if e.kind == EntityEventKind::ProductionFinished && e.entity_type == townhall {
            count += 1;
            if count == n {
                return Some(e.game_loop);
            }
        }
    }
    None
}

fn render_body(ui: &mut Ui, config: &AppConfig, rows: &[Row], lps: f64) {
    let lang = config.language;
    let size = size_subtitle(config);

    ui.label(
        RichText::new(t("insight.base_timings.header", lang))
            .strong()
            .size(size * 0.95),
    );
    ui.add_space(SPACE_S);

    for r in rows {
        render_row(ui, lang, r, lps);
    }

    ui.add_space(SPACE_M);
}

fn render_row(ui: &mut Ui, lang: crate::locale::Language, row: &Row, lps: f64) {
    let pov_str = row
        .pov_loop
        .map(|l| format_time(l, lps))
        .unwrap_or_else(|| "—".to_string());
    let opp_str = row
        .opp_loop
        .map(|l| format_time(l, lps))
        .unwrap_or_else(|| "—".to_string());

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tf(
                "insight.base_timings.line",
                lang,
                &[
                    ("ordinal", &row.ordinal.to_string()),
                    ("pov", &pov_str),
                    ("opp", &opp_str),
                ],
            ))
            .italics(),
        );
        if let (Some(p), Some(o)) = (row.pov_loop, row.opp_loop) {
            let delta = p as i64 - o as i64;
            let (color, _) = delta_color(delta);
            ui.label(
                RichText::new(format!("({})", format_signed_time(delta, lps)))
                    .italics()
                    .strong()
                    .color(color),
            );
        }
    });
}

fn format_time(game_loop: u32, lps: f64) -> String {
    let secs = loop_to_secs(game_loop, lps) as u32;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Para base timings, POV mais cedo (delta negativo) é vantagem.
fn delta_color(delta: i64) -> (Color32, ()) {
    if delta < 0 {
        (Color32::from_rgb(110, 190, 120), ())
    } else if delta > 0 {
        (Color32::from_rgb(220, 140, 80), ())
    } else {
        (Color32::from_gray(160), ())
    }
}
