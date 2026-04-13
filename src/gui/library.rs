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
}

pub fn show(
    ui: &mut Ui,
    library: &ReplayLibrary,
    current_path: Option<&Path>,
    config: &AppConfig,
    filter: &mut String,
) -> LibraryAction {
    let mut action = LibraryAction::None;

    // Header
    ui.horizontal(|ui| {
        ui.heading("Biblioteca");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("↻").on_hover_text("Recarregar lista").clicked() {
                action = LibraryAction::Refresh;
            }
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
                    .color(Color32::from_gray(160)),
            );
        }
        None => {
            ui.small(RichText::new("Diretório não definido").italics());
        }
    }

    // Filtro por nome de jogador. Case-insensitive, match por substring
    // em qualquer jogador da partida. Só afeta entradas `Parsed` —
    // Pending/Unsupported/Failed ficam escondidas enquanto o filtro
    // estiver ativo (não temos nomes para comparar).
    ui.horizontal(|ui| {
        ui.label("🔎");
        let resp = ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("filtrar por jogador…")
                .desired_width(f32::INFINITY),
        );
        if !filter.is_empty() && resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            filter.clear();
        }
    });

    // Pré-computa os índices visíveis com base no filtro. O vetor
    // `visible` é construído a cada frame, mas é uma varredura linear
    // barata (só comparamos strings já em memória) — a economia do
    // scroll virtualizado continua valendo porque só renderizamos as
    // linhas dentro da viewport.
    let needle = filter.trim().to_ascii_lowercase();
    let filter_active = !needle.is_empty();
    let visible: Vec<usize> = if filter_active {
        library
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| match &e.meta {
                MetaState::Parsed(meta) => meta
                    .players
                    .iter()
                    .any(|p| p.name.to_ascii_lowercase().contains(&needle)),
                _ => false,
            })
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..library.entries.len()).collect()
    };

    // Status da fila / contador
    let pending = library.pending_count();
    let total = library.entries.len();
    let shown = visible.len();
    if total > 0 {
        if filter_active {
            ui.small(format!("🔎 {shown}/{total} correspondem ao filtro"));
        } else if library.scanning {
            ui.small(format!("🔍 varrendo pasta… {total} encontrados"));
        } else if pending > 0 {
            ui.small(format!("🔄 {pending}/{total} lendo metadados…"));
        } else {
            ui.small(format!("{total} replays"));
        }
    } else if library.scanning {
        ui.small(RichText::new("🔍 varrendo pasta…").italics());
    } else if library.working_dir.is_some() {
        ui.small(RichText::new("(nenhum .SC2Replay nessa pasta)").italics());
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

    if filter_active && shown == 0 {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Nenhum replay corresponde ao filtro.")
                .italics()
                .color(Color32::from_gray(160)),
        );
        return action;
    }

    // Lista virtualizada: só as linhas visíveis são renderizadas.
    // Crítico com 4k+ entradas, onde o layout completo travaria o frame.
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

/// Altura (em pixels) de cada linha da lista virtualizada. Calculada a
/// partir das alturas atuais dos text styles para respeitar o
/// `font_scale` do config, já aplicado ao `Style` do contexto.
/// Todas as linhas — independentemente do estado — terminam com esta
/// altura porque `entry_row` força `set_min_height` dentro do Frame.
fn row_height(ui: &Ui) -> f32 {
    use egui::TextStyle;
    let body = ui.text_style_height(&TextStyle::Body);
    let small = ui.text_style_height(&TextStyle::Small);
    let gap = ui.spacing().item_spacing.y;
    // Conteúdo do Frame: 1 body + 2 small + 2 gaps entre eles.
    // Moldura do Frame: inner_margin 6+6 + stroke 0.5+0.5 ≈ 13.
    body + small * 2.0 + gap * 2.0 + 13.0
}

/// Altura de chrome do Frame (margem vertical 6+6 + stroke ~1).
/// Mantida em sincronia com `row_height` — se mudar um, mude o outro.
const FRAME_CHROME_V: f32 = 13.0;

/// Renderiza uma linha do catálogo. Retorna `true` se o usuário clicou
/// em uma entrada *carregável* (i.e. `Parsed`). Entradas `Unsupported`,
/// `Pending` ou `Failed` não respondem ao clique.
///
/// `row_h` é a altura fixa esperada pelo `ScrollArea::show_rows`. Para
/// garantir que **toda** linha ocupa exatamente essa altura — mesmo as
/// que mostram só 2 linhas de texto (Pending/Unsupported/Failed) — o
/// Frame força `set_min_height(row_h - chrome)` internamente antes de
/// desenhar qualquer label. Sem isso, linhas mais curtas desalinham o
/// scroll virtualizado porque `show_rows` assume altura uniforme.
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
            // Trava a altura INTERNA do Frame. Com isso, todo Frame
            // sai do layout com exatamente `row_h` (chrome + content),
            // independente de quantas labels foram desenhadas.
            ui.set_min_height(content_h);
            // Linha 1: filename ou matchup
            match &entry.meta {
                MetaState::Parsed(meta) => {
                    let matchup = matchup_label(meta, config);
                    ui.label(
                        RichText::new(matchup)
                            .strong()
                            .color(if is_current { Color32::LIGHT_GREEN } else { Color32::WHITE }),
                    );
                    // Linha 2: map + duração
                    let dur = format!(
                        "{:02}:{:02}",
                        meta.duration_seconds / 60,
                        meta.duration_seconds % 60
                    );
                    ui.small(format!("🗺 {} • ⏱ {}", meta.map, dur));
                    // Linha 3: MMRs + data
                    let mmrs: Vec<String> = meta
                        .players
                        .iter()
                        .map(|p| match p.mmr {
                            Some(v) => v.to_string(),
                            None => "—".to_string(),
                        })
                        .collect();
                    ui.small(format!(
                        "MMR {} • {}",
                        mmrs.join("/"),
                        short_datetime(&meta.datetime)
                    ));
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

    // Só registramos clique como "carregar" se a entrada for carregável.
    // Entradas Unsupported/Pending/Failed ficam inertes ao clique.
    loadable && inner.response.interact(Sense::click()).clicked()
}

fn matchup_label(meta: &ParsedMeta, _config: &AppConfig) -> String {
    if meta.players.is_empty() {
        return "(sem jogadores)".to_string();
    }
    let mut parts = Vec::with_capacity(meta.players.len());
    for p in &meta.players {
        parts.push(format!("{}({})", p.name, race_letter(&p.race)));
    }
    parts.join(" vs ")
}

fn race_letter(race: &str) -> char {
    crate::utils::race_letter(race)
}

fn short_datetime(dt: &str) -> String {
    // "2025-12-18T06:44:53" → "2025-12-18 06:44"
    if dt.len() >= 16 {
        let mut s = dt[..16].to_string();
        if let Some(pos) = s.find('T') {
            s.replace_range(pos..pos + 1, " ");
        }
        s
    } else {
        dt.to_string()
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
