// Configuração persistente do aplicativo GUI.
//
// Gravada em YAML em dirs::config_dir()/sc2-replay-utils/config.yaml,
// reusando a dep serde_yml que já é usada pelo CLI.
//
// #[serde(default)] no struct garante que adicionar novos campos no futuro
// não quebra arquivos de config antigos — os campos ausentes usam Default.

#[cfg(not(target_arch = "wasm32"))]
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::library::DateRange;
use crate::locale::Language;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Diretório de trabalho: onde o app procura replays para listar, carregar
    /// "mais recente" e observar via file watcher. Se `None`, o app usa o
    /// diretório padrão do SC2 detectado por `utils::sc2_default_dir()`.
    pub working_dir: Option<PathBuf>,
    /// Nicks do próprio usuário. Usado para identificar visualmente o próprio jogador.
    pub user_nicknames: Vec<String>,
    /// Limite padrão de tempo em segundos para os extracts (0 = sem limite).
    pub default_max_time: u32,
    /// Carregar o replay mais recente automaticamente ao abrir o app.
    pub auto_load_latest: bool,
    /// Habilitar o file watcher na pasta do SC2.
    pub watch_replays: bool,
    /// Ao detectar novo replay via watcher, carregar automaticamente.
    pub auto_load_on_new_replay: bool,
    /// Quando ligado, dispara classificação de aberturas em lote durante o
    /// scan inicial da biblioteca (cache hits ainda pendentes + replays
    /// recém-parseados). Quando desligado (default), o lote fica suspenso
    /// até o usuário clicar em "Classificar pendentes" nas Configurações.
    /// Não afeta o caminho de carregamento individual / watcher de replay
    /// novo — esses sempre classificam imediatamente.
    pub auto_classify_on_scan: bool,
    /// Tamanho base da fonte em pontos lógicos (HiDPI é tratado pelo egui).
    pub font_size: f32,
    /// UI language. Applies to menus, labels, tooltips, toasts and
    /// unit/structure names. Default English; the first-run language
    /// prompt persists the user's pick here.
    pub language: Language,
    /// `true` once the user has explicitly chosen a language. While
    /// `false`, the app shows a modal prompt on startup. Setting this
    /// retroactively lets us replace the old build-order-only locale
    /// selector without re-prompting users who already had a language
    /// configured.
    pub language_selected: bool,
    /// `true` once the user has explicitly checked "don't show again"
    /// on the startup disclaimer. While `false`, the disclaimer modal
    /// is shown on every launch (the user can always re-read it via
    /// Help → About). The same content is mirrored in the About window.
    pub disclaimer_acknowledged: bool,
    /// `true` once the user clicked Save on the first-run settings
    /// screen. While `false`, the settings window is shown as a
    /// blocking modal on launch with no dismiss path other than Save.
    /// New field with serde default `false` — existing configs load
    /// with this unset and go through the flow, but keep all their
    /// other values.
    pub settings_confirmed: bool,
    /// Filtro de período padrão da biblioteca (salvo entre sessões).
    /// `None` até o usuário (ou o auto-detect do primeiro launch) fixar
    /// uma escolha. Configs antigos que já tinham o valor serializado
    /// deserializam como `Some(...)` e pulam o auto-detect.
    pub library_date_range: Option<DateRange>,
    /// Filtro de raça do usuário na biblioteca (salvo entre sessões).
    /// `None` = todas as raças. Valores válidos: `'T'`, `'P'`, `'Z'`.
    pub library_race: Option<char>,
    /// Minuto do jogo usado pelo card de potencial de workers da aba
    /// Insights. Persistido pra permitir ajuste fino editando o YAML
    /// direto — não há UI intencionalmente, pra desencorajar mudanças
    /// casuais que mudem a baseline dos insights.
    #[serde(
        alias = "discovery_worker_minutes",
        default = "default_insight_worker_minutes"
    )]
    pub insight_worker_minutes: u32,
}

fn default_insight_worker_minutes() -> u32 {
    6
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            working_dir: None,
            user_nicknames: Vec::new(),
            default_max_time: 0,
            auto_load_latest: false,
            watch_replays: true,
            auto_load_on_new_replay: true,
            auto_classify_on_scan: false,
            font_size: 14.0,
            language: Language::default(),
            language_selected: false,
            disclaimer_acknowledged: false,
            settings_confirmed: false,
            library_date_range: None,
            library_race: None,
            insight_worker_minutes: default_insight_worker_minutes(),
        }
    }
}

impl AppConfig {
    /// Caminho do arquivo de configuração. Retorna None se `dirs::config_dir()`
    /// não conseguir resolver (muito raro).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("sc2-replay-utils").join("config.yaml"))
    }

    /// Carrega do disco. Falhas silenciosas viram Default + log em stderr.
    /// Wasm: sempre `Default::default()` — não temos disco; settings ficam
    /// efêmeros por sessão (TODO: pode evoluir pra `eframe::Storage`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        if !path.exists() {
            return Self::default();
        }
        match fs::read_to_string(&path) {
            Ok(text) => match serde_yml::from_str::<Self>(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("config: falha ao parsear {}: {}", path.display(), e);
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("config: falha ao ler {}: {}", path.display(), e);
                Self::default()
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn load() -> Self {
        let mut cfg = Self::default();
        // Em web não temos modal de language picker (UI nativa) nem flow de
        // first-run, então marcamos as flags como confirmadas pra cair direto
        // no Analysis screen.
        cfg.language_selected = true;
        cfg.disclaimer_acknowledged = true;
        cfg.settings_confirmed = true;
        cfg
    }

    /// Grava em disco, criando diretórios intermediários se necessário.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or_else(|| "config_dir indisponível".to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
        }
        let yaml = serde_yml::to_string(self).map_err(|e| format!("serialize: {e}"))?;
        fs::write(&path, yaml).map_err(|e| format!("write {}: {}", path.display(), e))?;
        Ok(())
    }

    /// Wasm: no-op, mudanças ficam só na sessão.
    #[cfg(target_arch = "wasm32")]
    pub fn save(&self) -> Result<(), String> {
        Ok(())
    }

    /// Retorna o diretório de trabalho efetivo: o valor persistido no config,
    /// ou — se vazio — o diretório padrão do SC2 detectado automaticamente.
    /// Esse é o único path que a UI usa para listar/observar replays.
    pub fn effective_working_dir(&self) -> Option<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.working_dir
                .clone()
                .or_else(crate::utils::sc2_default_dir)
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.working_dir.clone()
        }
    }

    /// `true` se `name` (case-insensitive) bate com algum nickname cadastrado.
    pub fn is_user(&self, name: &str) -> bool {
        self.user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
    }
}
