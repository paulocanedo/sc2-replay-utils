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

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::build_order::{classify_opening, extract_build_order};
use crate::cache::{ContentId, LibraryCache, LookupOutcome};
use crate::config::AppConfig;
use crate::replay::parse_replay;

use super::date::today_str;
use super::filter::{LibraryFilter, StatsFilterKey, matches_filter};
use super::stats::{LibraryStats, compute_library_stats, compute_nickname_frequencies};
use super::types::{LibraryEntry, MetaState, OpeningLabel, ParsedMeta, PlayerMeta};

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

/// A cada N updates do cache em memória durante o enriquecimento,
/// força um flush para disco mesmo que o pool ainda esteja trabalhando.
/// Garantia de progresso resiliente a crashes / kills — sem isso, o
/// cache só persistia no fim do enriquecimento (que pode levar horas
/// numa biblioteca grande) ou no shutdown gracioso.
const ENRICHMENT_FLUSH_BATCH: u32 = 25;

/// Tempo máximo entre flushes do cache durante o enriquecimento.
/// Complementar ao batch — garante que mesmo enriquecimentos
/// esparsos (1-2 por minuto) cheguem a disco regularmente.
const ENRICHMENT_FLUSH_INTERVAL: Duration = Duration::from_secs(30);

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

/// Mensagem enviada pelo worker de volta para a UI. Carrega
/// `size`/`content_id` quando disponíveis para que a UI insira no cache
/// sem re-hashear nem re-statar.
struct LibraryResult {
    path: PathBuf,
    mtime: Option<SystemTime>,
    size: Option<u64>,
    content_id: Option<ContentId>,
    outcome: ParseOutcome,
}

/// Arquivo descoberto pelo scanner de diretório em background.
struct ScanResult {
    path: PathBuf,
    filename: String,
    mtime: Option<SystemTime>,
    size: Option<u64>,
}

/// Item da fila de parsing — carrega o que sabemos sobre o arquivo no
/// momento em que decidimos parseá-lo. `content_id` vem pré-computado
/// quando o lookup do cache já hasheou (slow-path miss); senão é
/// computado pelo worker.
struct ParseQueueItem {
    path: PathBuf,
    mtime: Option<SystemTime>,
    size: Option<u64>,
    content_id: Option<ContentId>,
}

/// Mensagem enviada pelo scanner de diretório em background.
enum ScanMessage {
    Found(ScanResult),
    /// Varredura concluída. Contém o replay mais recente encontrado.
    Done { latest: Option<(PathBuf, SystemTime)> },
}

/// Resultado do pool de enriquecimento — uma classificação de abertura
/// por jogador, do mesmo `path`. O vetor preserva a ordem de
/// `ParsedMeta.players` e, garantidamente, tem
/// `players.len()` entradas (o worker substitui falhas por
/// `OpeningLabel::Unclassifiable`).
struct EnrichmentResult {
    path: PathBuf,
    openings: Vec<OpeningLabel>,
}

