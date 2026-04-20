// Renderização de uma entrada individual da biblioteca + helpers de
// metadados usados para filtragem, ordenação e exibição.

use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Ui};

use crate::colors::{
    race_color, ACCENT_DANGER, ACCENT_SUCCESS, LABEL_DIM, LABEL_SOFT, LABEL_STRONG, RACE_PROTOSS,
    RACE_TERRAN, RACE_ZERG,
};
use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::tokens::{size_body, size_caption, SPACE_S, SPACE_XS};

use super::types::{LibraryEntry, MetaState, ParsedMeta, PlayerMeta};

// ── Helpers de filtro/sort ────────────────────────────────────────────

pub(super) fn find_user_player<'a>(meta: &'a ParsedMeta, config: &AppConfig) -> Option<&'a PlayerMeta> {
    if config.user_nicknames.is_empty() {
        return None;
    }
    meta.players.iter().find(|p| {
        config
            .user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&p.name))
    })
}

pub(super) fn find_user_index(meta: &ParsedMeta, config: &AppConfig) -> Option<usize> {
    if config.user_nicknames.is_empty() {
        return None;
    }
    meta.players.iter().position(|p| {
        config
            .user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&p.name))
    })
}

pub(super) fn matchup_code(meta: &ParsedMeta, config: &AppConfig) -> String {
    if meta.players.len() != 2 {
        return String::new();
    }
    let ui = find_user_index(meta, config);
    let (first, second) = match ui {
        Some(0) => (0, 1),
        Some(1) => (1, 0),
        _ => (0, 1),
    };
    format!(
        "{}v{}",
        race_letter(&meta.players[first].race),
        race_letter(&meta.players[second].race)
    )
}

pub(super) fn get_duration(entry: &LibraryEntry) -> u32 {
    match &entry.meta {
        MetaState::Parsed(m) => m.duration_seconds,
        _ => 0,
    }
}

pub(super) fn get_user_mmr(entry: &LibraryEntry, config: &AppConfig) -> i32 {
    match &entry.meta {
        MetaState::Parsed(m) => find_user_player(m, config)
            .and_then(|p| p.mmr)
            .unwrap_or(0),
        _ => 0,
    }
}

pub(super) fn get_map(entry: &LibraryEntry) -> &str {
    match &entry.meta {
        MetaState::Parsed(m) => &m.map,
        _ => "",
    }
}

// ── UI components ────────────────────────────────────────────────────

/// Altura de cada linha da lista virtualizada.
///
/// Composição da zona esquerda (fonte única de verdade para manter a
/// lista em sincronia com o render):
/// - 1× Body (título "vs")
/// - 3× Small (🗺/⏱, MMR, opening label)
/// - 3× gap entre elas
/// Qualquer mudança aqui precisa casar com `render_parsed`.
pub(super) fn row_height(ui: &Ui) -> f32 {
    use egui::TextStyle;
    let body = ui.text_style_height(&TextStyle::Body);
    let small = ui.text_style_height(&TextStyle::Small);
    let gap = ui.spacing().item_spacing.y;
    body + small * 3.0 + gap * 3.0 + FRAME_CHROME_V
}

const FRAME_CHROME_V: f32 = 13.0;

// Race colours — distinct from the P1/P2 slot palette (red/blue) so
// "race" and "player" never collide visually. Single source of truth
// in `crate::colors` — this module re-exports under the Library's
// legacy names for grep-friendliness at call sites.
pub(super) const RACE_COLOR_TERRAN: Color32 = RACE_TERRAN;
pub(super) const RACE_COLOR_PROTOSS: Color32 = RACE_PROTOSS;
pub(super) const RACE_COLOR_ZERG: Color32 = RACE_ZERG;

/// Cor da borda esquerda baseada na raça.
fn race_border_color(race: &str) -> Color32 {
    race_color(race)
}

/// Zona central (fixa) do entry row. Hospeda W/L dot, badge de matchup
/// e ΔMMR. Largura escolhida para caber o badge (~64px) e paddings sem
/// quebrar em fontes maiores do slider de tamanho.
const CENTER_ZONE_W: f32 = 220.0;
/// Zona direita (fixa): hora, data e botão "abrir".
const RIGHT_ZONE_W: f32 = 140.0;

/// Paleta da linha atualmente aberta no painel de análise. Azul aço
/// apagado — convenção de "item ativo" que não compete com o verde
/// de WIN nem com o vermelho de LOSS.
const SELECTED_FILL: Color32 = Color32::from_rgb(32, 44, 60);
const SELECTED_STROKE: Color32 = Color32::from_rgb(110, 150, 200);
const SELECTED_LABEL: Color32 = Color32::from_rgb(170, 200, 235);

