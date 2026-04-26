// Ownership: `Screen` enum + `AppState` struct + métodos que manipulam
// estado interno (load/refresh/watcher/toast). Os renderizadores de
// painéis em `menu_bar`, `topbar`, `status_bar`, `central` e `modals`
// são `impl` separados sobre `AppState` espalhados pelos submódulos.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::Context;

use crate::build_order::{classify_opening, BuildOrderResult};
use crate::config::AppConfig;
use crate::library::{self, ParsedMeta, ReplayLibrary};
use crate::locale::{t, tf, Language};
use crate::map_image::{self, MapImage};
use crate::replay_state::LoadedReplay;
use crate::tabs::{self, Tab};
use crate::watcher::ReplayWatcher;

/// Janela (em segundos) suficiente para o classificador de abertura
/// produzir um rótulo estável. Alinhada com `T_FOLLOW_UP_END_SECS` em
/// `build_order::opening` e com `ENRICHMENT_PARSE_SECONDS` no scanner
/// da biblioteca — se o parse do `LoadedReplay` cobriu ao menos isto,
/// podemos ingerir a abertura direto, sem disparar o pool de
/// enriquecimento pra parsear o mesmo arquivo de novo.
const OPENING_CLASSIFICATION_WINDOW_SECS: u32 = 300;

use super::{apply_style, install_fonts};

pub(super) const TOAST_TTL: Duration = Duration::from_secs(4);

/// Tela atualmente ativa. A transição é dirigida por intent do usuário,
/// não pelo estado de `loaded` — ao voltar para `Library`, o replay
/// carregado permanece na memória e o usuário pode reentrar na análise.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Library,
    Analysis,
    Rename,
}

