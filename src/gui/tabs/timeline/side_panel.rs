//! Painel lateral — stats de um jogador renderizadas verticalmente.

use egui::{RichText, Ui};

use crate::colors::player_slot_color_bright;
use crate::replay::PlayerTimeline;

use super::entities::structure_attention_at;

/// Renderiza stats de um jogador verticalmente num painel lateral.
pub(super) fn player_side_panel(ui: &mut Ui, p: &PlayerTimeline, idx: usize, game_loop: u32) {
    let slot = player_slot_color_bright(idx);
    ui.add_space(4.0);
    ui.label(RichText::new(&p.name).strong().color(slot));
    ui.add_space(4.0);
    match p.stats_at(game_loop) {
        Some(s) => {
            let supply_cap = s.supply_made.min(200);
            let army = s.army_value_minerals + s.army_value_vespene;
            ui.monospace(format!("{}/{} supply", s.supply_used, supply_cap));
            ui.monospace(format!("{} min", s.minerals));
            ui.monospace(format!("{} gas", s.vespene));
            ui.monospace(format!("{} wks", s.workers));
            ui.monospace(format!("{} army", army));
        }
        None => {
            ui.weak("—");
        }
    }
    let (att, tot) = structure_attention_at(p, game_loop);
    let txt = if tot == 0 {
        "— bldg focus".to_string()
    } else {
        let pct = att as f32 * 100.0 / tot as f32;
        format!("{:.0}% bldg focus", pct)
    };
    ui.monospace(txt);
}
