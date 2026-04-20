// Design tokens — the single source of truth for spacing, typography,
// corner radii and stroke widths across the GUI.
//
// Every tab used to hand-roll its own numbers (2.0, 4.0, 10.0, 15.0, 16.0…).
// Funnel everything through this module so the look stays coherent and a
// future tweak is one-line.
//
// Tokens resolve against `AppConfig.font_size`, matching the existing
// `Heading ≈ base * 1.43` ratio set in `app::apply_style`.

use crate::config::AppConfig;

// ── Spacing ──────────────────────────────────────────────────────────
//
// XS/S = tight bar paddings; M = default gap; L/XL = section breaks;
// XXL = empty-state vertical padding.
pub const SPACE_XS: f32 = 2.0;
pub const SPACE_S: f32 = 4.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 12.0;
pub const SPACE_XL: f32 = 16.0;
pub const SPACE_XXL: f32 = 24.0;

// ── Typography ───────────────────────────────────────────────────────
//
// Four-step scale. All multipliers are resolved against the configured
// base (`font_size`) so the user's size slider keeps working.
//
// caption  (0.72×)  → small hints, legends, dim captions  (was 10/11/.small())
// body     (1.00×)  → default labels                       (matches TextStyle::Body)
// subtitle (1.15×)  → card titles, player names in cards   (was 15/16)
// title    (1.43×)  → section headings                     (matches .heading())

pub fn size_caption(cfg: &AppConfig) -> f32 {
    cfg.font_size * 0.72
}

pub fn size_body(cfg: &AppConfig) -> f32 {
    cfg.font_size
}

pub fn size_subtitle(cfg: &AppConfig) -> f32 {
    cfg.font_size * 1.15
}

pub fn size_title(cfg: &AppConfig) -> f32 {
    cfg.font_size * 1.43
}

// ── Shape ────────────────────────────────────────────────────────────

pub const RADIUS_CARD: f32 = 6.0;
pub const RADIUS_CHIP: f32 = 10.0;
pub const STROKE_HAIRLINE: f32 = 0.5;
pub const STROKE_ACCENT: f32 = 1.0;

// egui 0.32+ expects `i8` for inner margin components.
pub const CARD_INNER_MX: i8 = 12;
pub const CARD_INNER_MY: i8 = 10;
pub const CHIP_INNER_MX: i8 = 8;
pub const CHIP_INNER_MY: i8 = 3;

// ── Top / status bar heights ─────────────────────────────────────────

pub const TOPBAR_HEIGHT: f32 = 54.0;
pub const STATUSBAR_HEIGHT: f32 = 22.0;

// ── Insights tab — responsive masonry grid ───────────────────────────

pub const INSIGHT_CARD_MIN_W: f32 = 360.0;
pub const INSIGHT_COL_GAP: f32 = SPACE_M;
pub const INSIGHT_MAX_COLS: usize = 3;
