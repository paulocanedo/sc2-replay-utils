// Aba Chat — mensagens da partida em ordem cronológica.
//
// Renderiza o chat do replay como uma linha do tempo com timestamps em
// mm:ss. A cor do nome de cada jogador segue a convenção in-game do
// SC2: player1 = vermelho, player2 = azul. O lookup é feito a partir
// do índice do jogador em `loaded.timeline.players`.

use std::collections::HashMap;

use egui::{Color32, RichText, ScrollArea, Ui};

use crate::colors::{player_slot_color_bright, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::replay_state::{fmt_time, LoadedReplay};

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
    let Some(chat) = loaded.chat.as_ref() else {
        placeholder(ui, "Sem eventos de chat neste replay.");
        return;
    };

    if chat.entries.is_empty() {
        placeholder(ui, "Nenhuma mensagem de chat neste replay.");
        return;
    }

    // Lookup nome -> índice do jogador, para decidir a cor do slot
    // (P1 vermelho / P2 azul) de cada mensagem. Comparação
    // case-insensitive já que replays expõem o nome como string bruta.
    let name_to_idx: HashMap<String, usize> = loaded
        .timeline
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.to_ascii_lowercase(), i))
        .collect();

    ui.heading(format!("Chat ({} mensagens)", chat.entries.len()));
    ui.small("Clique em uma linha para destacar — funcionalidade virá em iterações futuras.");
    ui.add_space(4.0);

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &chat.entries {
                let time = fmt_time(entry.game_loop, chat.loops_per_second);
                let is_user = config.is_user(&entry.player_name);
                let slot_color = name_to_idx
                    .get(&entry.player_name.to_ascii_lowercase())
                    .map(|i| player_slot_color_bright(*i))
                    .unwrap_or(Color32::from_gray(180));

                ui.horizontal(|ui| {
                    ui.monospace(format!("[{time}]"));
                    // Nome do jogador sempre na cor do slot (P1/P2).
                    ui.label(
                        RichText::new(format!("{}:", entry.player_name))
                            .strong()
                            .color(slot_color),
                    );

                    // Mensagem: texto padrão. Não repetimos a cor do
                    // slot para não poluir o chat — a cor fica reservada
                    // ao identificador (nome).
                    ui.label(RichText::new(&entry.message));

                    if entry.recipient != "All" && !entry.recipient.is_empty() {
                        ui.small(format!("(→ {})", entry.recipient));
                    }

                    // Chip "Você" discreto só na linha do usuário.
                    if is_user {
                        ui.label(
                            RichText::new(" Você ")
                                .small()
                                .color(USER_CHIP_FG)
                                .background_color(USER_CHIP_BG),
                        );
                    }
                });
            }
        });
}

fn placeholder(ui: &mut Ui, msg: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("📜").size(48.0));
        ui.add_space(8.0);
        ui.label(RichText::new(msg).italics());
    });
}
