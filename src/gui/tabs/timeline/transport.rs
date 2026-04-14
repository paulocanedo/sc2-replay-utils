//! Transport bar — slider de scrubbing em estilo "transport bar" de
//! player de vídeo. O rail ocupa quase toda a largura disponível do
//! bottom panel para permitir arrasto granular. Botões de step permitem
//! avançar/retroceder 1 game loop (◂/▸) ou 1 segundo (|◂/▸|), com
//! hold-to-repeat.

use egui::{Slider, Ui};

use crate::replay::ReplayTimeline;

/// Delay antes de iniciar o repeat ao manter um botão pressionado.
const HOLD_INITIAL_DELAY: f32 = 0.30;
/// Intervalo entre steps durante hold-to-repeat (~15 steps/s).
const HOLD_REPEAT_INTERVAL: f32 = 0.066;

pub(super) fn transport_slider(
    ui: &mut Ui,
    tl: &ReplayTimeline,
    current_loop: &mut u32,
    max_loop: u32,
) {
    let one_second = tl.loops_per_second.round() as i64;

    ui.horizontal(|ui| {
        step_button(ui, "|◂", current_loop, -one_second, max_loop);
        step_button(ui, "◂", current_loop, -1, max_loop);
        step_button(ui, "▸", current_loop, 1, max_loop);
        step_button(ui, "▸|", current_loop, one_second, max_loop);
        ui.add_space(4.0);
        let slider_w = (ui.available_width() - 12.0).max(160.0);
        ui.spacing_mut().slider_width = slider_w;
        ui.add(
            Slider::new(current_loop, 0..=max_loop)
                .integer()
                .show_value(false),
        );
    });
}

/// Botão de step com hold-to-repeat. Um clique aplica `delta` uma vez;
/// manter pressionado repete após um delay inicial.
fn step_button(ui: &mut Ui, label: &str, current_loop: &mut u32, delta: i64, max_loop: u32) {
    let btn = ui.button(label);
    if btn.clicked() {
        apply_delta(current_loop, delta, max_loop);
    }
    if btn.is_pointer_button_down_on() {
        ui.ctx().request_repaint();
        let held = btn.interact_pointer_pos().map_or(0.0, |_| {
            ui.input(|i| i.pointer.press_start_time().map_or(0.0, |t| i.time - t))
        });
        if held > HOLD_INITIAL_DELAY as f64 {
            let dt = ui.input(|i| i.unstable_dt);
            // Accumulate fractional steps via the response's ID-based memory.
            let accum = ui.memory_mut(|mem| {
                let a = mem.data.get_temp_mut_or_default::<f32>(btn.id);
                *a += dt;
                *a
            });
            if accum >= HOLD_REPEAT_INTERVAL {
                let steps = (accum / HOLD_REPEAT_INTERVAL) as i64;
                apply_delta(current_loop, delta * steps, max_loop);
                ui.memory_mut(|mem| {
                    let a = mem.data.get_temp_mut_or_default::<f32>(btn.id);
                    *a -= steps as f32 * HOLD_REPEAT_INTERVAL;
                });
            }
        }
    } else {
        // Reset accumulator when button is released.
        ui.memory_mut(|mem| mem.data.remove::<f32>(btn.id));
    }
}

fn apply_delta(current_loop: &mut u32, delta: i64, max_loop: u32) {
    *current_loop = (*current_loop as i64 + delta).clamp(0, max_loop as i64) as u32;
}
