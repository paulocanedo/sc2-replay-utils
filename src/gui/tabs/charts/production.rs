// Gráfico de "lanes" de produção. Substitui o antigo gráfico exclusivo
// de workers — agora seleciona entre quatro views via pills no topo:
//
//   Workers | Army | Pesquisas | Upgrades
//
// `Workers` e `Army` compartilham o pipeline de extração e render
// (`production_lanes`), variando apenas o `LaneMode` consumido. As lanes
// se desenham igual: ícone da estrutura à esquerda + Gantt horizontal
// com baseline fina + blocos de produção/morph/impeded.
//
// `Army` adiciona o bloco `Impeded` (Terran com addon em construção,
// renderizado em `ACCENT_WARNING` — mesma cor do morph CC→Orbital, já
// que ambos indicam "estrutura ocupada/bloqueada") e, no modo Protoss
// pós-WarpGateResearch, troca
// o estilo de render por sub-trilhas thin estilo Hatchery — uma vez
// que warpgates podem warpinar várias unidades em rajadas paralelas
// entre estruturas distintas.
//
// `Pesquisas` e `Upgrades` ficam como stubs por enquanto — só o seletor
// fica visível com label "Em breve".

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::colors::{
    player_slot_color_bright, ACCENT_WARNING, BORDER, FOCUS_RING, LABEL_DIM, SURFACE_RAISED,
};
use crate::config::AppConfig;
use crate::locale::t;
use crate::production_lanes::{extract, BlockKind, LaneMode, ProductionBlock, StructureLane};
use crate::replay::is_zerg_hatch;
use crate::replay_state::{fmt_time, LoadedReplay};
use crate::tabs::timeline::structure_icon;
use crate::widgets::{chip, player_pov_pill, PlayerPickerSize};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProductionView {
    Workers,
    Army,
    Research,
    Upgrades,
}

impl Default for ProductionView {
    fn default() -> Self {
        ProductionView::Workers
    }
}

/// Estado UI persistente da seção (não volta pro `AppConfig`).
pub struct ProductionChartOptions {
    pub view: ProductionView,
    pub selected_player: usize,
    /// Janela de tempo visível em game loops. `None` = auto-fit.
    pub viewport: Option<(u32, u32)>,
}

impl Default for ProductionChartOptions {
    fn default() -> Self {
        Self {
            view: ProductionView::Workers,
            selected_player: 0,
            viewport: None,
        }
    }
}

