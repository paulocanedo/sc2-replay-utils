// Card de insight: distribuição de Chrono Boost (só Protoss).
//
// Agrega `chrono_boosts` das entradas do build_order por `EntryKind` e
// mostra quanto do orçamento de chrono foi pra eco (workers), army
// (unidades), tech (upgrades/research) ou estrutura. Não há divisão
// universalmente correta — o card expõe o balanço pra jogador checar
// se bate com seu game plan. Não renderiza nada pra outras raças.

use egui::{RichText, Ui};

use crate::build_order::{classify_entry, EntryKind};
use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::LoadedReplay;
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let Some(player) = loaded.timeline.players.get(player_idx) else {
        return;
    };
    if !is_protoss_race(&player.race) {
        return;
    }

    let Some(bo) = loaded.build_order.as_ref() else {
        return;
    };
    let Some(po) = bo.players.get(player_idx) else {
        return;
    };

    let mut eco: u32 = 0;
    let mut army: u32 = 0;
    let mut tech: u32 = 0;
    let mut structure: u32 = 0;
    for e in &po.entries {
        let n = e.chrono_boosts as u32;
        if n == 0 {
            continue;
        }
        match classify_entry(e) {
            EntryKind::Worker => eco += n,
            EntryKind::Unit => army += n,
            EntryKind::Research | EntryKind::Upgrade => tech += n,
            EntryKind::Structure => structure += n,
            EntryKind::Inject => {}
        }
    }
    let total = eco + army + tech + structure;

    let title = t("insight.chrono_distribution.title", lang).to_string();
    let help_text = t("insight.chrono_distribution.help", lang).to_string();

    insight_card(ui, config, "chrono_distribution", &title, &help_text, |ui| {
        render_body(ui, config, total, eco, army, tech, structure);
    });
}

fn render_body(
    ui: &mut Ui,
    config: &AppConfig,
    total: u32,
    eco: u32,
    army: u32,
    tech: u32,
    structure: u32,
) {
    let lang = config.language;
    let size = size_subtitle(config);

    if total == 0 {
        ui.label(
            RichText::new(t("insight.chrono_distribution.none", lang)).italics(),
        );
        ui.add_space(SPACE_M);
        return;
    }

    ui.vertical(|ui| {
        ui.label(RichText::new(t("insight.chrono_distribution.total", lang)).size(size * 0.85));
        ui.label(RichText::new(total.to_string()).size(size * 1.4).strong());
    });

    ui.add_space(SPACE_S);

    let cats: [(&str, u32); 4] = [
        ("insight.chrono_distribution.eco", eco),
        ("insight.chrono_distribution.army", army),
        ("insight.chrono_distribution.tech", tech),
        ("insight.chrono_distribution.structure", structure),
    ];
    for (label_key, count) in cats {
        if count == 0 {
            continue;
        }
        let pct = (count as f64 / total as f64 * 100.0).round() as u32;
        ui.label(
            RichText::new(format!(
                "• {}",
                tf(
                    "insight.chrono_distribution.category_line",
                    lang,
                    &[
                        ("label", t(label_key, lang)),
                        ("count", &count.to_string()),
                        ("pct", &pct.to_string()),
                    ],
                )
            ))
            .italics(),
        );
    }

    ui.add_space(SPACE_M);
}

fn is_protoss_race(race: &str) -> bool {
    race.starts_with('P') || race.starts_with('p')
}
