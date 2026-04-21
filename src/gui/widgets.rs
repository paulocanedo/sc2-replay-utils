// Shared GUI widgets.
//
// Every tab had its own variant of "small pill button", "info card",
// "player name + YOU chip", and "label/value row". Centralising keeps
// them visually coherent and lets a token tweak propagate automatically.
//
// All helpers are plain free functions — no state, no allocations
// beyond the string the caller already owns (egui's `RichText` takes
// `impl Into<WidgetText>` so stringy callers don't pay double).

use egui::{Align, Color32, InnerResponse, Layout, Margin, Response, RichText, Stroke, Ui};

use crate::colors::{
    player_slot_color, BORDER, CARD_FILL, LABEL_DIM, LABEL_STRONG, USER_CHIP_BG, USER_CHIP_FG,
};
use crate::config::AppConfig;
use crate::locale::{t, Language};
use crate::tokens::{
    size_caption, size_subtitle, CARD_INNER_MX, CARD_INNER_MY, CHIP_MIN_HEIGHT, RADIUS_CARD,
    RADIUS_CHIP, SHADOW_CARD, STROKE_HAIRLINE,
};

// ── Chip ─────────────────────────────────────────────────────────────
//
// Pill-shaped toggle button. When `selected`, uses an accent-tinted
// fill (falling back to a muted green if no accent is provided); when
// unselected, a neutral dark grey. Used for filters (Library),
// per-column toggles (Build Order), overlays (Timeline) and series
// toggles (Charts).

pub fn chip(ui: &mut Ui, label: &str, selected: bool, accent: Option<Color32>) -> Response {
    let fill = chip_fill(selected, accent);
    let text_color = if selected {
        Color32::WHITE
    } else {
        Color32::from_gray(160)
    };

    ui.add(
        egui::Button::new(RichText::new(label).color(text_color).small())
            .fill(fill)
            .corner_radius(RADIUS_CHIP)
            .min_size(egui::vec2(0.0, CHIP_MIN_HEIGHT)),
    )
}

/// Sugar: toggle a `bool` via a chip. Returns the `Response` so callers
/// can attach tooltips.
pub fn toggle_chip_bool(
    ui: &mut Ui,
    label: &str,
    flag: &mut bool,
    accent: Option<Color32>,
) -> Response {
    let resp = chip(ui, label, *flag, accent);
    if resp.clicked() {
        *flag = !*flag;
    }
    resp
}

fn chip_fill(selected: bool, accent: Option<Color32>) -> Color32 {
    if !selected {
        return Color32::from_gray(40);
    }
    match accent {
        None => Color32::from_rgb(55, 75, 55),
        Some(c) => Color32::from_rgb(
            (c.r() as u16 / 3) as u8 + 20,
            (c.g() as u16 / 3) as u8 + 20,
            (c.b() as u16 / 3) as u8 + 20,
        ),
    }
}

// ── Card ─────────────────────────────────────────────────────────────
//
// Raised surface with hairline border. If `accent` is supplied, paints
// a 3px left stripe in that colour (used for per-player cards to match
// their slot colour).

pub fn card<R>(
    ui: &mut Ui,
    accent: Option<Color32>,
    add: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let inner = egui::Frame::new()
        .fill(CARD_FILL)
        .stroke(Stroke::new(STROKE_HAIRLINE, BORDER))
        .corner_radius(RADIUS_CARD)
        .shadow(SHADOW_CARD)
        .inner_margin(Margin::symmetric(CARD_INNER_MX, CARD_INNER_MY))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });

    if let Some(color) = accent {
        let rect = inner.response.rect;
        let accent_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 3.0, rect.bottom()),
        );
        ui.painter().rect_filled(
            accent_rect,
            egui::CornerRadius {
                nw: RADIUS_CARD as u8,
                sw: RADIUS_CARD as u8,
                ne: 0,
                se: 0,
            },
            color,
        );
    }

    InnerResponse::new(inner.inner, inner.response)
}

// ── Player label ─────────────────────────────────────────────────────
//
// The "player name + optional YOU chip" composition. Renders inline
// inside the current layout so callers can wrap in `ui.horizontal`
// together with MMR / race metadata.

pub fn player_label(
    ui: &mut Ui,
    name: &str,
    player_idx: usize,
    is_you: bool,
    show_you_chip: bool,
    cfg: &AppConfig,
    lang: Language,
) {
    ui.label(
        RichText::new(name)
            .size(size_subtitle(cfg))
            .strong()
            .color(player_slot_color(player_idx)),
    );
    if is_you && show_you_chip {
        ui.label(
            RichText::new(t("common.you_chip", lang))
                .size(size_caption(cfg))
                .strong()
                .color(USER_CHIP_FG)
                .background_color(USER_CHIP_BG),
        );
    }
}

// ── Labeled value ────────────────────────────────────────────────────
//
// Two-column "key › value" row. Used inside Details popovers and any
// compact key/value surface.

pub fn labeled_value(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong().color(LABEL_STRONG));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new("›").color(Color32::from_gray(80)));
            ui.label(RichText::new(value).color(LABEL_DIM));
        });
    });
}

// ── Icon button ──────────────────────────────────────────────────────
//
// Small chromeless button suited for header affordances (back, reload,
// pick folder, rename…). `glyph` is typically a single emoji.

pub fn icon_button(ui: &mut Ui, glyph: &str, hover: &str) -> Response {
    ui.small_button(glyph).on_hover_text(hover)
}

