// Aba Gráficos — plot genérico de army (valor/quantidade, por jogador
// ou agrupado por tipo de unidade) + gráfico de eficiência de produção
// + cards de resumo numérico.
//
// O plot principal tem três eixos de configuração:
// - Métrica: Valor (minerals+gas, ou supply contribution por tipo) ou
//   Quantidade (contagem de entidades vivas).
// - Grupo: agregado por jogador (uma linha por jogador) ou agrupado por
//   tipo de unidade (uma linha por tipo; requer selecionar um jogador).
// - Amostragem: grade fixa de 5s, independente da resolução dos
//   eventos — evita serrilhado nas linhas ao usar dados do tracker.
//
// A identidade visual dos jogadores (P1 vermelho, P2 azul) permanece
// no modo agregado. No modo agrupado-por-tipo, cada linha ganha uma cor
// derivada do hash do tipo (estável entre renders).

use egui::{Color32, RichText, Ui};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints, Polygon};

use crate::balance_data;
use crate::colors::{player_slot_color_bright, USER_CHIP_BG, USER_CHIP_FG};
use crate::config::AppConfig;
use crate::locale::{localize, t, tf, Language};
use crate::production_efficiency::{EfficiencyTarget, ProductionEfficiencySeries};
use crate::replay::{is_structure_name, is_worker_name};
use crate::replay_state::{loop_to_secs, LoadedReplay};

/// Métrica mostrada no eixo Y do plot principal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChartMetric {
    /// Valor em minerals+gas (agregado) ou supply contribution (por tipo).
    Value,
    /// Número de entidades vivas.
    Count,
}

/// Opções de exibição do plot principal. Mantidas em `AppState` para
/// persistir entre trocas de aba.
pub struct ArmyChartOptions {
    pub metric: ChartMetric,
    /// Incluir unidades de army no agregado (sem efeito no modo por tipo).
    pub show_army: bool,
    /// Incluir workers no agregado (sem efeito no modo por tipo —
    /// workers aparecem como seu próprio tipo).
    pub show_workers: bool,
    /// Uma linha por tipo de unidade (vs. uma linha por jogador).
    pub group_by_type: bool,
    /// Jogador selecionado para o modo por-tipo. Ignorado em agregado.
    pub grouped_player: usize,
}

impl Default for ArmyChartOptions {
    fn default() -> Self {
        Self {
            metric: ChartMetric::Value,
            show_army: true,
            show_workers: false,
            group_by_type: false,
            grouped_player: 0,
        }
    }
}

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    army_opts: &mut ArmyChartOptions,
    efficiency_target: &mut EfficiencyTarget,
) {
    army_value_plot(ui, loaded, config, army_opts);
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

/// Passo da grade de amostragem do plot, em segundos. Suaviza a
/// visualização quando há muitos eventos em sucessão rápida.
const SAMPLE_STEP_SECS: u32 = 5;

// ── Classificação de tipos ──────────────────────────────────────────

/// Categoria de um `entity_type` para fins do plot. Não depende do
/// parser — usa as mesmas heurísticas de nome que `replay::classify`
/// expõe, mais um check extra para tumors (que o parser trata como
/// estrutura, mas `is_structure_name` não reconhece pelo nome).
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeKind {
    Worker,
    Army,
    /// Estrutura ou tumor — excluído do plot.
    Skip,
}

fn type_kind(name: &str) -> TypeKind {
    if is_worker_name(name) {
        TypeKind::Worker
    } else if is_structure_name(name) || name.starts_with("CreepTumor") {
        TypeKind::Skip
    } else {
        TypeKind::Army
    }
}

/// Nome canônico de uma unidade quando ela tem forma alternativa
/// (siege mode, burrow, transformação Hellion↔Hellbat, etc.). No
/// `alive_count` essas formas aparecem como chaves separadas — por
/// exemplo um Observer que entra em Surveillance Mode some como
/// `Observer` e ressurge como `ObserverSiegeMode`. Para o gráfico
/// "por tipo", tratamos os dois como a mesma unidade.
///
/// Retorna o próprio nome quando não há alias conhecido. A canonical
/// é deliberadamente o nome **ativo** (Observer, não
/// ObserverSiegeMode) pra bater com o `units.txt` do locale.
fn canonical_unit_name(name: &str) -> &str {
    match name {
        "ObserverSiegeMode" => "Observer",
        "OverseerSiegeMode" => "Overseer",
        "SiegeTankSieged" => "SiegeTank",
        "VikingAssault" => "VikingFighter",
        "WidowMineBurrowed" => "WidowMine",
        "Hellbat" => "Hellion",
        "WarpPrismPhasing" => "WarpPrism",
        "RoachBurrowed" => "Roach",
        "ZerglingBurrowed" => "Zergling",
        "BanelingBurrowed" => "Baneling",
        "InfestorBurrowed" => "Infestor",
        "LurkerMPBurrowed" => "LurkerMP",
        "SwarmHostBurrowedMP" => "SwarmHostMP",
        "QueenBurrowed" => "Queen",
        "UltraliskBurrowed" => "Ultralisk",
        "HydraliskBurrowed" => "Hydralisk",
        "DroneBurrowed" => "Drone",
        other => other,
    }
}

// ── Queries sobre `alive_count` ─────────────────────────────────────

/// Contagem de entidades vivas de `entity_type` no instante
/// `game_loop`, via binary search em `series` (ordenado por loop).
fn alive_at(series: &[(u32, i32)], game_loop: u32) -> i32 {
    let i = series.partition_point(|(l, _)| *l <= game_loop);
    if i == 0 { 0 } else { series[i - 1].1.max(0) }
}

/// Valor em minerals+gas do `ArmySnapshot` mais recente em ou antes de
/// `game_loop`. Usa busca linear porque `snapshots` é curto e a função
/// é chamada uma vez por ponto amostrado — binary search seria overkill.
fn snapshot_at<'a>(
    snaps: &'a [crate::army_value::ArmySnapshot],
    game_loop: u32,
) -> Option<&'a crate::army_value::ArmySnapshot> {
    let i = snaps.partition_point(|s| s.game_loop <= game_loop);
    if i == 0 { None } else { snaps.get(i - 1) }
}

