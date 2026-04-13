// Biblioteca de replays: varre o `working_dir` do usuário, lista os
// arquivos .SC2Replay e parseia os metadados de cada um em threads
// worker. A UI consulta o estado (Pending/Parsed/Failed) e preenche
// progressivamente conforme os resultados chegam.
//
// Modelo de paralelismo: a cada `refresh()` gastamos um "burst" de
// workers dedicados a essa batelada. Se a batelada tem mais que
// `PARALLEL_THRESHOLD` arquivos, subimos um pool de N threads
// (N = núcleos disponíveis, clampado em [2, 8]); abaixo disso fica
// uma única thread — mais simples e suficiente para bibliotecas
// pequenas. Os workers compartilham o mesmo canal de trabalho via
// `Arc<Mutex<Receiver>>`: a contenção é desprezível porque cada item
// leva ordens de grandeza mais tempo para parsear do que para tirar
// da fila. Ao fim da batelada, o `Sender` é dropado, os `recv()`
// retornam `Err` e os workers encerram naturalmente.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use egui::{Color32, Context, RichText, ScrollArea, Sense, Ui};

use crate::config::AppConfig;
use crate::replay::parse_replay;

// ── Filtro e ordenação ───────────────────────────────────────────────

#[derive(Default, PartialEq, Clone, Copy)]
pub enum OutcomeFilter {
    #[default]
    All,
    Wins,
    Losses,
}

#[derive(Default, Debug, PartialEq, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum DateRange {
    All,
    Today,
    #[default]
    ThisWeek,
    ThisMonth,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum SortOrder {
    #[default]
    Date,
    Duration,
    Mmr,
    Map,
}

pub struct LibraryFilter {
    pub search: String,
    pub race: Option<char>,
    pub outcome: OutcomeFilter,
    pub date_range: DateRange,
    pub sort: SortOrder,
    pub sort_ascending: bool,
}

impl Default for LibraryFilter {
    fn default() -> Self {
        Self {
            search: String::new(),
            race: None,
            outcome: OutcomeFilter::All,
            date_range: DateRange::default(),
            sort: SortOrder::Date,
            sort_ascending: false,
        }
    }
}

impl LibraryFilter {
    /// Inicializa o filtro restaurando preferências salvas no config.
    pub fn from_config(config: &crate::config::AppConfig) -> Self {
        Self {
            date_range: config.library_date_range,
            ..Self::default()
        }
    }
}

/// Acima deste número de novos arquivos a parsear em um único `refresh`,
/// a biblioteca passa a usar um pool multi-thread. Abaixo disso, um
/// worker único é suficiente.
const PARALLEL_THRESHOLD: usize = 100;

/// Limite superior do pool multi-thread (protege contra máquinas com
/// muitos núcleos onde o I/O do disco vira gargalo antes da CPU).
const MAX_WORKERS: usize = 8;

/// Metadados mínimos exibidos na biblioteca.
#[derive(Clone)]
pub struct ParsedMeta {
    pub map: String,
    pub datetime: String,
    pub duration_seconds: u32,
    pub game_loops: u32,
    pub players: Vec<PlayerMeta>,
}

#[derive(Clone)]
pub struct PlayerMeta {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub result: String,
}

#[derive(Clone)]
pub enum MetaState {
    Pending,
    Parsed(ParsedMeta),
    /// Replay válido, porém com número de jogadores ≠ 2. O app só
    /// suporta 1v1, então esses entries ficam visíveis mas não
    /// clicáveis. A string contém uma descrição curta (e.g.
    /// "não é 1v1 (4 jogadores)") para exibir na UI.
    Unsupported(String),
    Failed(String),
}

impl MetaState {
    fn is_loadable(&self) -> bool {
        matches!(self, MetaState::Parsed(_))
    }
}

pub struct LibraryEntry {
    pub path: PathBuf,
    pub filename: String,
    pub mtime: Option<SystemTime>,
    pub meta: MetaState,
}

/// Resultado final (já classificado) do parser para uma entrada.
/// `Pending` nunca aparece aqui — é o estado inicial antes do worker.
enum ParseOutcome {
    Parsed(ParsedMeta),
    Unsupported(String),
    Failed(String),
}

/// Mensagem enviada pelo worker de volta para a UI.
struct LibraryResult {
    path: PathBuf,
    mtime: Option<SystemTime>,
    outcome: ParseOutcome,
}

/// Arquivo descoberto pelo scanner de diretório em background.
struct ScanResult {
    path: PathBuf,
    filename: String,
    mtime: Option<SystemTime>,
}

/// Mensagem enviada pelo scanner de diretório em background.
enum ScanMessage {
    Found(ScanResult),
    /// Varredura concluída. Contém o replay mais recente encontrado.
    Done { latest: Option<(PathBuf, SystemTime)> },
}

