// Card lateral de detalhes da biblioteca. Aparece na borda direita do
// CentralPanel quando há `library_selection` definida (clique simples
// numa entry). Mostra metadados ricos que não cabem mais na lista
// simplificada — minimapa, datetime, duração, MMR + ΔMMR, opening por
// jogador, versão do replay — além do botão "Abrir análise" (atalho
// para o duplo-clique) e "×" para limpar a seleção.

#![allow(deprecated)]

use egui::{Color32, ColorImage, Pos2, Rect, RichText, ScrollArea, TextureOptions};

use crate::colors::{
    race_color, ACCENT_DANGER, ACCENT_SUCCESS, ACTIVE_FILL, BORDER, BORDER_STRONG, HOVER_FILL,
    LABEL_DIM, LABEL_SOFT, LABEL_STRONG, SURFACE_ALT, SURFACE_RAISED,
};
use crate::library::{LibraryAction, MetaState, ParsedMeta, PlayerMeta};
use crate::locale::{t, Language};
use crate::map_image::MapImage;
use crate::replay_state::format_date_short;
use crate::tokens::{
    size_body, size_caption, RADIUS_BUTTON, SPACE_M, SPACE_S, SPACE_XS,
};

use super::state::AppState;

/// Largura padrão do card. O `Panel::right` permite o usuário arrastar.
const DEFAULT_WIDTH: f32 = 340.0;
const MIN_WIDTH: f32 = 260.0;
const MAX_WIDTH: f32 = 480.0;
/// Lado da textura do minimapa dentro do card. Quadrado fixo — os
/// minimaps da Blizzard são todos cropados para conteúdo, então um
/// quadrado central encaixa em qualquer aspect razoável.
const MINIMAP_SIDE: f32 = 200.0;

impl AppState {
    /// Renderiza o card de detalhes lateral. **Sempre visível** — quando
    /// não há `library_selection`, mostra um placeholder convidando o
    /// usuário a clicar numa entry da lista. Devolve `LibraryAction` em
    /// dois casos: clique em "Abrir análise" (`Load`) e clique em "×"
    /// (`ClearSelection`); `None` nos demais frames.
    pub(super) fn show_library_detail_card(&mut self, ui: &mut egui::Ui) -> Option<LibraryAction> {
        let lang = self.config.language;
        let mut action: Option<LibraryAction> = None;

        // Resolve a seleção em uma struct `Resolved` ANTES de abrir o
        // painel — assim o closure não precisa pegar `&self.library` e
        // nós evitamos brigas de borrow com `&mut self.library_selection`
        // que o caller mantém vivo ao longo do método.
        let resolved = self.resolve_selection();

        egui::Panel::right("library_detail")
            .resizable(true)
            .default_width(DEFAULT_WIDTH)
            .min_width(MIN_WIDTH)
            .max_width(MAX_WIDTH)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE_ALT)
                    .inner_margin(egui::Margin::same(SPACE_M as i8)),
            )
            .show_inside(ui, |ui| {
                ScrollArea::vertical().show(ui, |ui| match resolved {
                    Resolved::Empty => detail_card_empty(ui, lang, &self.config),
                    Resolved::SelectionGone => {
                        // A entry sumiu (refresh, watcher, etc.) — devolve
                        // ClearSelection para o caller limpar o estado.
                        action = Some(LibraryAction::ClearSelection);
                        detail_card_empty(ui, lang, &self.config);
                    }
                    Resolved::Loaded { path, meta, minimap } => {
                        detail_card_filled(
                            ui,
                            &path,
                            &meta,
                            minimap.as_ref(),
                            lang,
                            &self.config,
                            &mut action,
                        );
                    }
                });
            });

        action
    }

    fn resolve_selection(&self) -> Resolved {
        let Some(path) = self.library_selection.clone() else {
            return Resolved::Empty;
        };
        let Some(entry) = self.library.entries.iter().find(|e| e.path == path) else {
            return Resolved::SelectionGone;
        };
        let MetaState::Parsed(meta) = &entry.meta else {
            return Resolved::SelectionGone;
        };
        let minimap = self
            .library_selection_minimap
            .as_ref()
            .filter(|_| self.library_selection_minimap_path.as_deref() == Some(&path))
            .map(|img| MapImage {
                width: img.width,
                height: img.height,
                rgba: img.rgba.clone(),
            });
        Resolved::Loaded {
            path,
            meta: meta.clone(),
            minimap,
        }
    }
}

