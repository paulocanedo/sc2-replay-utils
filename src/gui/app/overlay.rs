// Ponte entre o `AppState` e o servidor do overlay de transmissão.
//
// A UI thread é a única produtora de snapshots. A publicação é dirigida por
// evento (`overlay_dirty`), nunca por frame nem por timer: a projeção é
// O(entries) e uma biblioteca grande não pode pagá-la a 60 fps.
//
// Os pontos que marcam `overlay_dirty`:
//   - `app/mod.rs`  — a biblioteca mudou (scan, watcher, enriquecimento).
//   - `state.rs`    — um replay terminou de carregar.
//   - `app/mod.rs`  — os nicknames mudaram nas configurações.
//   - aqui          — logo após o servidor subir, para o primeiro request
//                     encontrar dados reais em vez de `OverlayData::default()`.

use std::sync::Arc;

use crate::locale::{t, tf};
use crate::ui_settings::OverlayUiStatus;

use super::state::AppState;

impl AppState {
    /// Reconstrói e publica o snapshot, se houver algo a publicar.
    ///
    /// No-op quando o servidor está desligado — mesmo que algo tenha
    /// marcado `overlay_dirty`, não há para quem publicar.
    pub(super) fn publish_overlay_if_dirty(&mut self) {
        if !self.overlay_dirty {
            return;
        }
        self.overlay_dirty = false;
        if self.overlay.is_none() {
            return;
        }
        // `today_str()` é reavaliado a cada publish, então a virada de
        // meia-noite se corrige sozinha no primeiro jogo do novo dia.
        let today = crate::library::today_str();
        let data =
            crate::library::overlay_snapshot::build(&self.library.entries, &self.config, &today);
        self.overlay_state.publish(data);
    }

    /// Aplica `config.overlay_enabled` / `config.overlay_port`: derruba o
    /// servidor atual e, se for o caso, sobe outro.
    ///
    /// Sempre derruba primeiro — sem malabarismo de porta e sem meio-estado
    /// se o novo bind falhar. `announce` liga o toast de sucesso (queremos
    /// no clique de Salvar, não em toda abertura do app); o toast de erro
    /// sai sempre.
    pub(super) fn apply_overlay_config(&mut self, announce: bool) {
        let lang = self.config.language;
        self.overlay = None;
        self.overlay_error = None;
        if !self.config.overlay_enabled {
            return;
        }
        match crate::overlay::start(self.config.overlay_port, Arc::clone(&self.overlay_state)) {
            Ok(handle) => {
                if announce {
                    self.set_toast(tf(
                        "toast.overlay_started",
                        lang,
                        &[("url", &handle.url())],
                    ));
                }
                self.overlay = Some(handle);
                self.overlay_dirty = true;
                self.publish_overlay_if_dirty();
            }
            Err(e) => {
                self.set_toast(tf("toast.overlay_error", lang, &[("err", &e)]));
                // Persiste além do TTL de 4 s do toast: é o que a linha de
                // status da janela de configurações mostra.
                self.overlay_error = Some(e);
            }
        }
    }

    /// `true` quando o servidor em execução (ou a ausência dele) já reflete
    /// o que está no config.
    ///
    /// Compara contra o que está **rodando**, e não contra um snapshot do
    /// config tirado no mesmo frame: o checkbox das configurações muda
    /// `overlay_enabled` no frame do clique, vários frames antes de o
    /// usuário apertar Salvar. Um "antes vs depois" dentro do frame do
    /// Save enxerga os dois valores já iguais e nunca dispara — foi
    /// exatamente esse o bug de "marquei a opção e nada acontece até
    /// reiniciar".
    ///
    /// Com o servidor parado e `overlay_enabled` ligado (bind falhou),
    /// devolve `false` de propósito: assim Salvar/Iniciar tentam de novo,
    /// que é o que se quer depois de liberar a porta.
    pub(super) fn overlay_matches_config(&self) -> bool {
        config_matches_running(
            self.config.overlay_enabled,
            self.config.overlay_port,
            self.overlay.as_ref().map(|h| h.bound_port),
        )
    }