pub struct ReplayLibrary {
    pub entries: Vec<LibraryEntry>,
    pub working_dir: Option<PathBuf>,
    /// Cache por caminho — preserva resultados entre refreshes. Guarda
    /// apenas estados "finais e estáveis" (`Parsed` e `Unsupported`);
    /// `Failed` e `Pending` nunca entram aqui — falhas são retentadas.
    /// O `SystemTime` é o mtime do arquivo quando foi parseado, usado
    /// para invalidar a entrada se o arquivo mudar.
    cache: HashMap<PathBuf, (SystemTime, MetaState)>,
    /// `true` quando o cache em memória diverge do que está em disco.
    cache_dirty: bool,
    /// Canal pelo qual os workers enviam resultados para a UI. Os
    /// workers clonam `tx_result`; o library retém o `Receiver`.
    tx_result: Sender<LibraryResult>,
    rx_result: Receiver<LibraryResult>,
    /// Canal de recepção de arquivos descobertos pelo scanner background.
    rx_scan: Option<Receiver<ScanMessage>>,
    /// `true` enquanto o scanner de diretório está rodando.
    pub scanning: bool,
    /// Replay mais recente encontrado pelo scanner (para `try_load_latest`).
    pub scan_latest: Option<PathBuf>,
    /// Acumulador de arquivos que precisam de parsing, preenchido
    /// progressivamente pelo scanner e despachado em lotes.
    scan_parse_queue: Vec<(PathBuf, Option<SystemTime>)>,
}

impl ReplayLibrary {
    pub fn new() -> Self {
        let (tx_result, rx_result) = mpsc::channel::<LibraryResult>();
        let cache = crate::cache::load();
        Self {
            entries: Vec::new(),
            working_dir: None,
            cache,
            cache_dirty: false,
            tx_result,
            rx_result,
            rx_scan: None,
            scanning: false,
            scan_latest: None,
            scan_parse_queue: Vec::new(),
        }
    }

    /// Recarrega a lista a partir do diretório informado. Inicia um
    /// scanner em background que descobre arquivos progressivamente —
    /// a UI não trava mesmo com dezenas de milhares de replays.
    pub fn refresh(&mut self, dir: &Path) {
        self.working_dir = Some(dir.to_path_buf());

        // Cancela scanner anterior (se houver): dropar o Receiver faz
        // o send() do scanner falhar e a thread encerrar.
        self.rx_scan = None;
        self.entries.clear();
        self.scan_parse_queue.clear();
        self.scan_latest = None;

        let (tx_scan, rx_scan) = mpsc::channel::<ScanMessage>();
        self.rx_scan = Some(rx_scan);
        self.scanning = true;

        let dir = dir.to_path_buf();
        let _ = thread::Builder::new()
            .name("replay-library-scanner".into())
            .spawn(move || {
                let mut latest: Option<(PathBuf, SystemTime)> = None;
                let mut queue = vec![dir];

                while let Some(d) = queue.pop() {
                    let entries = match fs::read_dir(&d) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            queue.push(path);
                            continue;
                        }
                        if !path
                            .extension()
                            .map_or(false, |ext| ext.eq_ignore_ascii_case("SC2Replay"))
                        {
                            continue;
                        }
                        let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
                        let filename = path
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());

                        if let Some(mt) = mtime {
                            if latest.as_ref().map_or(true, |(_, t)| mt > *t) {
                                latest = Some((path.clone(), mt));
                            }
                        }

                        if tx_scan
                            .send(ScanMessage::Found(ScanResult {
                                path,
                                filename,
                                mtime,
                            }))
                            .is_err()
                        {
                            return; // Receiver dropado (novo refresh), sair.
                        }
                    }
                }

