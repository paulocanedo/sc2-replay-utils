//! Painel lateral — stats de um jogador renderizadas verticalmente.

use egui::{RichText, Ui};

use crate::colors::player_slot_color_bright;
use crate::locale::{tf, Language};
use crate::replay::PlayerTimeline;

use super::entities::structure_attention_at;

/// Renderiza stats de um jogador verticalmente num painel lateral.
pub(super) fn player_side_panel(
    ui: &mut Ui,
    p: &PlayerTimeline,
    idx: usize,
    game_loop: u32,
    lang: Language,
) {
    let slot = player_slot_color_bright(idx);
    ui.add_space(4.0);
    ui.label(RichText::new(&p.name).strong().color(slot));
    ui.add_space(4.0);
    match p.stats_at(game_loop) {
        Some(s) => {
            let supply_cap = s.supply_made.min(200);
            let army = s.army_value_minerals + s.army_value_vespene;
            ui.monospace(tf(
                "timeline.stats.supply",
                lang,
                &[
                    ("used", &s.supply_used.to_string()),
                    ("cap", &supply_cap.to_string()),
                ],
            ));
            ui.monospace(tf(
                "timeline.stats.minerals",
                lang,
                &[("val", &s.minerals.to_string())],
            ));
            ui.monospace(tf(
                "timeline.stats.gas",
                lang,
                &[("val", &s.vespene.to_string())],
            ));
            ui.monospace(tf(
                "timeline.stats.workers",
                lang,
                &[("val", &s.workers.to_string())],
            ));
            ui.monospace(tf(
                "timeline.stats.army",
                lang,
                &[("val", &army.to_string())],
            ));
        }
        None => {
            ui.weak("—");
        }
    }
    let (att, tot) = structure_attention_at(p, game_loop);
    let txt = if tot == 0 {
        tf("timeline.stats.bldg_focus_none", lang, &[])
    } else {
        let pct = att as f32 * 100.0 / tot as f32;
        tf(
            "timeline.stats.bldg_focus",
            lang,
            &[("pct", &format!("{pct:.0}"))],
        )
    };
    ui.monospace(txt);
}
