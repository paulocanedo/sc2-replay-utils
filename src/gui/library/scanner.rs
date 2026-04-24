//! Scanner de diretório + pool de parsers.
//!
//! Há dois pools distintos:
//!
//! 1. **Pool de parse rápido (burst)**: a cada `refresh()` gastamos um
//!    burst de workers dedicados a essa batelada. Se a batelada tem mais
//!    que `PARALLEL_THRESHOLD` arquivos, subimos um pool de N threads
//!    (N = núcleos disponíveis, clampado em [2, 8]); abaixo disso fica
//!    uma única thread — mais simples e suficiente para bibliotecas
//!    pequenas. Os workers compartilham o mesmo canal via
//!    `Arc<Mutex<Receiver>>`: a contenção é desprezível porque cada item
//!    leva ordens de grandeza mais tempo para parsear do que para tirar
//!    da fila. Ao fim da batelada, o `Sender` é dropado, os `recv()`
//!    retornam `Err` e os workers encerram naturalmente. Este pool
//!    parseia apenas o cabeçalho do replay (max_time=1) — segundos de
//!    latência para bibliotecas com milhares de replays.
//!
//! 2. **Pool de enriquecimento (lazy)**: workers persistentes de baixa
//!    prioridade (via `sleep(ENRICHMENT_YIELD_MS)` antes de cada item)
//!    que consomem um segundo canal. Sua função é parsear ~5 min do
//!    replay e classificar o rótulo de abertura (feature experimental).
//!    Essa classificação não bloqueia a listagem — enquanto o pool
//!    trabalha, a UI exibe "—" e atualiza a linha quando o resultado
//!    chega. O cache bincode persiste o resultado, então nas próximas
//!    aberturas da biblioteca o rótulo aparece imediatamente.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use crate::build_order::{classify_opening, extract_build_order};
use crate::config::AppConfig;
use crate::replay::parse_replay;

use super::date::today_str;
use super::filter::{LibraryFilter, StatsFilterKey, matches_filter};
use super::stats::{LibraryStats, compute_library_stats, compute_nickname_frequencies};
use super::types::{LibraryEntry, MetaState, ParsedMeta, PlayerMeta};

/// Quantos segundos de game time parseamos *no pool de enriquecimento*
/// para extrair o rótulo de abertura. A janela interna de classificação
/// vai até `T_FOLLOW_UP_END` (5 min); usamos o mesmo valor aqui para
/// coletar todos os eventos relevantes sem pagar o custo de parsear o
/// replay inteiro. Replays mais curtos que isso são parseados por
/// inteiro (parse_replay respeita o `max_time`).
const ENRICHMENT_PARSE_SECONDS: u32 = 300;

/// Número de threads no pool de enriquecimento. Mantido baixo de
/// propósito — a feature é lazy/best-effort e não deve competir com o
/// pool principal de parse rápido.
const ENRICHMENT_WORKERS: usize = 2;

/// Pausa antes de cada item no pool de enriquecimento. Funciona como um
/// "yield voluntário" para o scheduler do SO: com 50 ms entre itens, os
/// dois workers processam no máximo ~40 replays/s, deixando CPU livre
/// para o pool principal e para a UI. Como é I/O-bound pesado, o
/// throughput real é muito menor; o valor serve principalmente para
/// garantir que um spike de enriquecimento não engasgue o frame do egui.
const ENRICHMENT_YIELD_MS: u64 = 50;

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

