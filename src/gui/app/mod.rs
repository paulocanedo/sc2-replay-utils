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
// A UI alterna entre três telas mutuamente exclusivas:
// - `Screen::Library`: a biblioteca de replays ocupa toda a janela.
// - `Screen::Analysis`: replay bar + tab bar + central panel + painel
//   direito de jogadores ocupam toda a janela.
// - `Screen::Rename`: barra de rename + central panel de preview.
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
//   - `topbar`     — topbars de Library, Rename e Analysis (+ tab bar).
//   - `status_bar` — status bar inferior persistente.
//   - `central`    — roteamento do painel central + `LibraryAction`.
//   - `modals`     — janelas modais (language prompt, about).

mod central;
mod menu_bar;
mod modals;
mod state;
mod status_bar;
mod topbar;

use std::time::Duration;

use egui::Context;

use crate::config::AppConfig;
use crate::library;
use crate::locale::{t, tf};
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

        // Polling do watcher ANTES de qualquer painel.
        self.poll_watcher(&ctx);
        // Drena resultados do worker da biblioteca.
        if self.library.poll() {
            ctx.request_repaint();
        }
        // Recompute derived library stats if entries, nicknames, or the
        // active filter changed — keeps the hero KPIs in sync with the
        // visible list.
        self.library
            .ensure_stats(&self.config, &self.library_filter);
        // Carrega o replay mais recente quando o scanner terminar.
        if self.pending_load_latest && !self.library.scanning {
            self.pending_load_latest = false;
            if let Some(path) = self.library.scan_latest.clone() {
                self.load_path(path);
            }
        }

        // Guarda: Tela Análise exige replay carregado. Se por qualquer
        // motivo o estado divergir, força fallback para a biblioteca.
        if self.screen == Screen::Analysis && self.loaded.is_none() {
            self.screen = Screen::Library;
        }

        // Top-level panels: use `.show(ctx, ...)` so each call updates the
        // ctx's pass_state.available_rect — otherwise sibling panels don't
        // know about each other's reservations and the CentralPanel can
        // grow past a bottom panel's top edge, letting its ScrollArea
        // paint over the status bar.
        //
        // The eframe-provided `ui` above is only used for modals / ctx
        // extraction; we deliberately do NOT nest panels inside it.
        self.show_menu_bar(&ctx);

        match self.screen {
            Screen::Library => self.show_library_topbar(&ctx),
            Screen::Rename => self.show_rename_topbar(&ctx),
            Screen::Analysis => self.show_analysis_topbar(&ctx),
        }

        self.show_status_bar(&ctx);

        let action = self.show_central(&ctx);
        self.handle_library_action(action);

        // Silence unused-var warning: `ui` is intentionally not used for
        // panel nesting (see note above). We keep it in the trait
        // signature for future modal-on-ui needs.
        let _ = ui;

        // Mantém repaint enquanto a biblioteca estiver parseando em background.
        library::keep_alive(&ctx, &self.library);

        // -------- Settings window --------
        let prev_effective_dir = self.config.effective_working_dir();
        let outcome = ui_settings::show(
            &ctx,
            &mut self.show_settings,
            &mut self.config,
            &mut self.nickname_input,
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

        // -------- About window --------
        if self.show_about {
            modals::about_window(&ctx, lang, &mut self.show_about);
        }

        // Mantém o ciclo de polling do watcher vivo mesmo sem input.
        if self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.library.save_cache();
        let _ = self.config.save();
    }
}

pub fn apply_style(ctx: &Context, config: &AppConfig) {
    ctx.set_visuals(if config.dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    });
    let mut style = (*ctx.global_style()).clone();
    let base = config.font_size.clamp(8.0, 28.0);
    for (text_style, font_id) in style.text_styles.iter_mut() {
        font_id.size = match text_style {
            egui::TextStyle::Small => (base * 0.72).round(),
            egui::TextStyle::Heading => (base * 1.43).round(),
            _ => base,
        };
    }
    ctx.set_global_style(style);
}
