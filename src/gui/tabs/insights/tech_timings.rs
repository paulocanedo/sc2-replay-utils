// Card de insight: timings de tech (mirror-only).
//
// Compara o instante em que cada jogador bateu milestones-chave de
// tech-tree (estruturas ou pesquisas). Fixos por raça — só renderiza
// em mirror match, já que comparar "quando chegou no Stimpack" com
// "quando chegou no Lair" é maçã-com-laranja.

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay::{EntityEventKind, PlayerTimeline};
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;
use super::util::{format_signed_time, mirror_match};

enum Milestone {
    /// Primeira `ProductionFinished` de uma estrutura com `entity_type`
    /// igual ao nome dado. Morphs do mesmo tipo (Hatchery→Lair) são
    /// detectados porque emitem `ProductionFinished` do novo tipo.
    Structure(&'static str),
    /// Primeira entrada em `player.upgrades` com o nome dado.
    Upgrade(&'static str),
}

fn milestones_for(race: char) -> &'static [(&'static str, Milestone)] {
    match race {
        'T' => &[
            ("insight.tech_timings.tr.factory", Milestone::Structure("Factory")),
            ("insight.tech_timings.tr.stimpack", Milestone::Upgrade("Stimpack")),
            ("insight.tech_timings.tr.starport", Milestone::Structure("Starport")),
        ],
        'P' => &[
            (
                "insight.tech_timings.pr.cyber",
                Milestone::Structure("CyberneticsCore"),
            ),
            ("insight.tech_timings.pr.warpgate", Milestone::Upgrade("WarpGate")),
            (
                "insight.tech_timings.pr.twilight",
                Milestone::Structure("TwilightCouncil"),
            ),
        ],
        'Z' => &[
            ("insight.tech_timings.zr.lair", Milestone::Structure("Lair")),
            (
                "insight.tech_timings.zr.roach_warren",
                Milestone::Structure("RoachWarren"),
            ),
            ("insight.tech_timings.zr.hive", Milestone::Structure("Hive")),
        ],
        _ => &[],
    }
}

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
    let milestones = milestones_for(race);
    if milestones.is_empty() {
        return;
    }
    let lps = loaded.timeline.loops_per_second.max(0.0001);

    let rows: Vec<Row> = milestones
        .iter()
        .map(|(label_key, m)| Row {
            label_key,
            pov_loop: milestone_loop(pov, m),
            opp_loop: milestone_loop(opp, m),
        })
        .collect();

    let title = t("insight.tech_timings.title", lang).to_string();
    let help_text = t("insight.tech_timings.help", lang).to_string();

    insight_card(ui, config, "tech_timings", &title, &help_text, |ui| {
        render_body(ui, config, &rows, lps);
    });
}

struct Row {
    label_key: &'static str,
    pov_loop: Option<u32>,
    opp_loop: Option<u32>,
}

fn milestone_loop(player: &PlayerTimeline, milestone: &Milestone) -> Option<u32> {
    match milestone {
        Milestone::Structure(name) => player
            .entity_events
            .iter()
            .find(|e| e.kind == EntityEventKind::ProductionFinished && e.entity_type == *name)
            .map(|e| e.game_loop),
        Milestone::Upgrade(name) => player
            .upgrades
            .iter()
            .find(|u| u.name == *name)
            .map(|u| u.game_loop),
    }
}

fn render_body(ui: &mut Ui, config: &AppConfig, rows: &[Row], lps: f64) {
    let lang = config.language;
    let size = size_subtitle(config);

    ui.label(
        RichText::new(t("insight.tech_timings.header", lang))
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
    let label = t(row.label_key, lang);
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
                "insight.tech_timings.line",
                lang,
                &[
                    ("label", label),
                    ("pov", &pov_str),
                    ("opp", &opp_str),
                ],
            ))
            .italics(),
        );
        if let (Some(p), Some(o)) = (row.pov_loop, row.opp_loop) {
            let delta = p as i64 - o as i64;
            let color = delta_color(delta);
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

/// Para tech, POV mais cedo (delta negativo) é vantagem.
fn delta_color(delta: i64) -> Color32 {
    if delta < 0 {
        Color32::from_rgb(110, 190, 120)
    } else if delta > 0 {
        Color32::from_rgb(220, 140, 80)
    } else {
        Color32::from_gray(160)
    }
}