pub struct ReplayLibrary {
    pub entries: Vec<LibraryEntry>,
    pub working_dir: Option<PathBuf>,
    /// Cache de duas camadas (path fingerprint + content hash) que
    /// sobrevive a path drift (separadores, casing, drive letter) e
    /// mtime drift (sync tools, antivírus). Ver `crate::cache`.
    cache: LibraryCache,
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
    scan_parse_queue: Vec<ParseQueueItem>,
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
    /// Updates ao `cache` desde o último flush em disco. Quando atinge
    /// `ENRICHMENT_FLUSH_BATCH`, força `save_cache()` mesmo com o pool
    /// de enriquecimento ainda trabalhando — garante que o trabalho
    /// progressivo sobreviva a kills/crashes.
    cache_updates_since_flush: u32,
    /// Timestamp do último flush; ver `ENRICHMENT_FLUSH_INTERVAL`.
    last_cache_flush: Instant,
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
            cache_updates_since_flush: 0,
            last_cache_flush: Instant::now(),
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
        // Canonicaliza o working_dir uma vez. Todo path produzido pelo
        // walk começa enraizado nesse dir canônico, então `entry.path()`
        // já vem em formato comparável byte-a-byte com o que está
        // gravado no cache. Sem isso, mudanças de slash/case no config
        // do usuário invalidam o cache inteiro silenciosamente.
        let dir = crate::cache::canonicalize_path(dir);
        self.working_dir = Some(dir.clone());

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
                        // Uma syscall só para pegar mtime + size (vs.
                        // duas chamadas separadas a `fs::metadata`).
                        let (mtime, size) = match fs::metadata(&path) {
                            Ok(m) => (m.modified().ok(), Some(m.len())),
                            Err(_) => (None, None),
                        };
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
                                size,
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
        // Canonicaliza primeiro — as comparações com `working_dir` e
        // com paths já em `entries` precisam ser estáveis. notify-rs
        // pode produzir paths num formato diferente do walk.
        let path = crate::cache::canonicalize_path(&path);
        if !self.path_under_working_dir(&path) {
            return;
        }
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        // Grava no cache cedo — se um refresh concorrente reencontrar o
        // arquivo, vai bater como cache hit (mesmo fingerprint) e não
        // re-parsear. Hash + size custam um read do arquivo (~200 KB);
        // qualquer falha aqui (arquivo sumiu, sem permissão) só
        // significa que o cache não vai persistir essa entrada — não é
        // erro de UX.
        let size = fs::metadata(&path).map(|m| m.len()).ok();
        let content_id = crate::cache::hash_file(&path);
        if let (Some(mt), Some(sz), Some(cid)) = (mtime, size, content_id) {
            self.cache.insert(path.clone(), sz, mt, cid, MetaState::Parsed(meta.clone()));
            self.cache_dirty = true;
            self.cache_updates_since_flush += 1;
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
        let path = crate::cache::canonicalize_path(&path);
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
        let size = fs::metadata(&path).map(|m| m.len()).ok();
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
        self.spawn_parse_burst(vec![ParseQueueItem {
            path,
            mtime,
            size,
            // Worker computa o hash — economizar uma leitura aqui não
            // vale o branching extra para o caso comum (watcher).
            content_id: None,
        }]);
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
    fn spawn_parse_burst(&self, items: Vec<ParseQueueItem>) {
        let n = items.len();
        let n_workers = if n > PARALLEL_THRESHOLD {
            thread::available_parallelism()
                .map(|v| v.get().clamp(2, MAX_WORKERS))
                .unwrap_or(4)
        } else {
            1
        };

        let (tx_work, rx_work) = mpsc::channel::<ParseQueueItem>();
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
                        Ok(item) => {
                            let outcome = parse_meta(&item.path);
                            // Calcula o hash quando não veio do
                            // lookup (slow-path miss). Pequena
                            // duplicação de read em cache misses do
                            // ingest_pending, mas mantém o caminho
                            // do scanner totalmente sem syscalls
                            // extras quando vem com hash.
                            let content_id =
                                item.content_id.or_else(|| crate::cache::hash_file(&item.path));
                            if tx
                                .send(LibraryResult {
                                    path: item.path,
                                    mtime: item.mtime,
                                    size: item.size,
                                    content_id,
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

        for item in items {
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
                        let meta = match (result.mtime, result.size) {
                            (Some(mt), Some(sz)) => match self.cache.lookup(
                                &result.path,
                                sz,
                                mt,
                                &result.path,
                            ) {
                                LookupOutcome::Hit { state, healed, .. } => {
                                    if healed {
                                        // Cura: a camada rápida foi
                                        // atualizada (path/mtime drift
                                        // recuperado via hash). Marca
                                        // dirty para o flush incremental
                                        // pegar.
                                        self.cache_dirty = true;
                                        self.cache_updates_since_flush += 1;
                                    }
                                    state
                                }
                                LookupOutcome::Miss { content_id } => {
                                    self.scan_parse_queue.push(ParseQueueItem {
                                        path: result.path.clone(),
                                        mtime: result.mtime,
                                        size: result.size,
                                        content_id,
                                    });
                                    MetaState::Pending
                                }
                            },
                            _ => {
                                // Sem mtime ou sem size — não dá pra
                                // checar fingerprint, vai pro parse.
                                self.scan_parse_queue.push(ParseQueueItem {
                                    path: result.path.clone(),
                                    mtime: result.mtime,
                                    size: result.size,
                                    content_id: None,
                                });
                                MetaState::Pending
                            }
                        };
                        // Cache hit (legítimo ou via cura): se algum
                        // jogador ainda está com `OpeningLabel::Pending`,
                        // enfileira para enriquecimento. Idempotente —
                        // dedup via `enrichment_in_flight`.
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
            // Snapshot dos campos não-outcome antes do match — o
            // pattern move o conteúdo de `outcome` e deixa `msg`
            // parcialmente movido.
            let LibraryResult {
                path,
                mtime,
                size,
                content_id,
                outcome,
            } = msg;
            let state = match outcome {
                ParseOutcome::Parsed(meta) => {
                    // Fresh parse: todos os jogadores chegam com
                    // `OpeningLabel::Pending` — enfileira para
                    // enriquecimento. Idempotente.
                    self.enqueue_enrichment_if_needed(&path, &meta);
                    let st = MetaState::Parsed(meta);
                    self.cache_insert_if_complete(&path, mtime, size, content_id, &st);
                    st
                }
                ParseOutcome::Unsupported(reason) => {
                    let st = MetaState::Unsupported(reason);
                    self.cache_insert_if_complete(&path, mtime, size, content_id, &st);
                    st
                }
                ParseOutcome::Failed(e) => MetaState::Failed(e),
            };
            if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
                entry.meta = state;
                updated = true;
            }
        }

        // Fase 3: Drena resultados do pool de enriquecimento. O worker
        // sempre devolve um vetor de tamanho `players.len()` (preenchido
        // com `Unclassifiable` em qualquer falha), então não precisamos
        // mais tratar vetor vazio como "skip" — qualquer resultado
        // marca o estado terminal e impede re-tentativa em launches
        // futuros.
        while let Ok(res) = self.rx_enrich_result.try_recv() {
            self.enrichment_in_flight.remove(&res.path);
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
            // Atualiza cache via path → content_id → MetaState. Pode
            // não estar sincronizado com `entries` se o usuário mudou
            // de diretório entre enqueue e resultado — mesmo assim
            // gravamos o rótulo calculado, é válido.
            if let Some(MetaState::Parsed(cached_meta)) =
                self.cache.state_mut_for_path(&res.path)
            {
                for (i, op) in res.openings.iter().enumerate() {
                    if let Some(player) = cached_meta.players.get_mut(i) {
                        player.opening = op.clone();
                    }
                }
                self.cache_dirty = true;
                self.cache_updates_since_flush += 1;
            }
        }

        if updated {
            self.stats_dirty = true;
        }
        // Política de flush do cache:
        //
        // 1) Quando tudo assentar (scanner + pool principal + pool de
        //    enriquecimento ociosos): flush imediato. Caso normal numa
        //    biblioteca pequena ou após enriquecimento completo.
        //
        // 2) Caso contrário, flush incremental: a cada
        //    `ENRICHMENT_FLUSH_BATCH` updates OU a cada
        //    `ENRICHMENT_FLUSH_INTERVAL`. Isso garante que kills/crashes
        //    durante enriquecimentos longos não percam todo o trabalho.
        if self.cache_dirty {
            let idle = !self.scanning
                && self.pending_count() == 0
                && self.enrichment_in_flight.is_empty();
            let batch_full = self.cache_updates_since_flush >= ENRICHMENT_FLUSH_BATCH;
            let interval_elapsed = self.last_cache_flush.elapsed() >= ENRICHMENT_FLUSH_INTERVAL
                && self.cache_updates_since_flush > 0;
            if idle || batch_full || interval_elapsed {
                self.save_cache();
                self.cache_updates_since_flush = 0;
                self.last_cache_flush = Instant::now();
            }
        }
        updated
    }

    /// Insere o `state` no cache se temos fingerprint completo
    /// (mtime/size/content_id). Caso falte algum, não dá pra montar
    /// fingerprint estável — pula o cache (próximo scan re-parseia,
    /// mas isso era o comportamento antigo também).
    fn cache_insert_if_complete(
        &mut self,
        path: &Path,
        mtime: Option<SystemTime>,
        size: Option<u64>,
        content_id: Option<ContentId>,
        state: &MetaState,
    ) {
        if let (Some(mt), Some(sz), Some(cid)) = (mtime, size, content_id) {
            self.cache
                .insert(path.to_path_buf(), sz, mt, cid, state.clone());
            self.cache_dirty = true;
            self.cache_updates_since_flush += 1;
        }
    }

    /// Enfileira `path` no pool de enriquecimento se a meta ainda
    /// precisa de rótulo (pelo menos um jogador com `OpeningLabel::Pending`).
    /// `Unclassifiable` é estado terminal — não enfileira de novo. Dedup
    /// via `enrichment_in_flight`. Apenas 1v1 (2 jogadores) — os demais
    /// já viraram `Unsupported` no parse rápido e não chegam aqui, mas
    /// o check é defensivo.
    fn enqueue_enrichment_if_needed(&mut self, path: &Path, meta: &ParsedMeta) {
        if meta.players.len() != 2 {
            return;
        }
        let needs = meta.players.iter().any(|p| p.opening.is_pending());
        if !needs {
            return;
        }
        if self.enrichment_in_flight.insert(path.to_path_buf()) {
            let _ = self.tx_enrich_work.send(path.to_path_buf());
        }
    }

    /// Quantos paths estão no pool de enriquecimento (em-vôo). Usado
    /// pela UI para mostrar "classificando aberturas (N)" enquanto o
    /// trabalho acontece em background.
    pub fn enrichment_in_flight_count(&self) -> usize {
        self.enrichment_in_flight.len()
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
                opening: OpeningLabel::Pending,
            })
            .collect(),
    })
}

/// Parseia ~5 min do replay e classifica a abertura de cada jogador.
/// Retorna um vetor sempre de tamanho 2, alinhado com
/// `ParsedMeta.players`. Qualquer falha (parse/extração) vira
/// `OpeningLabel::Unclassifiable` — estado terminal que impede o pool
/// de re-tentar o mesmo replay em launches futuros (a próxima
/// tentativa só acontece se o `mtime` do arquivo mudar, invalidando o
/// cache).
fn compute_openings(path: &Path) -> Vec<OpeningLabel> {
    let unclassifiable_pair = || vec![OpeningLabel::Unclassifiable, OpeningLabel::Unclassifiable];
    let data = match parse_replay(path, ENRICHMENT_PARSE_SECONDS) {
        Ok(d) => d,
        Err(_) => return unclassifiable_pair(),
    };
    if data.players.len() != 2 {
        return unclassifiable_pair();
    }
    match extract_build_order(&data) {
        Ok(bo) => bo
            .players
            .iter()
            .map(|p| OpeningLabel::Classified(
                classify_opening(p, bo.loops_per_second).to_display_string(),
            ))
            .collect(),
        Err(_) => unclassifiable_pair(),
    }
}
