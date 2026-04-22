// Renderização de uma entrada individual da biblioteca + helpers de
// metadados usados para filtragem, ordenação e exibição.
//
// O row é uma *lista enxuta*: nomes dos jogadores tingidos pela cor da
// raça, nome do mapa e (quando o usuário tem nickname registrado) o
// resultado WIN/LOSS. Detalhes ricos (datetime, MMR, opening, minimapa,
// versão) ficam no card lateral alimentado por `library_selection`.
//
// Interação:
// - Clique simples → `RowOutcome::Select` (alimenta o card)
// - Duplo clique  → `RowOutcome::Load`   (abre na aba Analysis)
// - Botão direito → menu "encontrar relacionados"

use egui::{Color32, RichText, Sense, Ui};

use crate::colors::{race_color, ACCENT_DANGER, ACCENT_SUCCESS, LABEL_DIM, LABEL_SOFT};
use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::tokens::{size_caption, SPACE_M, SPACE_S};

use super::types::{LibraryEntry, MetaState, ParsedMeta, PlayerMeta};

/// O que aconteceu ao interagir com uma linha da biblioteca.
///
/// `Select` vem do clique simples — apenas alimenta o card lateral.
/// `Load` vem do duplo clique (ou do botão "Abrir análise" no card) —
/// carrega o replay e troca para a tela `Analysis`.
/// `ApplyRelated` vem do menu de contexto (clique direito).
pub(super) enum RowOutcome {
    None,
    Select,
    Load,
    ApplyRelated(RelatedFilter),
}

/// Dimensão escolhida pelo usuário no menu "encontrar relacionados".
pub(super) enum RelatedFilter {
    Opponent(String),
    Matchup(String),
    Map(String),
    Opening(String),
}

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

/// Altura compacta da linha — uma única strip com nomes + mapa + W/L.
/// O conteúdo cabe em uma label `Body`; somamos o chrome do frame.
pub(super) fn row_height(ui: &Ui) -> f32 {
    use egui::TextStyle;
    let body = ui.text_style_height(&TextStyle::Body);
    body + FRAME_CHROME_V
}

const FRAME_CHROME_V: f32 = 14.0;

/// Cor da borda esquerda baseada na raça.
fn race_border_color(race: &str) -> Color32 {
    race_color(race)
}

/// Largura fixa da zona direita (W/L). Garante que a coluna do resultado
/// fique alinhada entre linhas, independentemente do comprimento dos
/// nomes ou do mapa.
const RIGHT_ZONE_W: f32 = 56.0;

// Paleta dos estados visuais do row.
const SELECTED_FILL: Color32 = Color32::from_rgb(32, 44, 60);
const SELECTED_STROKE: Color32 = Color32::from_rgb(110, 150, 200);
const CURRENT_STROKE: Color32 = Color32::from_rgb(80, 110, 150);

