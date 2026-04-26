// Gráfico de "lanes" de produção de worker — uma linha por townhall do
// jogador selecionado, com retângulos coloridos marcando os intervalos
// em que a estrutura estava produzindo (cor do jogador) ou em morph
// in-place (laranja, "impedimento"). Idle = só baseline fina.
//
// Render via `ui.painter_at()` clipado dentro de um rect alocado com
// `Sense::click_and_drag` — não usa `egui_plot` porque a estrutura é
// um Gantt simples e queremos controle preciso sobre as faixas de 8pt
// e o overlay de duração nos blocos largos. O retângulo de captura
// também é o que recebe scroll (zoom) e drag (pan).

use egui::{
    Align2, Color32, ComboBox, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};

use crate::colors::{
    player_slot_color_bright, ACCENT_WARNING, BORDER, FOCUS_RING, LABEL_DIM, SURFACE_RAISED,
};
use crate::config::AppConfig;
use crate::locale::t;
use crate::replay_state::{fmt_time, LoadedReplay};
use crate::tabs::timeline::structure_icon;
use crate::widgets::chip;
use crate::worker_production_chart::{extract, BlockKind, StructureLane};

/// Estado UI persistente da seção (não volta pro `AppConfig`).
pub struct WorkerProductionOptions {
    pub selected_player: usize,
    /// Janela de tempo visível em game loops. `None` = auto-fit
    /// (`[0, effective_end]`). Setado pela primeira interação de
    /// zoom/pan ou pelo "Reset" (que volta a `None`).
    pub view: Option<(u32, u32)>,
}

impl Default for WorkerProductionOptions {
    fn default() -> Self {
        Self {
            selected_player: 0,
            view: None,
        }
    }
}

