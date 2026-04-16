// Cards de resumo numérico (supply blocks + production efficiency).

use egui::{RichText, Ui};

use crate::colors::{player_slot_color_bright, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::locale::{t, tf, Language};
use crate::replay_state::LoadedReplay;

pub(super) fn summary_cards(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
    let lang = config.language;
    ui.columns(2, |cols| {
        // Card 1: supply blocks
        card(&mut cols[0], t("charts.card.supply_blocks", lang), |ui| {
            let lps = loaded.timeline.loops_per_second.max(0.0001);
            for (idx, p) in loaded.timeline.players.iter().enumerate() {
                let blocks = loaded
                    .supply_blocks_per_player
                    .get(idx)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let count = blocks.len();
                let total_loops: u32 =
                    blocks.iter().map(|b| b.end_loop.saturating_sub(b.start_loop)).sum();
                let total_secs = (total_loops as f64 / lps) as u32;
                player_line(
                    ui,
                    &p.name,
                    idx,
                    &tf(
                        "charts.supply_block.summary",
                        lang,
                        &[("count", &count.to_string()), ("secs", &total_secs.to_string())],
                    ),
                    config.is_user(&p.name),
                    lang,
                );
            }
        });

        // Card 2: production efficiency — separado em duas sub-colunas
        // lado a lado (Workers | Army). Workers continua vindo de
        // `production_gap.rs` (escalar canônico com MIN_IDLE_LOOPS/
        // backoff próprios — Zerg mostra "—" nessa coluna porque o
        // modelo de threshold não se aplica a larva-bandwidth). Army
        // é a média das amostras do time-series `efficiency_army` —
        // Zerg agora incluso (modelo larva-bandwidth).
        card(&mut cols[1], t("charts.card.production_efficiency", lang), |ui| {
            let has_any = loaded.production.is_some() || loaded.efficiency_army.is_some();
            if !has_any {
                ui.small(t("charts.card.empty", lang));
                return;
            }

            ui.columns(2, |sub| {
                // Coluna Workers.
                sub[0].label(
                    RichText::new(t("charts.card.efficiency.workers", lang))
                        .small()
                        .strong(),
                );
                if let Some(pg) = loaded.production.as_ref() {
                    for (idx, p) in pg.players.iter().enumerate() {
                        let value = if p.is_zerg {
                            "—".to_string()
                        } else {
                            format!("{:.1}%", p.efficiency_pct)
                        };
                        player_line(&mut sub[0], &p.name, idx, &value, config.is_user(&p.name), lang);
                    }
                } else {
                    sub[0].small(t("charts.card.empty", lang));
                }

                // Coluna Army.
                sub[1].label(
                    RichText::new(t("charts.card.efficiency.army", lang))
                        .small()
                        .strong(),
                );
                if let Some(series) = loaded.efficiency_army.as_ref() {
                    for (idx, p) in series.players.iter().enumerate() {
                        let value = if p.samples.is_empty() {
                            "—".to_string()
                        } else {
                            format!("{:.1}%", average_efficiency(&p.samples))
                        };
                        player_line(&mut sub[1], &p.name, idx, &value, config.is_user(&p.name), lang);
                    }
                } else {
                    sub[1].small(t("charts.card.empty", lang));
                }
            });
        });
    });

}

/// Média simples das `efficiency_pct` das amostras. As amostras vêm
/// de buckets de tamanho fixo (só o último pode ser parcial), então
/// a média aritmética é uma boa aproximação da média ponderada pelo
/// tempo — suficiente para um número de resumo no card.
fn average_efficiency(samples: &[crate::production_efficiency::EfficiencySample]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| s.efficiency_pct).sum();
    sum / samples.len() as f64
}

fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.group(|ui| {
        ui.set_min_height(100.0);
        ui.label(RichText::new(title).strong());
        ui.separator();
        body(ui);
    });
}

fn player_line(ui: &mut Ui, name: &str, index: usize, value: &str, is_user: bool, lang: Language) {
    ui.horizontal(|ui| {
        // Nome colorido com a cor do slot (P1 vermelho, P2 azul). Se é
        // o usuário, adiciona um chip "You" discreto logo depois —
        // sem sequestrar a cor do nome, que pertence ao slot.
        let name_text = RichText::new(name)
            .small()
            .strong()
            .color(player_slot_color_bright(index));
        ui.label(name_text);
        if is_user {
            ui.label(
                RichText::new(format!("{} ", t("common.you_chip", lang)))
                    .small()
                    .color(USER_CHIP_FG)
                    .background_color(USER_CHIP_BG),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.monospace(value);
        });
    });
}
