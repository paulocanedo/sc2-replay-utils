// Módulo raiz do `app`: plumbing dos submódulos + `impl eframe::App` que
// orquestra o frame.
//
// NOTE on `.show(ctx, ...)` vs `.show_inside(ui, ...)`:
// egui 0.34 deprecates the `.show(ctx, ...)` path in favor of
// `.show_inside(ui, ...)`, but **only** the former calls
// `pass_state.allocate_*_panel(...)`, which is what shrinks
// `ctx.available_rect()` so the next panel sees the reservation. With
// eframe's new `fn ui(&mut self, ui: &mut Ui, ...)` signature, the root
// `ui` happens to be a fresh background-layer Ui whose cursor cooperates
// with `show_inside`'s cursor-mutation in isolation — but the central
// panel's ScrollArea was still painting over the bottom status bar in
// practice. Migrating the four top-level panels (menu, topbar, status,
// central) to `.show(ctx, ...)` fixes the overlap reliably. We accept
// the deprecation warnings in the affected modules via
// `#[allow(deprecated)]`; the path is still fully supported by egui
// (deprecation is advisory, not removal).
//
// A UI alterna entre duas telas mutuamente exclusivas:
// - `Screen::Library`: a biblioteca de replays ocupa toda a janela.
// - `Screen::Analysis`: replay bar + tab bar + central panel + painel
//   direito de jogadores ocupam toda a janela.
//
// Em todas as telas há uma status bar inferior persistente exibindo o
// replay atualmente carregado, o estado do watcher e os toasts.
//
// On first launch (or whenever `config.language_selected` is false),
// a blocking modal prompts the user to pick a language before any
// other UI is reachable. Persisting the choice sets
// `language_selected = true`.
//
// Organização dos submódulos:
//   - `state`      — `Screen`, `AppState`, métodos de ownership.
//   - `menu_bar`   — barra de menu superior.
//   - `topbar`     — topbars de Library e Analysis (+ tab bar).
//   - `status_bar` — status bar inferior persistente.
//   - `central`    — roteamento do painel central + `LibraryAction`.
//   - `modals`     — janelas modais (language prompt, about).

mod central;
#[cfg(not(target_arch = "wasm32"))]
mod library_detail;
mod menu_bar;
mod modals;
mod state;
mod status_bar;
mod topbar;

use std::sync::Arc;
use std::time::Duration;

use egui::{Context, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};

use crate::colors::{
    self, ACTIVE_FILL, BORDER, BORDER_STRONG, FOCUS_RING, HOVER_FILL, LABEL_SOFT, LABEL_STRONG,
    SELECTION_BG, SURFACE, SURFACE_ALT, SURFACE_RAISED,
};
use crate::config::AppConfig;
use crate::library;
use crate::locale::{t, tf};
use crate::tokens::{
    RADIUS_BUTTON, RADIUS_WINDOW, SHADOW_POPUP, SHADOW_WINDOW, SPACE_M, SPACE_S, SPACE_XS,
    STROKE_HAIRLINE,
};
use crate::ui_settings;

pub use state::{AppState, Screen};

impl eframe::App for AppState {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // -------- First-run language prompt (modal) --------
        // Renders before anything else and blocks interaction elsewhere
        // by simply not painting the rest of the UI when open.
        if !self.config.language_selected {
            modals::language_prompt(&ctx, &mut self.language_draft, &mut self.config);
            // While the modal is open we still want a repaint so that
            // the language preview updates immediately.
            ctx.request_repaint();
            return;
        }

        let lang = self.config.language;

        // -------- Startup disclaimer (modal) --------
        // Shown on every launch until the user explicitly opts out via
        // the "don't show again" checkbox. While open, we paint nothing
        // else — the user must acknowledge before reaching the rest of
        // the UI. The same content is mirrored in Help → About so it
        // remains accessible after dismissal.
        if !self.config.disclaimer_acknowledged && !self.disclaimer_dismissed_session {
            modals::disclaimer_prompt(
                &ctx,
                lang,
                &mut self.disclaimer_dont_show_again,
                &mut self.disclaimer_dismissed_session,
                &mut self.config,
            );
            ctx.request_repaint();
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Polling do watcher ANTES de qualquer painel. Também roda
            // antes do gate de settings forçado para que o scan da
            // biblioteca progrida enquanto o usuário está na tela inicial
            // (populando as sugestões de nickname).
            self.poll_watcher(&ctx);
            // Drena progresso da carga em andamento (replay em background).
            self.poll_load(&ctx);
            // Drena resultados do worker da biblioteca.
            if self.library.poll() {
                ctx.request_repaint();
            }
            // Recompute derived library stats if entries, nicknames, or
            // the active filter changed.
            self.library
                .ensure_stats(&self.config, &self.library_filter);
        }
        // Web: drain any replay bytes uploaded via the browser file picker.
        #[cfg(target_arch = "wasm32")]
        self.drain_pending_upload();