/// Possíveis estados de resolução da seleção atual da biblioteca.
enum Resolved {
    /// Nenhuma entry selecionada — card mostra placeholder.
    Empty,
    /// Havia seleção, mas a entry sumiu da lista (refresh, etc.) —
    /// caller deve limpar o estado.
    SelectionGone,
    /// Entry parseada e pronta para renderizar.
    Loaded {
        path: std::path::PathBuf,
        meta: ParsedMeta,
        minimap: Option<MapImage>,
    },
}

/// Conteúdo do card quando há entry selecionada e parseada.
#[allow(clippy::too_many_arguments)]
fn detail_card_filled(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    meta: &ParsedMeta,
    minimap: Option<&MapImage>,
    lang: Language,
    config: &crate::config::AppConfig,
    action: &mut Option<LibraryAction>,
) {
    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    // ── Header: filename + close ──────────────────
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&filename)
                .size(size_caption(config))
                .monospace()
                .color(LABEL_DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("×")
                .on_hover_text(t("library.detail.close_tooltip", lang))
                .clicked()
            {
                *action = Some(LibraryAction::ClearSelection);
            }
        });
    });
    ui.add_space(SPACE_S);

    // ── Minimapa ──────────────────────────────────
    minimap_panel(ui, minimap, &meta.map, lang);
    ui.add_space(SPACE_S);

    // ── Botão primário: Abrir análise ──────────────
    if primary_open_button(ui, &t("library.detail.open_analysis", lang), config).clicked() {
        *action = Some(LibraryAction::Load(path.to_path_buf()));
    }
    ui.add_space(SPACE_M);

    ui.separator();
    ui.add_space(SPACE_S);

    // ── Quando + duração + versão ──────────────────
    section_label(ui, &t("library.detail.section.match", lang), config);
    let (date_part, time_part) = split_datetime(&meta.datetime);
    let date_display = if date_part.is_empty() {
        meta.datetime.clone()
    } else {
        format_date_short(&meta.datetime, lang)
    };
    info_row(
        ui,
        &t("library.detail.played_at", lang),
        &if time_part.is_empty() {
            date_display.clone()
        } else {
            format!("{date_display} \u{2022} {time_part}")
        },
        config,
    );
    info_row(
        ui,
        &t("library.detail.duration", lang),
        &fmt_duration(meta.duration_seconds),
        config,
    );
    info_row(
        ui,
        &t("library.detail.version", lang),
        meta.version.as_deref().unwrap_or("—"),
        config,
    );
    ui.add_space(SPACE_M);

    // ── Jogadores: nome · MMR (+ delta no usuário) ─
    section_label(ui, &t("library.detail.section.players", lang), config);
    let user_idx = find_user_index(meta, &config.user_nicknames);
    for (i, p) in meta.players.iter().enumerate() {
        player_row(ui, p, user_idx == Some(i), meta, i, config, lang);
    }
    ui.add_space(SPACE_M);

    // ── Resumo da abertura por jogador ─────────────
    section_label(ui, &t("library.detail.section.openings", lang), config);
    for p in meta.players.iter() {
        opening_row(ui, p, config, lang);
    }
    ui.add_space(SPACE_M);

    // ── File: caminho relativo + reveal no file manager ──
    section_label(ui, &t("library.detail.section.file", lang), config);
    let path_display = relative_path_display(path, config.effective_working_dir().as_deref());
    ui.add(
        egui::Label::new(
            RichText::new(path_display)
                .size(size_caption(config))
                .monospace()
                .color(LABEL_STRONG),
        )
        .wrap(),
    );
    ui.add_space(SPACE_XS);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let btn = egui::Button::new(
            RichText::new(t("library.detail.show_in_explorer", lang)).size(size_body(config)),
        );
        if ui.add_sized([ui.available_width(), 28.0], btn).clicked() {
            reveal_in_file_manager(path);
        }
    }
}