pub struct AppState {
    pub config: AppConfig,
    pub loaded: Option<LoadedReplay>,
    pub load_error: Option<String>,
    pub active_tab: Tab,
    pub screen: Screen,
    pub show_settings: bool,
    pub nickname_input: String,
    pub watcher: Option<ReplayWatcher>,
    pub toast: Option<(String, Instant)>,
    pub library: ReplayLibrary,
    pub library_filter: library::LibraryFilter,
    /// Whether the left filter sidebar on the Library screen is expanded.
    /// Toggled by the ☰ button in the Library topbar; persisted only in
    /// session memory (not saved to disk).
    pub library_sidebar_open: bool,
    /// Caminho do replay atualmente *selecionado* na biblioteca (clique
    /// único). Diferente de `loaded`: selecionar apenas alimenta o card
    /// lateral de detalhes; carregar (duplo-clique ou botão "Abrir
    /// análise") atualiza `loaded` e troca para a tela `Analysis`.
    /// `None` colapsa o card de detalhes e devolve a largura à lista.
    pub library_selection: Option<PathBuf>,
    /// Marcação múltipla na biblioteca (checkbox por linha). Usada para
    /// ações em lote como "salvar como…". É estado de UI puro, não
    /// persistido no config — limpa em `refresh_library` (paths podem
    /// desaparecer).
    pub library_selected: HashSet<PathBuf>,
    /// Template aplicado ao nome de destino quando o usuário salva
    /// cópias dos replays marcados. Mesmas variáveis do template de
    /// rename (`{datetime}`, `{map}`, `{p1}`, …). Quando um replay não
    /// tem metadados parseáveis, cai no nome de arquivo original.
    pub library_save_template: String,
    /// Minimapa carregado para `library_selection`. Cache simples: ao
    /// selecionar outra entrada, descarregamos o anterior e reabrimos o
    /// MPQ do novo. `None` significa "não tentado" ou "falhou" — o card
    /// renderiza um placeholder no lugar.
    pub library_selection_minimap: Option<MapImage>,
    /// Caminho do replay para o qual `library_selection_minimap` foi
    /// resolvido. Usado para detectar mudança de seleção e disparar o
    /// recarregamento do minimapa (sem reentrar no MPQ a cada frame).
    pub library_selection_minimap_path: Option<PathBuf>,
    /// Game loop selecionado no slider da aba Timeline (mini-mapa).
    /// Resetado a cada `load_path` para que troca de replay sempre
    /// comece em t=0.
    pub timeline_tab_loop: u32,
    /// Playback state da aba Timeline. `true` = auto-advance do
    /// `timeline_tab_loop` em tempo real, multiplicado por
    /// `timeline_playback_speed`. Clicar no slider não pausa — o usuário
    /// pode scrubar com playback ativo.
    pub timeline_playing: bool,
    /// Multiplicador de velocidade do playback. Gira entre 1× → 2× → 4× →
    /// 8× → 1× ao clicar no botão de velocidade.
    pub timeline_playback_speed: u8,
    /// Opções do plot principal de army (métrica, grouping, checkboxes).
    pub charts_army_opts: tabs::charts::ArmyChartOptions,
    /// Estado do gráfico de produção de worker (jogador selecionado).
    pub charts_worker_prod_opts: tabs::charts::WorkerProductionOptions,
    pub show_about: bool,
    pub timeline_show_heatmap: bool,
    pub timeline_show_creep: bool,
    pub timeline_show_map: bool,
    /// Overlay de Fog of War no minimapa: quando ativo, escurece áreas
    /// sem visão do `timeline_fog_player` no instante atual.
    pub timeline_show_fog: bool,
    /// Slot do jogador cujo ponto de vista é usado pelo overlay de FOG.
    /// Clamp em `players.len() - 1` no consumer.
    pub timeline_fog_player: usize,
    /// Quando o cursor está sobre um chip do `unit_column`, guarda
    /// `(slot_idx, canonical_type)` pra que o minimap desenhe um halo
    /// nas instâncias correspondentes. Resetado a `None` no começo de
    /// cada frame da Timeline — vida do hover ligada ao frame ativo.
    pub timeline_hovered_entity: Option<(usize, String)>,
    /// Template de renomeação em lote.
    pub rename_template: String,
    /// Previews gerados a partir do template + biblioteca.
    pub rename_previews: Vec<(PathBuf, String)>,
    /// Status da última operação de rename.
    pub rename_status: Option<String>,
    /// Carregamento do replay mais recente adiado até o scanner terminar.
    pub pending_load_latest: bool,
    /// Auto-detect pendente do `DateRange` inicial da biblioteca: quando
    /// o config não tem um `library_date_range` persistido, percorremos
    /// as janelas de tempo (Today → ThisWeek → ThisMonth → All) depois
    /// que o scan termina e persistimos a primeira não-vazia. Flag de
    /// sessão previne re-execução no mesmo run; biblioteca vazia NÃO
    /// persiste nada (próximo launch tenta de novo).
    pub pending_date_range_autodetect: bool,
    /// Transient draft of the language picker (first-run modal). We keep
    /// the draft separate from `config.language` so cancelling the
    /// modal leaves the real config alone.
    pub language_draft: Language,
    /// Draft of the "don't show again" checkbox in the startup
    /// disclaimer modal. Only persisted (into
    /// `config.disclaimer_acknowledged`) when the user clicks continue
    /// with the box checked.
    pub disclaimer_dont_show_again: bool,
    /// Session-only flag: once the user clicks continue on the
    /// disclaimer modal, suppress it for the rest of this run even if
    /// they didn't tick "don't show again" (in which case it will
    /// re-appear on the next launch).
    pub disclaimer_dismissed_session: bool,
    /// Índice do jogador de referência na aba Insights. `None` até
    /// o primeiro render pós-load, que resolve pelo nickname do usuário
    /// (cai em 0 se não houver match). Resetado a cada novo replay.
    pub insights_pov: Option<usize>,
}

impl AppState {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        install_fonts(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        apply_style(&cc.egui_ctx, &config);

        let library_filter = library::LibraryFilter::from_config(&config);
        let pending_date_range_autodetect = config.library_date_range.is_none();
        let language_draft = config.language;
        let mut me = Self {
            config,
            loaded: None,
            load_error: None,
            active_tab: Tab::Timeline,
            screen: Screen::Library,
            show_settings: false,
            nickname_input: String::new(),
            watcher: None,
            toast: None,
            library: ReplayLibrary::new(),
            library_filter,
            library_sidebar_open: true,
            library_selection: None,
            library_selected: HashSet::new(),
            library_save_template: crate::rename::DEFAULT_TEMPLATE.to_string(),
            library_selection_minimap: None,
            library_selection_minimap_path: None,
            timeline_tab_loop: 0,
            timeline_playing: false,
            timeline_playback_speed: 1,
            charts_army_opts: tabs::charts::ArmyChartOptions::default(),
            charts_worker_prod_opts: tabs::charts::WorkerProductionOptions::default(),
            show_about: false,
            timeline_show_heatmap: false,
            timeline_show_creep: true,
            timeline_show_map: true,
            timeline_show_fog: false,
            timeline_fog_player: 0,
            timeline_hovered_entity: None,
            rename_template: crate::rename::DEFAULT_TEMPLATE.to_string(),
            rename_previews: Vec::new(),
            rename_status: None,
            pending_load_latest: false,
            pending_date_range_autodetect,
            language_draft,
            disclaimer_dont_show_again: false,
            disclaimer_dismissed_session: false,
            insights_pov: None,
        };
        me.restart_watcher();
        me.refresh_library();
        if me.config.auto_load_latest {
            me.pending_load_latest = true;
        }
        me
    }