        // -------- First-run forced settings (modal) --------
        // Mostrado quando `settings_confirmed` é false — inclui todos
        // os usuários existentes na primeira execução após esta feature
        // (via `#[serde(default)]` o campo ausente vira false). A única
        // saída é clicar em Save; ao salvar, a flag é persistida e o
        // gate não aparece mais. Blocos de auto-load / autodetect ficam
        // adiante para não disparar durante o setup.
        #[cfg(not(target_arch = "wasm32"))]
        if !self.config.settings_confirmed {
            let prev_effective_dir = self.config.effective_working_dir();
            let mut dummy_open = true;
            let outcome = ui_settings::show(
                &ctx,
                &mut dummy_open,
                &mut self.config,
                &mut self.nickname_input,
                self.library.nickname_frequencies().unwrap_or(&[]),
                /* force_initial */ true,
            );
            if outcome.saved {
                self.config.settings_confirmed = true;
                match self.config.save() {
                    Ok(()) => self.set_toast(t("toast.settings_saved", lang).to_string()),
                    Err(e) => {
                        self.set_toast(tf("toast.save_error", lang, &[("err", &e)]))
                    }
                }
                apply_style(&ctx, &self.config);
                self.restart_watcher();
                if self.config.effective_working_dir() != prev_effective_dir {
                    self.refresh_library();
                }
            } else if outcome.reset_defaults {
                // Reset Defaults está oculto no modo force_initial, mas
                // se vier (defensivo), reaplica o estilo sem marcar
                // settings_confirmed.
                apply_style(&ctx, &self.config);
            }
            ctx.request_repaint();
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Carrega o replay mais recente quando o scanner terminar.
            if self.pending_load_latest && !self.library.scanning {
                self.pending_load_latest = false;
                if let Some(path) = self.library.scan_latest.clone() {
                    self.load_path(path, &ctx);
                }
            }

            // Auto-detect do DateRange no primeiro launch (config sem
            // `library_date_range` persistido). Só roda depois que o scan
            // termina e todas as entries viraram `Parsed`. Se nenhum
            // replay for encontrado, não persistimos nada — o próximo
            // launch tenta de novo. A flag garante execução única por
            // sessão.
            if self.pending_date_range_autodetect
                && !self.library.scanning
                && self.library.pending_count() == 0
            {
                self.pending_date_range_autodetect = false;
                let today = library::today_str();
                if let Some(chosen) =
                    library::detect_best_date_range(&self.library.entries, &today)
                {
                    self.library_filter.date_range = chosen;
                    self.config.library_date_range = Some(chosen);
                    if let Err(e) = self.config.save() {
                        self.set_toast(tf("toast.save_config_error", lang, &[("err", &e)]));
                    } else {
                        let range_label = library::date_range_label(chosen, &self.config);
                        self.set_toast(tf(
                            "toast.date_range_autodetect",
                            lang,
                            &[("range", &range_label)],
                        ));
                    }
                    ctx.request_repaint();
                }
            }

            // Guarda: Tela Análise exige replay carregado. Se por
            // qualquer motivo o estado divergir, força fallback para a
            // biblioteca. (Não aplica em wasm — Analysis com sem replay
            // mostra o "empty state" com botão de upload.)
            if self.screen == Screen::Analysis && self.loaded.is_none() {
                self.screen = Screen::Library;
            }

            // F5 atalha o "Refresh library" do menu View. Só dispara na
            // tela de biblioteca para não surpreender em Analysis.
            if self.screen == Screen::Library
                && ctx.input(|i| i.key_pressed(egui::Key::F5))
            {
                self.refresh_library();
            }
        }