                let _ = tx_scan.send(ScanMessage::Done { latest });
            });
    }

    /// Sobe um pool efêmero de workers para processar `paths` e retorna
    /// imediatamente. Os workers encerram sozinhos quando a fila esvazia
    /// (drop do `tx_work` ao final desta função fecha o canal de entrada).
    fn spawn_parse_burst(&self, paths: Vec<(PathBuf, Option<SystemTime>)>) {
        let n = paths.len();
        let n_workers = if n > PARALLEL_THRESHOLD {
            thread::available_parallelism()
                .map(|v| v.get().clamp(2, MAX_WORKERS))
                .unwrap_or(4)
        } else {
            1
        };

        let (tx_work, rx_work) = mpsc::channel::<(PathBuf, Option<SystemTime>)>();
        let rx_work = Arc::new(Mutex::new(rx_work));

        for i in 0..n_workers {
            let rx = Arc::clone(&rx_work);
            let tx = self.tx_result.clone();
            let _ = thread::Builder::new()
                .name(format!("replay-library-parser-{i}"))
                .spawn(move || loop {
                    let next = {
                        let guard = match rx.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        guard.recv()
                    };
                    match next {
                        Ok((p, mtime)) => {
                            let outcome = parse_meta(&p);
                            if tx
                                .send(LibraryResult {
                                    path: p,
                                    mtime,
                                    outcome,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                });
        }

        for item in paths {
            let _ = tx_work.send(item);
        }
    }

    /// Drena resultados prontos dos workers e do scanner. Retorna `true`
    /// se alguma entrada foi atualizada (para a UI pedir repaint).
    pub fn poll(&mut self) -> bool {
        let mut updated = false;

        // Fase 1: Drena arquivos descobertos pelo scanner background.
        if self.rx_scan.is_some() {
            for _ in 0..500 {
                let msg = match self.rx_scan.as_ref().unwrap().try_recv() {
                    Ok(m) => m,
                    Err(_) => break,
                };
                match msg {
                    ScanMessage::Found(result) => {
                        let meta = match (self.cache.get(&result.path), result.mtime) {
                            (Some((cached_mtime, state)), Some(mt)) if *cached_mtime == mt => {
                                state.clone()
                            }
                            _ => {
                                self.scan_parse_queue
                                    .push((result.path.clone(), result.mtime));
                                MetaState::Pending
                            }
                        };
                        self.entries.push(LibraryEntry {
                            path: result.path,
                            filename: result.filename,
                            mtime: result.mtime,
                            meta,
                        });
                        updated = true;
                    }
                    ScanMessage::Done { latest } => {
                        self.scanning = false;
                        self.rx_scan = None;
                        self.scan_latest = latest.map(|(p, _)| p);
                        // Ordena por mtime decrescente agora que temos tudo.
                        self.entries.sort_by(|a, b| b.mtime.cmp(&a.mtime));
                        // Despacha remanescentes para parsing.
                        if !self.scan_parse_queue.is_empty() {
                            let batch = std::mem::take(&mut self.scan_parse_queue);
                            self.spawn_parse_burst(batch);
                        }
                        updated = true;
                        break;
                    }
                }
            }
            // Despacha lotes intermediários para começar parsing cedo.
            if self.scan_parse_queue.len() >= 200 {
                let batch = std::mem::take(&mut self.scan_parse_queue);
                self.spawn_parse_burst(batch);
            }
        }

        // Fase 2: Drena resultados de parsing dos workers.
        while let Ok(msg) = self.rx_result.try_recv() {
            let state = match msg.outcome {
                ParseOutcome::Parsed(meta) => {
                    let st = MetaState::Parsed(meta);
                    if let Some(mt) = msg.mtime {
                        self.cache.insert(msg.path.clone(), (mt, st.clone()));
                        self.cache_dirty = true;
                    }
                    st
                }
                ParseOutcome::Unsupported(reason) => {
                    let st = MetaState::Unsupported(reason);
                    if let Some(mt) = msg.mtime {
                        self.cache.insert(msg.path.clone(), (mt, st.clone()));
                        self.cache_dirty = true;
                    }
                    st
                }
                ParseOutcome::Failed(e) => MetaState::Failed(e),
            };
            if let Some(entry) = self.entries.iter_mut().find(|e| e.path == msg.path) {
                entry.meta = state;
                updated = true;
            }
        }
        // Salva o cache quando todos os workers terminaram.
        if self.cache_dirty && !self.scanning && self.pending_count() == 0 {
            self.save_cache();
        }
        updated
    }

    /// Persiste o cache em disco (se houver mudanças pendentes).
    pub fn save_cache(&mut self) {
        if self.cache_dirty {
            crate::cache::save(&self.cache);
            self.cache_dirty = false;
        }
    }

    /// Quantas entradas ainda estão em Pending (para mostrar barra de progresso).
    pub fn pending_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.meta, MetaState::Pending))
            .count()
    }
}

fn parse_meta(path: &Path) -> ParseOutcome {
    // max_time=1 evita processar a maior parte dos eventos. Só precisamos
    // dos metadados (map, datetime, game_loops, jogadores).
    let data = match parse_replay(path, 1) {
        Ok(d) => d,
        Err(e) => return ParseOutcome::Failed(e),
    };
    if data.players.len() != 2 {
        return ParseOutcome::Unsupported(format!(
            "não é 1v1 ({} jogadores)",
            data.players.len()
        ));
    }
    ParseOutcome::Parsed(ParsedMeta {
        map: data.map,
        datetime: data.datetime,
        duration_seconds: data.duration_seconds,
        game_loops: data.game_loops,
        players: data
            .players
            .into_iter()
            .map(|p| PlayerMeta {
                name: p.name,
                race: p.race,
                mmr: p.mmr,
                result: p.result.clone().unwrap_or_default(),
            })
            .collect(),
    })
}

