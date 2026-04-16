// Plot principal de army (valor/quantidade, agregado/por tipo).

use egui::{Color32, RichText, Ui};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints, Polygon};

use crate::colors::player_slot_color_bright;
use crate::config::AppConfig;
use crate::locale::{localize, t, tf, Language};
use crate::replay::is_worker_name;
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::SPACE_XL;
use crate::widgets::toggle_chip_bool;
use crate::{army_value, balance_data};

use super::classify::*;
use super::{ArmyChartOptions, ChartMetric};

pub(super) struct Series {
    pub name: String,
    pub color: Color32,
    pub width: f32,
    pub points: Vec<[f64; 2]>,
}

pub(super) fn army_value_plot(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    opts: &mut ArmyChartOptions,
) {
    let lang = config.language;

    // Header + controles.
    ui.horizontal(|ui| {
        ui.heading(t("charts.army.title", lang));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(t("charts.army.metric", lang));
        ui.radio_value(&mut opts.metric, ChartMetric::Value, t("charts.army.metric.value", lang));
        ui.radio_value(&mut opts.metric, ChartMetric::Count, t("charts.army.metric.count", lang));
        ui.add_space(SPACE_XL);
        toggle_chip_bool(ui, t("charts.army.group_by_type", lang), &mut opts.group_by_type, None);

        if opts.group_by_type {
            ui.add_space(SPACE_XL);
            ui.label(t("charts.army.player_label", lang));
            let player_count = loaded.timeline.players.len();
            if opts.grouped_player >= player_count && player_count > 0 {
                opts.grouped_player = 0;
            }
            let selected_name = loaded
                .timeline
                .players
                .get(opts.grouped_player)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("charts_grouped_player")
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    for (idx, p) in loaded.timeline.players.iter().enumerate() {
                        ui.selectable_value(&mut opts.grouped_player, idx, &p.name);
                    }
                });
        } else {
            ui.add_space(SPACE_XL);
            // No modo agregado, os chips Army/Workers controlam o que
            // entra na soma. Impede desmarcar ambos simultaneamente.
            let only_army = opts.show_army && !opts.show_workers;
            let only_workers = !opts.show_army && opts.show_workers;
            let army_label = t("charts.army.show", lang);
            let workers_label = t("charts.workers.show", lang);
            let army_resp = toggle_chip_bool(ui, army_label, &mut opts.show_army, None);
            if only_army && !opts.show_army {
                opts.show_army = true; // não permite desmarcar o último ativo
            }
            let _ = army_resp;
            let workers_resp = toggle_chip_bool(ui, workers_label, &mut opts.show_workers, None);
            if only_workers && !opts.show_workers {
                opts.show_workers = true;
            }
            let _ = workers_resp;
        }
    });

    let Some(army) = loaded.army.as_ref() else {
        ui.label(RichText::new(t("charts.army.no_data", lang)).italics());
        return;
    };
    if army.players.is_empty() || loaded.timeline.players.is_empty() {
        ui.label(RichText::new(t("charts.no_players", lang)).italics());
        return;
    }

    let lps = army.loops_per_second;
    let duration_secs = loaded.timeline.duration_seconds.max(1);

    // Grade de amostragem de 5s. Inclui t=0 e o fim da partida.
    let mut sample_secs: Vec<u32> = (0..=duration_secs).step_by(SAMPLE_STEP_SECS as usize).collect();
    if *sample_secs.last().unwrap_or(&0) != duration_secs {
        sample_secs.push(duration_secs);
    }

    // ── Constrói as séries (nome, cor, pontos) a serem plotadas.
    let metric = opts.metric;
    let base_build = loaded.timeline.base_build;

    let series_list: Vec<Series> = if opts.group_by_type {
        grouped_series(
            &sample_secs,
            lps,
            loaded,
            opts.grouped_player,
            metric,
            base_build,
            lang,
        )
    } else {
        aggregate_series(&sample_secs, lps, army, loaded, config, opts, metric)
    };

    // Pré-computa supply snapshots e supply blocks — como no original —
    // mas só populamos quando em modo agregado: no modo por-tipo o
    // conceito de "supply block do jogador X" não mapeia 1:1 pra cada
    // linha, então evitamos o ruído visual (ver comentário na seção de
    // desenho abaixo).
    let supply_snapshots: Vec<(String, Vec<(f64, i32, i32)>)> = if opts.group_by_type {
        Vec::new()
    } else {
        army.players
            .iter()
            .map(|p| {
                let snaps: Vec<(f64, i32, i32)> = p
                    .snapshots
                    .iter()
                    .map(|s| (loop_to_secs(s.game_loop, lps), s.supply_used, s.supply_made))
                    .collect();
                (p.name.clone(), snaps)
            })
            .collect()
    };

    let block_intervals: Vec<(String, Vec<(f64, f64)>)> = if opts.group_by_type {
        Vec::new()
    } else {
        loaded
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
            .collect()
    };

    let y_label = if metric == ChartMetric::Count {
        t("charts.axis.count", lang)
    } else {
        t("charts.axis.army", lang)
    };

    Plot::new("army_value_plot")
        .legend(Legend::default())
        .height(360.0)
        .allow_boxed_zoom(true)
        .x_axis_label(t("charts.axis.time", lang))
        .y_axis_label(y_label)
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
            let t_sec = point.x;

            let (named_key, anon_key) = match metric {
                ChartMetric::Count => ("charts.tooltip.count_named", "charts.tooltip.count_anon"),
                ChartMetric::Value => ("charts.tooltip.army_named", "charts.tooltip.army_anon"),
            };

            let ss_str = format!("{ss:02}");
            let mut text = if !name.is_empty() {
                tf(
                    named_key,
                    lang,
                    &[
                        ("name", name),
                        ("mm", &mm.to_string()),
                        ("ss", &ss_str),
                        ("value", &val_fmt),
                    ],
                )
            } else {
                tf(
                    anon_key,
                    lang,
                    &[
                        ("mm", &mm.to_string()),
                        ("ss", &ss_str),
                        ("value", &val_fmt),
                    ],
                )
            };

            // Supply do jogador hovered — só faz sentido no modo agregado.
            if !supply_snapshots.is_empty() && !name.is_empty() {
                if let Some((_, snaps)) =
                    supply_snapshots.iter().find(|(n, _)| n == name)
                {
                    let idx = snaps.partition_point(|(s, _, _)| *s <= t_sec);
                    let pick = if idx > 0 { Some(&snaps[idx - 1]) } else { snaps.first() };
                    if let Some((_, used, made)) = pick {
                        text.push('\n');
                        text.push_str(&tf(
                            "charts.tooltip.supply_line",
                            lang,
                            &[("used", &used.to_string()), ("made", &made.to_string())],
                        ));
                    }
                }
            }

            let blocked: Vec<&str> = block_intervals
                .iter()
                .filter(|(_, ivs)| ivs.iter().any(|&(s, e)| t_sec >= s && t_sec <= e))
                .map(|(n, _)| n.as_str())
                .collect();
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
            // Pico global de Y, para dimensionar os retângulos de supply block.
            let y_max = series_list
                .iter()
                .flat_map(|s| s.points.iter().map(|p| p[1]))
                .fold(0.0_f64, f64::max)
                .max(1.0)
                * 1.05;

            // Supply blocks — apenas no modo agregado (ver nota acima).
            if !opts.group_by_type {
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
            }

            for s in series_list {
                let pts: PlotPoints = s.points.into_iter().collect();
                let line = Line::new(s.name, pts).color(s.color).width(s.width);
                plot_ui.line(line);
            }
        });
}

