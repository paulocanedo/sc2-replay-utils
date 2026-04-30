//! Transport bar — controles de reprodução da timeline em estilo
//! "transport bar" de player de vídeo. Contém, em ordem:
//!   - Play/Pause (toggle; rewind ao clicar no fim do replay)
//!   - Speed switch (gira em 1× → 2× → 4× → 8× → 1×)
//!   - Slider de scrubbing ocupando o restante da largura
//!
//! Playback: quando `playing` é `true`, o wrapper da aba chama
//! [`advance_playback`] antes do render, que avança `current_loop`
//! proporcionalmente a `speed × loops_per_second × dt`, acumulando
//! resíduo fracionário entre frames (senão a 60fps/1× `round` trunca
//! para 0 e o tempo nunca andaria). Pausa automaticamente ao atingir
//! o final do replay.

use egui::{Align, Button, Color32, Id, Layout, Response, RichText, Slider, Ui};

use crate::colors::FOCUS_RING;
use crate::replay::ReplayTimeline;
use crate::replay_state::fmt_time;
use crate::tokens::{RADIUS_BUTTON, SPACE_S};

/// Velocidades suportadas pelo botão de speed. Ciclamos nessa ordem ao
/// clicar; também é o conjunto de valores válidos de
/// `AppState.timeline_playback_speed`.
const SPEEDS: [u8; 4] = [1, 2, 4, 8];

/// Largura fixa para o botão de play/pause e de velocidade, de modo que
/// o layout não "pule" quando o rótulo muda (▶ ↔ ⏸ / "1×" ↔ "8×").
const CTRL_BUTTON_WIDTH: f32 = 36.0;

/// Chave do acumulador fracionário de playback no `egui::Memory`. Como
/// existe apenas uma timeline ativa por vez, um ID global basta — o
/// valor é zerado ao pausar ou pelo `reset_playback_accumulator`.
const PLAYBACK_ACCUM_ID: &str = "timeline_playback_accum";

pub(super) fn transport_slider(
    ui: &mut Ui,
    tl: &ReplayTimeline,
    current_loop: &mut u32,
    max_loop: u32,
    playing: &mut bool,
    speed: &mut u8,
) {
    let time_label = format!(
        "{} / {}",
        fmt_time(*current_loop, tl.loops_per_second),
        fmt_time(tl.game_loops, tl.loops_per_second),
    );
    ui.horizontal(|ui| {
        play_pause_button(ui, playing, current_loop, max_loop);
        speed_button(ui, speed, *playing);
        ui.add_space(SPACE_S);
        // Reserva o slot mais à direita pro rótulo de tempo; o slider
        // consome a largura restante. `right_to_left` encaixa o rótulo
        // à direita e deixa o slider alinhado pela esquerda dele.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.monospace(time_label);
            ui.add_space(SPACE_S);
            let slider_w = (ui.available_width() - 12.0).max(160.0);
            ui.spacing_mut().slider_width = slider_w;
            let slider_resp = ui.add(
                Slider::new(current_loop, 0..=max_loop)
                    .integer()
                    .show_value(false),
            );
            // Scrubbing manual descarta o resíduo fracionário do playback
            // para não "saltar" um frame extra quando o usuário solta o
            // mouse.
            if slider_resp.dragged() {
                reset_playback_accumulator(ui.ctx());
            }
        });
    });
}

/// Avança `current_loop` com base no tempo decorrido (`dt`), respeitando
/// `speed`. Pausa ao atingir `max_loop`.
///
/// Usa um acumulador fracionário em `egui::Memory`: a 60fps com
/// `loops_per_second ≈ 22.4` e speed=1, o avanço por frame é ~0.37 — se
/// convertêssemos direto para `u32`, o tempo nunca andaria. Acumulamos
/// a parte fracionária entre frames e avançamos o loop só quando passa
/// de 1.
pub(super) fn advance_playback(
    tl: &ReplayTimeline,
    current_loop: &mut u32,
    max_loop: u32,
    playing: &mut bool,
    speed: u8,
    dt: f32,
    ctx: &egui::Context,
) {
    if !*playing {
        reset_playback_accumulator(ctx);
        return;
    }
    if *current_loop >= max_loop {
        *playing = false;
        *current_loop = max_loop;
        reset_playback_accumulator(ctx);
        return;
    }

    let advance = tl.loops_per_second as f32 * speed as f32 * dt;
    let id = Id::new(PLAYBACK_ACCUM_ID);
    let accum: f32 = ctx.memory(|m| m.data.get_temp(id).unwrap_or(0.0));
    let total = accum + advance;
    let whole = total.floor();
    let remainder = total - whole;
    ctx.memory_mut(|m| m.data.insert_temp(id, remainder));

    let whole = whole as i64;
    if whole <= 0 {
        return;
    }
    let next = *current_loop as i64 + whole;
    if next >= max_loop as i64 {
        *current_loop = max_loop;
        *playing = false;
        reset_playback_accumulator(ctx);
    } else {
        *current_loop = next as u32;
    }
}

