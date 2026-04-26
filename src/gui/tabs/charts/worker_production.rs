// Gráfico de "lanes" de produção de worker — uma linha por townhall do
// jogador selecionado, com retângulos coloridos marcando os intervalos
// em que a estrutura estava produzindo (cor do jogador) ou em morph
// in-place (laranja, "impedimento"). Idle = só baseline fina.
//
// Render via `ui.painter()` — não usa `egui_plot` porque a estrutura é
// um Gantt simples e queremos controle preciso sobre as faixas de 8pt
// e o overlay de duração nos blocos largos.

use egui::{Align2, Color32, ComboBox, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::colors::{player_slot_color_bright, ACCENT_WARNING, BORDER, LABEL_DIM, SURFACE_RAISED};
use crate::config::AppConfig;
use crate::locale::t;
use crate::replay_state::{fmt_time, LoadedReplay};
use crate::tabs::timeline::structure_icon;
use crate::worker_production_chart::{extract, BlockKind};

/// Estado UI persistente da seção (não volta pro `AppConfig`).
pub struct WorkerProductionOptions {
    pub selected_player: usize,
}

impl Default for WorkerProductionOptions {
    fn default() -> Self {
        Self { selected_player: 0 }
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

    // Seletor de jogador.
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
    });

    let lanes_per_player = extract(&loaded.timeline);
    let lanes = &lanes_per_player[opts.selected_player];
    if lanes.lanes.is_empty() {
        ui.label(egui::RichText::new(t("charts.worker_production.empty", lang)).italics());
        return;
    }

    let lps = loaded.timeline.loops_per_second;
    let game_end_loop = effective_end_loop(loaded);

    let player_color = player_slot_color_bright(opts.selected_player);

    let total_width = ui.available_width();
    let track_x_start = ICON_SIZE + LEFT_GUTTER * 2.0;
    let track_width = (total_width - track_x_start - RIGHT_PAD).max(50.0);

    draw_time_axis(ui, track_x_start, track_width, game_end_loop, lps);

    for lane in &lanes.lanes {
        draw_lane(
            ui,
            lane,
            track_x_start,
            track_width,
            game_end_loop,
            lps,
            player_color,
        );
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

fn draw_time_axis(ui: &mut Ui, track_x: f32, track_w: f32, end_loop: u32, lps: f64) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), TIME_AXIS_HEIGHT),
        Sense::hover(),
    );
    let painter = ui.painter();
    let baseline_y = rect.bottom() - 2.0;
    let track_left = rect.left() + track_x;
    let track_right = track_left + track_w;
    painter.line_segment(
        [
            Pos2::new(track_left, baseline_y),
            Pos2::new(track_right, baseline_y),
        ],
        Stroke::new(1.0, BORDER),
    );

    // Tick a cada 2 minutos. Em replays muito curtos, cai pra 30s.
    let total_secs = (end_loop as f64 / lps.max(1.0)) as u32;
    let step_secs: u32 = if total_secs > 1200 {
        180
    } else if total_secs > 600 {
        120
    } else if total_secs > 240 {
        60
    } else {
        30
    };

    let font = FontId::proportional(11.0);
    let mut t_secs = 0u32;
    while t_secs <= total_secs {
        let frac = t_secs as f32 / total_secs.max(1) as f32;
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
        t_secs += step_secs;
    }
}

fn draw_lane(
    ui: &mut Ui,
    lane: &crate::worker_production_chart::StructureLane,
    track_x: f32,
    track_w: f32,
    end_loop: u32,
    lps: f64,
    player_color: Color32,
) {
    ui.add_space(ROW_GAP);
    let (row_rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        Sense::hover(),
    );

    // Ícone à esquerda.
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
        // Fallback sutil — placeholder neutro com a inicial do tipo.
        let painter = ui.painter();
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

    let painter = ui.painter();
    let track_left = row_rect.left() + track_x;
    let track_right = track_left + track_w;
    let block_top = row_rect.center().y - BLOCK_HEIGHT * 0.5;
    let block_bot = row_rect.center().y + BLOCK_HEIGHT * 0.5;

    // Faixa de vida da estrutura: desenha baseline fina onde a estrutura
    // está viva (do born ao died/end).
    let lane_end_loop = lane.died_loop.unwrap_or(end_loop).min(end_loop);
    if lane_end_loop > lane.born_loop {
        let x0 = loop_to_x(lane.born_loop, end_loop, track_left, track_w);
        let x1 = loop_to_x(lane_end_loop, end_loop, track_left, track_w);
        painter.line_segment(
            [
                Pos2::new(x0, row_rect.center().y),
                Pos2::new(x1, row_rect.center().y),
            ],
            Stroke::new(1.0, BORDER),
        );
        // Marca o born com um tick vertical curto.
        painter.line_segment(
            [
                Pos2::new(x0, block_top - 2.0),
                Pos2::new(x0, block_bot + 2.0),
            ],
            Stroke::new(1.0, BORDER),
        );
    }

    // Blocos.
    let font = FontId::proportional(10.0);
    for block in &lane.blocks {
        let start = block.start_loop.min(end_loop);
        let end = block.end_loop.min(end_loop);
        if end <= start {
            continue;
        }
        let x0 = loop_to_x(start, end_loop, track_left, track_w);
        let x1 = loop_to_x(end, end_loop, track_left, track_w);
        let rect = Rect::from_min_max(Pos2::new(x0, block_top), Pos2::new(x1, block_bot));
        let color = match block.kind {
            BlockKind::Producing => player_color,
            BlockKind::Morphing => ACCENT_WARNING,
        };
        painter.rect_filled(rect, 1.5, color);

        if rect.width() >= MIN_LABEL_WIDTH {
            let dur = end.saturating_sub(start);
            let label = fmt_duration(dur, lps);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                font.clone(),
                contrast_text_color(color),
            );
        }
    }

    // Borda direita do track (visual).
    let _ = track_right; // referência reservada caso queiramos pintar uma régua de fim.
    // Sublinha a faixa do track no topo do row para harmonizar com o
    // axis acima — opcional, aqui só por consistência visual.
    let _ = block_top;
    let _ = StrokeKind::Inside;
}

fn loop_to_x(game_loop: u32, end_loop: u32, track_left: f32, track_w: f32) -> f32 {
    let frac = game_loop as f32 / end_loop.max(1) as f32;
    track_left + frac.clamp(0.0, 1.0) * track_w
}

fn fmt_duration(loops: u32, lps: f64) -> String {
    fmt_time(loops, lps)
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