/// Resultado do pool de enriquecimento — uma classificação de abertura
/// por jogador, do mesmo `path`. O vetor preserva a ordem de
/// `ParsedMeta.players`.
struct EnrichmentResult {
    path: PathBuf,
    openings: Vec<Option<String>>,
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
    /// Canal de envio de trabalho para o pool de enriquecimento. É
    /// mantido vivo durante toda a vida do `ReplayLibrary` para que os
    /// workers persistentes continuem bloqueando em `recv()`; nunca
    /// dropamos — o SO limpa as threads no shutdown do processo.
    tx_enrich_work: Sender<PathBuf>,
    /// Canal de recepção dos resultados de enriquecimento.
    rx_enrich_result: Receiver<EnrichmentResult>,
    /// Paths atualmente enfileirados no pool de enriquecimento (dedup).
    /// Impede re-envios redundantes quando a mesma entrada é polada em
    /// sucessivos `poll()`s antes do worker começar a processá-la.
    enrichment_in_flight: HashSet<PathBuf>,
    /// Derived cache: aggregates computed from `entries` on demand.
    /// Invalidated whenever `entries` mutates or when the nickname list
    /// in `AppConfig` differs from the one used to build the cache.
    cached_stats: Option<LibraryStats>,
    stats_dirty: bool,
    cached_nicknames: Vec<String>,
    /// Nickname frequency aggregation over *all* entries (not filtered).
    /// Used by the settings modal to suggest nicks the user hasn't
    /// registered yet. Lives outside `LibraryStats` on purpose: that one
    /// runs over the filtered view, whereas suggestions must reflect the
    /// entire library regardless of the user's active filters.
    cached_nickname_frequencies: Option<Vec<(String, u32)>>,
    /// Snapshot of the filter used to compute `cached_stats`. Invalidates
    /// the cache when the user toggles a filter in the sidebar so the
    /// hero KPIs stay in sync with the visible list.
    cached_filter_key: Option<StatsFilterKey>,
}