// ── UI ────────────────────────────────────────────────────────────────────────

/// Ação solicitada pelo usuário ao interagir com o painel.
pub enum LibraryAction {
    None,
    Load(PathBuf),
    Refresh,
    PickWorkingDir(PathBuf),
    OpenRename,
    SaveDateRange(DateRange),
}

pub fn show(
    ui: &mut Ui,
    library: &ReplayLibrary,
    current_path: Option<&Path>,
    config: &AppConfig,
    filter: &mut LibraryFilter,
) -> LibraryAction {
    let mut action = LibraryAction::None;

    // ── Header ───────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.heading("Biblioteca");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("↻").on_hover_text("Recarregar lista").clicked() {
                action = LibraryAction::Refresh;
            }
            if ui.small_button("🔎").on_hover_text("Zoom / configurações").clicked() {}
            if ui.small_button("✏").on_hover_text("Renomear replays em lote").clicked() {
                action = LibraryAction::OpenRename;
            }
            if ui
                .small_button("📂")
                .on_hover_text("Escolher diretório de trabalho")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    action = LibraryAction::PickWorkingDir(p);
                }
            }
        });
    });

    match library.working_dir.as_ref() {
        Some(dir) => {
            ui.small(
                RichText::new(format!("📁 {}", dir.display()))
                    .color(Color32::from_gray(120)),
            );
        }
        None => {
            ui.small(RichText::new("Diretório não definido").italics());
        }
    }

    ui.add_space(4.0);

    // ── Barra de busca + contagem/sort ───────────────────────────────
    ui.horizontal(|ui| {
        ui.label("🔎");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut filter.search)
                .hint_text("Buscar jogador, mapa ou matchup…")
                .desired_width(ui.available_width() - 150.0),
        );
        if !filter.search.is_empty() && resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            filter.search.clear();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let total = library.entries.len();
            let sort_label = match filter.sort {
                SortOrder::Date => "Data",
                SortOrder::Duration => "Duração",
                SortOrder::Mmr => "MMR",
                SortOrder::Map => "Mapa",
            };
            let arrow = if filter.sort_ascending { "↑" } else { "↓" };
            egui::ComboBox::from_id_salt("library_sort")
                .selected_text(format!("{total} replays {arrow}"))
                .width(120.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut filter.sort, SortOrder::Date, "Data");
                    ui.selectable_value(&mut filter.sort, SortOrder::Duration, "Duração");
                    ui.selectable_value(&mut filter.sort, SortOrder::Mmr, "MMR");
                    ui.selectable_value(&mut filter.sort, SortOrder::Map, "Mapa");
                    ui.separator();
                    let asc_label = if filter.sort_ascending { "▸ Crescente" } else { "  Crescente" };
                    let desc_label = if !filter.sort_ascending { "▸ Decrescente" } else { "  Decrescente" };
                    if ui.selectable_label(filter.sort_ascending, asc_label).clicked() {
                        filter.sort_ascending = true;
                    }
                    if ui.selectable_label(!filter.sort_ascending, desc_label).clicked() {
                        filter.sort_ascending = false;
                    }
                });
            let _ = sort_label; // utilizado no ComboBox acima
        });
    });

    ui.add_space(2.0);

    // ── Chips de filtro rápido ────────────────────────────────────────
    let has_nicknames = !config.user_nicknames.is_empty();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        let todos_active = filter.race.is_none()
            && filter.outcome == OutcomeFilter::All
            && filter.date_range == DateRange::All;
        if chip(ui, "Todos", todos_active, None).clicked() {
            filter.race = None;
            filter.outcome = OutcomeFilter::All;
            filter.date_range = DateRange::All;
        }

        ui.add_space(4.0);

        for (label, letter, color) in [
            ("Terran", 'T', RACE_COLOR_TERRAN),
            ("Protoss", 'P', RACE_COLOR_PROTOSS),
            ("Zerg", 'Z', RACE_COLOR_ZERG),
        ] {
            let selected = filter.race == Some(letter);
            let resp = chip(ui, label, selected, Some(color));
            if resp.clicked() && has_nicknames {
                filter.race = if selected { None } else { Some(letter) };
            }
            if !has_nicknames {
                resp.on_hover_text("Configure seus nicknames para filtrar por raça");
            }
        }

        ui.add_space(4.0);

        let wins_selected = filter.outcome == OutcomeFilter::Wins;
        let resp = chip(ui, "Vitórias", wins_selected, Some(Color32::from_rgb(80, 180, 80)));
        if resp.clicked() && has_nicknames {
            filter.outcome = if wins_selected { OutcomeFilter::All } else { OutcomeFilter::Wins };
        }
        if !has_nicknames {
            resp.on_hover_text("Configure seus nicknames para filtrar por resultado");
        }

        let losses_selected = filter.outcome == OutcomeFilter::Losses;
        let resp = chip(ui, "Derrotas", losses_selected, Some(Color32::from_rgb(180, 80, 80)));
        if resp.clicked() && has_nicknames {
            filter.outcome = if losses_selected { OutcomeFilter::All } else { OutcomeFilter::Losses };
        }
        if !has_nicknames {
            resp.on_hover_text("Configure seus nicknames para filtrar por resultado");
        }

        ui.add_space(4.0);

        let prev_date_range = filter.date_range;
        let date_label = match filter.date_range {
            DateRange::All => "Sempre",
            DateRange::Today => "Hoje",
            DateRange::ThisWeek => "Semana",
            DateRange::ThisMonth => "Mês",
        };
        let date_active = filter.date_range != DateRange::All;
        let date_text_color = if date_active { Color32::WHITE } else { Color32::from_gray(160) };
        egui::ComboBox::from_id_salt("date_range_chip")
            .selected_text(RichText::new(format!("{date_label} ▾")).color(date_text_color).small())
            .width(80.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter.date_range, DateRange::All, "Sempre");
                ui.selectable_value(&mut filter.date_range, DateRange::Today, "Hoje");
                ui.selectable_value(&mut filter.date_range, DateRange::ThisWeek, "Esta semana");
                ui.selectable_value(&mut filter.date_range, DateRange::ThisMonth, "Este mês");
            });
        if filter.date_range != prev_date_range {
            action = LibraryAction::SaveDateRange(filter.date_range);
        }
    });

    ui.add_space(2.0);

    // ── Status ───────────────────────────────────────────────────────
    if library.scanning {
        ui.small(
            RichText::new(format!("🔍 varrendo pasta… {} encontrados", library.entries.len()))
                .italics(),
        );
    } else {
        let pending = library.pending_count();
        if pending > 0 {
            ui.small(format!("🔄 {pending}/{} lendo metadados…", library.entries.len()));
        }
    }

    ui.separator();

    if library.entries.is_empty() && library.working_dir.is_none() {
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Defina um 'Diretório de trabalho' (botão 📂 acima ou em Configurações) para listar seus replays aqui.",
            )
            .italics(),
        );
        return action;
    }

    // ── Filtragem ────────────────────────────────────────────────────
    let needle = filter.search.trim().to_ascii_lowercase();
    let search_active = !needle.is_empty();
    let any_filter_active = search_active
        || filter.race.is_some()
        || filter.outcome != OutcomeFilter::All
        || filter.date_range != DateRange::All;

    let today = today_str();

    let mut visible: Vec<usize> = library
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| match &e.meta {
            MetaState::Parsed(meta) => {
                if search_active {
                    let name_match = meta
                        .players
                        .iter()
                        .any(|p| p.name.to_ascii_lowercase().contains(&needle));
                    let map_match = meta.map.to_ascii_lowercase().contains(&needle);
                    let mc = matchup_code(meta, config);
                    let matchup_match = mc.to_ascii_lowercase().contains(&needle);
                    if !(name_match || map_match || matchup_match) {
                        return false;
                    }
                }
                if let Some(race_ch) = filter.race {
                    let user = find_user_player(meta, config);
                    let matches = user
                        .map_or(false, |p| race_letter(&p.race) == race_ch);
                    if !matches {
                        return false;
                    }
                }
                match filter.outcome {
                    OutcomeFilter::All => {}
                    OutcomeFilter::Wins => {
                        let won = find_user_player(meta, config)
                            .map_or(false, |p| p.result == "Win");
                        if !won {
                            return false;
                        }
                    }
                    OutcomeFilter::Losses => {
                        let lost = find_user_player(meta, config)
                            .map_or(false, |p| p.result == "Loss");
                        if !lost {
                            return false;
                        }
                    }
                }
                if !matches_date_range(&meta.datetime, filter.date_range, &today) {
                    return false;
                }
                true
            }
            _ => !any_filter_active,
        })
        .map(|(i, _)| i)
        .collect();

    // ── Ordenação ────────────────────────────────────────────────────
    match filter.sort {
        SortOrder::Date => {
            // Já ordenado por mtime no entries vec. Se ascendente, inverter.
            if filter.sort_ascending {
                visible.reverse();
            }
        }
        SortOrder::Duration => {
            visible.sort_by(|&a, &b| {
                let da = get_duration(&library.entries[a]);
                let db = get_duration(&library.entries[b]);
                if filter.sort_ascending { da.cmp(&db) } else { db.cmp(&da) }
            });
        }
        SortOrder::Mmr => {
            visible.sort_by(|&a, &b| {
                let ma = get_user_mmr(&library.entries[a], config);
                let mb = get_user_mmr(&library.entries[b], config);
                if filter.sort_ascending { ma.cmp(&mb) } else { mb.cmp(&ma) }
            });
        }
        SortOrder::Map => {
            visible.sort_by(|&a, &b| {
                let ma = get_map(&library.entries[a]);
                let mb = get_map(&library.entries[b]);
                if filter.sort_ascending { ma.cmp(mb) } else { mb.cmp(ma) }
            });
        }
    }

    let shown = visible.len();

    if any_filter_active && shown == 0 {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Nenhum replay corresponde ao filtro.")
                .italics()
                .color(Color32::from_gray(160)),
        );
        return action;
    }

    if any_filter_active {
        ui.small(
            RichText::new(format!("🔎 {shown}/{} correspondem ao filtro", library.entries.len()))
                .color(Color32::from_gray(140)),
        );
    }

    // ── Lista virtualizada ───────────────────────────────────────────
    let row_h = row_height(ui);
    ScrollArea::vertical()
        .id_salt("library_list")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, shown, |ui, row_range| {
            for virtual_idx in row_range {
                let idx = visible[virtual_idx];
                let entry = &library.entries[idx];
                let is_current = current_path.map_or(false, |cp| cp == entry.path);
                if entry_row(ui, entry, is_current, config, row_h) {
                    action = LibraryAction::Load(entry.path.clone());
                }
            }
        });

    action
}

