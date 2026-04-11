// Aba Build Order — legenda + busca no topo, depois duas colunas
// lado a lado, uma por jogador. Cada coluna mostra: cabeçalho com
// nome/race/MMR + toggles de filtro (workers, units) + tabela
// scrollable com mm:ss, supply, tipo, ação. A identidade visual de
// cada jogador segue a convenção in-game do SC2: P1 = vermelho,
// P2 = azul (borda do frame). O realce "Você" é secundário: tom
// esverdeado discreto apenas no título.
//
// O `entry.game_loop` que recebemos do extrator já é o instante de
// **início** da ação (start time). A aba só formata e renderiza.

use egui::{Color32, Frame, Grid, Id, RichText, ScrollArea, Stroke, TextEdit, Ui};

use crate::build_order::{classify_entry, EntryKind, PlayerBuildOrder};
use crate::colors::{player_slot_color, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::replay_state::{fmt_time, LoadedReplay};

/// Todas as categorias, na ordem de exibição da legenda / filtros.
const ALL_KINDS: [EntryKind; 5] = [
    EntryKind::Worker,
    EntryKind::Unit,
    EntryKind::Structure,
    EntryKind::Research,
    EntryKind::Upgrade,
];

/// Estado persistente dos filtros da coluna de um jogador. Guardado
/// na memória do egui via `ui.data_mut` com um id estável por slot,
/// então sobrevive entre frames sem precisar de campo no `AppState`.
/// Ambos os toggles começam ligados (tudo visível).
#[derive(Clone, Copy, Debug)]
struct BuildOrderFilter {
    show_workers: bool,
    show_units: bool,
}

impl Default for BuildOrderFilter {
    fn default() -> Self {
        Self {
            show_workers: true,
            show_units: true,
        }
    }
}

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

    // ── Busca global (ação) ──────────────────────────────────────
    // Uma query só, aplicada a todos os jogadores. Persistida na
    // memória do egui para sobreviver a recargas de replay.
    let search_id = Id::new("bo_search_query");
    let mut search: String = ui
        .ctx()
        .data(|d| d.get_temp::<String>(search_id))
        .unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label("🔎");
        let resp = ui.add(
            TextEdit::singleline(&mut search)
                .hint_text("buscar ação... (ex: Marine, Stimpack)")
                .desired_width(260.0),
        );
        if !search.is_empty() && ui.small_button("×").on_hover_text("limpar busca").clicked() {
            search.clear();
        }
        if resp.changed() || search.is_empty() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(search_id, search.clone()));
        }
    });

    // ── Legenda dos tipos ────────────────────────────────────────
    // Chips coloridos mostrando cada categoria com sua letra e nome.
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("legenda:").small().italics());
        for kind in ALL_KINDS {
            legend_chip(ui, kind);
        }
    });
    ui.separator();

    let query_lower = search.to_ascii_lowercase();

    let n = players.len().min(2).max(1);
    ui.columns(n, |cols| {
        for (i, player) in players.iter().take(n).enumerate() {
            let ui = &mut cols[i];
            let is_user = config.is_user(&player.name);
            player_column(ui, player, i, lps, is_user, &query_lower);
        }
    });
}

/// Pequeno chip colorido exibindo uma categoria na legenda: fundo
/// com a cor da categoria, letra em negrito e, ao lado, o nome
/// completo em texto neutro pra leitura.
fn legend_chip(ui: &mut Ui, kind: EntryKind) {
    let color = kind_color(kind);
    ui.label(
        RichText::new(format!(" {} ", kind.short_letter()))
            .monospace()
            .strong()
            .color(Color32::BLACK)
            .background_color(color),
    )
    .on_hover_text(kind.full_name());
    ui.small(kind.full_name());
    ui.add_space(4.0);
}