/// Estado vazio — convida o usuário a clicar numa entry e explica o
/// duplo-clique. Renderizado tanto na ausência de seleção quanto
/// quando a seleção apontava para uma entry que sumiu.
fn detail_card_empty(ui: &mut egui::Ui, lang: Language, config: &crate::config::AppConfig) {
    let avail_h = ui.available_height().max(120.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), avail_h),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(avail_h * 0.25);
            ui.label(RichText::new("📋").size(40.0));
            ui.add_space(SPACE_M);
            ui.label(
                RichText::new(t("library.detail.empty.title", lang))
                    .strong()
                    .color(LABEL_STRONG),
            );
            ui.add_space(SPACE_XS);
            ui.label(
                RichText::new(t("library.detail.empty.hint", lang))
                    .size(size_caption(config))
                    .color(LABEL_DIM),
            );
        },
    );
}

/// Mostra o minimapa carregado ou um placeholder centralizado quando o
/// arquivo de mapa não pôde ser resolvido (replay sem `cache_handles`,
/// arquivo do BNet Cache ausente, etc.).
fn minimap_panel(
    ui: &mut egui::Ui,
    minimap: Option<&MapImage>,
    map_name: &str,
    lang: Language,
) {
    let avail_w = ui.available_width();
    let side = MINIMAP_SIDE.min(avail_w);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_gray(18));
        match minimap {
            Some(img) => {
                let key = format!("library_detail_minimap:{}", map_name);
                let texture = ui.ctx().load_texture(
                    key,
                    map_image_to_color_image(img),
                    TextureOptions::LINEAR,
                );
                painter.image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t("library.detail.minimap_unavailable", lang),
                    egui::FontId::proportional(12.0),
                    LABEL_DIM,
                );
            }
        }
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, Color32::from_gray(60)),
            egui::StrokeKind::Outside,
        );
        ui.add_space(SPACE_XS);
        ui.label(
            RichText::new(map_name)
                .size(12.0)
                .strong()
                .color(LABEL_STRONG),
        );
    });
}

fn section_label(ui: &mut egui::Ui, text: &str, config: &crate::config::AppConfig) {
    ui.label(
        RichText::new(text)
            .size(size_caption(config))
            .strong()
            .color(LABEL_DIM),
    );
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str, config: &crate::config::AppConfig) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(size_caption(config))
                .color(LABEL_SOFT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .size(size_body(config))
                    .color(LABEL_STRONG),
            );
        });
    });
}

fn player_row(
    ui: &mut egui::Ui,
    p: &PlayerMeta,
    is_user: bool,
    meta: &ParsedMeta,
    idx: usize,
    config: &crate::config::AppConfig,
    lang: Language,
) {
    let race_letter_ch = crate::utils::race_letter(&p.race);
    let race_col = race_color(&p.race);
    let mmr_text = match p.mmr {
        Some(v) => v.to_string(),
        None => "—".into(),
    };
    let delta_text: Option<(String, Color32)> = (|| {
        if !is_user || meta.players.len() != 2 {
            return None;
        }
        let user_mmr = p.mmr?;
        let opp = meta.players.get(1 - idx)?;
        let opp_mmr = opp.mmr?;
        let d = user_mmr - opp_mmr;
        let (color, text) = if d > 0 {
            (ACCENT_SUCCESS, format!("Δ +{d}"))
        } else if d < 0 {
            (ACCENT_DANGER, format!("Δ {d}"))
        } else {
            (LABEL_DIM, "Δ 0".to_string())
        };
        Some((text, color))
    })();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(race_letter_ch.to_string())
                .strong()
                .monospace()
                .color(race_col),
        );
        let name = RichText::new(&p.name).color(Color32::WHITE);
        let name = if is_user { name.strong() } else { name };
        ui.label(name);
        if is_user {
            ui.label(
                RichText::new(t("library.detail.you_chip", lang))
                    .size(size_caption(config))
                    .color(LABEL_DIM),
            );
        }
        // Right-to-left: o primeiro widget adicionado vai para a borda
        // direita. Queremos o MMR colado na direita e o ΔMMR logo à
        // esquerda dele (ordem visual: `Δ -25   4085`), então o MMR
        // vai *primeiro*, depois o gap, depois o delta.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(mmr_text)
                    .monospace()
                    .size(size_body(config))
                    .color(LABEL_STRONG),
            );
            if let Some((dt, dc)) = delta_text.as_ref() {
                ui.add_space(SPACE_S);
                ui.label(
                    RichText::new(dt)
                        .size(size_caption(config))
                        .strong()
                        .color(*dc),
                );
            }
        });
    });
}