pub(super) fn entry_row(
    ui: &mut Ui,
    entry: &LibraryEntry,
    is_current: bool,
    config: &AppConfig,
    row_h: f32,
) -> bool {
    let lang = config.language;
    let loadable = entry.meta.is_loadable();
    // "Selected" = azul aço apagado, padrão convencional de item ativo.
    // Verde ficava reservado para o sentido WIN (ACCENT_SUCCESS) e
    // saturava a linha quando o usuário clicava numa vitória.
    let fill = if is_current {
        SELECTED_FILL
    } else if matches!(entry.meta, MetaState::Unsupported(_)) {
        Color32::from_gray(22)
    } else {
        Color32::from_gray(28)
    };
    let stroke = if is_current {
        egui::Stroke::new(1.0, SELECTED_STROKE)
    } else if matches!(entry.meta, MetaState::Unsupported(_)) {
        egui::Stroke::new(0.5, Color32::from_gray(50))
    } else {
        egui::Stroke::new(0.5, Color32::from_gray(60))
    };

    let content_h = (row_h - FRAME_CHROME_V).max(0.0);

    let inner = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(content_h);

            match &entry.meta {
                MetaState::Parsed(meta) => {
                    render_parsed(ui, meta, config, is_current, content_h);
                }
                MetaState::Pending => {
                    ui.label(RichText::new(&entry.filename).monospace());
                    ui.small(RichText::new(t("library.entry.parsing", lang)).italics());
                }
                MetaState::Unsupported(reason) => {
                    ui.label(
                        RichText::new(&entry.filename)
                            .monospace()
                            .color(Color32::from_gray(140)),
                    );
                    ui.small(
                        RichText::new(tf(
                            "library.entry.unsupported",
                            lang,
                            &[("reason", reason)],
                        ))
                        .color(Color32::from_rgb(210, 170, 60))
                        .italics(),
                    );
                }
                MetaState::Failed(err) => {
                    ui.label(RichText::new(&entry.filename).monospace());
                    ui.small(
                        RichText::new(tf("library.entry.failed", lang, &[("err", err)]))
                            .color(Color32::LIGHT_RED)
                            .italics(),
                    );
                }
            }
        });

    // Pinta a borda esquerda colorida por raça (sobre o frame já renderizado).
    if let MetaState::Parsed(meta) = &entry.meta {
        let user_idx = find_user_index(meta, config).unwrap_or(0);
        let border_color = race_border_color(&meta.players[user_idx].race);
        let rect = inner.response.rect;
        let border_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 3.5, rect.bottom()),
        );
        ui.painter().rect_filled(border_rect, 4.0, border_color);
    }

    loadable && inner.response.interact(Sense::click()).clicked()
}