// ── UI principal ────────────────────────────────────────────────────

fn army_value_plot(
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
        ui.add_space(16.0);
        ui.checkbox(&mut opts.group_by_type, t("charts.army.group_by_type", lang));

        if opts.group_by_type {
            ui.add_space(16.0);
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
            ui.add_space(16.0);
            // No modo agregado, as checkboxes Army/Workers controlam o
            // que entra na soma. Impede desmarcar ambos simultaneamente.
            let only_army = opts.show_army && !opts.show_workers;
            let only_workers = !opts.show_army && opts.show_workers;
            let army_label = t("charts.army.show", lang);
            let workers_label = t("charts.workers.show", lang);
            if only_army {
                ui.add_enabled(false, egui::Checkbox::new(&mut opts.show_army, army_label));
            } else {
                ui.checkbox(&mut opts.show_army, army_label);
            }
            if only_workers {
                ui.add_enabled(false, egui::Checkbox::new(&mut opts.show_workers, workers_label));
            } else {
                ui.checkbox(&mut opts.show_workers, workers_label);
            }
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
    army: &crate::army_value::ArmyValueResult,
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

struct Series {
    name: String,
    color: Color32,
    width: f32,
    points: Vec<[f64; 2]>,
}

/// Paleta de cores estável por nome de tipo. Hash simples (FNV-1a) sobre
/// o nome bruto para escolher um índice na paleta — mesmo tipo sempre
/// recebe a mesma cor, independente da ordem no HashMap ou do idioma.
fn type_palette_color(name: &str) -> Color32 {
    // Paleta com cores vibrantes e diferenciáveis em fundo escuro.
    // Evita vermelho/azul puros (reservados para P1/P2 no modo agregado).
    const PALETTE: &[Color32] = &[
        Color32::from_rgb(0xFF, 0xB3, 0x00), // âmbar
        Color32::from_rgb(0x00, 0xC8, 0x96), // turquesa
        Color32::from_rgb(0xC4, 0x7B, 0xFF), // lilás
        Color32::from_rgb(0xFF, 0x8E, 0x8E), // coral
        Color32::from_rgb(0x66, 0xD9, 0xEF), // ciano claro
        Color32::from_rgb(0xA6, 0xE2, 0x2E), // lima
        Color32::from_rgb(0xFF, 0x6B, 0xB5), // rosa quente
        Color32::from_rgb(0xFD, 0x97, 0x1F), // laranja
        Color32::from_rgb(0x81, 0xB0, 0xFF), // azul claro
        Color32::from_rgb(0xE8, 0xE0, 0x6F), // amarelo pálido
        Color32::from_rgb(0xB2, 0xD0, 0x7F), // verde oliva
        Color32::from_rgb(0xD7, 0x99, 0x70), // bronze
    ];
    let mut h: u64 = 0xcbf29ce484222325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    PALETTE[(h as usize) % PALETTE.len()]
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
