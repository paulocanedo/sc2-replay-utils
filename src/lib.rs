//! Library crate. Module decls live here; `src/main.rs` is a thin entry
//! that constructs `AppState` for native or wasm targets.
//!
//! `AppState` is shared between targets. Modules whose backing infra
//! (filesystem, threads, OS dialogs, Battle.net Cache lookups) doesn't
//! exist on the web — `library`, `watcher`, `cache`, `rename`, and the
//! real `map_image` implementation — are gated out for wasm32. Consumers
//! in `app/state.rs` and the surrounding GUI use
//! `#[cfg(not(target_arch = "wasm32"))]` on the specific fields, methods,
//! and match arms that touch them.

#![allow(dead_code)]

// ── Domain modules (compile on every target) ──
mod replay;
mod balance_data;
mod build_order;
mod chat;
mod army_value;
mod production_gap;
mod production_efficiency;
mod army_production_by_battle;
mod loss_analysis;
mod supply_block;
mod worker_potential;
mod production_lanes;
mod utils;

// ── map_image: native real, wasm type stub ──
//
// `MapImage` is referenced as `Option<MapImage>` field type in
// `replay_state` so the type must exist on every target. The stub keeps
// the type but never produces a value — `LoadedReplay::from_bytes`
// always sets `map_image: None` on wasm.
#[cfg(not(target_arch = "wasm32"))]
mod map_image;
#[cfg(target_arch = "wasm32")]
mod map_image {
    pub struct MapImage {
        pub width: u32,
        pub height: u32,
        pub rgba: Vec<u8>,
    }
}

// ── Native-only modules ──
//
// Filesystem-, thread-, or OS-dialog-heavy code with no meaningful wasm
// equivalent. Re-exported as `crate::xxx` (flat) to match the import
// convention already in use across `src/gui/**`.
#[cfg(not(target_arch = "wasm32"))]
#[path = "gui/watcher.rs"]
mod watcher;
#[cfg(not(target_arch = "wasm32"))]
#[path = "gui/cache.rs"]
mod cache;
#[cfg(not(target_arch = "wasm32"))]
#[path = "gui/library/mod.rs"]
mod library;
// Wasm stub: only `DateRange` is referenced in non-cfg-gated code
// (config.rs's `AppConfig::library_date_range`). Other library types
// stay native-only — call sites that touch them are cfg-gated.
#[cfg(target_arch = "wasm32")]
mod library {
    use serde::{Deserialize, Serialize};

    #[derive(Default, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
    pub enum DateRange {
        All,
        Today,
        #[default]
        ThisWeek,
        ThisMonth,
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[path = "gui/rename.rs"]
mod rename;

// ── Shared GUI tree (compiles for both targets) ──
#[path = "gui/colors.rs"]
mod colors;
#[path = "gui/config.rs"]
mod config;
#[path = "gui/tokens.rs"]
mod tokens;
#[path = "gui/widgets.rs"]
mod widgets;
#[path = "gui/replay_state.rs"]
mod replay_state;
#[path = "gui/ui_settings.rs"]
mod ui_settings;
#[path = "gui/locale.rs"]
mod locale;
#[path = "gui/salt.rs"]
mod salt;
#[path = "gui/tabs/mod.rs"]
mod tabs;
#[path = "gui/app/mod.rs"]
mod app;

pub use app::AppState;
