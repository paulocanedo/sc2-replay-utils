// Entry point do binário GUI (sc2-replay-utils).
//
// Plumbing puro: declara via #[path] os módulos de domínio em src/ e os
// módulos exclusivos da GUI em src/gui/, depois chama eframe::run_native.

#![windows_subsystem = "windows"]
#![allow(dead_code)]

// Módulos de domínio (parser + extractors puros sobre ReplayTimeline).
#[path = "../replay/mod.rs"]
mod replay;
#[path = "../balance_data.rs"]
mod balance_data;
#[path = "../build_order/mod.rs"]
mod build_order;
#[path = "../chat.rs"]
mod chat;
#[path = "../army_value.rs"]
mod army_value;
#[path = "../production_gap.rs"]
mod production_gap;
#[path = "../production_efficiency.rs"]
mod production_efficiency;
#[path = "../supply_block.rs"]
mod supply_block;
#[path = "../utils.rs"]
mod utils;
#[path = "../map_image/mod.rs"]
mod map_image;

// Módulos exclusivos da GUI.
#[path = "../gui/colors.rs"]
mod colors;
#[path = "../gui/config.rs"]
mod config;
#[path = "../gui/replay_state.rs"]
mod replay_state;
#[path = "../gui/watcher.rs"]
mod watcher;
#[path = "../gui/ui_settings.rs"]
mod ui_settings;
#[path = "../gui/locale.rs"]
mod locale;
#[path = "../gui/salt.rs"]
mod salt;
#[path = "../gui/cache.rs"]
mod cache;
#[path = "../gui/library/mod.rs"]
mod library;
#[path = "../gui/rename.rs"]
mod rename;
#[path = "../gui/tabs/mod.rs"]
mod tabs;
#[path = "../gui/app.rs"]
mod app;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([800.0, 520.0])
            .with_title("sc2-replay-utils"),
        ..Default::default()
    };

    eframe::run_native(
        "sc2-replay-utils",
        opts,
        Box::new(|cc| Ok(Box::new(app::AppState::new(cc)) as Box<dyn eframe::App>)),
    )
}
