// Aba Gráficos — plot de army value ao longo do tempo (egui_plot) +
// cards de resumo numérico (pico de army value, supply blocks,
// production gap, upgrades).
//
// A identidade visual de cada jogador segue a convenção in-game do
// SC2: player1 = vermelho, player2 = azul. Isso se aplica às linhas
// do plot e aos nomes dos jogadores nos cards de resumo, mantendo a
// correspondência visual com a sidebar.

use egui::{RichText, Ui};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::colors::{player_slot_color_bright, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::replay_state::{loop_to_secs, LoadedReplay};

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
    army_value_plot(ui, loaded, config);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);
    summary_cards(ui, loaded, config);
}

fn army_value_plot(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
    ui.heading("Army Value ao longo do tempo");

    let Some(army) = loaded.army.as_ref() else {
        ui.label(RichText::new("Dados de army value indisponíveis.").italics());
        return;
    };
    if army.players.is_empty() {
        ui.label(RichText::new("Sem jogadores.").italics());
        return;
    }

    let lps = army.loops_per_second;

    Plot::new("army_value_plot")
        .legend(Legend::default())
        .height(280.0)
        .x_axis_label("tempo (s)")
        .y_axis_label("army value")
        .show(ui, |plot_ui| {
            for (idx, player) in army.players.iter().enumerate() {
                let is_user = config.is_user(&player.name);
                let points: PlotPoints = player
                    .snapshots
                    .iter()
                    .map(|s| [loop_to_secs(s.game_loop, lps), s.army_total as f64])
                    .collect();
                let name = if is_user {
                    format!("{} (Você)", player.name)
                } else {
                    player.name.clone()
                };
                // Cor do slot (versão brighter para fundo escuro do plot).
                // P1 vermelho, P2 azul — igual à sidebar.
                let line = Line::new(points)
                    .name(name)
                    .color(player_slot_color_bright(idx))
                    .width(if is_user { 2.5 } else { 1.8 });
                plot_ui.line(line);
            }
        });
}

fn summary_cards(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
    ui.columns(4, |cols| {
        // Card 1: pico de army value
        card(&mut cols[0], "Pico de Army Value", |ui| {
            if let Some(army) = loaded.army.as_ref() {
                for (idx, p) in army.players.iter().enumerate() {
                    let peak = p.snapshots.iter().map(|s| s.army_total).max().unwrap_or(0);
                    player_line(ui, &p.name, idx, &format!("{peak}"), config.is_user(&p.name));
                }
            } else {
                ui.small("—");
            }
        });

        // Card 2: supply blocks
        card(&mut cols[1], "Supply Blocks", |ui| {
            let lps = loaded.raw.loops_per_second.max(0.0001);
            for (idx, p) in loaded.raw.players.iter().enumerate() {
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
                    &format!("{count} ({}s)", total_secs),
                    config.is_user(&p.name),
                );
            }
        });

        // Card 3: production gap / efficiency
        card(&mut cols[2], "Eficiência Produção", |ui| {
            if let Some(pg) = loaded.production.as_ref() {
                for (idx, p) in pg.players.iter().enumerate() {
                    player_line(
                        ui,
                        &p.name,
                        idx,
                        &format!("{:.1}%", p.efficiency_pct),
                        config.is_user(&p.name),
                    );
                }
            } else {
                ui.small("—");
            }
        });

        // Card 4: upgrades
        card(&mut cols[3], "Upgrades", |ui| {
            for (idx, p) in loaded.raw.players.iter().enumerate() {
                player_line(
                    ui,
                    &p.name,
                    idx,
                    &format!("{}", p.upgrades.len()),
                    config.is_user(&p.name),
                );
            }
        });
    });
}

fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.group(|ui| {
        ui.set_min_height(100.0);
        ui.label(RichText::new(title).strong());
        ui.separator();
        body(ui);
    });
}

fn player_line(ui: &mut Ui, name: &str, index: usize, value: &str, is_user: bool) {
    ui.horizontal(|ui| {
        // Nome colorido com a cor do slot (P1 vermelho, P2 azul). Se é
        // o usuário, adiciona um chip "Você" discreto logo depois —
        // sem sequestrar a cor do nome, que pertence ao slot.
        let name_text = RichText::new(name)
            .small()
            .strong()
            .color(player_slot_color_bright(index));
        ui.label(name_text);
        if is_user {
            ui.label(
                RichText::new(" Você ")
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
