// Janelas modais — ambas renderizadas pelo loop principal quando a flag
// correspondente está ativa:
// - `language_prompt`: first-run; bloqueia a UI até escolha de idioma.
// - `about_window`: janela de créditos acionada pelo menu Help.

use egui::{Context, RichText};

use crate::config::AppConfig;
use crate::locale::{t, tf, Language};

/// First-run modal that forces the user to pick a UI language. Uses a
/// bilingual title/description so it's intelligible regardless of the
/// default. Once confirmed, `config.language_selected` is set and the
/// rest of the app becomes reachable.
pub(super) fn language_prompt(ctx: &Context, draft: &mut Language, config: &mut AppConfig) {
    egui::Window::new(t("language_prompt.title", *draft))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(t("language_prompt.description", *draft));
                ui.add_space(12.0);
                for &lang in Language::all() {
                    ui.radio_value(draft, lang, lang.label());
                }
                ui.add_space(16.0);
                if ui
                    .add_sized(
                        [160.0, 32.0],
                        egui::Button::new(
                            RichText::new(t("language_prompt.confirm", *draft)).strong(),
                        ),
                    )
                    .clicked()
                {
                    config.language = *draft;
                    config.language_selected = true;
                    let _ = config.save();
                }
                ui.add_space(4.0);
            });
        });
}

pub(super) fn about_window(ctx: &Context, lang: Language, show_about: &mut bool) {
    egui::Window::new(t("about.title", lang))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading(t("app.title", lang));
                ui.label(tf(
                    "about.version",
                    lang,
                    &[("version", env!("CARGO_PKG_VERSION"))],
                ));
                ui.add_space(12.0);
                ui.label(t("about.description", lang));
                ui.add_space(12.0);
                ui.label(RichText::new(t("about.author_label", lang)).strong());
                ui.label(t("about.author_name", lang));
                ui.add_space(12.0);
                ui.label(RichText::new(t("about.tech_label", lang)).strong());
                ui.label(t("about.tech_value", lang));
                ui.add_space(16.0);
                if ui.button(t("about.close", lang)).clicked() {
                    *show_about = false;
                }
                ui.add_space(4.0);
            });
        });
}