fn player_column(
    ui: &mut Ui,
    player: &PlayerBuildOrder,
    index: usize,
    lps: f64,
    is_user: bool,
    query_lower: &str,
) {
    let slot = player_slot_color(index);
    let frame = Frame::group(ui.style())
        .fill(Color32::from_gray(28))
        .stroke(Stroke::new(1.8, slot));

    // Id estável por slot (e não por nome) — evita recriar o estado
    // de filtro quando um replay diferente é carregado com o mesmo
    // jogador em outra posição.
    let filter_id = Id::new(("bo_filter", index));
    let mut filter: BuildOrderFilter = ui
        .ctx()
        .data(|d| d.get_temp::<BuildOrderFilter>(filter_id))
        .unwrap_or_default();

    frame.show(ui, |ui| {
        // Cabeçalho: nome, raça, MMR e toggles de filtro.
        ui.horizontal_wrapped(|ui| {
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

            // Toggles alinhados à direita — não competem com o nome.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let units_resp = ui
                    .toggle_value(
                        &mut filter.show_units,
                        RichText::new("U").monospace().strong(),
                    )
                    .on_hover_text("Mostrar unidades de combate (exclui workers)");
                let workers_resp = ui
                    .toggle_value(
                        &mut filter.show_workers,
                        RichText::new("W").monospace().strong(),
                    )
                    .on_hover_text("Mostrar workers (SCV / Probe / Drone / MULE)");
                if units_resp.changed() || workers_resp.changed() {
                    ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter));
                }
            });
        });
        ui.separator();

        if player.entries.is_empty() {
            ui.label(RichText::new("(nenhuma entrada)").italics());
            return;
        }

        ScrollArea::vertical()
            .id_salt(format!("bo_{}", index))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Grid::new(format!("bo_grid_{}", index))
                    .num_columns(5)
                    .spacing([12.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("início").small().strong());
                        ui.label(RichText::new("fim").small().strong());
                        ui.label(RichText::new("supply").small().strong());
                        ui.label(RichText::new("tipo").small().strong());
                        ui.label(RichText::new("ação").small().strong());
                        ui.end_row();

                        let mut rendered = 0usize;
                        for entry in &player.entries {
                            let kind = classify_entry(entry);

                            // Filtros: categoria (W/U) + busca textual.
                            match kind {
                                EntryKind::Worker if !filter.show_workers => continue,
                                EntryKind::Unit if !filter.show_units => continue,
                                _ => {}
                            }
                            if !query_lower.is_empty()
                                && !entry.action.to_ascii_lowercase().contains(query_lower)
                            {
                                continue;
                            }

                            ui.monospace(fmt_time(entry.game_loop, lps));
                            ui.monospace(fmt_time(entry.finish_loop, lps));
                            ui.monospace(format!("{:>3}/{:<3}", entry.supply, entry.supply_made));

                            let color = kind_color(kind);
                            ui.label(
                                RichText::new(kind.short_letter())
                                    .monospace()
                                    .strong()
                                    .color(color),
                            )
                            .on_hover_text(kind.full_name());

                            let action = if entry.count > 1 {
                                format!("{} x{}", entry.action, entry.count)
                            } else {
                                entry.action.clone()
                            };
                            ui.label(action);
                            ui.end_row();
                            rendered += 1;
                        }

                        if rendered == 0 {
                            ui.label(
                                RichText::new("(nada corresponde aos filtros)")
                                    .italics()
                                    .color(Color32::from_gray(140)),
                            );
                            ui.end_row();
                        }
                    });
            });
    });
}

/// Cor característica de cada categoria. Escolhidas pra serem
/// distinguíveis mesmo em scan rápido e não colidirem com as cores
/// de slot (P1 vermelho / P2 azul) usadas na borda do card.
fn kind_color(kind: EntryKind) -> Color32 {
    match kind {
        EntryKind::Worker => Color32::from_rgb(120, 200, 140),   // verde suave
        EntryKind::Unit => Color32::from_gray(200),              // cinza claro
        EntryKind::Structure => Color32::from_rgb(230, 170, 80), // laranja
        EntryKind::Research => Color32::from_rgb(180, 140, 230), // roxo
        EntryKind::Upgrade => Color32::from_rgb(240, 210, 120),  // amarelo/dourado
    }
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