        // Top-level panels: use `.show(ctx, ...)` so each call updates the
        // ctx's pass_state.available_rect — otherwise sibling panels don't
        // know about each other's reservations and the CentralPanel can
        // grow past a bottom panel's top edge, letting its ScrollArea
        // paint over the status bar.
        //
        // The eframe-provided `ui` above is only used for modals / ctx
        // extraction; we deliberately do NOT nest panels inside it.
        // The hamburger ☰ that used to live here is rendered as the
        // leftmost widget of each topbar — see `topbar.rs`.

        match self.screen {
            #[cfg(not(target_arch = "wasm32"))]
            Screen::Library => self.show_library_topbar(&ctx),
            Screen::Analysis => self.show_analysis_topbar(&ctx),
        }

        self.show_status_bar(&ctx);

        #[cfg(not(target_arch = "wasm32"))]
        {
            let action = self.show_central(&ctx);
            self.handle_library_action(action, &ctx);
        }
        #[cfg(target_arch = "wasm32")]
        self.show_central(&ctx);

        // Silence unused-var warning: `ui` is intentionally not used for
        // panel nesting (see note above). We keep it in the trait
        // signature for future modal-on-ui needs.
        let _ = ui;

        // Mantém repaint enquanto a biblioteca estiver parseando em background.
        #[cfg(not(target_arch = "wasm32"))]
        library::keep_alive(&ctx, &self.library);

        // -------- Settings window --------
        #[cfg(not(target_arch = "wasm32"))]
        {
            let prev_effective_dir = self.config.effective_working_dir();
            let outcome = ui_settings::show(
                &ctx,
                &mut self.show_settings,
                &mut self.config,
                &mut self.nickname_input,
                self.library.nickname_frequencies().unwrap_or(&[]),
                /* force_initial */ false,
            );
            if outcome.saved {
                match self.config.save() {
                    Ok(()) => self.set_toast(t("toast.settings_saved", lang).to_string()),
                    Err(e) => self.set_toast(tf("toast.save_error", lang, &[("err", &e)])),
                }
                apply_style(&ctx, &self.config);
                self.restart_watcher();
                if self.config.effective_working_dir() != prev_effective_dir {
                    self.refresh_library();
                }
            } else if outcome.reset_defaults {
                apply_style(&ctx, &self.config);
                if self.config.effective_working_dir() != prev_effective_dir {
                    self.refresh_library();
                }
            }
        }

        // -------- About window --------
        if self.show_about {
            modals::about_window(&ctx, lang, &mut self.show_about);
        }

        // -------- Tab-scoped experimental warnings --------
        // Session-only: ack lasts until app exit. Painted after central
        // so the user sees the underlying tab; egui::Window floats on
        // top.
        if self.screen == Screen::Analysis {
            match self.active_tab {
                crate::tabs::Tab::Timeline
                    if !self.timeline_experimental_dismissed_session =>
                {
                    modals::timeline_experimental_prompt(
                        &ctx,
                        lang,
                        &mut self.timeline_experimental_dismissed_session,
                    );
                }
                crate::tabs::Tab::Insights
                    if !self.insights_experimental_dismissed_session =>
                {
                    modals::insights_experimental_prompt(
                        &ctx,
                        lang,
                        &mut self.insights_experimental_dismissed_session,
                    );
                }
                _ => {}
            }
        }

        // Mantém o ciclo de polling do watcher vivo mesmo sem input.
        #[cfg(not(target_arch = "wasm32"))]
        if self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.library.save_cache();
        let _ = self.config.save();
    }
}

pub fn apply_style(ctx: &Context, config: &AppConfig) {
    let mut visuals = egui::Visuals::dark();
    apply_dark_palette(&mut visuals);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.global_style()).clone();
    let base = config.font_size.clamp(8.0, 28.0);
    for (text_style, font_id) in style.text_styles.iter_mut() {
        font_id.size = match text_style {
            egui::TextStyle::Small => (base * 0.72).round(),
            egui::TextStyle::Heading => (base * 1.43).round(),
            _ => base,
        };
    }

    // Spacing/rhythm: slightly more generous than egui defaults so the
    // UI breathes. Button padding in particular pushes chip/button
    // height above the default 18px so they align with label baselines.
    style.spacing.item_spacing = egui::vec2(SPACE_M, SPACE_S);
    style.spacing.button_padding = egui::vec2(SPACE_M, SPACE_XS);
    style.spacing.menu_margin = egui::Margin::symmetric(SPACE_S as i8, SPACE_XS as i8);

    ctx.set_global_style(style);
}