const ICON_SIZE: f32 = 28.0;
const ROW_HEIGHT: f32 = 32.0;
const ROW_GAP: f32 = 4.0;
const BLOCK_HEIGHT: f32 = 11.0; // ~8pt em DPI 1.0 + folga visual
const RIGHT_PAD: f32 = 8.0;
const LEFT_GUTTER: f32 = 8.0;
const MIN_LABEL_WIDTH: f32 = 36.0;
const TIME_AXIS_HEIGHT: f32 = 18.0;
/// Limite mínimo de janela visível (em loops) para evitar que o usuário
/// dê zoom até converter o gráfico num único pixel. ~5s em Faster.
const MIN_VIEW_LOOPS: u32 = 112;

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    opts: &mut WorkerProductionOptions,
) {
    let lang = config.language;

    ui.add_space(12.0);
    ui.heading(t("charts.worker_production.title", lang));

    let players = &loaded.timeline.players;
    if players.is_empty() {
        ui.label(t("charts.no_players", lang));
        return;
    }
    if opts.selected_player >= players.len() {
        opts.selected_player = 0;
    }

    // Cabeçalho: seletor + reset.
    ui.horizontal(|ui| {
        ui.label(t("charts.worker_production.player", lang));
        let selected_name = players[opts.selected_player].name.clone();
        ComboBox::from_id_salt("worker_production_player")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for (idx, p) in players.iter().enumerate() {
                    ui.selectable_value(&mut opts.selected_player, idx, &p.name);
                }
            });
        ui.separator();
        if chip(
            ui,
            t("charts.worker_production.reset_view", lang),
            opts.view.is_none(),
            None,
        )
        .on_hover_text(t("charts.worker_production.reset_view.hint", lang))
        .clicked()
        {
            opts.view = None;
        }
    });

    let lanes_per_player = extract(&loaded.timeline);
    let lanes = &lanes_per_player[opts.selected_player];
    if lanes.lanes.is_empty() {
        ui.label(egui::RichText::new(t("charts.worker_production.empty", lang)).italics());
        return;
    }

    let lps = loaded.timeline.loops_per_second;
    let game_end_loop = effective_end_loop(loaded);

    // Resolve a view atual (auto-fit quando None).
    let (view_start, view_end) = opts.view.unwrap_or((0, game_end_loop));
    let view_start = view_start.min(game_end_loop.saturating_sub(MIN_VIEW_LOOPS));
    let view_end = view_end.min(game_end_loop).max(view_start + MIN_VIEW_LOOPS);

    let player_color = player_slot_color_bright(opts.selected_player);

    // Aloca um rect único para todo o gráfico (axis + lanes). Esse rect
    // serve dois propósitos: (1) capturar drag/scroll para zoom/pan,
    // (2) clip do painter para que blocos fora da view não vazem.
    let total_w = ui.available_width();
    let n_lanes = lanes.lanes.len();
    let total_h = TIME_AXIS_HEIGHT
        + n_lanes as f32 * (ROW_HEIGHT + ROW_GAP)
        + ROW_GAP;
    let (chart_rect, response) =
        ui.allocate_exact_size(Vec2::new(total_w, total_h), Sense::click_and_drag());

    let track_x_start = ICON_SIZE + LEFT_GUTTER * 2.0;
    let track_left = chart_rect.left() + track_x_start;
    let track_width = (chart_rect.width() - track_x_start - RIGHT_PAD).max(50.0);

    // Aplica input. Mutate em `opts.view` se houver mudança.
    apply_zoom_pan(
        &response,
        ui,
        track_left,
        track_width,
        game_end_loop,
        view_start,
        view_end,
        &mut opts.view,
    );

    // Recalcula a view (apply_zoom_pan pode tê-la atualizado).
    let (view_start, view_end) = opts.view.unwrap_or((0, game_end_loop));
    let view_start = view_start.min(game_end_loop.saturating_sub(MIN_VIEW_LOOPS));
    let view_end = view_end.min(game_end_loop).max(view_start + MIN_VIEW_LOOPS);

    let painter = ui.painter_at(chart_rect);

    // Eixo de tempo no topo.
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

    // Lanes.
    let mut row_top = chart_rect.top() + TIME_AXIS_HEIGHT + ROW_GAP;
    for lane in &lanes.lanes {
        let row_rect =
            Rect::from_min_size(Pos2::new(chart_rect.left(), row_top), Vec2::new(total_w, ROW_HEIGHT));
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
            lps,
            player_color,
        );
        row_top += ROW_HEIGHT + ROW_GAP;
    }

    // Cursor crosshair: linha vertical no hover, com tempo formatado
    // junto ao topo do eixo.
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
            let loop_at = view_start + (frac.clamp(0.0, 1.0) * (view_end - view_start) as f32) as u32;
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

    // Pan: drag horizontal converte pixel em loops via fator atual.
    let drag_dx = response.drag_delta().x;
    if drag_dx.abs() > 0.01 && track_width > 0.0 {
        let loops_per_pixel = view_w / track_width as f64;
        let delta_loops = -(drag_dx as f64 * loops_per_pixel);
        let new_start = (view_start as f64 + delta_loops)
            .clamp(0.0, (full_end as f64 - view_w).max(0.0));
        let new_start = new_start as u32;
        let new_end = new_start + view_w as u32;
        *view = Some((new_start, new_end.min(full_end)));
    }

    // Double-click: reset auto-fit.
    if response.double_clicked() {
        *view = None;
        return;
    }

    // Zoom: scroll wheel centrado no cursor.
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

            // 1 unit de scroll = ~10% de zoom. Negativo (scroll down) =
            // zoom out; positivo = zoom in.
            let zoom_factor = (-scroll_y as f64 * 0.0015).exp();
            let new_w = (view_w * zoom_factor)
                .clamp(MIN_VIEW_LOOPS as f64, full_end as f64);
            let new_start = (cursor_loop - cursor_frac * new_w).max(0.0);
            let new_end = (new_start + new_w).min(full_end as f64);
            // Reajusta start se end estourou (preserva largura).
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
    // Primeiro tick alinhado ao múltiplo de step_secs.
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
    lps: f64,
    player_color: Color32,
) {
    // Ícone à esquerda — fica fixo (não escala com zoom).
    let icon_top = row_rect.top() + (ROW_HEIGHT - ICON_SIZE) * 0.5;
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

    let block_top = row_rect.center().y - BLOCK_HEIGHT * 0.5;
    let block_bot = row_rect.center().y + BLOCK_HEIGHT * 0.5;

    // Faixa de vida da estrutura: baseline fina do born até died/end,
    // mas só dentro da view.
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
            // Tick vertical no born (apenas se visível).
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

    // Blocos.
    let font = FontId::proportional(10.0);
    for block in &lane.blocks {
        let s = block.start_loop.max(view_start).min(view_end);
        let e = block.end_loop.max(view_start).min(view_end);
        if e <= s {
            continue;
        }
        let x0 = loop_to_x(s, view_start, view_end, track_left, track_w);
        let x1 = loop_to_x(e, view_start, view_end, track_left, track_w);
        let rect = Rect::from_min_max(Pos2::new(x0, block_top), Pos2::new(x1, block_bot));
        let color = match block.kind {
            BlockKind::Producing => player_color,
            BlockKind::Morphing => ACCENT_WARNING,
        };
        painter.rect_filled(rect, 1.5, color);

        // Label de duração: usa a duração total do bloco (não a recortada),
        // exibida apenas se o segmento visível tiver largura mínima.
        if rect.width() >= MIN_LABEL_WIDTH {
            let total_dur = block.end_loop.saturating_sub(block.start_loop);
            let label = fmt_time(total_dur, lps);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                font.clone(),
                contrast_text_color(color),
            );
        }
    }
}

fn loop_to_x(game_loop: u32, view_start: u32, view_end: u32, track_left: f32, track_w: f32) -> f32 {
    let view_w = view_end.saturating_sub(view_start).max(1);
    let frac = (game_loop.saturating_sub(view_start)) as f32 / view_w as f32;
    track_left + frac.clamp(0.0, 1.0) * track_w
}

/// Texto preto sobre cores claras / branco sobre cores escuras.
/// Heurística simples baseada na luminância percebida.
fn contrast_text_color(bg: Color32) -> Color32 {
    let r = bg.r() as f32;
    let g = bg.g() as f32;
    let b = bg.b() as f32;
    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
    if lum > 150.0 {
        Color32::from_rgb(20, 20, 20)
    } else {
        Color32::from_rgb(240, 240, 240)
    }
}