    /// Estado do servidor no formato que `ui_settings` entende (um struct
    /// plano, sem tipos de `crate::overlay` — aquele módulo compila em wasm).
    pub(super) fn overlay_ui_status(&self) -> OverlayUiStatus {
        match self.overlay.as_ref() {
            Some(h) => OverlayUiStatus {
                running: true,
                bound_port: h.bound_port,
                url: h.url(),
                error: None,
            },
            None => OverlayUiStatus {
                running: false,
                bound_port: 0,
                url: String::new(),
                error: self.overlay_error.clone(),
            },
        }
    }

    /// Botões da seção de overlay que agem na hora, sem depender de Salvar.
    ///
    /// `apply` (Iniciar/Reiniciar servidor) também persiste o config: o
    /// usuário que liga a feature pelo botão espera encontrá-la ligada no
    /// próximo launch, sem ter de lembrar de apertar Salvar também.
    pub(super) fn handle_overlay_actions(
        &mut self,
        apply: bool,
        open_folder: bool,
        restore_defaults: bool,
    ) {
        let lang = self.config.language;
        if apply {
            self.apply_overlay_config(/* announce */ true);
            if let Err(e) = self.config.save() {
                self.set_toast(tf("toast.save_config_error", lang, &[("err", &e)]));
            }
        }
        if open_folder {
            match crate::overlay::assets::ensure_dir() {
                Ok(dir) => crate::utils::open_directory(&dir),
                Err(e) => self.set_toast(tf("toast.overlay_error", lang, &[("err", &e)])),
            }
        }
        if restore_defaults {
            match crate::overlay::assets::restore_defaults() {
                Ok(_) => {
                    self.set_toast(t("toast.overlay_defaults_restored", lang).to_string())
                }
                Err(e) => self.set_toast(tf("toast.overlay_error", lang, &[("err", &e)])),
            }
        }
    }
}

/// Núcleo puro de [`AppState::overlay_matches_config`], separado para ser
/// testável — `AppState` precisa de um `eframe::CreationContext` e não dá
/// para instanciar num teste unitário.
///
/// `running_port` é a porta do servidor em execução, ou `None` se não há
/// servidor.
fn config_matches_running(enabled: bool, port: u16, running_port: Option<u16>) -> bool {
    match running_port {
        Some(running) => enabled && running == port,
        None => !enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::config_matches_running as matches;

    #[test]
    fn nothing_to_do_when_state_already_matches() {
        assert!(matches(false, 8722, None), "desligado e parado");
        assert!(matches(true, 8722, Some(8722)), "ligado na porta certa");
    }

    #[test]
    fn enabling_the_checkbox_must_trigger_a_start() {
        // REGRESSÃO: era este o bug de "marquei a opção e nada acontece até
        // reiniciar o app". A versão antiga comparava o config contra um
        // snapshot dele mesmo tirado no mesmo frame do Salvar — mas o
        // checkbox já tinha mudado o valor frames antes, então os dois lados
        // chegavam iguais e o start nunca disparava. Comparar contra o que
        // está RODANDO não tem esse ponto cego.
        assert!(!matches(true, 8722, None));
    }

    #[test]
    fn disabling_must_trigger_a_stop_and_port_change_a_rebind() {
        assert!(!matches(false, 8722, Some(8722)), "precisa parar");
        assert!(!matches(true, 9000, Some(8722)), "precisa rebindar");
    }

    #[test]
    fn a_failed_bind_stays_retryable() {
        // Ligado mas sem servidor é o estado de "a porta estava ocupada".
        // Precisa continuar devolvendo false para que Salvar / Iniciar
        // tentem de novo depois que o usuário liberar a porta.
        assert!(!matches(true, 8722, None));
    }
}
