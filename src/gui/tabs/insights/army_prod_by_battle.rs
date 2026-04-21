// Card de insight: eficiência de produção de army entre as 3 primeiras
// grandes lutas.
//
// Reporta até 3 valores — um por segmento, cada segmento encerrado pelo
// início de uma grande luta (mesmo threshold do card Army Trades).
// A contagem zera a cada marco: segmento N+1 começa exatamente no
// start_loop da luta N, sem gap e sem double-count.
//
// Contabiliza apenas producers de army **ativos** (exclui Barracks/
// Factory/Starport enquanto estão construindo addon; morph Gateway→
// WarpGate já é tratado via `army_capacity`). O segmento 1 só começa
// quando o primeiro producer fica pronto (tech-gated). Jogadores Zerg
// ficam de fora — `army_capacity` só cobre Terran/Protoss por design.

use egui::{Color32, RichText, Ui};

use crate::army_production_by_battle::{
    extract_army_production_by_battle, BattleSegmentEfficiency,
};
use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let lps = loaded.timeline.loops_per_second.max(0.0001);

    let data = extract_army_production_by_battle(&loaded.timeline, player_idx);

    let title = t("insight.army_prod_by_battle.title", lang).to_string();
    let help_text = t("insight.army_prod_by_battle.help", lang).to_string();

    insight_card(
        ui,
        config,
        "army_prod_by_battle",
        &title,
        &help_text,
        |ui| render_body(ui, config, &data.segments, lps),
    );
}

fn render_body(
    ui: &mut Ui,
    config: &AppConfig,
    segments: &[BattleSegmentEfficiency],
    lps: f64,
) {
    let lang = config.language;
    let size = size_subtitle(config);

    if segments.is_empty() {
        ui.label(
            RichText::new(t("insight.army_prod_by_battle.no_battles", lang)).italics(),
        );
        ui.add_space(SPACE_M);
        return;
    }

    ui.label(
        RichText::new(t("insight.army_prod_by_battle.header", lang))
            .strong()
            .size(size * 0.95),
    );
    ui.add_space(SPACE_S);

    for seg in segments {
        let start_secs = loop_to_secs(seg.start_loop, lps) as u32;
        let end_secs = loop_to_secs(seg.end_loop, lps) as u32;
        let pct_color = severity_color(seg.efficiency_pct);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tf(
                    "insight.army_prod_by_battle.segment_line",
                    lang,
                    &[
                        ("index", &seg.index.to_string()),
                        ("start_mm", &(start_secs / 60).to_string()),
                        ("start_ss", &format!("{:02}", start_secs % 60)),
                        ("end_mm", &(end_secs / 60).to_string()),
                        ("end_ss", &format!("{:02}", end_secs % 60)),
                    ],
                ))
                .italics(),
            );
            ui.label(
                RichText::new(format!("{:.0}%", seg.efficiency_pct))
                    .italics()
                    .strong()
                    .color(pct_color),
            );
        });
    }

    ui.add_space(SPACE_M);
}

fn severity_color(pct: f64) -> Color32 {
    if pct >= 80.0 {
        Color32::from_rgb(110, 190, 120)
    } else if pct >= 60.0 {
        Color32::from_rgb(220, 190, 90)
    } else {
        Color32::from_rgb(220, 140, 80)
    }
}