    /// Recarrega a biblioteca a partir do diretório de trabalho efetivo
    /// (persistido no config ou auto-detectado a partir do SC2).
    pub(super) fn refresh_library(&mut self) {
        if let Some(dir) = self.config.effective_working_dir() {
            self.library.refresh(&dir);
        }
        // Paths podem ter sumido após o rescan — descarta a marcação.
        self.library_selected.clear();
    }

    /// Copia os replays atualmente marcados na biblioteca (`library_selected`)
    /// para uma pasta escolhida pelo usuário via diálogo nativo. Aplica o
    /// `library_save_template` para gerar o nome de destino; quando o
    /// replay não tem metadados parseáveis (Pending/Failed/Unsupported)
    /// ou o template não pode ser expandido, cai no nome de arquivo
    /// original. No-op se a marcação está vazia ou se o diálogo for
    /// cancelado.
    pub(super) fn copy_selected_replays(&mut self) {
        let lang = self.config.language;
        if self.library_selected.is_empty() {
            return;
        }
        let Some(dest) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        if !dest.exists() {
            if let Err(e) = fs::create_dir_all(&dest) {
                self.set_toast(tf(
                    "toast.copy_mkdir_err",
                    lang,
                    &[("err", &e.to_string())],
                ));
                return;
            }
        }
        let mut ok = 0usize;
        let mut errors: Vec<String> = Vec::new();
        for src in &self.library_selected {
            let target_name = expand_save_name(src, &self.library, &self.library_save_template);
            let Some(target_name) = target_name else { continue };
            let target = dest.join(&target_name);
            match fs::copy(src, &target) {
                Ok(_) => ok += 1,
                Err(e) => errors.push(format!("{}: {e}", src.display())),
            }
        }
        if errors.is_empty() {
            self.set_toast(tf(
                "toast.copy_ok",
                lang,
                &[
                    ("count", &ok.to_string()),
                    ("dir", &dest.display().to_string()),
                ],
            ));
        } else {
            self.set_toast(tf(
                "toast.copy_partial",
                lang,
                &[
                    ("ok", &ok.to_string()),
                    ("err_count", &errors.len().to_string()),
                ],
            ));
            eprintln!("library copy errors:\n{}", errors.join("\n"));
        }
    }

    pub(super) fn try_load_latest(&mut self) {
        // Se o scanner já rodou, usa o resultado dele (sem I/O extra).
        if let Some(p) = self.library.scan_latest.clone() {
            self.load_path(p);
            return;
        }
        let lang = self.config.language;
        let Some(dir) = self.config.effective_working_dir() else {
            self.set_toast(t("toast.no_working_dir", lang).to_string());
            return;
        };
        match crate::utils::find_latest_replay(&dir) {
            Some(p) => self.load_path(p),
            None => self.set_toast(tf(
                "toast.no_replays_found",
                lang,
                &[("dir", &dir.display().to_string())],
            )),
        }
    }

    pub(super) fn load_path(&mut self, p: PathBuf) {
        let max_time = self.config.default_max_time;
        let lang = self.config.language;
        match LoadedReplay::load(&p, max_time) {
            Ok(r) => {
                self.loaded = Some(r);
                self.load_error = None;
                // Reset do scrubbing da aba Timeline: replay novo
                // sempre começa em t=0.
                self.timeline_tab_loop = 0;
                // Pausa ao trocar de replay — evita "perseguir" a
                // transição com playback ligado do replay anterior.
                self.timeline_playing = false;
                self.timeline_playback_speed = 1;
                self.timeline_fog_player = 0;
                // Reset do POV da aba Insights: novo replay
                // re-resolve o default via user_nicknames.
                self.insights_pov = None;
                // Carregar com sucesso sempre transiciona para a Tela
                // Análise — é a única forma de chegar lá.
                self.screen = Screen::Analysis;
            }
            Err(e) => {
                self.load_error = Some(tf(
                    "error.load_failed",
                    lang,
                    &[("path", &p.display().to_string()), ("err", &e.to_string())],
                ));
            }
        }
    }

    pub(super) fn restart_watcher(&mut self) {
        self.watcher = None;
        if !self.config.watch_replays {
            return;
        }
        let Some(dir) = self.config.effective_working_dir() else {
            return;
        };
        match ReplayWatcher::start(&dir) {
            Ok(w) => self.watcher = Some(w),
            Err(e) => eprintln!("watcher: falha ao observar {}: {}", dir.display(), e),
        }
    }