const ICON_SIZE: f32 = 28.0;
// Workers e Army compartilham as mesmas dimensões. Antes Workers tinha
// row=32/block=11 — a faixa ocupava só 34% da altura da linha contra
// os 28px do ícone à esquerda, deixando um espaço vertical morto
// considerável e tornando ícones de impedimento (CC→Orbital) e
// sub-trilhas thin de drones Zerg quase ilegíveis. Usar 36/22 em ambos
// os modos casa com a altura do ícone e dá espaço pros 3 slots de
// larva renderizarem com folga.
const ROW_HEIGHT_WORKERS: f32 = 36.0;
const ROW_HEIGHT_ARMY: f32 = 36.0;
const ROW_GAP: f32 = 4.0;
const BLOCK_HEIGHT_WORKERS: f32 = 22.0;
const BLOCK_HEIGHT_ARMY: f32 = 22.0;
const RIGHT_PAD: f32 = 8.0;
const LEFT_GUTTER: f32 = 8.0;
const TIME_AXIS_HEIGHT: f32 = 18.0;
const MIN_VIEW_LOOPS: u32 = 112;

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    opts: &mut ProductionChartOptions,
) {
    let lang = config.language;

    ui.add_space(12.0);
    ui.heading(t("charts.production.title", lang));

    // Seletor de view (Workers / Army / Pesquisas / Upgrades).
    ui.horizontal(|ui| {
        for (view, key) in [
            (ProductionView::Workers, "charts.production.view.workers"),
            (ProductionView::Army, "charts.production.view.army"),
            (ProductionView::Research, "charts.production.view.research"),
            (ProductionView::Upgrades, "charts.production.view.upgrades"),
        ] {
            if chip(ui, t(key, lang), opts.view == view, None).clicked() {
                opts.view = view;
            }
        }
    });

    let players = &loaded.timeline.players;
    if players.is_empty() {
        ui.label(t("charts.no_players", lang));
        return;
    }
    if opts.selected_player >= players.len() {
        opts.selected_player = 0;
    }

    // Pesquisas/Upgrades continuam como stubs até serem implementados.
    // Workers e Army usam o mesmo pipeline (`production_lanes`), variando
    // apenas o `LaneMode` consumido.
    match opts.view {
        ProductionView::Research | ProductionView::Upgrades => {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(t("charts.production.coming_soon", lang)).italics(),
            );
            return;
        }
        _ => {}
    }

    let mode = match opts.view {
        ProductionView::Workers => LaneMode::Workers,
        ProductionView::Army => LaneMode::Army,
        _ => unreachable!(),
    };

    // Cabeçalho: seletor de jogador + reset.
    ui.horizontal(|ui| {
        ui.label(t("charts.production.player", lang));
        for (idx, p) in players.iter().enumerate() {
            if player_pov_pill(
                ui,
                &p.name,
                &p.race,
                idx,
                config.is_user(&p.name),
                idx == opts.selected_player,
                PlayerPickerSize::Small,
                config,
                lang,
            )
            .clicked()
            {
                opts.selected_player = idx;
            }
        }
        ui.separator();
        if chip(
            ui,
            t("charts.production.reset_view", lang),
            opts.viewport.is_none(),
            None,
        )
        .on_hover_text(t("charts.production.reset_view.hint", lang))
        .clicked()
        {
            opts.viewport = None;
        }
    });

    let lanes_per_player = extract(&loaded.timeline, mode);
    let lanes = &lanes_per_player[opts.selected_player];
    if lanes.lanes.is_empty() {
        let key = match mode {
            LaneMode::Workers => "charts.production.empty.workers",
            LaneMode::Army => "charts.production.empty.army",
        };
        ui.label(egui::RichText::new(t(key, lang)).italics());
        return;
    }

    let lps = loaded.timeline.loops_per_second;
    let game_end_loop = effective_end_loop(loaded);

    let (view_start, view_end) = opts.viewport.unwrap_or((0, game_end_loop));
    let view_start = view_start.min(game_end_loop.saturating_sub(MIN_VIEW_LOOPS));
    let view_end = view_end.min(game_end_loop).max(view_start + MIN_VIEW_LOOPS);

    let player_color = player_slot_color_bright(opts.selected_player);

    let row_height = match mode {
        LaneMode::Workers => ROW_HEIGHT_WORKERS,
        LaneMode::Army => ROW_HEIGHT_ARMY,
    };
    let block_height = match mode {
        LaneMode::Workers => BLOCK_HEIGHT_WORKERS,
        LaneMode::Army => BLOCK_HEIGHT_ARMY,
    };

    let total_w = ui.available_width();
    let n_lanes = lanes.lanes.len();
    let total_h = TIME_AXIS_HEIGHT + n_lanes as f32 * (row_height + ROW_GAP) + ROW_GAP;
    let (chart_rect, response) =
        ui.allocate_exact_size(Vec2::new(total_w, total_h), Sense::click_and_drag());

    let track_x_start = ICON_SIZE + LEFT_GUTTER * 2.0;
    // Não aplicamos `round()` aqui: o egui renderiza com sub-pixel,
    // então pequenas variações fracionárias em `chart_rect.width()`
    // (causadas, p.ex., pelo fade da scrollbar do `ScrollArea` pai)
    // se traduzem em jitter sub-pixel invisível. Arredondar amplifica
    // essas variações para saltos visíveis de 1 pixel.
    let track_left = chart_rect.left() + track_x_start;
    let track_width = (chart_rect.width() - track_x_start - RIGHT_PAD).max(50.0);

    apply_zoom_pan(
        &response,
        ui,
        track_left,
        track_width,
        game_end_loop,
        view_start,
        view_end,
        &mut opts.viewport,
    );

    let (view_start, view_end) = opts.viewport.unwrap_or((0, game_end_loop));
    let view_start = view_start.min(game_end_loop.saturating_sub(MIN_VIEW_LOOPS));
    let view_end = view_end.min(game_end_loop).max(view_start + MIN_VIEW_LOOPS);

    let painter = ui.painter_at(chart_rect);

    let axis_top = chart_rect.top();
    draw_time_axis(
        &painter,
        track_left,
        track_width,
        axis_top,
        view_start,
        view_end,
        lps,
    );

    let mut row_top = chart_rect.top() + TIME_AXIS_HEIGHT + ROW_GAP;
    for lane in &lanes.lanes {
        let row_rect = Rect::from_min_size(
            Pos2::new(chart_rect.left(), row_top),
            Vec2::new(total_w, row_height),
        );
        draw_lane(
            ui,
            &painter,
            lane,
            row_rect,
            track_left,
            track_width,
            view_start,
            view_end,
            game_end_loop,
            player_color,
            row_height,
            block_height,
        );
        row_top += row_height + ROW_GAP;
    }

    if let Some(hover_pos) = response.hover_pos() {
        if hover_pos.x >= track_left && hover_pos.x <= track_left + track_width {
            painter.line_segment(
                [
                    Pos2::new(hover_pos.x, axis_top + TIME_AXIS_HEIGHT - 2.0),
                    Pos2::new(hover_pos.x, chart_rect.bottom()),
                ],
                Stroke::new(1.0, FOCUS_RING.gamma_multiply(0.6)),
            );
            let frac = (hover_pos.x - track_left) / track_width;
            let loop_at = view_start
                + (frac.clamp(0.0, 1.0) * (view_end - view_start) as f32) as u32;
            painter.text(
                Pos2::new(hover_pos.x + 4.0, axis_top + 1.0),
                Align2::LEFT_TOP,
                fmt_time(loop_at, lps),
                FontId::proportional(11.0),
                FOCUS_RING,
            );
        }
    }
}

