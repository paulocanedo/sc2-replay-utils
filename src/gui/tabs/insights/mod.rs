// Aba Insights — cards com insights automáticos sobre o replay
// carregado, do ponto de vista de um jogador escolhido.
//
// POV padrão: primeiro jogador cujo nickname bate com
// `AppConfig.user_nicknames`; se nenhum bater, cai em 0. O usuário pode
// trocar no ComboBox do topo pra ver os insights do adversário.

pub mod army_prod_by_battle;
pub mod army_trades;
pub mod base_timings;
pub mod card;
pub mod chrono_distribution;
pub mod economy_gap;
pub mod grid;
pub mod inject_efficiency;
pub mod production_idle;
pub mod resources_unspent;
pub mod supply_block;
pub mod tech_timings;
pub mod turning_point;
pub mod util;
pub mod worker_potential;

use egui::{ScrollArea, Ui};

use crate::config::AppConfig;
use crate::locale::t;
use crate::replay_state::LoadedReplay;
use crate::tokens::{SPACE_M, SPACE_S};
use crate::widgets::player_pov_pill;

/// Retorna `Some(target_loop)` quando algum card pediu seek pra aba
/// Timeline (hoje, apenas o card Turning Point). Caller (central.rs)
/// aplica: `timeline_tab_loop = target` + `active_tab = Tab::Timeline`.
pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    pov: &mut Option<usize>,
) -> Option<u32> {
    let lang = config.language;

    // Lazy-init do POV no primeiro render pós-load: resolve pelo
    // nickname do usuário (ou cai em 0).
    if pov.is_none() {
        let idx = loaded
            .user_player_index(&config.user_nicknames)
            .unwrap_or(0);
        *pov = Some(idx);
    }
    // Garante que o índice persistido continua válido mesmo se o
    // número de jogadores mudar (não deveria, mas defensivo).
    let player_count = loaded.timeline.players.len();
    if let Some(i) = *pov {
        if i >= player_count {
            *pov = Some(0);
        }
    }
    let selected = pov.unwrap_or(0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        ui.label(t("insights.pov_label", lang));
        for (idx, p) in loaded.timeline.players.iter().enumerate() {
            let is_user = config.is_user(&p.name);
            let is_selected = idx == selected;
            if player_pov_pill(
                ui,
                &p.name,
                &p.race,
                idx,
                is_user,
                is_selected,
                config,
                lang,
            )
            .clicked()
            {
                *pov = Some(idx);
            }
        }
    });
    ui.add_space(SPACE_M);

    let mut seek_request: Option<u32> = None;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(target) = grid::render_masonry(ui, loaded, config, selected) {
                seek_request = Some(target);
            }
        });
    seek_request
}
