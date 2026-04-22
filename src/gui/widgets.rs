// Shared GUI widgets.
//
// Every tab had its own variant of "small pill button", "info card",
// "player name + YOU chip", and "label/value row". Centralising keeps
// them visually coherent and lets a token tweak propagate automatically.
//
// All helpers are plain free functions — no state, no allocations
// beyond the string the caller already owns (egui's `RichText` takes
// `impl Into<WidgetText>` so stringy callers don't pay double).

use egui::{
    Align, Color32, FontId, InnerResponse, Layout, Margin, Response, RichText, Sense, Stroke, Ui,
};

use crate::colors::{
    player_slot_color, player_slot_color_bright, race_color, ACCENT_DANGER, BORDER, CARD_FILL,
    LABEL_DIM, LABEL_STRONG, USER_CHIP_BG, USER_CHIP_FG,
};
use crate::config::AppConfig;
use crate::locale::{t, Language};
use crate::tokens::{
    size_body, size_caption, size_subtitle, size_title, CARD_INNER_MX, CARD_INNER_MY,
    CHIP_MIN_HEIGHT, RADIUS_CARD, RADIUS_CHIP, SHADOW_CARD, SPACE_S, SPACE_XS, STROKE_HAIRLINE,
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

// ── Removable chip ───────────────────────────────────────────────────
//
// Pill com label + ícone × à direita. Clique em qualquer parte retorna
// `clicked()`. O × é desenhado com `line_segment` (não depende da fonte
// conter o glyph ✕/✖, que falha em algumas famílias). No hover, o fundo
// ganha um tint de perigo e o × fica vermelho vivo para sinalizar que a
// ação é destrutiva (remover o filtro).

pub fn removable_chip(ui: &mut Ui, label: &str, cfg: &AppConfig) -> Response {
    let font = FontId::proportional(size_caption(cfg));
    let text_color = Color32::WHITE;

    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), text_color);

    let pad_x = 10.0;
    let gap = 6.0;
    let icon_size: f32 = 9.0;
    let height = CHIP_MIN_HEIGHT.max(galley.size().y + 6.0);
    let width = pad_x + galley.size().x + gap + icon_size + pad_x;

    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), Sense::click());

    let hovered = response.hovered();

    // Background
    let base_fill = Color32::from_rgb(48, 56, 72);
    let hover_fill = Color32::from_rgb(78, 50, 56);
    let fill = if hovered { hover_fill } else { base_fill };
    let stroke_col = if hovered {
        ACCENT_DANGER
    } else {
        Color32::from_gray(70)
    };
    ui.painter().rect(
        rect,
        RADIUS_CHIP,
        fill,
        Stroke::new(1.0, stroke_col),
        egui::StrokeKind::Inside,
    );

    // Label
    let label_y = rect.center().y - galley.size().y / 2.0;
    let label_pos = egui::pos2(rect.left() + pad_x, label_y);
    ui.painter().galley(label_pos, galley, text_color);

    // × icon (desenhado com duas linhas cruzadas)
    let icon_center = egui::pos2(rect.right() - pad_x - icon_size / 2.0, rect.center().y);
    let arm = icon_size * 0.5;
    let icon_color = if hovered {
        ACCENT_DANGER
    } else {
        Color32::from_gray(170)
    };
    let stroke = Stroke::new(1.6, icon_color);
    ui.painter().line_segment(
        [
            icon_center + egui::vec2(-arm, -arm),
            icon_center + egui::vec2(arm, arm),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            icon_center + egui::vec2(-arm, arm),
            icon_center + egui::vec2(arm, -arm),
        ],
        stroke,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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

// ── Player identity ──────────────────────────────────────────────────
//
// The canonical "race · name · YOU?" composition shared by the analysis
// topbar, timeline side panel and build-order card header. Name colour
// is always `player_slot_color_bright(idx)` and race is always rendered
// as a `race_badge` — keeps the visual language consistent across the
// GUI while the caller varies density (compact vs normal) per context.

/// Vertical rhythm for the player identity block. `Compact` suits
/// single-line bars (topbar, chat); `Normal` is the default card
/// header size used by side panels and the build-order column.
#[derive(Clone, Copy, Debug)]
pub enum NameDensity {
    Compact,
    Normal,
}

impl NameDensity {
    fn name_size(self, cfg: &AppConfig) -> f32 {
        match self {
            Self::Compact => size_caption(cfg),
            Self::Normal => size_subtitle(cfg),
        }
    }

    /// Race icon side length. SVG logos have less optical weight than
    /// glyphs at the same pixel size, so we render them one typographic
    /// tier above the surrounding name — otherwise the badge reads as
    /// tiny in dense bars. In Normal density subtitle is already the
    /// name size, so we bump to title to keep the one-tier gap.
    fn icon_size(self, cfg: &AppConfig) -> f32 {
        match self {
            Self::Compact => size_body(cfg),
            Self::Normal => size_title(cfg),
        }
    }
}

/// Race icon rendered inline with the player name. For the four known
/// races (T/P/Z/R) we render the native SC2 logo as an SVG — these are
/// instantly recognisable and colour-coded on their own, so we drop the
/// pill background to avoid competing with the logo's own shape.
/// Unknown races keep the text-pill fallback so the badge never
/// disappears silently when parsing drifts.
///
/// The icon is sized one typographic tier above the name (see
/// `NameDensity::icon_size`) so it reads with the same optical weight
/// as the glyphs next to it. Caller places it inline inside an
/// `ui.horizontal`.
pub fn race_badge(ui: &mut Ui, race: &str, density: NameDensity, cfg: &AppConfig) -> Response {
    let letter = crate::utils::race_letter(race);
    let side = density.icon_size(cfg);
    let icon_size = egui::vec2(side, side);
    match letter {
        'T' => ui.add(
            egui::Image::new(egui::include_image!("../../assets/race/terran.svg"))
                .fit_to_exact_size(icon_size),
        ),
        'P' => ui.add(
            egui::Image::new(egui::include_image!("../../assets/race/protoss.svg"))
                .fit_to_exact_size(icon_size),
        ),
        'Z' => ui.add(
            egui::Image::new(egui::include_image!("../../assets/race/zerg.svg"))
                .fit_to_exact_size(icon_size),
        ),
        'R' => ui.add(
            egui::Image::new(egui::include_image!("../../assets/race/random.svg"))
                .fit_to_exact_size(icon_size),
        ),
        _ => race_badge_text_fallback(ui, letter, race, cfg),
    }
}

/// Text-pill fallback for races we couldn't classify. Keeps the old
/// coloured pill so unknown values remain visible rather than silently
/// taking zero space.
fn race_badge_text_fallback(ui: &mut Ui, letter: char, race: &str, cfg: &AppConfig) -> Response {
    let fill = race_color(race);
    let font = FontId::monospace(size_caption(cfg));
    let galley =
        ui.painter().layout_no_wrap(letter.to_string(), font, Color32::WHITE);

    let pad_x = 6.0;
    let pad_y = 2.0;
    let size = egui::vec2(
        galley.size().x + pad_x * 2.0,
        galley.size().y + pad_y * 2.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
    let text_pos = egui::pos2(
        rect.center().x - galley.size().x / 2.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text_pos, galley, Color32::WHITE);
    response
}

/// Canonical YOU chip. Keeps foreground/background harmonised with the
/// selection palette instead of the slot colour, so it never fights the
/// player name for attention.
pub fn you_chip_label(cfg: &AppConfig, lang: Language) -> RichText {
    RichText::new(format!(" {} ", t("common.you_chip", lang).trim()))
        .size(size_caption(cfg))
        .strong()
        .color(USER_CHIP_FG)
        .background_color(USER_CHIP_BG)
}

/// Full "race · name · YOU?" row. Must be called inside an
/// `ui.horizontal` (or an explicit `left_to_right` layout when the
/// parent is RTL). The caller owns the surrounding spacing.
pub fn player_identity(
    ui: &mut Ui,
    name: &str,
    race: &str,
    player_idx: usize,
    is_user: bool,
    density: NameDensity,
    cfg: &AppConfig,
    lang: Language,
) {
    race_badge(ui, race, density, cfg);
    ui.label(
        RichText::new(name)
            .size(density.name_size(cfg))
            .strong()
            .color(player_slot_color_bright(player_idx)),
    );
    if is_user {
        ui.label(you_chip_label(cfg, lang));
    }
}

// ── Player POV pill ──────────────────────────────────────────────────
//
// Clickable pill wrapping a `player_identity(Compact)`. Used by the
// Insights tab to pick a point-of-view — replaces a plain ComboBox so
// both players are visible at a glance and the selection surfaces in
// the shared visual language (race badge + bright slot colour).
//
// Selected: filled with a subtle tint of the slot colour + slot stroke.
// Unselected: neutral grey fill + hairline border.

pub fn player_pov_pill(
    ui: &mut Ui,
    name: &str,
    race: &str,
    player_idx: usize,
    is_user: bool,
    selected: bool,
    cfg: &AppConfig,
    lang: Language,
) -> Response {
    let slot = player_slot_color(player_idx);
    let (fill, stroke) = if selected {
        let tint = Color32::from_rgb(
            40 + slot.r() / 5,
            40 + slot.g() / 5,
            40 + slot.b() / 5,
        );
        (tint, Stroke::new(1.5, slot))
    } else {
        (Color32::from_gray(36), Stroke::new(STROKE_HAIRLINE, BORDER))
    };

    let inner = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(RADIUS_CHIP)
        .inner_margin(Margin::symmetric(crate::tokens::SPACE_M as i8, SPACE_XS as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_S;
                player_identity(
                    ui,
                    name,
                    race,
                    player_idx,
                    is_user,
                    NameDensity::Compact,
                    cfg,
                    lang,
                );
            });
        });

    ui.interact(inner.response.rect, inner.response.id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
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

// ── Copy-to-clipboard glyph ─────────────────────────────────────────
//
// Single source of truth for every "copy to clipboard" affordance in
// the app. Rendering an SVG (over the ad-hoc 📋 emoji we used before)
// keeps the glyph crisp at any DPI and identical across host font
// stacks — the emoji fell back to very different shapes on Linux vs
// Windows. The SVG is white so callers can `.tint()` freely.

fn copy_icon_image(side: f32) -> egui::Image<'static> {
    egui::Image::new(egui::include_image!("../../assets/icons/copy.svg"))
        .fit_to_exact_size(egui::vec2(side, side))
        .tint(Color32::from_gray(210))
}

/// Icon-only copy button sized to the surrounding body text. Mirrors
/// `icon_button` but swaps the glyph for the shared SVG.
pub fn copy_icon_button(ui: &mut Ui, hover: &str) -> Response {
    let side = ui.text_style_height(&egui::TextStyle::Body);
    ui.add(egui::Button::image(copy_icon_image(side)))
        .on_hover_text(hover)
}

/// Copy button with an inline label. For surfaces where the explicit
/// "Copy to clipboard" text matters (e.g., modals).
pub fn copy_labeled_button(ui: &mut Ui, label: &str) -> Response {
    let side = ui.text_style_height(&egui::TextStyle::Body);
    ui.add(egui::Button::image_and_text(copy_icon_image(side), label))
}

