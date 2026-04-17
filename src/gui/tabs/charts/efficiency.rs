// Gráfico de eficiência de produção (time series).

use egui::{RichText, Ui};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints};

use crate::colors::player_slot_color_bright;
use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::production_efficiency::{EfficiencyTarget, ProductionEfficiencySeries};
use crate::replay_state::{loop_to_secs, LoadedReplay};

pub(super) fn efficiency_plot(
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


    Plot::new(format!("efficiency_plot_t{}", *target as u8))
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
                if p.samples.is_empty() {
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