/// Builds the dark palette used everywhere in the app. Pulls colours
/// from `crate::colors` so the three-tier surface scale and the slot /
/// race palettes share one source of truth.
fn apply_dark_palette(v: &mut egui::Visuals) {
    let r_button = CornerRadius::same(RADIUS_BUTTON as u8);

    // ── Backgrounds ──────────────────────────────────────────────────
    v.window_fill = SURFACE;
    v.panel_fill = SURFACE;
    v.faint_bg_color = SURFACE_ALT;
    v.extreme_bg_color = egui::Color32::from_gray(14);
    v.code_bg_color = SURFACE_ALT;

    v.window_stroke = Stroke::new(STROKE_HAIRLINE, BORDER);
    v.window_corner_radius = CornerRadius::same(RADIUS_WINDOW as u8);
    v.menu_corner_radius = CornerRadius::same(RADIUS_BUTTON as u8);
    v.window_shadow = SHADOW_WINDOW;
    v.popup_shadow = SHADOW_POPUP;

    // ── Widget states ────────────────────────────────────────────────
    // `noninteractive` is used for labels/backgrounds — keep the stroke
    // visible so `insight_card` still reads the hairline colour from
    // `widgets.noninteractive.bg_stroke.color`.
    v.widgets.noninteractive.bg_fill = SURFACE_RAISED;
    v.widgets.noninteractive.weak_bg_fill = SURFACE_ALT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(STROKE_HAIRLINE, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, LABEL_STRONG);
    v.widgets.noninteractive.corner_radius = r_button;

    v.widgets.inactive.bg_fill = egui::Color32::from_gray(33);
    v.widgets.inactive.weak_bg_fill = egui::Color32::from_gray(28);
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, LABEL_SOFT);
    v.widgets.inactive.corner_radius = r_button;

    v.widgets.hovered.bg_fill = HOVER_FILL;
    v.widgets.hovered.weak_bg_fill = HOVER_FILL;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, egui::Color32::WHITE);
    v.widgets.hovered.corner_radius = r_button;

    v.widgets.active.bg_fill = ACTIVE_FILL;
    v.widgets.active.weak_bg_fill = ACTIVE_FILL;
    v.widgets.active.bg_stroke = Stroke::new(1.0, FOCUS_RING);
    v.widgets.active.fg_stroke = Stroke::new(1.0, egui::Color32::WHITE);
    v.widgets.active.corner_radius = r_button;

    v.widgets.open.bg_fill = ACTIVE_FILL;
    v.widgets.open.weak_bg_fill = HOVER_FILL;
    v.widgets.open.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    v.widgets.open.fg_stroke = Stroke::new(1.0, LABEL_STRONG);
    v.widgets.open.corner_radius = r_button;

    // ── Selection / focus ────────────────────────────────────────────
    v.selection.bg_fill = SELECTION_BG;
    v.selection.stroke = Stroke::new(1.0, FOCUS_RING);
    v.hyperlink_color = FOCUS_RING;

    // ── Semantic text colours ────────────────────────────────────────
    v.warn_fg_color = colors::ACCENT_WARNING;
    v.error_fg_color = colors::ACCENT_DANGER;

    // Subtle: striped tables look nicer against SURFACE_ALT.
    v.striped = true;
    // Keep the fill that trails a slider knob coloured — matches
    // FOCUS_RING so all "you can interact here" affordances read alike.
    v.slider_trailing_fill = true;
}

/// Registers Inter (UI) and JetBrains Mono (monospace) as the primary
/// fonts, keeping egui's default fallbacks for glyphs Inter/JB Mono
/// don't cover (CJK, emoji). Called once from `AppState::new`.
pub fn install_fonts(ctx: &Context) {
    const INTER: &[u8] =
        include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    const JETBRAINS_MONO: &[u8] =
        include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

    let mut fonts = FontDefinitions::default();

    fonts
        .font_data
        .insert("Inter".to_owned(), Arc::new(FontData::from_static(INTER)));
    fonts.font_data.insert(
        "JetBrainsMono".to_owned(),
        Arc::new(FontData::from_static(JETBRAINS_MONO)),
    );

    // Prepend our fonts to each family so Inter/JB Mono render first,
    // with egui's defaults (Ubuntu-Light, Hack, Noto Emoji) as fallback.
    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, "Inter".to_owned());
    }
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.insert(0, "JetBrainsMono".to_owned());
    }

    ctx.set_fonts(fonts);
}