fn effective_end_loop(loaded: &LoadedReplay) -> u32 {
    let game_loops = loaded.timeline.game_loops.max(1);
    let max_seconds = loaded.timeline.max_time_seconds;
    if max_seconds == 0 {
        game_loops
    } else {
        let max_loops = (max_seconds as f64 * loaded.timeline.loops_per_second).round() as u32;
        game_loops.min(max_loops.max(1))
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_zoom_pan(
    response: &egui::Response,
    ui: &Ui,
    track_left: f32,
    track_width: f32,
    full_end: u32,
    view_start: u32,
    view_end: u32,
    view: &mut Option<(u32, u32)>,
) {
    let view_w = (view_end - view_start) as f64;

    let drag_dx = response.drag_delta().x;
    if drag_dx.abs() > 0.01 && track_width > 0.0 {
        let loops_per_pixel = view_w / track_width as f64;
        let delta_loops = -(drag_dx as f64 * loops_per_pixel);
        let new_start =
            (view_start as f64 + delta_loops).clamp(0.0, (full_end as f64 - view_w).max(0.0));
        let new_start = new_start as u32;
        let new_end = new_start + view_w as u32;
        *view = Some((new_start, new_end.min(full_end)));
    }

    if response.double_clicked() {
        *view = None;
        return;
    }

    if response.hovered() {
        let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_y.abs() > 0.1 {
            let cursor_x = response
                .hover_pos()
                .map(|p| p.x)
                .unwrap_or(track_left + track_width * 0.5);
            let cursor_frac =
                ((cursor_x - track_left) / track_width).clamp(0.0, 1.0) as f64;
            let cursor_loop = view_start as f64 + cursor_frac * view_w;

            let zoom_factor = (-scroll_y as f64 * 0.0015).exp();
            let new_w =
                (view_w * zoom_factor).clamp(MIN_VIEW_LOOPS as f64, full_end as f64);
            let new_start = (cursor_loop - cursor_frac * new_w).max(0.0);
            let new_end = (new_start + new_w).min(full_end as f64);
            let new_start = (new_end - new_w).max(0.0);
            *view = Some((new_start as u32, new_end as u32));
        }
    }
}

fn draw_time_axis(
    painter: &egui::Painter,
    track_left: f32,
    track_w: f32,
    top_y: f32,
    view_start: u32,
    view_end: u32,
    lps: f64,
) {
    let baseline_y = top_y + TIME_AXIS_HEIGHT - 2.0;
    let track_right = track_left + track_w;
    painter.line_segment(
        [
            Pos2::new(track_left, baseline_y),
            Pos2::new(track_right, baseline_y),
        ],
        Stroke::new(1.0, BORDER),
    );

    let view_secs = ((view_end - view_start) as f64 / lps.max(1.0)) as u32;
    let step_secs: u32 = if view_secs > 1200 {
        180
    } else if view_secs > 600 {
        120
    } else if view_secs > 240 {
        60
    } else if view_secs > 120 {
        30
    } else if view_secs > 60 {
        15
    } else {
        5
    };

    let font = FontId::proportional(11.0);
    let start_secs = (view_start as f64 / lps.max(1.0)) as u32;
    let end_secs = (view_end as f64 / lps.max(1.0)) as u32;
    let first_tick = ((start_secs + step_secs - 1) / step_secs) * step_secs;
    let mut t_secs = first_tick;
    while t_secs <= end_secs {
        let frac = (t_secs - start_secs) as f32 / view_secs.max(1) as f32;
        let x = track_left + frac * track_w;
        painter.line_segment(
            [Pos2::new(x, baseline_y - 3.0), Pos2::new(x, baseline_y)],
            Stroke::new(1.0, BORDER),
        );
        let label = format!("{}:{:02}", t_secs / 60, t_secs % 60);
        painter.text(
            Pos2::new(x, baseline_y - 4.0),
            Align2::CENTER_BOTTOM,
            label,
            font.clone(),
            LABEL_DIM,
        );
        t_secs = t_secs.saturating_add(step_secs);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_lane(
    ui: &Ui,
    painter: &egui::Painter,
    lane: &StructureLane,
    row_rect: Rect,
    track_left: f32,
    track_w: f32,
    view_start: u32,
    view_end: u32,
    end_loop: u32,
    player_color: Color32,
    row_height: f32,
    block_height: f32,
) {
    // Ícone da estrutura à esquerda — fixo, não escala com zoom.
    let icon_top = row_rect.top() + (row_height - ICON_SIZE) * 0.5;
    let icon_rect = Rect::from_min_size(
        Pos2::new(row_rect.left() + LEFT_GUTTER, icon_top),
        Vec2::splat(ICON_SIZE),
    );
    if let Some(src) = structure_icon(lane.canonical_type) {
        egui::Image::new(src)
            .fit_to_exact_size(Vec2::splat(ICON_SIZE))
            .paint_at(ui, icon_rect);
    } else {
        painter.rect_filled(icon_rect, 4.0, SURFACE_RAISED);
        painter.text(
            icon_rect.center(),
            Align2::CENTER_CENTER,
            lane.canonical_type
                .chars()
                .next()
                .map(|c| c.to_string())
                .unwrap_or_default(),
            FontId::proportional(14.0),
            LABEL_DIM,
        );
    }

    let block_top = row_rect.center().y - block_height * 0.5;
    let block_bot = row_rect.center().y + block_height * 0.5;

    // Faixa de vida da estrutura (baseline + tick no born).
    let lane_end_loop = lane.died_loop.unwrap_or(end_loop).min(end_loop);
    if lane_end_loop > lane.born_loop {
        let s = lane.born_loop.max(view_start);
        let e = lane_end_loop.min(view_end);
        if e > s {
            let x0 = loop_to_x(s, view_start, view_end, track_left, track_w);
            let x1 = loop_to_x(e, view_start, view_end, track_left, track_w);
            painter.line_segment(
                [
                    Pos2::new(x0, row_rect.center().y),
                    Pos2::new(x1, row_rect.center().y),
                ],
                Stroke::new(1.0, BORDER),
            );
            if lane.born_loop >= view_start && lane.born_loop <= view_end {
                painter.line_segment(
                    [
                        Pos2::new(x0, block_top - 2.0),
                        Pos2::new(x0, block_bot + 2.0),
                    ],
                    Stroke::new(1.0, BORDER),
                );
            }
        }
    }

    let lane_is_zerg_hatch = is_zerg_hatch(lane.canonical_type);

    // Identifica quais blocos vão para sub-trilhas paralelas e atribui
    // a cada um um índice vertical (0..n) via interval scheduling, pra
    // que produções simultâneas (ex.: drones nascendo em larvas
    // distintas da mesma Hatch) apareçam empilhadas em vez de
    // sobrepostas na mesma faixa thin centralizada.
    //
    // Aplica para: lanes Zerg Hatch/Lair/Hive (modo Workers e Army), e
    // blocos Producing pós-WarpGateResearch em lanes Protoss.
    let thin_indices: Vec<usize> = (0..lane.blocks.len())
        .filter(|&i| {
            let b = &lane.blocks[i];
            if !matches!(b.kind, BlockKind::Producing) {
                return false;
            }
            let post_reactor = lane
                .reactor_since_loop
                .map(|r| b.start_loop >= r)
                .unwrap_or(false);
            if post_reactor {
                return false;
            }
            lane_is_zerg_hatch
                || lane
                    .warpgate_since_loop
                    .map(|wg| b.start_loop >= wg)
                    .unwrap_or(false)
        })
        .collect();
    let thin_tracks: Vec<usize> = if thin_indices.is_empty() {
        Vec::new()
    } else {
        let refs: Vec<&ProductionBlock> = thin_indices.iter().map(|&i| &lane.blocks[i]).collect();
        assign_parallel_tracks(&refs)
    };
    let mut thin_track_by_block: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::with_capacity(thin_indices.len());
    for (k, &i) in thin_indices.iter().enumerate() {
        thin_track_by_block.insert(i, thin_tracks[k]);
    }
    let n_thin_tracks = thin_tracks.iter().copied().max().map(|m| m + 1).unwrap_or(1);
    // Distribui as N trilhas dentro de block_height com 1px de gap entre
    // elas. Saturamos altura mínima em 2px pra paralelismos altos
    // permanecerem visíveis.
    let thin_line_h = ((block_height - n_thin_tracks.saturating_sub(1) as f32)
        / n_thin_tracks.max(1) as f32)
        .max(2.0);

    for (i, block) in lane.blocks.iter().enumerate() {
        let s = block.start_loop.max(view_start).min(view_end);
        let e = block.end_loop.max(view_start).min(view_end);
        if e <= s {
            continue;
        }
        let x0 = loop_to_x(s, view_start, view_end, track_left, track_w);
        let x1 = loop_to_x(e, view_start, view_end, track_left, track_w);

        let block_post_reactor = matches!(block.kind, BlockKind::Producing)
            && lane
                .reactor_since_loop
                .map(|r| block.start_loop >= r)
                .unwrap_or(false);

        let color = match block.kind {
            BlockKind::Producing => player_color,
            // Morph in-place (CC→Orbital/PF) e Impeded (addon Terran em
            // construção) compartilham a mesma cor: ambos representam
            // "estrutura existe mas está bloqueada para a função normal".
            BlockKind::Morphing | BlockKind::Impeded => ACCENT_WARNING,
        };

        if block_post_reactor {
            // Lane Terran com reactor anexado: duas faixas top/bottom
            // representando capacidade paralela 2x. O `sub_track` do
            // bloco (0 ou 1) decide qual metade ocupar, com gap fino
            // entre elas para distinguir visualmente.
            let gap = 1.0;
            let half_h = ((block_height - gap) * 0.5).max(3.0);
            let (top, bot) = if block.sub_track == 0 {
                (block_top, block_top + half_h)
            } else {
                (block_bot - half_h, block_bot)
            };
            let rect = Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x1, bot));
            painter.rect_filled(rect, 1.5, color);
        } else if let Some(&track_idx) = thin_track_by_block.get(&i) {
            // Sub-trilha thin para Hatch Zerg / WarpGate pós-research:
            // cada produção paralela ganha sua própria linha vertical
            // (interval scheduling). Sem isso, drones nascendo
            // simultaneamente de larvas distintas se sobrepõem na
            // mesma posição central e somem visualmente.
            let top = block_top + track_idx as f32 * (thin_line_h + 1.0);
            let rect = Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x1, top + thin_line_h));
            painter.rect_filled(rect, 1.0, color);
        } else {
            let rect = Rect::from_min_max(Pos2::new(x0, block_top), Pos2::new(x1, block_bot));
            painter.rect_filled(rect, 1.5, color);

            // Blocos `Morphing` (CC→Orbital/PF) e `Impeded` (addon Terran
            // em construção) desenham o ícone do motivo do impedimento
            // centralizado na faixa: Orbital/PF mostra o destino do
            // morph; Impeded mostra o Reactor/TechLab. Producing fica
            // sem ícone — o `player_color` e o ícone da estrutura na
            // coluna esquerda já comunicam o que está sendo produzido,
            // e ícones por unidade poluiriam runs longos de Marines/etc.
            if matches!(block.kind, BlockKind::Morphing | BlockKind::Impeded) {
                if let Some(name) = block.produced_type {
                    if let Some(src) = structure_icon(name) {
                        let icon_size = (block_height - 2.0).max(0.0);
                        let avail_w = rect.width() - 2.0;
                        if icon_size >= 6.0 && avail_w >= icon_size {
                            let icon_rect = Rect::from_center_size(
                                rect.center(),
                                Vec2::splat(icon_size),
                            );
                            egui::Image::new(src)
                                .fit_to_exact_size(Vec2::splat(icon_size))
                                .paint_at(ui, icon_rect);
                        }
                    }
                }
            }
        }
    }
}

