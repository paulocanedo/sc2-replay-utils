// Entry point do binário GUI (sc2-replay-gui).
//
// Este arquivo é apenas plumbing: declara via #[path] os módulos de
// src/ (compartilhados com o CLI) e os módulos novos em src/gui/,
// depois chama eframe::run_native. Toda a lógica está em src/gui/.

// Os módulos de src/ são compartilhados com o CLI (src/main.rs) e nem
// todas as funções são exercidas pelo binário GUI — silenciamos os
// dead_code warnings do nível do binário inteiro.
#![allow(dead_code)]

// Módulos de domínio (compartilhados com src/main.rs via #[path]).
#[path = "../replay.rs"]
mod replay;
#[path = "../build_order.rs"]
mod build_order;
#[path = "../chat.rs"]
mod chat;
#[path = "../army_value.rs"]
mod army_value;
#[path = "../production_gap.rs"]
mod production_gap;
#[path = "../supply_block.rs"]
mod supply_block;
#[path = "../utils.rs"]
mod utils;

// Auxiliares puxados pela cadeia de compile dos módulos acima.
// Incluídos mesmo não sendo usados diretamente pela GUI para evitar
// quebras de símbolos caso exista alguma dependência cruzada.
#[path = "../icons.rs"]
mod icons;
#[path = "../all_image.rs"]
mod all_image;
#[path = "../army_value_image.rs"]
mod army_value_image;
#[path = "../build_order_image.rs"]
mod build_order_image;
#[path = "../production_gap_image.rs"]
mod production_gap_image;
#[path = "../supply_block_image.rs"]
mod supply_block_image;
#[path = "../commands.rs"]
mod commands;

// Módulos exclusivos da GUI.
#[path = "../gui/build_times.rs"]
mod build_times;
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
#[path = "../gui/library.rs"]
mod library;
#[path = "../gui/tabs/mod.rs"]
mod tabs;
#[path = "../gui/app.rs"]
mod app;

fn main() -> eframe::Result<()> {
    dotenvy::dotenv().ok();

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