fn opening_row(
    ui: &mut egui::Ui,
    p: &PlayerMeta,
    config: &crate::config::AppConfig,
    lang: Language,
) {
    let opening = p.opening.as_classified().unwrap_or("—");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_XS;
        ui.label(
            RichText::new(format!("{}:", p.name))
                .size(size_caption(config))
                .color(LABEL_SOFT),
        );
        ui.label(
            RichText::new(opening)
                .size(size_caption(config))
                .color(LABEL_STRONG),
        );
    });
    let _ = lang; // reservado p/ futuras strings traduzidas no opening
}

// ── Botão "Abrir análise" pintado à mão ──────────────────────────────
//
// Por que não `egui::Button::new(...)` puro? Mesmo com `.fill()`, só o
// estado idle é sobrescrito — hover/pressed continuam saindo do tema
// global e o feedback visual fica preso ao mesmo cinza dos outros
// botões. Pintar à mão nos dá os três estados (idle/hover/active) sem
// mexer no estilo global. Estilo intencionalmente discreto: tons da
// escala neutra de superfície, sem cor de marca, sem ícone — o card
// inteiro já é o foco e o título do botão basta para a affordance.

fn primary_open_button(
    ui: &mut egui::Ui,
    label: &str,
    config: &crate::config::AppConfig,
) -> egui::Response {
    use egui::{FontId, Sense, Stroke, StrokeKind};

    let height = 32.0;
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), Sense::click());

    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();

    let (fill, stroke_col) = if pressed {
        (ACTIVE_FILL, BORDER_STRONG)
    } else if hovered {
        (HOVER_FILL, BORDER_STRONG)
    } else {
        (SURFACE_RAISED, BORDER)
    };

    ui.painter().rect(
        rect,
        RADIUS_BUTTON,
        fill,
        Stroke::new(1.0, stroke_col),
        StrokeKind::Inside,
    );

    let text_color = if hovered || pressed {
        egui::Color32::WHITE
    } else {
        LABEL_STRONG
    };
    let label_galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(size_body(config)),
        text_color,
    );
    let label_pos = egui::pos2(
        rect.center().x - label_galley.size().x / 2.0,
        rect.center().y - label_galley.size().y / 2.0,
    );
    ui.painter().galley(label_pos, label_galley, text_color);

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ── Helpers locais ───────────────────────────────────────────────────

fn fmt_duration(secs: u32) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{m:02}:{s:02}")
}

fn split_datetime(dt: &str) -> (String, String) {
    if dt.len() >= 16 {
        (dt[..10].to_string(), dt[11..16].to_string())
    } else if dt.len() >= 10 {
        (dt[..10].to_string(), String::new())
    } else {
        (dt.to_string(), String::new())
    }
}

fn find_user_index(meta: &ParsedMeta, nicknames: &[String]) -> Option<usize> {
    if nicknames.is_empty() {
        return None;
    }
    meta.players.iter().position(|p| {
        nicknames.iter().any(|n| n.eq_ignore_ascii_case(&p.name))
    })
}

fn map_image_to_color_image(img: &MapImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba)
}

/// Caminho a exibir na seção "File": relativo à pasta-base do config quando
/// possível, com fallback para o path absoluto. O `strip_prefix` da std falha
/// se o replay estiver fora da pasta-base — nesse caso mostramos o absoluto
/// para o usuário ainda conseguir localizar o arquivo.
fn relative_path_display(path: &std::path::Path, base: Option<&std::path::Path>) -> String {
    if let Some(base) = base {
        if let Ok(rel) = path.strip_prefix(base) {
            return rel.display().to_string();
        }
    }
    path.display().to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn reveal_in_file_manager(path: &std::path::Path) {
    use std::process::Command;
    #[cfg(target_os = "windows")]
    {
        // `explorer.exe /select,<path>` exige que `<path>` esteja entre aspas
        // para nomes com espaços, mas o auto-quoting do `Command::arg` envolveria
        // a string inteira (`"/select,..."`) — formato que o explorer não
        // interpreta. Usamos `raw_arg` para montar a linha de comando manualmente.
        use std::os::windows::process::CommandExt;
        let _ = Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        // xdg-open não tem "selecionar"; abrir o diretório-pai.
        if let Some(parent) = path.parent() {
            let _ = Command::new("xdg-open").arg(parent).spawn();
        }
    }
}
