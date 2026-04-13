// Configuração persistente do aplicativo GUI.
//
// Gravada em YAML em dirs::config_dir()/sc2-replay-utils/config.yaml,
// reusando a dep serde_yml que já é usada pelo CLI.
//
// #[serde(default)] no struct garante que adicionar novos campos no futuro
// não quebra arquivos de config antigos — os campos ausentes usam Default.

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
    /// Tema escuro (true) ou claro (false).
    pub dark_mode: bool,
    /// Carregar o replay mais recente automaticamente ao abrir o app.
    pub auto_load_latest: bool,
    /// Habilitar o file watcher na pasta do SC2.
    pub watch_replays: bool,
    /// Ao detectar novo replay via watcher, carregar automaticamente.
    pub auto_load_on_new_replay: bool,
    /// Tamanho base da fonte em pontos lógicos (HiDPI é tratado pelo egui).
    pub font_size: f32,
    /// Idioma da UI para nomes de unidades/pesquisas.
    pub language: Language,
    /// Filtro de período padrão da biblioteca (salvo entre sessões).
    pub library_date_range: DateRange,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            working_dir: None,
            user_nicknames: Vec::new(),
            default_max_time: 0,
            dark_mode: true,
            auto_load_latest: false,
            watch_replays: true,
            auto_load_on_new_replay: true,
            font_size: 14.0,
            language: Language::default(),
            library_date_range: DateRange::default(),
        }
    }
}

impl AppConfig {
    /// Caminho do arquivo de configuração. Retorna None se `dirs::config_dir()`
    /// não conseguir resolver (muito raro).
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("sc2-replay-utils").join("config.yaml"))
    }

    /// Carrega do disco. Falhas silenciosas viram Default + log em stderr.
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

    /// Grava em disco, criando diretórios intermediários se necessário.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or_else(|| "config_dir indisponível".to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
        }
        let yaml = serde_yml::to_string(self).map_err(|e| format!("serialize: {e}"))?;
        fs::write(&path, yaml).map_err(|e| format!("write {}: {}", path.display(), e))?;
        Ok(())
    }

    /// Retorna o diretório de trabalho efetivo: o valor persistido no config,
    /// ou — se vazio — o diretório padrão do SC2 detectado automaticamente.
    /// Esse é o único path que a UI usa para listar/observar replays.
    pub fn effective_working_dir(&self) -> Option<PathBuf> {
        self.working_dir
            .clone()
            .or_else(crate::utils::sc2_default_dir)
    }

    /// `true` se `name` (case-insensitive) bate com algum nickname cadastrado.
    pub fn is_user(&self, name: &str) -> bool {
        self.user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
    }
}