/// Atribui a cada bloco uma sub-trilha paralela usando interval
/// scheduling clássico: percorre os blocos por `start_loop` e coloca
/// em qualquer trilha cujo último intervalo já terminou; cria uma
/// trilha nova se nenhuma estiver livre. Output paralelo a `blocks`
/// (mesmo length, mesmo índice).
fn assign_parallel_tracks(blocks: &[&ProductionBlock]) -> Vec<usize> {
    let mut tracks_end: Vec<u32> = Vec::new();
    let mut assigned = vec![0usize; blocks.len()];
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by_key(|&i| blocks[i].start_loop);
    for &i in &order {
        let b = blocks[i];
        if let Some(idx) = tracks_end.iter().position(|&e| e <= b.start_loop) {
            tracks_end[idx] = b.end_loop;
            assigned[i] = idx;
        } else {
            assigned[i] = tracks_end.len();
            tracks_end.push(b.end_loop);
        }
    }
    assigned
}

fn loop_to_x(
    game_loop: u32,
    view_start: u32,
    view_end: u32,
    track_left: f32,
    track_w: f32,
) -> f32 {
    let view_w = view_end.saturating_sub(view_start).max(1);
    let frac = (game_loop.saturating_sub(view_start)) as f32 / view_w as f32;
    track_left + frac.clamp(0.0, 1.0) * track_w
}

