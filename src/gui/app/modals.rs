// Janelas modais — todas renderizadas pelo loop principal quando a flag
// correspondente está ativa:
// - `language_prompt`: first-run; bloqueia a UI até escolha de idioma.
// - `disclaimer_prompt`: aviso legal/desenvolvimento; mostrado a cada
//   abertura até o usuário marcar "não exibir mais".
// - `about_window`: janela de créditos acionada pelo menu Help — espelha
//   o conteúdo do disclaimer para que o usuário possa revisitá-lo.

use egui::{Context, RichText};

use crate::config::AppConfig;
use crate::locale::{t, tf, Language};

/// Project repository on GitHub. Source of truth for the disclaimer
/// hyperlinks and the About window — kept in one place so a future
/// fork only edits one constant.
const REPO_URL: &str = "https://github.com/paulocanedo/sc2-replay-utils";
const ISSUES_URL: &str = "https://github.com/paulocanedo/sc2-replay-utils/issues";

/// Disclaimer bullet keys, in display order. Shared between the startup
/// modal and the About window so the two views never drift.
const DISCLAIMER_BULLET_KEYS: &[&str] = &[
    "disclaimer.unofficial",
    "disclaimer.in_development",
    "disclaimer.supply_block",
    "disclaimer.feedback",
    "disclaimer.open_source",
];

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

/// Startup disclaimer modal. Shown whenever
/// `!config.disclaimer_acknowledged && !dismissed_session`. The user
/// can either acknowledge for the session only (button alone) or
/// permanently (checkbox + button — sets `disclaimer_acknowledged`
/// and persists). The same bullet content is mirrored in
/// [`about_window`] so users who clicked through can re-read it later.
pub(super) fn disclaimer_prompt(
    ctx: &Context,
    lang: Language,
    dont_show_again: &mut bool,
    dismissed_session: &mut bool,
    config: &mut AppConfig,
) {
    egui::Window::new(t("disclaimer.title", lang))
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.add_space(4.0);
            ui.heading(t("disclaimer.title", lang));
            ui.add_space(6.0);
            ui.label(t("disclaimer.intro", lang));
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            disclaimer_bullets(ui, lang);

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            ui.hyperlink_to(t("disclaimer.issues_link", lang), ISSUES_URL);
            ui.hyperlink_to(t("disclaimer.repo_link", lang), REPO_URL);

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);

            ui.checkbox(dont_show_again, t("disclaimer.dont_show_again", lang));
            ui.add_space(8.0);

            ui.vertical_centered(|ui| {
                if ui
                    .add_sized(
                        [220.0, 32.0],
                        egui::Button::new(
                            RichText::new(t("disclaimer.acknowledge", lang)).strong(),
                        ),
                    )
                    .clicked()
                {
                    if *dont_show_again {
                        config.disclaimer_acknowledged = true;
                        let _ = config.save();
                    }
                    *dismissed_session = true;
                }
                ui.add_space(4.0);
            });
        });
}

pub(super) fn about_window(ctx: &Context, lang: Language, show_about: &mut bool) {
    egui::Window::new(t("about.title", lang))
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(560.0);
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
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(RichText::new(t("about.disclaimer_heading", lang)).strong());
            ui.add_space(6.0);
            disclaimer_bullets(ui, lang);

            ui.add_space(8.0);
            ui.hyperlink_to(t("disclaimer.issues_link", lang), ISSUES_URL);
            ui.hyperlink_to(t("disclaimer.repo_link", lang), REPO_URL);

            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                if ui.button(t("about.close", lang)).clicked() {
                    *show_about = false;
                }
                ui.add_space(4.0);
            });
        });
}

/// Renders the shared bullet list of disclaimer items. Used by both
/// the startup modal and the About window to keep the two surfaces in
/// sync — adding a new bullet only requires updating
/// [`DISCLAIMER_BULLET_KEYS`] and the locale tables.
fn disclaimer_bullets(ui: &mut egui::Ui, lang: Language) {
    for (i, key) in DISCLAIMER_BULLET_KEYS.iter().enumerate() {
        if i > 0 {
            ui.add_space(6.0);
        }
        ui.label(format!("•  {}", t(key, lang)));
    }
}

/// Bullet keys shown by [`timeline_experimental_prompt`].
const TIMELINE_EXPERIMENTAL_BULLET_KEYS: &[&str] = &[
    "experimental.timeline.bullet_movement",
    "experimental.timeline.bullet_fog",
];

/// Tab-scoped warning shown the first time the user opens the Timeline
/// tab in a session. Lists known limitations of the visualization
/// (interpolated unit movement, incomplete Fog of War). Dismissal lives
/// only in `dismissed_session` — flag is reset on each launch by design.
pub(super) fn timeline_experimental_prompt(
    ctx: &Context,
    lang: Language,
    dismissed_session: &mut bool,
) {
    egui::Window::new(t("experimental.timeline.title", lang))
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.add_space(4.0);
            ui.heading(t("experimental.timeline.title", lang));
            ui.add_space(6.0);
            ui.label(t("experimental.timeline.intro", lang));
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            for (i, key) in TIMELINE_EXPERIMENTAL_BULLET_KEYS.iter().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }
                ui.label(format!("•  {}", t(key, lang)));
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.vertical_centered(|ui| {
                if ui
                    .add_sized(
                        [220.0, 32.0],
                        egui::Button::new(
                            RichText::new(t("experimental.timeline.acknowledge", lang)).strong(),
                        ),
                    )
                    .clicked()
                {
                    *dismissed_session = true;
                }
                ui.add_space(4.0);
            });
        });
}

/// Tab-scoped warning shown the first time the user opens the Insights
/// tab in a session. Calculations there are still being refined and may
/// report inaccurate values. Dismissal lives only in
/// `dismissed_session`.
pub(super) fn insights_experimental_prompt(
    ctx: &Context,
    lang: Language,
    dismissed_session: &mut bool,
) {
    egui::Window::new(t("experimental.insights.title", lang))
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.add_space(4.0);
            ui.heading(t("experimental.insights.title", lang));
            ui.add_space(6.0);
            ui.label(t("experimental.insights.message", lang));
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.vertical_centered(|ui| {
                if ui
                    .add_sized(
                        [220.0, 32.0],
                        egui::Button::new(
                            RichText::new(t("experimental.insights.acknowledge", lang)).strong(),
                        ),
                    )
                    .clicked()
                {
                    *dismissed_session = true;
                }
                ui.add_space(4.0);
            });
        });
}
