// Aba Gráficos — plot de army value + gráfico de eficiência de
// produção (workers ou army, seletor por radio) + cards de resumo
// numérico (supply blocks e eficiência média).
//
// A identidade visual de cada jogador segue a convenção in-game do
// SC2: player1 = vermelho, player2 = azul. Isso se aplica às linhas
// do plot e aos nomes dos jogadores nos cards de resumo, mantendo a
// correspondência visual com a sidebar.

use egui::{Color32, RichText, Ui};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints, Polygon};

use crate::colors::{player_slot_color_bright, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::locale::{t, tf, Language};
use crate::production_efficiency::{EfficiencyTarget, ProductionEfficiencySeries};
use crate::replay_state::{loop_to_secs, LoadedReplay};

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    show_army: &mut bool,
    show_workers: &mut bool,
    efficiency_target: &mut EfficiencyTarget,
) {
    army_value_plot(ui, loaded, config, show_army, show_workers);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);
    efficiency_plot(ui, loaded, config, efficiency_target);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);
    summary_cards(ui, loaded, config);
}

/// Custo em minerals de um worker (SCV / Probe / Drone).
const WORKER_MINERAL_COST: i32 = 50;

fn army_value_for_snapshot(
    s: &crate::army_value::ArmySnapshot,
    show_army: bool,
    show_workers: bool,
) -> f64 {
    let army = if show_army { s.army_total } else { 0 };
    let workers = if show_workers { s.workers * WORKER_MINERAL_COST } else { 0 };
    (army + workers) as f64
}