/// Atalhos de teclado para scrubbing fino: setas movem o `current_loop`
/// pela timeline. Modificadores ajustam o tamanho do passo:
/// - sem modificador: 22 loops (~1s de jogo @ 22.4 lps)
/// - Ctrl: 100 loops (passo grosso)
/// - Alt: 1 loop (frame-a-frame)
/// Alt tem precedência sobre Ctrl quando ambos estão pressionados.
pub(super) fn handle_keyboard_scrub(
    ctx: &egui::Context,
    current_loop: &mut u32,
    max_loop: u32,
) {
    let (left, right, alt, ctrl) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.modifiers.alt,
            i.modifiers.ctrl,
        )
    });
    if !left && !right {
        return;
    }
    let step: i64 = if alt {
        1
    } else if ctrl {
        100
    } else {
        22
    };
    let delta = if right { step } else { -step };
    let next = (*current_loop as i64 + delta).clamp(0, max_loop as i64);
    *current_loop = next as u32;
    reset_playback_accumulator(ctx);
}

fn reset_playback_accumulator(ctx: &egui::Context) {
    let id = Id::new(PLAYBACK_ACCUM_ID);
    ctx.memory_mut(|m| m.data.insert_temp(id, 0.0_f32));
}

fn play_pause_button(ui: &mut Ui, playing: &mut bool, current_loop: &mut u32, max_loop: u32) {
    let (glyph, hover) = if *playing {
        ("⏸", "Pause")
    } else {
        ("▶", "Play")
    };
    let resp = ctrl_button(ui, glyph, *playing);
    if resp.on_hover_text(hover).clicked() {
        // Clicar em Play no fim do replay reinicia do zero (comportamento
        // de replay de video player). Caso contrário, apenas alterna.
        if !*playing && *current_loop >= max_loop {
            *current_loop = 0;
        }
        *playing = !*playing;
    }
}

fn speed_button(ui: &mut Ui, speed: &mut u8, playing: bool) {
    // Quando playing=false, o botão fica "atenuado" visualmente para
    // reforçar que a velocidade só tem efeito durante reprodução.
    let resp = ctrl_button(ui, &format!("{}×", speed), playing);
    if resp.on_hover_text("Playback speed").clicked() {
        *speed = next_speed(*speed);
    }
}

fn next_speed(current: u8) -> u8 {
    let idx = SPEEDS.iter().position(|&s| s == current).unwrap_or(0);
    SPEEDS[(idx + 1) % SPEEDS.len()]
}

/// Botão pill com aparência consistente para os controles de transport
/// (play/pause e velocidade). `highlighted` pinta o fundo com o accent
/// para indicar estado ativo (reproduzindo).
fn ctrl_button(ui: &mut Ui, label: &str, highlighted: bool) -> Response {
    let (fill, text) = if highlighted {
        (
            Color32::from_rgb(
                FOCUS_RING.r() / 2 + 20,
                FOCUS_RING.g() / 2 + 20,
                FOCUS_RING.b() / 2 + 20,
            ),
            Color32::WHITE,
        )
    } else {
        (Color32::from_gray(40), Color32::from_gray(210))
    };
    ui.add(
        Button::new(RichText::new(label).color(text).monospace())
            .fill(fill)
            .corner_radius(RADIUS_BUTTON)
            .min_size(egui::vec2(CTRL_BUTTON_WIDTH, 0.0)),
    )
}
