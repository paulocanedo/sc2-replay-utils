// Card de insight: principais perdas.
//
// Lista as N unidades/estruturas mais caras que o POV perdeu para o
// adversário (ordenadas por custo total em recursos). Ajuda a lembrar
// dos momentos críticos que custaram o jogo: Battlecruiser perdido,
// Nexus caído, Siege Tank pego por flanking, etc.

use egui::{RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{localize, t, tf};
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use crate::loss_analysis::{player_losses, DeathEvent};

use super::card::insight_card;

/// Quantas perdas mostrar no topo da lista.
const TOP_N: usize = 5;

/// Custo mínimo (minerals + vespene) pra uma perda entrar na lista.
/// Filtra ruído — overlords, zerglings isolados, etc. que por si só
/// raramente são a origem de uma derrota.
const MIN_TOTAL_VALUE: u32 = 100;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let lps = loaded.timeline.loops_per_second.max(0.0001);

    let mut losses = player_losses(&loaded.timeline, player_idx);
    losses.retain(|d| d.total_value() >= MIN_TOTAL_VALUE);
    losses.sort_by(|a, b| b.total_value().cmp(&a.total_value()));
    losses.truncate(TOP_N);

    let title = t("insight.key_losses.title", lang).to_string();
    let help_text = t("insight.key_losses.help", lang).to_string();

    insight_card(ui, config, "key_losses", &title, &help_text, |ui| {
        render_body(ui, config, &losses, lps);
    });
}

fn render_body(ui: &mut Ui, config: &AppConfig, losses: &[DeathEvent], lps: f64) {
    let lang = config.language;
    let size = size_subtitle(config);

    if losses.is_empty() {
        ui.label(
            RichText::new(t("insight.key_losses.no_losses", lang))
                .italics(),
        );
        ui.add_space(SPACE_M);
        return;
    }

    ui.label(
        RichText::new(t("insight.key_losses.header", lang))
            .strong()
            .size(size * 0.95),
    );
    ui.add_space(SPACE_S);

    for d in losses {
        let start_secs = loop_to_secs(d.game_loop, lps) as u32;
        let mm = start_secs / 60;
        let ss = start_secs % 60;
        let name = localize(&d.entity_type, lang);
        ui.label(
            RichText::new(format!(
                "• {}",
                tf(
                    "insight.key_losses.line",
                    lang,
                    &[
                        ("unit", name),
                        ("minerals", &d.minerals.to_string()),
                        ("vespene", &d.vespene.to_string()),
                        ("mm", &mm.to_string()),
                        ("ss", &format!("{ss:02}")),
                    ],
                )
            ))
            .italics(),
        );
    }

    ui.add_space(SPACE_M);
}
