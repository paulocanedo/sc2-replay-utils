#![cfg_attr(all(not(target_arch = "wasm32"), target_os = "windows"), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(not(target_arch = "wasm32"))]
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
        Box::new(|cc| Ok(Box::new(sc2_replay_utils::AppState::new(cc)) as Box<dyn eframe::App>)),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let canvas = web_sys::window()
        .expect("no global window")
        .document()
        .expect("no document")
        .get_element_by_id("the_canvas_id")
        .expect("canvas element with id 'the_canvas_id' not found")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("element 'the_canvas_id' is not a <canvas>");

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(sc2_replay_utils::AppState::new(cc)) as Box<dyn eframe::App>)),
            )
            .await
            .expect("eframe failed to start");
    });
}
