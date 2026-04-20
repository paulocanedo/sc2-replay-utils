// Componente genérico de card de insight.
//
// Todo card compartilha a mesma casca: título + botão (?) que abre um
// popup com a explicação, e um `body` livre pra cada card renderizar
// seus números/charts. O popup fecha clicando fora (comportamento
// padrão do egui PopupCloseBehavior::CloseOnClickOutside).

use egui::{Frame, Id, Margin, RichText, Ui};

use crate::config::AppConfig;
use crate::tokens::{
    size_subtitle, CARD_INNER_MX, CARD_INNER_MY, RADIUS_CARD, SHADOW_CARD, SPACE_M, SPACE_S,
    STROKE_HAIRLINE,
};

// Key used by `grid.rs` to pass a per-row target inner height so cards
// in the same row end up visually aligned. Read inside `insight_card`;
// a value of 0.0 (or absent) means "render at natural height".
pub const MIN_INNER_H_KEY: &str = "insights_card_min_inner_h";

/// Renderiza um card com título, botão de ajuda e corpo custom.
///
/// - `id_salt` identifica o popup de ajuda (precisa ser estável por
///   card para o estado persistir entre frames).
/// - `help_text` suporta múltiplas linhas — use `\n` na string.
pub fn insight_card(
    ui: &mut Ui,
    config: &AppConfig,
    _id_salt: &str,
    title: &str,
    help_text: &str,
    body: impl FnOnce(&mut Ui),
) {
    let stroke_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let min_inner_h: f32 = ui
        .ctx()
        .data(|d| d.get_temp::<f32>(Id::new(MIN_INNER_H_KEY)).unwrap_or(0.0));
    Frame::new()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .stroke(egui::Stroke::new(STROKE_HAIRLINE, stroke_color))
        .corner_radius(RADIUS_CARD)
        .shadow(SHADOW_CARD)
        .inner_margin(Margin::symmetric(CARD_INNER_MX, CARD_INNER_MY))
        .show(ui, |ui| {
            if min_inner_h > 0.0 {
                ui.set_min_height(min_inner_h);
            }
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title)
                        .size(size_subtitle(config))
                        .strong(),
                );
                let help_button = ui.small_button("?");
                egui::Popup::from_toggle_button_response(&help_button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui: &mut egui::Ui| {
                        ui.set_min_width(320.0);
                        ui.set_max_width(420.0);
                        ui.label(RichText::new(help_text));
                    });
            });
            ui.add_space(SPACE_S);
            body(ui);
            ui.add_space(SPACE_M);
        });
    ui.add_space(SPACE_M);
}
