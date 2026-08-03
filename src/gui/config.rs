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
    /// Filtro "somente ladder" da biblioteca (salvo entre sessões).
    /// Default `false` — quem já usa o app continua vendo customs e
    /// partidas contra IA até optar por escondê-las. Não afeta o
    /// overlay, que é sempre restrito a ladder 1v1.
    pub library_ladder_only: bool,
    /// Minuto do jogo usado pelo card de potencial de workers da aba
    /// Insights. Persistido pra permitir ajuste fino editando o YAML
    /// direto — não há UI intencionalmente, pra desencorajar mudanças
    /// casuais que mudem a baseline dos insights.
    #[serde(
        alias = "discovery_worker_minutes",
        default = "default_insight_worker_minutes"
    )]
    pub insight_worker_minutes: u32,
    /// Liga o servidor HTTP do overlay de transmissão (fonte Navegador do
    /// OBS). Escuta apenas em `127.0.0.1` — nunca alcançável pela rede.
    /// Default desligado para que nenhuma instalação existente ganhe um
    /// socket ouvindo só por atualizar o app.
    pub overlay_enabled: bool,
    /// Porta TCP do overlay. Mude apenas em caso de conflito: o OBS guarda
    /// a URL completa na fonte, então trocar a porta exige atualizar a
    /// fonte lá também (por isso o app nunca escolhe outra porta sozinho).
    #[serde(default = "default_overlay_port")]
    pub overlay_port: u16,
    /// Qual dos `user_nicknames` o overlay trata como "eu". `None` (default)
    /// = todos, que é o comportamento de sempre.
    ///
    /// Existe para quem tem mais de uma conta e transmite em uma só: sem
    /// isso, uma partida da outra conta entraria no placar do dia, no saldo
    /// de MMR e no recorde por raça ao vivo. A UI só oferece nicks que já
    /// estão em `user_nicknames`; um valor órfão (nick removido depois, ou
    /// YAML editado à mão) cai de volta em "todos" via
    /// [`AppConfig::overlay_nickname_effective`] em vez de zerar o overlay.
    ///
    /// Vale só para o overlay — a biblioteca continua reconhecendo todos os
    /// nicknames do usuário.
    pub overlay_nickname: Option<String>,
}

fn default_overlay_port() -> u16 {
    8722
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
            library_ladder_only: false,
            insight_worker_minutes: default_insight_worker_minutes(),
            overlay_enabled: false,
            overlay_port: default_overlay_port(),
            overlay_nickname: None,
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

    /// Nickname escolhido para o overlay, **desde que ainda exista** na lista
    /// de nicknames. `None` = sem restrição (usar todos).
    ///
    /// O casamento é case-insensitive e o valor devolvido é o da lista, não
    /// o que está salvo em `overlay_nickname`: a grafia exibida acompanha o
    /// que o usuário cadastrou em Apelidos.
    ///
    /// A revalidação acontece aqui, e não só na UI, porque `config.yaml` é
    /// editável à mão e um nick órfão não pode significar "nenhuma partida é
    /// minha" — o overlay ficaria vazio sem nada na tela explicando por quê.
    pub fn overlay_nickname_effective(&self) -> Option<&str> {
        let chosen = self.overlay_nickname.as_deref()?;
        self.user_nicknames
            .iter()
            .find(|n| n.eq_ignore_ascii_case(chosen))
            .map(|n| n.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(nicks: &[&str], chosen: Option<&str>) -> AppConfig {
        AppConfig {
            user_nicknames: nicks.iter().map(|n| n.to_string()).collect(),
            overlay_nickname: chosen.map(|n| n.to_string()),
            ..AppConfig::default()
        }
    }

    #[test]
    fn no_choice_means_no_restriction() {
        assert_eq!(cfg(&["Kerrigan", "Alt"], None).overlay_nickname_effective(), None);
    }

    #[test]
    fn a_chosen_nickname_resolves_to_the_spelling_from_the_list() {
        let c = cfg(&["Kerrigan", "Alt"], Some("kerrigan"));
        assert_eq!(c.overlay_nickname_effective(), Some("Kerrigan"));
    }

    /// Config gravado por uma versão anterior do app: o campo não existe no
    /// YAML. Precisa carregar como "sem restrição" em vez de falhar o parse
    /// inteiro e derrubar o usuário para o `Default` (perdendo pasta,
    /// nicknames e idioma de uma vez).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn an_older_config_without_the_field_still_loads() {
        let yaml = "user_nicknames:\n  - Kerrigan\noverlay_enabled: true\noverlay_port: 8722\n";
        let c: AppConfig = serde_yml::from_str(yaml).expect("config antigo deve carregar");
        assert_eq!(c.overlay_nickname, None);
        assert_eq!(c.user_nicknames, vec!["Kerrigan".to_string()]);
        assert!(c.overlay_enabled);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_choice_survives_a_save_load_round_trip() {
        let saved = serde_yml::to_string(&cfg(&["Kerrigan", "Smurf"], Some("Smurf"))).unwrap();
        let back: AppConfig = serde_yml::from_str(&saved).unwrap();
        assert_eq!(back.overlay_nickname.as_deref(), Some("Smurf"));
    }

    #[test]
    fn an_orphaned_choice_falls_back_to_all_nicknames() {
        // Nick removido da lista depois de escolhido, ou YAML editado à mão.
        // Precisa virar "todos" — nunca "nenhum", que deixaria o overlay
        // vazio sem explicação.
        assert_eq!(cfg(&["Kerrigan"], Some("Removed")).overlay_nickname_effective(), None);
        assert_eq!(cfg(&[], Some("Kerrigan")).overlay_nickname_effective(), None);
    }
}