    pub(super) fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// Aplica uma nova seleção (ou limpa) na biblioteca. Carrega o
    /// minimapa correspondente *sincronamente* — custo aceitável (TGA
    /// de minimapa decodifica em milissegundos). Se a entrada não tiver
    /// `cache_handles` cacheados (cache antigo) ou se a resolução falhar,
    /// o minimapa fica `None` e o card mostra um placeholder.
    pub(super) fn set_library_selection(&mut self, sel: Option<PathBuf>) {
        if self.library_selection == sel {
            return;
        }
        self.library_selection = sel.clone();
        self.library_selection_minimap = None;
        self.library_selection_minimap_path = None;
        let Some(path) = sel else { return };
        let Some(entry) = self.library.entries.iter().find(|e| e.path == path) else {
            return;
        };
        let crate::library::MetaState::Parsed(meta) = &entry.meta else {
            return;
        };
        match map_image::load_for_replay(&meta.map, &meta.cache_handles) {
            Ok(img) => {
                self.library_selection_minimap = Some(img);
                self.library_selection_minimap_path = Some(path);
            }
            Err(e) => {
                eprintln!("library minimap: {e}");
            }
        }
    }

    pub(super) fn poll_watcher(&mut self, ctx: &Context) {
        let Some(w) = self.watcher.as_ref() else { return };
        if let Some(path) = w.poll_latest() {
            let lang = self.config.language;
            let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();

            if self.config.auto_load_on_new_replay {
                self.load_path(path.clone());
                // load_path pode ter falhado (replay corrompido, não-1v1…).
                // Só derivamos meta quando o LoadedReplay atual é
                // exatamente este path — caso contrário caímos no
                // ingest_pending para que o pool da biblioteca tente.
                let derived = self
                    .loaded
                    .as_ref()
                    .filter(|l| l.path == path)
                    .and_then(|l| build_ingest_meta(l, self.config.default_max_time));
                match derived {
                    Some(meta) => self.library.ingest_parsed(path.clone(), mtime, meta),
                    None => self.library.ingest_pending(path.clone(), mtime),
                }
                self.set_toast(tf(
                    "toast.new_replay_loaded",
                    lang,
                    &[("file", &file_name(&path))],
                ));
            } else {
                self.library.ingest_pending(path.clone(), mtime);
                self.set_toast(tf(
                    "toast.new_replay_available",
                    lang,
                    &[("file", &file_name(&path))],
                ));
            }
            ctx.request_repaint();
        }
    }

    pub(super) fn toast_visible(&self) -> Option<&str> {
        let (msg, t) = self.toast.as_ref()?;
        if t.elapsed() < TOAST_TTL {
            Some(msg.as_str())
        } else {
            None
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Resolve o nome de destino para um replay marcado: aplica o template
/// quando o replay tem metadados parseáveis, caso contrário cai no nome
/// de arquivo original. Devolve `None` apenas se o path não tiver
/// componente final (nunca deveria acontecer pra paths vindos da
/// biblioteca).
fn expand_save_name(src: &Path, library: &ReplayLibrary, template: &str) -> Option<String> {
    let entry = library.entries.iter().find(|e| e.path == src);
    if let Some(entry) = entry {
        if let crate::library::MetaState::Parsed(meta) = &entry.meta {
            if let Some(name) = crate::rename::expand_template(template, meta) {
                return Some(name);
            }
        }
    }
    src.file_name().map(|s| s.to_string_lossy().into_owned())
}

/// Deriva `ParsedMeta` de um `LoadedReplay` pronto. Preenche `opening`
/// diretamente do `build_order` já extraído quando o parse cobriu a
/// janela completa de classificação (`OPENING_CLASSIFICATION_WINDOW_SECS`),
/// caso contrário deixa `None` para que o pool de enriquecimento da
/// biblioteca complete depois parseando só os 5 min necessários.
fn build_ingest_meta(loaded: &LoadedReplay, default_max_time: u32) -> Option<ParsedMeta> {
    let mut meta = ParsedMeta::from_timeline(&loaded.timeline)?;
    let cover_window = default_max_time == 0
        || default_max_time >= OPENING_CLASSIFICATION_WINDOW_SECS
        || loaded.timeline.duration_seconds <= default_max_time;
    if cover_window {
        fill_openings_from_build_order(&mut meta, loaded.build_order.as_ref());
    }
    Some(meta)
}

fn fill_openings_from_build_order(meta: &mut ParsedMeta, bo: Option<&BuildOrderResult>) {
    let Some(bo) = bo else { return };
    for (i, p) in bo.players.iter().enumerate() {
        if let Some(pm) = meta.players.get_mut(i) {
            pm.opening = Some(classify_opening(p, bo.loops_per_second).to_display_string());
        }
    }
}