/// Séries agregadas (uma por jogador). Usa `army.players[*].snapshots`
/// para `Value` e `timeline.players[*].alive_count` para `Count`.
fn aggregate_series(
    sample_secs: &[u32],
    lps: f64,
    army: &army_value::ArmyValueResult,
    loaded: &LoadedReplay,
    config: &AppConfig,
    opts: &ArmyChartOptions,
    metric: ChartMetric,
) -> Vec<Series> {
    army.players
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let is_user = config.is_user(&p.name);
            let timeline_player = loaded.timeline.players.get(idx);

            let points: Vec<[f64; 2]> = sample_secs
                .iter()
                .map(|&secs| {
                    let game_loop = (secs as f64 * lps).round() as u32;
                    let y = match metric {
                        ChartMetric::Value => {
                            let s = snapshot_at(&p.snapshots, game_loop);
                            let army_val =
                                if opts.show_army { s.map(|s| s.army_total).unwrap_or(0) } else { 0 };
                            let workers_val = if opts.show_workers {
                                s.map(|s| s.workers * WORKER_MINERAL_COST).unwrap_or(0)
                            } else {
                                0
                            };
                            (army_val + workers_val) as f64
                        }
                        ChartMetric::Count => {
                            let Some(tp) = timeline_player else { return [secs as f64, 0.0] };
                            let mut total = 0i32;
                            for (name, series) in &tp.alive_count {
                                match type_kind(name) {
                                    TypeKind::Worker if opts.show_workers => {
                                        total += alive_at(series, game_loop);
                                    }
                                    TypeKind::Army if opts.show_army => {
                                        total += alive_at(series, game_loop);
                                    }
                                    _ => {}
                                }
                            }
                            total as f64
                        }
                    };
                    [secs as f64, y]
                })
                .collect();

            Series {
                name: p.name.clone(),
                color: player_slot_color_bright(idx),
                width: if is_user { 2.5 } else { 1.8 },
                points,
            }
        })
        .collect()
}