fn army_value_plot(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    show_army: &mut bool,
    show_workers: &mut bool,
) {
    let lang = config.language;
    ui.horizontal(|ui| {
        ui.heading(t("charts.army.title", lang));
        ui.add_space(16.0);
        // Impede desmarcar ambos: se um está desmarcado, o outro fica desabilitado
        let only_army = *show_army && !*show_workers;
        let only_workers = !*show_army && *show_workers;
        let army_label = t("charts.army.show", lang);
        let workers_label = t("charts.workers.show", lang);
        if only_army {
            ui.add_enabled(false, egui::Checkbox::new(show_army, army_label));
        } else {
            ui.checkbox(show_army, army_label);
        }
        if only_workers {
            ui.add_enabled(false, egui::Checkbox::new(show_workers, workers_label));
        } else {
            ui.checkbox(show_workers, workers_label);
        }
    });

    let Some(army) = loaded.army.as_ref() else {
        ui.label(RichText::new(t("charts.army.no_data", lang)).italics());
        return;
    };
    if army.players.is_empty() {
        ui.label(RichText::new(t("charts.no_players", lang)).italics());
        return;
    }

    let lps = army.loops_per_second;

    // Pré-computa supply snapshots por jogador: Vec<(nome, Vec<(secs, used, made)>)>
    // Usado no tooltip para mostrar supply no instante do cursor.
    let supply_snapshots: Vec<(String, Vec<(f64, i32, i32)>)> = army
        .players
        .iter()
        .map(|p| {
            let snaps: Vec<(f64, i32, i32)> = p
                .snapshots
                .iter()
                .map(|s| (loop_to_secs(s.game_loop, lps), s.supply_used, s.supply_made))
                .collect();
            (p.name.clone(), snaps)
        })
        .collect();

    // Pré-computa intervalos de supply block em segundos: Vec<(nome, Vec<(start_s, end_s)>)>
    let block_intervals: Vec<(String, Vec<(f64, f64)>)> = loaded
        .supply_blocks_per_player
        .iter()
        .enumerate()
        .map(|(idx, blocks)| {
            let name = loaded
                .timeline
                .players
                .get(idx)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let intervals: Vec<(f64, f64)> = blocks
                .iter()
                .map(|b| (loop_to_secs(b.start_loop, lps), loop_to_secs(b.end_loop, lps)))
                .collect();
            (name, intervals)
        })
        .collect();

    Plot::new("army_value_plot")
        .legend(Legend::default())
        .height(360.0)
        .allow_boxed_zoom(true)
        .x_axis_label(t("charts.axis.time", lang))
        .y_axis_label(t("charts.axis.army", lang))
        .x_axis_formatter(|mark: GridMark, _range| {
            let total_secs = mark.value as u32;
            format!("{}:{:02}", total_secs / 60, total_secs % 60)
        })
        .y_axis_formatter(|mark: GridMark, _range| {
            let v = mark.value as i64;
            if v >= 1000 {
                format!("{}.{:03}", v / 1000, (v % 1000).abs())
            } else {
                format!("{v}")
            }
        })
        .label_formatter(move |name, point| {
            let secs = point.x as u32;
            let mm = secs / 60;
            let ss = secs % 60;
            let val = point.y as i64;
            let val_fmt = if val >= 1000 {
                format!("{}.{:03}", val / 1000, (val % 1000).abs())
            } else {
                format!("{val}")
            };
            let t = point.x;

            // Supply do jogador hovered (busca snapshot mais próximo do cursor)
            let supply_str = if !name.is_empty() {
                supply_snapshots
                    .iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, snaps)| {
                        // Busca o snapshot com tempo <= t mais próximo
                        let idx = snaps.partition_point(|(s, _, _)| *s <= t);
                        if idx > 0 { Some(&snaps[idx - 1]) } else { snaps.first() }
                    })
                    .map(|(_, used, made)| {
                        format!(
                            "\n{}",
                            tf(
                                "charts.tooltip.supply_line",
                                lang,
                                &[("used", &used.to_string()), ("made", &made.to_string())],
                            ),
                        )
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let blocked: Vec<&str> = block_intervals
                .iter()
                .filter(|(_, ivs)| ivs.iter().any(|&(s, e)| t >= s && t <= e))
                .map(|(n, _)| n.as_str())
                .collect();
            let ss_str = format!("{ss:02}");
            let mut text = if !name.is_empty() {
                let base = tf(
                    "charts.tooltip.army_named",
                    lang,
                    &[
                        ("name", name),
                        ("mm", &mm.to_string()),
                        ("ss", &ss_str),
                        ("value", &val_fmt),
                    ],
                );
                format!("{base}{supply_str}")
            } else {
                tf(
                    "charts.tooltip.army_anon",
                    lang,
                    &[
                        ("mm", &mm.to_string()),
                        ("ss", &ss_str),
                        ("value", &val_fmt),
                    ],
                )
            };
            for who in &blocked {
                text.push('\n');
                text.push_str(&tf(
                    "charts.tooltip.supply_blocked",
                    lang,
                    &[("who", who)],
                ));
            }
            text
        })
        .show(ui, |plot_ui| {
            let sa = *show_army;
            let sw = *show_workers;
            // Altura máxima dos retângulos de supply block = pico global de army
            let y_max = army
                .players
                .iter()
                .flat_map(|p| p.snapshots.iter().map(|s| army_value_for_snapshot(s, sa, sw)))
                .fold(0.0_f64, f64::max)
                .max(1000.0)
                * 1.05;

            // Desenha supply blocks como retângulos semi-transparentes (atrás das linhas)
            for (idx, blocks) in loaded.supply_blocks_per_player.iter().enumerate() {
                let base_color = player_slot_color_bright(idx);
                let fill = Color32::from_rgba_unmultiplied(
                    base_color.r(),
                    base_color.g(),
                    base_color.b(),
                    25,
                );
                let stroke_color = Color32::from_rgba_unmultiplied(
                    base_color.r(),
                    base_color.g(),
                    base_color.b(),
                    60,
                );
                for block in blocks {
                    let x0 = loop_to_secs(block.start_loop, lps);
                    let x1 = loop_to_secs(block.end_loop, lps);
                    let rect: PlotPoints = vec![
                        [x0, 0.0],
                        [x1, 0.0],
                        [x1, y_max],
                        [x0, y_max],
                    ]
                    .into();
                    let poly = Polygon::new("", rect)
                        .fill_color(fill)
                        .stroke(egui::Stroke::new(1.0, stroke_color))
                        .allow_hover(false);
                    plot_ui.polygon(poly);
                }
            }

            // Linhas de army value por jogador
            for (idx, player) in army.players.iter().enumerate() {
                let is_user = config.is_user(&player.name);
                let points: PlotPoints = player
                    .snapshots
                    .iter()
                    .map(|s| [loop_to_secs(s.game_loop, lps), army_value_for_snapshot(s, sa, sw)])
                    .collect();
                let name = player.name.clone();
                let line = Line::new(name, points)
                    .color(player_slot_color_bright(idx))
                    .width(if is_user { 2.5 } else { 1.8 });
                plot_ui.line(line);
            }
        });
}

fn efficiency_plot(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    target: &mut EfficiencyTarget,
) {
    let lang = config.language;
    ui.horizontal(|ui| {
        ui.heading(t("charts.efficiency.title", lang));
        ui.add_space(16.0);
        ui.radio_value(target, EfficiencyTarget::Workers, t("charts.workers.show", lang));
        ui.radio_value(target, EfficiencyTarget::Army, t("charts.army.show", lang));
    });

    let series_opt: Option<&ProductionEfficiencySeries> = match *target {
        EfficiencyTarget::Workers => loaded.efficiency_workers.as_ref(),
        EfficiencyTarget::Army => loaded.efficiency_army.as_ref(),
    };
    let Some(series) = series_opt else {
        ui.label(RichText::new(t("charts.efficiency.no_data", lang)).italics());
        return;
    };
    if series.players.is_empty() {
        ui.label(RichText::new(t("charts.no_players", lang)).italics());
        return;
    }

    let lps = series.loops_per_second;

    // Nota para jogadores Zerg — sem linha plotada (suporte em breve).
    for p in &series.players {
        if p.is_zerg {
            ui.label(
                RichText::new(tf(
                    "charts.efficiency.zerg_tbd",
                    lang,
                    &[("player", &p.name)],
                ))
                    .italics()
                    .small(),
            );
        }
    }

    Plot::new("efficiency_plot")
        .legend(Legend::default())
        .height(280.0)
        .allow_boxed_zoom(true)
        .include_y(0.0)
        .include_y(100.0)
        .x_axis_label(t("charts.axis.time", lang))
        .y_axis_label(t("charts.axis.efficiency", lang))
        .x_axis_formatter(|mark: GridMark, _range| {
            let total_secs = mark.value as u32;
            format!("{}:{:02}", total_secs / 60, total_secs % 60)
        })
        .y_axis_formatter(|mark: GridMark, _range| format!("{}%", mark.value as i32))
        .label_formatter(move |name, point| {
            let secs = point.x as u32;
            let mm = secs / 60;
            let ss = secs % 60;
            let ss_str = format!("{ss:02}");
            let pct = format!("{:.1}", point.y);
            if name.is_empty() {
                tf(
                    "charts.tooltip.efficiency_anon",
                    lang,
                    &[("mm", &mm.to_string()), ("ss", &ss_str), ("pct", &pct)],
                )
            } else {
                tf(
                    "charts.tooltip.efficiency_named",
                    lang,
                    &[
                        ("name", name),
                        ("mm", &mm.to_string()),
                        ("ss", &ss_str),
                        ("pct", &pct),
                    ],
                )
            }
        })
        .show(ui, |plot_ui| {
            for (idx, p) in series.players.iter().enumerate() {
                if p.is_zerg || p.samples.is_empty() {
                    continue;
                }
                let is_user = config.is_user(&p.name);
                let points: PlotPoints = p
                    .samples
                    .iter()
                    .map(|s| [loop_to_secs(s.game_loop, lps), s.efficiency_pct])
                    .collect();
                let line = Line::new(p.name.clone(), points)
                    .color(player_slot_color_bright(idx))
                    .width(if is_user { 2.5 } else { 1.8 });
                plot_ui.line(line);
            }
        });
}

fn summary_cards(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig) {
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
        // backoff próprios). Army é a média das amostras do novo
        // time-series `efficiency_army`. Zerg aparece com traço curto.
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
                        let value = if p.is_zerg || p.samples.is_empty() {
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
                RichText::new(format!("{} ", t("charts.you_chip", lang)))
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
