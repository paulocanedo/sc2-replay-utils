// Aba Build Order — duas colunas lado a lado, uma por jogador.
// Cada coluna mostra: cabeçalho com nome/race/MMR + tabela scrollable
// com mm:ss, supply, ação. A identidade visual de cada jogador segue
// a convenção in-game do SC2: P1 = vermelho, P2 = azul (borda do
// frame). O realce "Você" é secundário e discreto.

use egui::{Color32, Frame, Grid, RichText, ScrollArea, Stroke, Ui};

use crate::build_order::PlayerBuildOrder;
use crate::colors::{player_slot_color, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::replay_state::{fmt_time, LoadedReplay};

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
    let Some(bo) = loaded.build_order.as_ref() else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Build Order não disponível para este replay.").italics());
        });
        return;
    };

    let lps = bo.loops_per_second;
    let players = &bo.players;

    if players.is_empty() {
        ui.label("Nenhum jogador encontrado.");
        return;
    }

    let n = players.len().min(2).max(1);
    ui.columns(n, |cols| {
        for (i, player) in players.iter().take(n).enumerate() {
            let ui = &mut cols[i];
            let is_user = config.is_user(&player.name);
            player_column(ui, player, i, lps, is_user);
        }
    });
}

fn player_column(
    ui: &mut Ui,
    player: &PlayerBuildOrder,
    index: usize,
    lps: f64,
    is_user: bool,
) {
    // Borda sempre na cor do slot (P1 vermelho, P2 azul); o fill
    // permanece neutro — o único realce "Você" nesta aba é uma cor
    // esverdeada discreta aplicada somente ao título (nome do jogador).
    let slot = player_slot_color(index);
    let frame = Frame::group(ui.style())
        .fill(Color32::from_gray(28))
        .stroke(Stroke::new(1.8, slot));

    frame.show(ui, |ui| {
        // Cabeçalho
        ui.horizontal_wrapped(|ui| {
            // Título do jogador: branco no caso normal, tom esverdeado
            // discreto (USER_CHIP_FG) quando é o usuário. Sem chip
            // adicional nem background — o realce é só na cor do texto.
            let name_color = if is_user {
                USER_CHIP_FG
            } else {
                Color32::WHITE
            };
            ui.label(RichText::new(&player.name).strong().color(name_color));
            ui.label(
                RichText::new(format!("({})", race_short(&player.race)))
                    .color(Color32::from_gray(200)),
            );
            if let Some(mmr) = player.mmr {
                ui.small(format!("MMR {mmr}"));
            }
        });
        ui.separator();

        if player.entries.is_empty() {
            ui.label(RichText::new("(nenhuma entrada)").italics());
            return;
        }

        ScrollArea::vertical()
            .id_salt(format!("bo_{}", player.name))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Grid::new(format!("bo_grid_{}", player.name))
                    .num_columns(4)
                    .spacing([12.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("tempo").small().strong());
                        ui.label(RichText::new("supply").small().strong());
                        ui.label(RichText::new("").small());
                        ui.label(RichText::new("ação").small().strong());
                        ui.end_row();

                        for entry in &player.entries {
                            ui.monospace(fmt_time(entry.game_loop, lps));
                            ui.monospace(format!("{:>3}", entry.supply));
                            let icon = if entry.is_upgrade {
                                "▲"
                            } else if entry.is_structure {
                                "■"
                            } else {
                                "●"
                            };
                            ui.label(icon);
                            let action = if entry.count > 1 {
                                format!("{} x{}", entry.action, entry.count)
                            } else {
                                entry.action.clone()
                            };
                            ui.label(action);
                            ui.end_row();
                        }
                    });
            });
    });
}

fn race_short(race: &str) -> &str {
    match race.to_ascii_lowercase().as_str() {
        "terr" | "terran" => "Terran",
        "prot" | "protoss" => "Protoss",
        "zerg" => "Zerg",
        other => {
            let _ = other;
            race
        }
    }
}
