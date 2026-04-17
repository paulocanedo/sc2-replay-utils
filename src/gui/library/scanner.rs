//! Scanner de diretório + pool de parsers.
//!
//! Modelo de paralelismo: a cada `refresh()` gastamos um "burst" de
//! workers dedicados a essa batelada. Se a batelada tem mais que
//! `PARALLEL_THRESHOLD` arquivos, subimos um pool de N threads
//! (N = núcleos disponíveis, clampado em [2, 8]); abaixo disso fica
//! uma única thread — mais simples e suficiente para bibliotecas
//! pequenas. Os workers compartilham o mesmo canal de trabalho via
//! `Arc<Mutex<Receiver>>`: a contenção é desprezível porque cada item
//! leva ordens de grandeza mais tempo para parsear do que para tirar
//! da fila. Ao fim da batelada, o `Sender` é dropado, os `recv()`
//! retornam `Err` e os workers encerram naturalmente.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use crate::config::AppConfig;
use crate::replay::parse_replay;

use super::stats::{LibraryStats, compute_library_stats};
use super::types::{LibraryEntry, MetaState, ParsedMeta, PlayerMeta};

/// Acima deste número de novos arquivos a parsear em um único `refresh`,
/// a biblioteca passa a usar um pool multi-thread. Abaixo disso, um
/// worker único é suficiente.
const PARALLEL_THRESHOLD: usize = 100;

/// Limite superior do pool multi-thread (protege contra máquinas com
/// muitos núcleos onde o I/O do disco vira gargalo antes da CPU).
const MAX_WORKERS: usize = 8;

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
    /// Derived cache: aggregates computed from `entries` on demand.
    /// Invalidated whenever `entries` mutates or when the nickname list
    /// in `AppConfig` differs from the one used to build the cache.
    cached_stats: Option<LibraryStats>,
    stats_dirty: bool,
    cached_nicknames: Vec<String>,
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
            cached_stats: None,
            stats_dirty: true,
            cached_nicknames: Vec::new(),
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
        self.stats_dirty = true;
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
        if updated {
            self.stats_dirty = true;
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

    /// Recomputes the derived stats cache if `entries` has mutated since
    /// last call, or if the user's nickname list has changed. Cheap to
    /// call on every frame: the fast path is a dirty-flag check and a
    /// slice comparison of nicknames (typically ≤3 strings).
    pub fn ensure_stats(&mut self, config: &AppConfig) {
        let nicknames_changed = self.cached_nicknames != config.user_nicknames;
        if self.stats_dirty || self.cached_stats.is_none() || nicknames_changed {
            self.cached_stats = Some(compute_library_stats(&self.entries, config));
            self.stats_dirty = false;
            if nicknames_changed {
                self.cached_nicknames = config.user_nicknames.clone();
            }
        }
    }

    /// Returns the last computed stats snapshot. Call `ensure_stats` on
    /// the same frame first — otherwise the snapshot may be stale.
    pub fn stats(&self) -> Option<&LibraryStats> {
        self.cached_stats.as_ref()
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