/// Internal split of `entry_row` for `MetaState::Parsed` entries — the
/// three-zone layout: left (flex, match summary), center (fixed, outcome
/// e matchup badge e ΔMMR), right (fixed, time e date).
fn render_parsed(
    ui: &mut Ui,
    meta: &ParsedMeta,
    config: &AppConfig,
    is_current: bool,
    content_h: f32,
) {
    let user_idx = find_user_index(meta, config);
    let has_user = user_idx.is_some();
    let mc = matchup_code(meta, config);

    let vs_label = if meta.players.len() == 2 {
        format!("{} vs {}", meta.players[0].name, meta.players[1].name)
    } else {
        meta.players
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(" vs ")
    };

    let dur = format!(
        "{:02}:{:02}",
        meta.duration_seconds / 60,
        meta.duration_seconds % 60
    );

    let mmr_line = mmr_line_text(meta, user_idx);
    let (short_date, time_part) = split_datetime(&meta.datetime);

    // Posicionamento por rects absolutos: `ui.horizontal` + `allocate_ui_with_layout`
    // deixavam cada zona variar de X quando o conteúdo da esquerda era largo
    // (o `allocate_*` cresce com o conteúdo e empurra os siblings). Com rects
    // explícitos, as três zonas ficam pixel-perfect alinhadas entre linhas.
    //
    // Âncora da zona central: meio real da strip — não o "meio do que sobra
    // depois da esquerda". Com a esquerda sendo flex, ancorar via offset
    // deixava a badge colada na direita em janelas largas.
    let total_w = ui.available_width();
    let (strip_rect, _) = ui.allocate_exact_size(
        egui::vec2(total_w, content_h),
        Sense::hover(),
    );

    let center_x_start =
        ((total_w - CENTER_ZONE_W) / 2.0).clamp(180.0, total_w - CENTER_ZONE_W - RIGHT_ZONE_W);

    let left_rect = egui::Rect::from_min_size(
        strip_rect.left_top(),
        egui::vec2(center_x_start, content_h),
    );
    let center_rect = egui::Rect::from_min_size(
        strip_rect.left_top() + egui::vec2(center_x_start, 0.0),
        egui::vec2(CENTER_ZONE_W, content_h),
    );
    let right_rect = egui::Rect::from_min_size(
        strip_rect.right_top() - egui::vec2(RIGHT_ZONE_W, 0.0),
        egui::vec2(RIGHT_ZONE_W, content_h),
    );

    // ── LEFT ZONE (flex) ─────────────────────────────────────
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            // `shrink_clip_rect` (intersect) — NOT `set_clip_rect` (replace).
            // `left_rect` is derived from `allocate_exact_size`, which doesn't
            // bounds-check against the parent ui's clip. For the last visible
            // row in a ScrollArea, `left_rect` can spill below the
            // ScrollArea's `content_clip_rect`. A `set_clip_rect(left_rect)`
            // call would REPLACE the narrower scroll clip with this broader
            // rect — letting the row's name/map/MMR text bleed into whatever
            // is rendered below the ScrollArea (in our case, the bottom
            // status bar). Intersecting preserves the scroll-area clip.
            ui.shrink_clip_rect(left_rect);
            ui.label(RichText::new(&vs_label).strong().color(if is_current {
                SELECTED_LABEL
            } else {
                Color32::WHITE
            }));
            ui.small(
                RichText::new(format!("🗺 {} • ⏱ {dur}", meta.map)).color(LABEL_DIM),
            );
            let mmr_color = if has_user { LABEL_SOFT } else { LABEL_DIM };
            ui.small(RichText::new(mmr_line).color(mmr_color));
            ui.small(RichText::new(opening_line_text(meta, user_idx)).color(LABEL_DIM));
        },
    );

    // ── CENTER ZONE (fixed) ──────────────────────────────────
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(center_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            draw_outcome_dot(ui, meta, user_idx, config);
            draw_matchup_badge(ui, meta, user_idx, &mc, config);
            draw_mmr_delta(ui, meta, user_idx, config);
        },
    );

    // ── RIGHT ZONE (fixed) ───────────────────────────────────
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(egui::Layout::top_down(egui::Align::Max)),
        |ui| {
            ui.label(
                RichText::new(&time_part)
                    .strong()
                    .size(size_body(config))
                    .color(LABEL_STRONG),
            );
            ui.small(RichText::new(&short_date).color(LABEL_DIM));
        },
    );
}

/// Linha compacta com a abertura de cada jogador:
///   "PlayerA: 14 Pool — Speedling · PlayerB: 1 Rax FE — Stim Timing"
/// Quando há usuário identificado, ele vem primeiro. Jogadores sem
/// `opening` (falha de extração) recebem "—" para manter o formato.
fn opening_line_text(meta: &ParsedMeta, user_idx: Option<usize>) -> String {
    if meta.players.len() != 2 {
        return meta
            .players
            .iter()
            .map(|p| format!("{}: {}", p.name, p.opening.as_deref().unwrap_or("—")))
            .collect::<Vec<_>>()
            .join(" · ");
    }
    let (first, second) = match user_idx {
        Some(0) => (0, 1),
        Some(1) => (1, 0),
        _ => (0, 1),
    };
    let a = &meta.players[first];
    let b = &meta.players[second];
    format!(
        "{}: {} · {}: {}",
        a.name,
        a.opening.as_deref().unwrap_or("—"),
        b.name,
        b.opening.as_deref().unwrap_or("—"),
    )
}

fn mmr_line_text(meta: &ParsedMeta, user_idx: Option<usize>) -> String {
    let mmrs: Vec<String> = meta
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let v = match p.mmr {
                Some(v) => v.to_string(),
                None => "—".into(),
            };
            if user_idx == Some(i) {
                format!("‹{v}›")
            } else {
                v
            }
        })
        .collect();
    format!("MMR {}", mmrs.join(" / "))
}

