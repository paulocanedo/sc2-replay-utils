// Componente genérico de card de insight.
//
// Todo card compartilha a mesma casca: título + botão (?) que abre um
// popup com a explicação, e um `body` livre pra cada card renderizar
// seus números/charts. O popup fecha clicando fora (comportamento
// padrão do egui PopupCloseBehavior::CloseOnClickOutside).

use egui::{Frame, Margin, RichText, Ui};

use crate::config::AppConfig;
use crate::tokens::{
    size_subtitle, CARD_INNER_MX, CARD_INNER_MY, RADIUS_CARD, SPACE_M, SPACE_S, STROKE_HAIRLINE,
};

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
    Frame::new()
        .stroke(egui::Stroke::new(STROKE_HAIRLINE, stroke_color))
        .corner_radius(RADIUS_CARD)
        .inner_margin(Margin::symmetric(CARD_INNER_MX, CARD_INNER_MY))
        .show(ui, |ui| {
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
