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
pub const RADIUS_BUTTON: f32 = 4.0;
pub const RADIUS_WINDOW: f32 = 8.0;
pub const STROKE_HAIRLINE: f32 = 0.5;
pub const STROKE_ACCENT: f32 = 1.0;

/// Minimum chip height. Padronizes pill-shaped toggles so labels of
/// different lengths don't make adjacent chips jump in height.
pub const CHIP_MIN_HEIGHT: f32 = 22.0;

// ── Shadows ──────────────────────────────────────────────────────────
//
// egui packs Shadow into 16 bits: offset is `[i8; 2]` and blur/spread
// are `u8`. Keep values small — large shadows on every card kill the
// "flat dark" aesthetic we're aiming for.

/// Subtle shadow used by raised cards. One pixel down + soft blur is
/// enough to read as "above the surface" without competing with content.
pub const SHADOW_CARD: egui::epaint::Shadow = egui::epaint::Shadow {
    offset: [0, 2],
    blur: 8,
    spread: 0,
    color: egui::Color32::from_black_alpha(55),
};

/// Window-level shadow for modals / popups. Slightly more pronounced
/// than `SHADOW_CARD` so floating surfaces clearly detach from the
/// background.
pub const SHADOW_WINDOW: egui::epaint::Shadow = egui::epaint::Shadow {
    offset: [0, 6],
    blur: 16,
    spread: 0,
    color: egui::Color32::from_black_alpha(72),
};

/// Popover shadow — between card and window.
pub const SHADOW_POPUP: egui::epaint::Shadow = egui::epaint::Shadow {
    offset: [0, 4],
    blur: 10,
    spread: 0,
    color: egui::Color32::from_black_alpha(60),
};

// egui 0.32+ expects `i8` for inner margin components.
pub const CARD_INNER_MX: i8 = 12;
pub const CARD_INNER_MY: i8 = 10;
pub const CHIP_INNER_MX: i8 = 8;
pub const CHIP_INNER_MY: i8 = 3;

/// Library list rows align horizontally with cards but use compressed
/// vertical padding so a virtualized list of dozens of rows stays scannable.
pub const ROW_INNER_MX: i8 = CARD_INNER_MX;
pub const ROW_INNER_MY: i8 = CARD_INNER_MY / 2;

// ── Control sizing ────────────────────────────────────────────────────
//
// Three steps for buttons / inputs / transport controls. Keep these in
// sync with `CHIP_MIN_HEIGHT` so a chip and a small button share a
// baseline.
pub const CONTROL_HEIGHT_S: f32 = CHIP_MIN_HEIGHT;  // 22
pub const CONTROL_HEIGHT_M: f32 = 28.0;             // search inputs, toolbar buttons
pub const CONTROL_HEIGHT_L: f32 = 36.0;             // transport / play-pause

// ── Library entry row geometry ───────────────────────────────────────
//
// Frame chrome height around the row's body text. Add to a row's text
// height to compute the total slot height.
pub const FRAME_CHROME_V: f32 = 14.0;

/// Width of the leading checkbox column in library entry rows. Sized
/// for comfortable click target + visual margin.
pub const CHECKBOX_COL_W: f32 = 22.0;

/// Right-side metadata zone width (length, race chips, win/loss badge).
/// Hardcoded so the zone aligns vertically across rows of varying middle
/// content.
pub const ROW_RIGHT_ZONE_W: f32 = 110.0;

// ── Top / status bar heights ─────────────────────────────────────────
//
// Heights derive from typography so the chrome scales with the user's
// `font_size` slider. Without this, tall fonts clip against fixed bars.

/// Topbar fits a title-sized line plus generous breathing room.
pub fn topbar_height(cfg: &AppConfig) -> f32 {
    size_title(cfg) + 2.0 * SPACE_L
}

/// Statusbar fits a body-sized line with tight padding.
pub fn statusbar_height(cfg: &AppConfig) -> f32 {
    size_body(cfg) + 2.0 * SPACE_S
}

// ── Iconography — phosphor glyphs sized against the type scale ───────
//
// Phosphor renders inline as font glyphs. The 1.1× factor compensates
// for the optical weight difference between glyphs and surrounding text.

pub fn icon_size_caption(cfg: &AppConfig) -> f32 {
    size_caption(cfg) * 1.1
}
pub fn icon_size_body(cfg: &AppConfig) -> f32 {
    size_body(cfg) * 1.1
}
pub fn icon_size_title(cfg: &AppConfig) -> f32 {
    size_title(cfg) * 1.1
}

// ── Insights tab — responsive masonry grid ───────────────────────────

pub const INSIGHT_CARD_MIN_W: f32 = 360.0;
pub const INSIGHT_COL_GAP: f32 = SPACE_M;
pub const INSIGHT_MAX_COLS: usize = 3;