impl ReplayLibrary {
    pub fn new() -> Self {
        let (tx_result, rx_result) = mpsc::channel::<LibraryResult>();
        let (tx_enrich_work, rx_enrich_work) = mpsc::channel::<PathBuf>();
        let (tx_enrich_result, rx_enrich_result) = mpsc::channel::<EnrichmentResult>();
        let cache = crate::cache::load();

        // Pool persistente de enriquecimento. Os workers bloqueiam em
        // `recv()` esperando trabalho; sleep + yield antes de cada item
        // serve como throttle simples para dar prioridade ao pool
        // principal e à UI.
        let rx_enrich_work = Arc::new(Mutex::new(rx_enrich_work));
        for i in 0..ENRICHMENT_WORKERS {
            let rx = Arc::clone(&rx_enrich_work);
            let tx = tx_enrich_result.clone();
            let _ = thread::Builder::new()
                .name(format!("replay-library-enricher-{i}"))
                .spawn(move || loop {
                    // Yield antes de pegar o próximo item — a thread
                    // fica dormindo mesmo que a fila esteja cheia, o
                    // que na prática "despriotiza" o pool.
                    thread::sleep(Duration::from_millis(ENRICHMENT_YIELD_MS));
                    let next = {
                        let guard = match rx.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        guard.recv()
                    };
                    match next {
                        Ok(path) => {
                            let openings = compute_openings(&path);
                            if tx
                                .send(EnrichmentResult {
                                    path,
                                    openings,
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
            tx_enrich_work,
            rx_enrich_result,
            enrichment_in_flight: HashSet::new(),
            cached_stats: None,
            stats_dirty: true,
            cached_nicknames: Vec::new(),
            cached_nickname_frequencies: None,
            cached_filter_key: None,
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

    /// Integra um replay recém-detectado (tipicamente via watcher) cuja
    /// metadata já foi derivada de um `ReplayTimeline` carregado. Evita
    /// re-parsear o MPQ — a metadata vem do mesmo stream canônico que a
    /// tela de análise consome. Também grava no `cache` para que um
    /// `refresh()` futuro trate o arquivo como cache hit.
    ///
    /// - Se `path` não está abaixo do `working_dir` atual, ignora.
    /// - Se já existe entry com este `path`, atualiza em lugar (caso de
    ///   sobrescrita do arquivo); senão insere no início (replay novo
    ///   tem o maior mtime por definição).
    pub fn ingest_parsed(
        &mut self,
        path: PathBuf,
        mtime: Option<SystemTime>,
        meta: ParsedMeta,
    ) {
        if !self.path_under_working_dir(&path) {
            return;
        }
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        // Grava no cache cedo — se um refresh concorrente reencontrar o
        // arquivo, vai bater como cache hit (mesmo mtime) e não re-parsear.
        if let Some(mt) = mtime {
            self.cache.insert(
                path.clone(),
                (mt, MetaState::Parsed(meta.clone())),
            );
            self.cache_dirty = true;
        }

        // Enfileira enriquecimento se algum jogador ainda está sem
        // `opening` (idempotente via `enrichment_in_flight`).
        self.enqueue_enrichment_if_needed(&path, &meta);

        if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
            entry.mtime = mtime;
            entry.filename = filename;
            entry.meta = MetaState::Parsed(meta);
        } else {
            self.entries.insert(
                0,
                LibraryEntry {
                    path,
                    filename,
                    mtime,
                    meta: MetaState::Parsed(meta),
                },
            );
        }
        self.stats_dirty = true;
    }

    /// Variante sem metadata derivada — insere com `Pending` e despacha
    /// para o pool de parse existente. Usada quando `auto_load` está
    /// desligado ou quando o load da análise falhou.
    pub fn ingest_pending(&mut self, path: PathBuf, mtime: Option<SystemTime>) {
        if !self.path_under_working_dir(&path) {
            return;
        }
        if self.entries.iter().any(|e| e.path == path) {
            return;
        }
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.entries.insert(
            0,
            LibraryEntry {
                path: path.clone(),
                filename,
                mtime,
                meta: MetaState::Pending,
            },
        );
        self.stats_dirty = true;
        self.spawn_parse_burst(vec![(path, mtime)]);
    }

    fn path_under_working_dir(&self, path: &Path) -> bool {
        match &self.working_dir {
            Some(dir) => path.starts_with(dir),
            None => false,
        }
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
                        // Dedup: o watcher pode ter ingerido este path
                        // durante um refresh em curso (ver `ingest_*`).
                        // Nesse caso a entry já está em `entries` — pular
                        // evita duplicatas.
                        if self.entries.iter().any(|e| e.path == result.path) {
                            continue;
                        }
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
                        // Cache hit com opening=None: enfileira para
                        // enriquecimento (v1 desta feature ainda não tem
                        // rótulo gravado). Idempotente — dedup via
                        // enrichment_in_flight.
                        if let MetaState::Parsed(parsed) = &meta {
                            self.enqueue_enrichment_if_needed(&result.path, parsed);
                        }
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
                    // Fresh parse chegou com opening=None — enfileira
                    // para enriquecimento. Idempotente.
                    self.enqueue_enrichment_if_needed(&msg.path, &meta);
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

        // Fase 3: Drena resultados do pool de enriquecimento. Atualiza
        // o rótulo `opening` de cada jogador na entrada e no cache. Se
        // o vetor vier vazio (parse/extract falhou), marcamos `path`
        // como concluído para não tentar de novo nesta sessão — o
        // próximo ciclo do app tenta novamente depois do usuário fechar.
        while let Ok(res) = self.rx_enrich_result.try_recv() {
            self.enrichment_in_flight.remove(&res.path);
            if res.openings.is_empty() {
                continue;
            }
            // Atualiza entrada.
            if let Some(entry) = self.entries.iter_mut().find(|e| e.path == res.path) {
                if let MetaState::Parsed(meta) = &mut entry.meta {
                    for (i, op) in res.openings.iter().enumerate() {
                        if let Some(player) = meta.players.get_mut(i) {
                            player.opening = op.clone();
                        }
                    }
                    updated = true;
                }
            }
            // Atualiza cache (pode não estar sincronizado com `entries`
            // se o usuário mudou de diretório entre enqueue e resultado
            // — mesmo assim gravamos o rótulo calculado, é válido).
            if let Some((_, MetaState::Parsed(cached_meta))) = self.cache.get_mut(&res.path) {
                for (i, op) in res.openings.iter().enumerate() {
                    if let Some(player) = cached_meta.players.get_mut(i) {
                        player.opening = op.clone();
                    }
                }
                self.cache_dirty = true;
            }
        }

        if updated {
            self.stats_dirty = true;
        }
        // Salva o cache quando tudo assentar: scanner, pool principal
        // e pool de enriquecimento ociosos. Enriquecimento pode rodar
        // por minutos em bibliotecas grandes; a escrita fica para
        // quando ele terminar, evitando syscalls redundantes.
        if self.cache_dirty
            && !self.scanning
            && self.pending_count() == 0
            && self.enrichment_in_flight.is_empty()
        {
            self.save_cache();
        }
        updated
    }

    /// Enfileira `path` no pool de enriquecimento se a meta ainda
    /// precisa de rótulo (pelo menos um jogador com `opening: None`).
    /// Dedup via `enrichment_in_flight`. Apenas 1v1 (2 jogadores) —
    /// os demais já viram `Unsupported` no parse rápido e não chegam
    /// aqui, mas o check é defensivo.
    fn enqueue_enrichment_if_needed(&mut self, path: &Path, meta: &ParsedMeta) {
        if meta.players.len() != 2 {
            return;
        }
        let needs = meta.players.iter().any(|p| p.opening.is_none());
        if !needs {
            return;
        }
        if self.enrichment_in_flight.insert(path.to_path_buf()) {
            let _ = self.tx_enrich_work.send(path.to_path_buf());
        }
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
    /// last call, or if the user's nickname list / active library filter
    /// has changed. Cheap to call on every frame: the fast path is a few
    /// equality checks (dirty flag, nickname slice, filter key).
    pub fn ensure_stats(&mut self, config: &AppConfig, filter: &LibraryFilter) {
        let key = StatsFilterKey::from(filter);
        let nicknames_changed = self.cached_nicknames != config.user_nicknames;
        let filter_changed = self.cached_filter_key.as_ref() != Some(&key);
        if self.stats_dirty
            || self.cached_stats.is_none()
            || nicknames_changed
            || filter_changed
        {
            let today = today_str();
            let filtered = self
                .entries
                .iter()
                .filter(|e| matches_filter(e, filter, config, &today));
            self.cached_stats = Some(compute_library_stats(filtered, config));
            self.cached_nickname_frequencies =
                Some(compute_nickname_frequencies(&self.entries));
            self.stats_dirty = false;
            if nicknames_changed {
                self.cached_nicknames = config.user_nicknames.clone();
            }
            self.cached_filter_key = Some(key);
        }
    }

    /// Returns the last computed stats snapshot. Call `ensure_stats` on
    /// the same frame first — otherwise the snapshot may be stale.
    pub fn stats(&self) -> Option<&LibraryStats> {
        self.cached_stats.as_ref()
    }

    /// Returns player-name frequencies across the whole library (not
    /// filter-aware). Populated by `ensure_stats`. Used by the settings
    /// modal to suggest nicks the user hasn't registered yet.
    pub fn nickname_frequencies(&self) -> Option<&[(String, u32)]> {
        self.cached_nickname_frequencies.as_deref()
    }
}

fn parse_meta(path: &Path) -> ParseOutcome {
    // max_time=1 evita processar a maior parte dos eventos. Só precisamos
    // dos metadados (map, datetime, game_loops, jogadores). O rótulo de
    // abertura é preenchido depois, em background, pelo pool de
    // enriquecimento — ver `compute_openings`.
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
        version: Some(data.version),
        cache_handles: data.cache_handles,
        players: data
            .players
            .into_iter()
            .map(|p| PlayerMeta {
                name: p.name,
                race: p.race,
                mmr: p.mmr,
                result: p.result.clone().unwrap_or_default(),
                // Enrichment preenche depois; placeholder "—" na UI.
                opening: None,
            })
            .collect(),
    })
}

/// Parseia ~5 min do replay e classifica a abertura de cada jogador.
/// Retorna um vetor alinhado com `ParsedMeta.players`. Em qualquer
/// falha (parse/extração), retorna um vetor vazio — a chamadora deve
/// tratar o vetor de tamanho errado como "enriquecimento falhou" e
/// manter `opening: None` no meta.
fn compute_openings(path: &Path) -> Vec<Option<String>> {
    let data = match parse_replay(path, ENRICHMENT_PARSE_SECONDS) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    if data.players.len() != 2 {
        return Vec::new();
    }
    match extract_build_order(&data) {
        Ok(bo) => bo
            .players
            .iter()
            .map(|p| Some(classify_opening(p, bo.loops_per_second).to_display_string()))
            .collect(),
        Err(_) => vec![None; data.players.len()],
    }
}