// ── Helpers de filtro/sort ────────────────────────────────────────────

fn find_user_player<'a>(meta: &'a ParsedMeta, config: &AppConfig) -> Option<&'a PlayerMeta> {
    if config.user_nicknames.is_empty() {
        return None;
    }
    meta.players.iter().find(|p| {
        config
            .user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&p.name))
    })
}

fn find_user_index(meta: &ParsedMeta, config: &AppConfig) -> Option<usize> {
    if config.user_nicknames.is_empty() {
        return None;
    }
    meta.players.iter().position(|p| {
        config
            .user_nicknames
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&p.name))
    })
}

fn matchup_code(meta: &ParsedMeta, config: &AppConfig) -> String {
    if meta.players.len() != 2 {
        return String::new();
    }
    let ui = find_user_index(meta, config);
    let (first, second) = match ui {
        Some(0) => (0, 1),
        Some(1) => (1, 0),
        _ => (0, 1),
    };
    format!(
        "{}v{}",
        race_letter(&meta.players[first].race),
        race_letter(&meta.players[second].race)
    )
}

fn today_str() -> String {
    #[cfg(target_os = "windows")]
    {
        use std::mem::MaybeUninit;
        unsafe {
            let mut st = MaybeUninit::<winapi_local::SYSTEMTIME>::uninit();
            winapi_local::GetLocalTime(st.as_mut_ptr());
            let st = st.assume_init();
            format!("{:04}-{:02}-{:02}", st.w_year, st.w_month, st.w_day)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let days = now / 86400;
        let (y, m, d) = civil_from_days(days as i64);
        format!("{y:04}-{m:02}-{d:02}")
    }
}

#[cfg(target_os = "windows")]
mod winapi_local {
    #[repr(C)]
    #[allow(dead_code)]
    pub struct SYSTEMTIME {
        pub w_year: u16,
        pub w_month: u16,
        pub w_day_of_week: u16,
        pub w_day: u16,
        pub w_hour: u16,
        pub w_minute: u16,
        pub w_second: u16,
        pub w_milliseconds: u16,
    }
    unsafe extern "system" {
        pub fn GetLocalTime(lp: *mut SYSTEMTIME);
    }
}

/// Days since epoch → (year, month, day). Algorithm from Howard Hinnant.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Day of week (0=Monday .. 6=Sunday) from (y, m, d).
fn day_of_week(y: i32, m: u32, d: u32) -> u32 {
    // Tomohiko Sakamoto's algorithm
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let dow = (y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + d as i32) % 7;
    // Sakamoto: 0=Sunday. Convert to 0=Monday.
    ((dow + 6) % 7) as u32
}

fn parse_date(dt: &str) -> Option<(i32, u32, u32)> {
    if dt.len() < 10 {
        return None;
    }
    let y: i32 = dt[..4].parse().ok()?;
    let m: u32 = dt[5..7].parse().ok()?;
    let d: u32 = dt[8..10].parse().ok()?;
    Some((y, m, d))
}

fn matches_date_range(datetime: &str, range: DateRange, today: &str) -> bool {
    match range {
        DateRange::All => true,
        DateRange::Today => datetime.starts_with(today),
        DateRange::ThisWeek => {
            let Some((ty, tm, td)) = parse_date(today) else { return true; };
            let Some((ry, rm, rd)) = parse_date(datetime) else { return false; };
            let today_dow = day_of_week(ty, tm, td);
            // Monday of this week
            let today_days = days_from_civil(ty, tm, td);
            let week_start = today_days - today_dow as i64;
            let replay_days = days_from_civil(ry, rm, rd);
            replay_days >= week_start && replay_days <= today_days
        }
        DateRange::ThisMonth => {
            if today.len() < 7 {
                return true;
            }
            datetime.starts_with(&today[..7])
        }
    }
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

fn get_duration(entry: &LibraryEntry) -> u32 {
    match &entry.meta {
        MetaState::Parsed(m) => m.duration_seconds,
        _ => 0,
    }
}

fn get_user_mmr(entry: &LibraryEntry, config: &AppConfig) -> i32 {
    match &entry.meta {
        MetaState::Parsed(m) => find_user_player(m, config)
            .and_then(|p| p.mmr)
            .unwrap_or(0),
        _ => 0,
    }
}

fn get_map(entry: &LibraryEntry) -> &str {
    match &entry.meta {
        MetaState::Parsed(m) => &m.map,
        _ => "",
    }
}

// ── UI components ────────────────────────────────────────────────────

fn chip(ui: &mut Ui, label: &str, selected: bool, accent: Option<Color32>) -> egui::Response {
    let fill = if selected {
        accent.map_or(Color32::from_rgb(55, 75, 55), |c| {
            Color32::from_rgb(
                (c.r() as u16 / 3) as u8 + 20,
                (c.g() as u16 / 3) as u8 + 20,
                (c.b() as u16 / 3) as u8 + 20,
            )
        })
    } else {
        Color32::from_gray(40)
    };
    let text_color = if selected {
        Color32::WHITE
    } else {
        Color32::from_gray(160)
    };

    let icon = if accent.is_some() {
        if selected {
            format!("■ {label}")
        } else {
            format!("□ {label}")
        }
    } else {
        label.to_string()
    };

    ui.add(
        egui::Button::new(RichText::new(icon).color(text_color).small())
            .fill(fill)
            .corner_radius(12.0),
    )
}

/// Altura de cada linha da lista virtualizada.
fn row_height(ui: &Ui) -> f32 {
    use egui::TextStyle;
    let body = ui.text_style_height(&TextStyle::Body);
    let small = ui.text_style_height(&TextStyle::Small);
    let gap = ui.spacing().item_spacing.y;
    body + small * 2.0 + gap * 2.0 + FRAME_CHROME_V
}

const FRAME_CHROME_V: f32 = 13.0;

// Cores de raça — distintas das cores de slot P1/P2 (vermelho/azul)
// para que "raça" e "jogador" nunca se confundam visualmente.
const RACE_COLOR_TERRAN: Color32 = Color32::from_rgb(90, 130, 180);   // azul aço
const RACE_COLOR_PROTOSS: Color32 = Color32::from_rgb(120, 180, 100); // verde dourado
const RACE_COLOR_ZERG: Color32 = Color32::from_rgb(160, 80, 150);     // roxo magenta

/// Cor da borda esquerda baseada na raça.
fn race_border_color(race: &str) -> Color32 {
    match race_letter(race) {
        'T' => RACE_COLOR_TERRAN,
        'P' => RACE_COLOR_PROTOSS,
        'Z' => RACE_COLOR_ZERG,
        _ => Color32::from_gray(100),
    }
}

fn entry_row(
    ui: &mut Ui,
    entry: &LibraryEntry,
    is_current: bool,
    config: &AppConfig,
    row_h: f32,
) -> bool {
    let loadable = entry.meta.is_loadable();
    let fill = if is_current {
        Color32::from_rgb(24, 48, 24)
    } else if matches!(entry.meta, MetaState::Unsupported(_)) {
        Color32::from_gray(22)
    } else {
        Color32::from_gray(28)
    };
    let stroke = if is_current {
        egui::Stroke::new(1.5, Color32::LIGHT_GREEN)
    } else if matches!(entry.meta, MetaState::Unsupported(_)) {
        egui::Stroke::new(0.5, Color32::from_gray(50))
    } else {
        egui::Stroke::new(0.5, Color32::from_gray(60))
    };

    let content_h = (row_h - FRAME_CHROME_V).max(0.0);

    let inner = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(content_h);

            match &entry.meta {
                MetaState::Parsed(meta) => {
                    let user_idx = find_user_index(meta, config);
                    let mc = matchup_code(meta, config);

                    // Player names label: "Player1 vs Player2"
                    let vs_label = if meta.players.len() == 2 {
                        format!("{} vs {}", meta.players[0].name, meta.players[1].name)
                    } else {
                        meta.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" vs ")
                    };

                    let dur = format!(
                        "{:02}:{:02}",
                        meta.duration_seconds / 60,
                        meta.duration_seconds % 60
                    );

                    let mmrs: Vec<String> = meta
                        .players
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let v = match p.mmr {
                                Some(v) => v.to_string(),
                                None => "—".into(),
                            };
                            if user_idx == Some(i) {
                                format!("{v}")
                            } else {
                                v
                            }
                        })
                        .collect();

                    let (short_date, time_part) = split_datetime(&meta.datetime);

                    ui.horizontal(|ui| {
                        // ── Coluna esquerda ──
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&vs_label)
                                    .strong()
                                    .color(if is_current {
                                        Color32::LIGHT_GREEN
                                    } else {
                                        Color32::WHITE
                                    }),
                            );
                            ui.small(
                                RichText::new(format!("🗺 {} • ⏱ {dur} • {short_date}", meta.map))
                                    .color(Color32::from_gray(140)),
                            );
                            let mmr_user = user_idx.and_then(|i| meta.players[i].mmr);
                            let mmr_text = format!("MMR {}", mmrs.join(" / "));
                            if mmr_user.is_some() {
                                ui.small(
                                    RichText::new(mmr_text)
                                        .color(Color32::from_gray(140))
                                        .strong(),
                                );
                            } else {
                                ui.small(
                                    RichText::new(mmr_text).color(Color32::from_gray(140)),
                                );
                            }
                        });

                        // ── Coluna direita ──
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // Botão "abrir"
                                let btn = ui.add(
                                    egui::Button::new(
                                        RichText::new("abrir").color(Color32::from_gray(180)),
                                    )
                                    .fill(Color32::from_gray(45))
                                    .corner_radius(4.0),
                                );
                                if btn.clicked() {
                                    // Handled below via inner.response
                                }

                                ui.add_space(8.0);

                                ui.vertical(|ui| {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Min),
                                        |ui| {
                                            ui.label(
                                                RichText::new(&mc)
                                                    .strong()
                                                    .size(ui.text_style_height(&egui::TextStyle::Body) * 1.1)
                                                    .color(Color32::from_gray(200)),
                                            );
                                        },
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Min),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!("{short_date} "))
                                                    .small()
                                                    .color(Color32::from_gray(100)),
                                            );
                                            ui.label(
                                                RichText::new(&time_part)
                                                    .small()
                                                    .strong()
                                                    .color(Color32::from_gray(200)),
                                            );
                                        },
                                    );
                                });
                            },
                        );
                    });
                }
                MetaState::Pending => {
                    ui.label(RichText::new(&entry.filename).monospace());
                    ui.small(RichText::new("lendo metadados…").italics());
                }
                MetaState::Unsupported(reason) => {
                    ui.label(
                        RichText::new(&entry.filename)
                            .monospace()
                            .color(Color32::from_gray(140)),
                    );
                    ui.small(
                        RichText::new(format!("⚠ não suportado: {reason}"))
                            .color(Color32::from_rgb(210, 170, 60))
                            .italics(),
                    );
                }
                MetaState::Failed(err) => {
                    ui.label(RichText::new(&entry.filename).monospace());
                    ui.small(
                        RichText::new(format!("falha: {err}"))
                            .color(Color32::LIGHT_RED)
                            .italics(),
                    );
                }
            }
        });

    // Pinta a borda esquerda colorida por raça (sobre o frame já renderizado).
    if let MetaState::Parsed(meta) = &entry.meta {
        let user_idx = find_user_index(meta, config).unwrap_or(0);
        let border_color = race_border_color(&meta.players[user_idx].race);
        let rect = inner.response.rect;
        let border_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 3.5, rect.bottom()),
        );
        ui.painter().rect_filled(border_rect, 4.0, border_color);
    }

    loadable && inner.response.interact(Sense::click()).clicked()
}

fn race_letter(race: &str) -> char {
    crate::utils::race_letter(race)
}

fn split_datetime(dt: &str) -> (String, String) {
    // "2025-12-18T06:44:53" → ("2025-12-18", "06:44")
    if dt.len() >= 16 {
        let date = dt[..10].to_string();
        let time = dt[11..16].to_string();
        (date, time)
    } else if dt.len() >= 10 {
        (dt[..10].to_string(), String::new())
    } else {
        (dt.to_string(), String::new())
    }
}

/// Helper para a `app.rs` pedir repaint quando houver trabalho em andamento.
pub fn keep_alive(ctx: &Context, library: &ReplayLibrary) {
    if library.scanning {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    } else if library.pending_count() > 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}
