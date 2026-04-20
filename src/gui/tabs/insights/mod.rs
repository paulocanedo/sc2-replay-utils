// Aba Insights — cards com insights automáticos sobre o replay
// carregado, do ponto de vista de um jogador escolhido.
//
// POV padrão: primeiro jogador cujo nickname bate com
// `AppConfig.user_nicknames`; se nenhum bater, cai em 0. O usuário pode
// trocar no ComboBox do topo pra ver os insights do adversário.

pub mod card;
pub mod worker_potential;

use egui::{ScrollArea, Ui};

use crate::config::AppConfig;
use crate::locale::t;
use crate::replay_state::LoadedReplay;
use crate::tokens::SPACE_M;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, pov: &mut Option<usize>) {
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
        ui.label(t("insights.pov_label", lang));
        let selected_name = loaded
            .timeline
            .players
            .get(selected)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("insights_pov")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for (idx, p) in loaded.timeline.players.iter().enumerate() {
                    let mut cur = selected;
                    if ui.selectable_value(&mut cur, idx, &p.name).clicked() {
                        *pov = Some(idx);
                    }
                }
            });
    });
    ui.add_space(SPACE_M);

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            worker_potential::show(ui, loaded, config, selected);
        });
}