pub(super) fn entry_row(
    ui: &mut Ui,
    entry: &LibraryEntry,
    is_current: bool,
    is_selected: bool,
    config: &AppConfig,
    row_h: f32,
) -> RowOutcome {
    let lang = config.language;
    let loadable = entry.meta.is_loadable();

    let fill = if is_selected {
        SELECTED_FILL
    } else if matches!(entry.meta, MetaState::Unsupported(_)) {
        Color32::from_gray(22)
    } else {
        Color32::from_gray(28)
    };
    let stroke = if is_selected {
        egui::Stroke::new(1.0, SELECTED_STROKE)
    } else if is_current {
        egui::Stroke::new(1.0, CURRENT_STROKE)
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
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(content_h);

            match &entry.meta {
                MetaState::Parsed(meta) => {
                    render_parsed(ui, meta, config, content_h);
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

    // Pinta a borda esquerda colorida pela raça do usuário (ou P1).
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

    let row_resp = inner.response.interact(Sense::click());
    let mut outcome = if loadable && row_resp.double_clicked() {
        RowOutcome::Load
    } else if loadable && row_resp.clicked() {
        RowOutcome::Select
    } else {
        RowOutcome::None
    };

    // Menu de contexto: "encontrar relacionados" (1v1 parseado).
    if let MetaState::Parsed(meta) = &entry.meta {
        if meta.players.len() == 2 {
            row_resp.context_menu(|ui| {
                let user_idx = find_user_index(meta, config);
                ui.label(
                    RichText::new(t("library.related.menu.title", lang))
                        .small()
                        .color(LABEL_DIM),
                );

                if let Some(ui_idx) = user_idx {
                    let opp_name = meta.players[1 - ui_idx].name.clone();
                    if ui
                        .button(tf(
                            "library.related.menu.vs_opponent",
                            lang,
                            &[("name", &opp_name)],
                        ))
                        .clicked()
                    {
                        outcome = RowOutcome::ApplyRelated(RelatedFilter::Opponent(opp_name));
                        ui.close();
                    }
                }

                let mc = matchup_code(meta, config);
                if !mc.is_empty()
                    && ui
                        .button(tf(
                            "library.related.menu.same_matchup",
                            lang,
                            &[("code", &mc)],
                        ))
                        .clicked()
                {
                    outcome = RowOutcome::ApplyRelated(RelatedFilter::Matchup(mc));
                    ui.close();
                }

                if ui
                    .button(tf(
                        "library.related.menu.same_map",
                        lang,
                        &[("map", &meta.map)],
                    ))
                    .clicked()
                {
                    outcome = RowOutcome::ApplyRelated(RelatedFilter::Map(meta.map.clone()));
                    ui.close();
                }

                if let Some(op) = find_user_player(meta, config).and_then(|p| p.opening.clone()) {
                    if ui
                        .button(tf(
                            "library.related.menu.same_opening",
                            lang,
                            &[("opening", &op)],
                        ))
                        .clicked()
                    {
                        outcome = RowOutcome::ApplyRelated(RelatedFilter::Opening(op));
                        ui.close();
                    }
                }
            });
        }
    }

    outcome
}

/// Layout do row 1v1 parseado: nomes coloridos por raça (usuário em
/// destaque), nome do mapa em tom dim e tag W/L à direita quando
/// aplicável. Tudo em uma única strip horizontal.
fn render_parsed(ui: &mut Ui, meta: &ParsedMeta, config: &AppConfig, content_h: f32) {
    let user_idx = find_user_index(meta, config);

    let total_w = ui.available_width();
    let (strip_rect, _) = ui.allocate_exact_size(
        egui::vec2(total_w, content_h),
        Sense::hover(),
    );

    let right_rect = egui::Rect::from_min_size(
        strip_rect.right_top() - egui::vec2(RIGHT_ZONE_W, 0.0),
        egui::vec2(RIGHT_ZONE_W, content_h),
    );
    let left_rect = egui::Rect::from_min_max(
        strip_rect.left_top(),
        egui::pos2(strip_rect.right() - RIGHT_ZONE_W, strip_rect.bottom()),
    );

    // ── LEFT: nomes coloridos · mapa ─────────────────────────
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.shrink_clip_rect(left_rect);
            ui.spacing_mut().item_spacing.x = SPACE_S;

            if meta.players.len() == 2 {
                player_label(ui, &meta.players[0], user_idx == Some(0));
                ui.label(RichText::new(t("common.vs", config.language)).color(LABEL_DIM));
                player_label(ui, &meta.players[1], user_idx == Some(1));
            } else {
                for (i, p) in meta.players.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new(t("common.vs", config.language)).color(LABEL_DIM));
                    }
                    player_label(ui, p, user_idx == Some(i));
                }
            }

            ui.add_space(SPACE_M);
            ui.label(
                RichText::new(format!("· {}", meta.map))
                    .color(LABEL_SOFT),
            );
        },
    );

    // ── RIGHT: WIN/LOSS (apenas com user identificado) ──────
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            let result = user_idx
                .and_then(|i| meta.players.get(i))
                .map(|p| p.result.as_str());
            match result {
                Some("Win") => {
                    ui.label(
                        RichText::new(t("library.outcome.win", config.language))
                            .size(size_caption(config))
                            .strong()
                            .color(ACCENT_SUCCESS),
                    );
                }
                Some("Loss") => {
                    ui.label(
                        RichText::new(t("library.outcome.loss", config.language))
                            .size(size_caption(config))
                            .strong()
                            .color(ACCENT_DANGER),
                    );
                }
                _ => {}
            }
        },
    );
}

/// "T firebat" com o nome em branco e a letra da raça colorida. O
/// jogador identificado como o usuário recebe peso `strong` para
/// destacá-lo de relance na lista.
fn player_label(ui: &mut Ui, p: &PlayerMeta, is_user: bool) {
    let race_letter_ch = race_letter(&p.race);
    ui.label(
        RichText::new(race_letter_ch.to_string())
            .strong()
            .monospace()
            .color(race_color(&p.race)),
    );
    let name = RichText::new(&p.name).color(Color32::WHITE);
    let name = if is_user { name.strong() } else { name };
    ui.label(name);
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