/// W/L outcome dot in the center zone. Hidden (empty spacer) when the
/// user isn't identified — keeps the row height constant across rows
/// to preserve virtualization.
fn draw_outcome_dot(ui: &mut Ui, meta: &ParsedMeta, user_idx: Option<usize>, config: &AppConfig) {
    let height = size_body(config);
    let result = user_idx.and_then(|i| meta.players.get(i)).map(|p| p.result.as_str());
    match result {
        Some("Win") => {
            ui.label(
                RichText::new("WIN")
                    .size(size_caption(config))
                    .strong()
                    .color(ACCENT_SUCCESS),
            );
        }
        Some("Loss") => {
            ui.label(
                RichText::new("LOSS")
                    .size(size_caption(config))
                    .strong()
                    .color(ACCENT_DANGER),
            );
        }
        _ => {
            // Blank line of same height preserves vertical layout.
            ui.allocate_exact_size(egui::vec2(1.0, height), Sense::hover());
        }
    }
}

/// Painted two-half matchup badge. Left half uses the user's race colour
/// (or player-0's race if no user), right half the opponent's. Centered
/// white text displays the matchup code (e.g. "TvZ").
fn draw_matchup_badge(
    ui: &mut Ui,
    meta: &ParsedMeta,
    user_idx: Option<usize>,
    code: &str,
    config: &AppConfig,
) {
    let badge_w = 70.0;
    let badge_h = size_body(config) + 8.0;
    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(badge_w, badge_h), Sense::hover());

    if meta.players.len() < 2 {
        ui.painter()
            .rect_filled(rect, 4.0, Color32::from_gray(40));
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            code,
            FontId::proportional(size_caption(config)),
            LABEL_STRONG,
        );
        return;
    }

    let (first_idx, second_idx) = match user_idx {
        Some(0) => (0, 1),
        Some(1) => (1, 0),
        _ => (0, 1),
    };
    let left_color = dim(race_color(&meta.players[first_idx].race));
    let right_color = dim(race_color(&meta.players[second_idx].race));
    let mid = rect.center().x;
    let left_rect = Rect::from_min_max(rect.left_top(), egui::pos2(mid, rect.bottom()));
    let right_rect = Rect::from_min_max(egui::pos2(mid, rect.top()), rect.right_bottom());

    ui.painter().rect_filled(
        left_rect,
        CornerRadius {
            nw: 4,
            sw: 4,
            ne: 0,
            se: 0,
        },
        left_color,
    );
    ui.painter().rect_filled(
        right_rect,
        CornerRadius {
            nw: 0,
            sw: 0,
            ne: 4,
            se: 4,
        },
        right_color,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        code,
        FontId::proportional(size_body(config)),
        Color32::WHITE,
    );
    ui.add_space(SPACE_XS);
}

/// ±ΔMMR line. Positive = green with '+', negative = red, zero = dim.
/// Hidden (blank spacer) when either MMR is missing or no user.
fn draw_mmr_delta(ui: &mut Ui, meta: &ParsedMeta, user_idx: Option<usize>, config: &AppConfig) {
    let height = size_caption(config);
    let delta: Option<(i32, Color32, String)> = (|| {
        let user_idx = user_idx?;
        if meta.players.len() < 2 {
            return None;
        }
        let user_mmr = meta.players.get(user_idx)?.mmr?;
        let opp_idx = 1 - user_idx;
        let opp_mmr = meta.players.get(opp_idx)?.mmr?;
        let d = user_mmr - opp_mmr;
        let (color, text) = if d > 0 {
            (ACCENT_SUCCESS, format!("Δ +{d}"))
        } else if d < 0 {
            (ACCENT_DANGER, format!("Δ {d}"))
        } else {
            (LABEL_DIM, "Δ 0".to_string())
        };
        Some((d, color, text))
    })();
    match delta {
        Some((_, color, text)) => {
            ui.label(RichText::new(text).size(size_caption(config)).color(color));
        }
        None => {
            ui.allocate_exact_size(egui::vec2(1.0, height + SPACE_S), Sense::hover());
        }
    }
}

/// Slightly dims a race colour so the white overlay text keeps contrast
/// on the matchup badge. The raw race palette is tuned for the left
/// accent stripe (a 3 px bar), which doesn't need text on top of it.
fn dim(c: Color32) -> Color32 {
    Color32::from_rgb(
        (c.r() as u16 * 7 / 10) as u8,
        (c.g() as u16 * 7 / 10) as u8,
        (c.b() as u16 * 7 / 10) as u8,
    )
}

pub(super) fn race_letter(race: &str) -> char {
    crate::utils::race_letter(race)
}

pub(super) fn split_datetime(dt: &str) -> (String, String) {
    // "2025-12-18T06:44:53" → ("2025-12-18", "06:44")
    if dt.len() >= 16 {
        let date = dt[..10].to_string();
        let time = dt[11..16].to_string();
        (date, time)
    } else if dt.len() >= 10 {
        (dt[..10].to_string(), String::new())
    } else {
        (dt.to_string(), String::new())
    }
}