/// Séries por tipo de unidade para o jogador selecionado. Filtra:
/// - estruturas e tumors (via `type_kind == Skip`);
/// - workers (SCV/Probe/Drone/MULE) — eles dominam visualmente o
///   gráfico e já têm visibilidade no plot de Production Efficiency;
/// - qualquer tipo sem `supply_cost_x10` na balance data (Beacons,
///   Larva, Changelings, AutoTurret, Broodling, LocustMP, etc.).
///
/// Unifica morphs-irmãos via [`canonical_unit_name`] — Observer e
/// ObserverSiegeMode viram uma única linha "Observer", etc.
fn grouped_series(
    sample_secs: &[u32],
    lps: f64,
    loaded: &LoadedReplay,
    player_idx: usize,
    metric: ChartMetric,
    base_build: u32,
    lang: Language,
) -> Vec<Series> {
    use std::collections::HashMap;

    let Some(tp) = loaded.timeline.players.get(player_idx) else {
        return Vec::new();
    };

    // Agrupa as séries por forma canônica (Observer + ObserverSiegeMode
    // → "Observer") aplicando os filtros de whitelist. Usa `String` como
    // chave pra desacoplar do lifetime de `tp.alive_count`.
    let mut groups: HashMap<String, Vec<&Vec<(u32, i32)>>> = HashMap::new();
    for (name, series) in &tp.alive_count {
        // Filtro 1: descarta estruturas e tumors.
        if matches!(type_kind(name), TypeKind::Skip) {
            continue;
        }
        // Filtro 2: descarta workers — linha que destoa do restante e
        // já tem seu próprio gráfico de eficiência.
        if is_worker_name(name) {
            continue;
        }
        // Filtro 3: descarta tipos ausentes da balance data (sem supply
        // cost) — essa é a heurística canônica de "unidade real de
        // jogo" no codebase. Pega Beacons, Larva, Changelings,
        // AutoTurret, Broodling, LocustMP, MULE, etc.
        if balance_data::supply_cost_x10(name, base_build) == 0 {
            continue;
        }
        // Filtro 4: descarta séries que nunca ficaram positivas (parser
        // registrou só eventos cancelados cedo).
        if series.iter().all(|(_, c)| *c <= 0) {
            continue;
        }
        let canonical = canonical_unit_name(name).to_string();
        groups.entry(canonical).or_default().push(series);
    }

    // Ordena por nome canônico para estabilidade visual entre renders.
    let mut entries: Vec<(String, Vec<&Vec<(u32, i32)>>)> = groups.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    entries
        .into_iter()
        .map(|(canonical, series_refs)| {
            let value_weight = match metric {
                ChartMetric::Value => {
                    balance_data::supply_cost_x10(&canonical, base_build) as f64 / 10.0
                }
                ChartMetric::Count => 1.0,
            };
            let points: Vec<[f64; 2]> = sample_secs
                .iter()
                .map(|&secs| {
                    let game_loop = (secs as f64 * lps).round() as u32;
                    // Soma os alive_at de todas as formas do grupo
                    // (canônica + alternativas) no mesmo instante.
                    let count: i32 =
                        series_refs.iter().map(|s| alive_at(s, game_loop)).sum();
                    [secs as f64, count as f64 * value_weight]
                })
                .collect();
            let display_name = localize(&canonical, lang).to_string();
            // Cor estável por tipo (mesmo tipo → mesma cor entre jogadores).
            let color = type_palette_color(&canonical);
            Series {
                name: display_name,
                color,
                width: 1.6,
                points,
            }
        })
        .collect()
}
