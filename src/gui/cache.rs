// Cache persistente da biblioteca de replays — duas camadas keyed por
// conteúdo + fingerprint de caminho.
//
// Por que duas camadas?
//
// O cache anterior (versões ≤6) usava `(PathBuf, SystemTime mtime)` como
// chave única. Isso falhava em dois cenários reais no Windows:
//
// 1. **Path drift**: o `working_dir` do usuário podia mudar de
//    `C:\Users\paulo\…` para `C:/Users/Paulo/…` (separador, casing, ou
//    drive letter), produzindo chaves diferentes para o mesmo arquivo.
//    `HashMap<PathBuf, _>` compara bytes — o lookup silenciosamente
//    perdia.
// 2. **mtime drift**: OneDrive/Dropbox/antivírus dão "touch" no mtime
//    sem mudar conteúdo. A comparação estrita `==` invalidava a entrada
//    e re-disparava o parse.
//
// A solução híbrida:
//
// - Camada rápida (`paths`): `HashMap<canonical_path, (size, mtime,
//   content_id)>`. Lookup só com `metadata()`, sem ler o arquivo.
// - Camada lenta (`contents`): `HashMap<content_id, MetaState>` onde
//   `content_id` é o BLAKE3 de 32 bytes do arquivo inteiro. Quando a
//   camada rápida erra, hasheamos o conteúdo e procuramos aqui — se
//   bater, "curamos" a camada rápida com o novo fingerprint e
//   reaproveitamos o `MetaState`.
//
// Resultado: caso normal não paga I/O extra; mtime/path drift se
// auto-cura no primeiro scan após o drift; arquivos genuinamente novos
// caem no parse normal.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::library::{MetaState, OpeningLabel, ParsedMeta, PlayerMeta};

// CACHE_VERSION bumps:
// - 2→3: adicionado rótulo de abertura.
// - 3→4: corrigido fuso horário do `datetime`.
// - 4→5: adicionado `version` + `cache_handles`.
// - 5→6: `opening` virou enum (`Pending`/`Classified`/`Unclassifiable`).
// - 6→7: cache reorganizado em duas camadas (path fingerprint + content
//   hash) para sobreviver a path/mtime drift.
const CACHE_VERSION: u32 = 7;
const CACHE_FILE: &str = "library_meta.bin";

/// 32 bytes do BLAKE3 do conteúdo do replay. Pequeno o suficiente para
/// caber confortavelmente em memória mesmo numa biblioteca grande
/// (32 B × 100k replays ≈ 3 MB) e estável entre execuções.
pub type ContentId = [u8; 32];

/// Fingerprint barato de um arquivo no disco — o que conseguimos saber
/// só com `fs::metadata()`. Combinado com `content_id`, define a entrada
/// na camada rápida do cache.
#[derive(Clone, Debug)]
pub struct PathFingerprint {
    pub size: u64,
    pub mtime: SystemTime,
    pub content_id: ContentId,
}

// ── Tipos serializáveis (desacoplados dos tipos da UI) ───────────────

#[derive(Serialize, Deserialize)]
struct DiskCache {
    version: u32,
    /// Cada path conhecido com seu fingerprint mais recente. Vários
    /// paths podem mapear para o mesmo `content_id` (cópias do mesmo
    /// replay em pastas diferentes — comum quando o usuário move
    /// arquivos entre Accounts).
    paths: Vec<DiskPathFingerprint>,
    /// Estado parseado indexado por hash de conteúdo. É aqui que o
    /// trabalho caro vive; a camada `paths` é só o índice rápido.
    contents: Vec<DiskContentEntry>,
}

#[derive(Serialize, Deserialize)]
struct DiskPathFingerprint {
    path: String,
    size: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    content_id: ContentId,
}

#[derive(Serialize, Deserialize)]
struct DiskContentEntry {
    content_id: ContentId,
    state: CachedMetaState,
}

#[derive(Serialize, Deserialize)]
enum CachedMetaState {
    Parsed(CachedParsedMeta),
    Unsupported(String),
}

#[derive(Serialize, Deserialize)]
struct CachedParsedMeta {
    map: String,
    datetime: String,
    duration_seconds: u32,
    game_loops: u32,
    version: Option<String>,
    cache_handles: Vec<String>,
    players: Vec<CachedPlayerMeta>,
}

#[derive(Serialize, Deserialize)]
struct CachedPlayerMeta {
    name: String,
    race: String,
    mmr: Option<i32>,
    result: String,
    opening: CachedOpeningLabel,
}

#[derive(Serialize, Deserialize)]
enum CachedOpeningLabel {
    Pending,
    Classified(String),
    Unclassifiable,
}

// ── Conversões ───────────────────────────────────────────────────────

fn system_time_to_parts(t: SystemTime) -> (u64, u32) {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs(), d.subsec_nanos()),
        Err(_) => (0, 0),
    }
}

fn parts_to_system_time(secs: u64, nanos: u32) -> SystemTime {
    UNIX_EPOCH + Duration::new(secs, nanos)
}

fn to_cached_opening(o: &OpeningLabel) -> CachedOpeningLabel {
    match o {
        OpeningLabel::Pending => CachedOpeningLabel::Pending,
        OpeningLabel::Classified(s) => CachedOpeningLabel::Classified(s.clone()),
        OpeningLabel::Unclassifiable => CachedOpeningLabel::Unclassifiable,
    }
}

fn from_cached_opening(c: CachedOpeningLabel) -> OpeningLabel {
    match c {
        CachedOpeningLabel::Pending => OpeningLabel::Pending,
        CachedOpeningLabel::Classified(s) => OpeningLabel::Classified(s),
        CachedOpeningLabel::Unclassifiable => OpeningLabel::Unclassifiable,
    }
}

fn to_cached_meta(meta: &ParsedMeta) -> CachedParsedMeta {
    CachedParsedMeta {
        map: meta.map.clone(),
        datetime: meta.datetime.clone(),
        duration_seconds: meta.duration_seconds,
        game_loops: meta.game_loops,
        version: meta.version.clone(),
        cache_handles: meta.cache_handles.clone(),
        players: meta
            .players
            .iter()
            .map(|p| CachedPlayerMeta {
                name: p.name.clone(),
                race: p.race.clone(),
                mmr: p.mmr,
                result: p.result.clone(),
                opening: to_cached_opening(&p.opening),
            })
            .collect(),
    }
}

fn from_cached_meta(c: CachedParsedMeta) -> ParsedMeta {
    ParsedMeta {
        map: c.map,
        datetime: c.datetime,
        duration_seconds: c.duration_seconds,
        game_loops: c.game_loops,
        version: c.version,
        cache_handles: c.cache_handles,
        players: c
            .players
            .into_iter()
            .map(|p| PlayerMeta {
                name: p.name,
                race: p.race,
                mmr: p.mmr,
                result: p.result,
                opening: from_cached_opening(p.opening),
            })
            .collect(),
    }
}

fn to_cached_state(state: &MetaState) -> Option<CachedMetaState> {
    match state {
        MetaState::Parsed(m) => Some(CachedMetaState::Parsed(to_cached_meta(m))),
        MetaState::Unsupported(r) => Some(CachedMetaState::Unsupported(r.clone())),
        _ => None,
    }
}

fn from_cached_state(c: CachedMetaState) -> MetaState {
    match c {
        CachedMetaState::Parsed(m) => MetaState::Parsed(from_cached_meta(m)),
        CachedMetaState::Unsupported(r) => MetaState::Unsupported(r),
    }
}

// ── Helpers públicos: canonicalização e hash ─────────────────────────

/// Canonicaliza um path para uso como chave do cache. No Windows, usa
/// `dunce` para evitar o prefixo `\\?\` que `fs::canonicalize` adiciona
/// (e que quebra a igualdade byte-a-byte com paths produzidos por
/// `fs::read_dir`). Em qualquer SO, resolve `..`, `.`, e symlinks.
///
/// Falha (arquivo não existe, sem permissão) → cai no path original.
/// Isso preserva o comportamento "best effort": uma chave malformada é
/// melhor do que travar o scan.
pub fn canonicalize_path(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Calcula o `ContentId` (BLAKE3) de um arquivo. Lê o arquivo inteiro
/// na memória — replays são pequenos (~50-500 KB), então isso é
/// barato. Retorna `None` em qualquer falha de I/O.
pub fn hash_file(path: &Path) -> Option<ContentId> {
    let bytes = fs::read(path).ok()?;
    Some(*blake3::hash(&bytes).as_bytes())
}

// ── Cache em memória (duas camadas) ──────────────────────────────────

/// Cache em memória da biblioteca. Owns ambas as camadas e expõe API
/// que combina as duas.
///
/// **Não é Sync/Send-friendly por design** — o `ReplayLibrary` é
/// single-threaded; pools de worker enviam resultados via canal.
pub struct LibraryCache {
    paths: HashMap<PathBuf, PathFingerprint>,
    contents: HashMap<ContentId, MetaState>,
}

impl LibraryCache {
    pub fn empty() -> Self {
        Self {
            paths: HashMap::new(),
            contents: HashMap::new(),
        }
    }

    /// Resolve um path para um `MetaState` cacheado. Tenta a camada
    /// rápida (path+size+mtime) primeiro; em miss, hasheia o conteúdo
    /// e procura na camada lenta. Em caso de cura (cache hit pelo
    /// hash), o fingerprint da camada rápida é atualizado em memória
    /// para o próximo scan bater direto.
    ///
    /// O `content_id` é devolvido em ambos os casos de miss
    /// (`Miss { content_id: Some(_) }`) para que o chamador possa
    /// reusá-lo no `insert` pós-parse, sem hashear duas vezes.
    pub fn lookup(
        &mut self,
        canonical_path: &Path,
        size: u64,
        mtime: SystemTime,
        on_disk_path: &Path,
    ) -> LookupOutcome {
        // Camada rápida — só `metadata()` foi necessário.
        if let Some(fp) = self.paths.get(canonical_path) {
            if fp.size == size && fp.mtime == mtime {
                if let Some(state) = self.contents.get(&fp.content_id) {
                    return LookupOutcome::Hit {
                        state: state.clone(),
                        content_id: fp.content_id,
                        healed: false,
                    };
                }
            }
        }

        // Camada lenta — lê e hasheia o arquivo. Custa um read,
        // economiza um parse + (potencialmente) ~5 min de
        // enriquecimento por replay.
        let content_id = match hash_file(on_disk_path) {
            Some(c) => c,
            None => return LookupOutcome::Miss { content_id: None },
        };

        if let Some(state) = self.contents.get(&content_id).cloned() {
            // Cura: atualiza/insere o fingerprint da camada rápida.
            self.paths.insert(
                canonical_path.to_path_buf(),
                PathFingerprint {
                    size,
                    mtime,
                    content_id,
                },
            );
            return LookupOutcome::Hit {
                state,
                content_id,
                healed: true,
            };
        }

        LookupOutcome::Miss {
            content_id: Some(content_id),
        }
    }

    /// Insere ou atualiza ambas as camadas para um path recém-parseado.
    /// O `content_id` deve ter sido calculado pelo chamador — geralmente
    /// pelo `hash_file` antes ou durante o parse.
    pub fn insert(
        &mut self,
        canonical_path: PathBuf,
        size: u64,
        mtime: SystemTime,
        content_id: ContentId,
        state: MetaState,
    ) {
        self.paths.insert(
            canonical_path,
            PathFingerprint {
                size,
                mtime,
                content_id,
            },
        );
        // `to_cached_state` filtra Pending/Failed; só persistem
        // `Parsed`/`Unsupported`. Usa o mesmo critério aqui em memória
        // para manter consistência on-disk e em-memória.
        if matches!(&state, MetaState::Parsed(_) | MetaState::Unsupported(_)) {
            self.contents.insert(content_id, state);
        }
    }

    /// Acesso mutável ao `MetaState` cacheado para um dado path. Usado
    /// pelo pool de enriquecimento para preencher `opening` *in place*
    /// sem ter que reinserir.
    pub fn state_mut_for_path(&mut self, canonical_path: &Path) -> Option<&mut MetaState> {
        let content_id = self.paths.get(canonical_path)?.content_id;
        self.contents.get_mut(&content_id)
    }

    /// Para fins de UI / diagnóstico — quantas entradas na camada de
    /// conteúdo.
    #[allow(dead_code)]
    pub fn content_count(&self) -> usize {
        self.contents.len()
    }
}

/// Resultado de um `LibraryCache::lookup`.
pub enum LookupOutcome {
    /// Hit no cache — `state` é a metadata cacheada. `healed=true`
    /// indica que a camada rápida foi atualizada em memória durante o
    /// lookup (path/mtime drift recuperado via hash); a chamadora deve
    /// marcar o cache como sujo.
    Hit {
        state: MetaState,
        content_id: ContentId,
        healed: bool,
    },
    /// Miss — replay precisa ser parseado. Quando `content_id` está
    /// presente, a camada lenta já hasheou o arquivo e o chamador pode
    /// reusar o hash no `insert` pós-parse (sem segunda leitura).
    /// `None` indica que o hash falhou (I/O error) — chamador deve
    /// recalcular ou desistir do cache.
    Miss { content_id: Option<ContentId> },
}

// ── Diretório e caminho ──────────────────────────────────────────────

fn cache_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sc2-replay-utils").join("cache"))
}

fn cache_path() -> Option<PathBuf> {
    cache_dir().map(|d| d.join(CACHE_FILE))
}

// ── API pública ──────────────────────────────────────────────────────

/// Carrega o cache do disco. Retorna estrutura vazia se o arquivo não
/// existir, estiver corrompido, ou tiver versão incompatível.
pub fn load() -> LibraryCache {
    let path = match cache_path() {
        Some(p) if p.exists() => p,
        _ => return LibraryCache::empty(),
    };
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cache: falha ao ler {}: {e}", path.display());
            return LibraryCache::empty();
        }
    };
    let disk: DiskCache = match bitcode::deserialize(&bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cache: falha ao decodificar {}: {e}", path.display());
            return LibraryCache::empty();
        }
    };
    if disk.version != CACHE_VERSION {
        eprintln!(
            "cache: versão incompatível ({} != {CACHE_VERSION}), descartando",
            disk.version
        );
        return LibraryCache::empty();
    }

    let mut paths = HashMap::with_capacity(disk.paths.len());
    for entry in disk.paths {
        paths.insert(
            PathBuf::from(entry.path),
            PathFingerprint {
                size: entry.size,
                mtime: parts_to_system_time(entry.mtime_secs, entry.mtime_nanos),
                content_id: entry.content_id,
            },
        );
    }

    let mut contents = HashMap::with_capacity(disk.contents.len());
    for entry in disk.contents {
        contents.insert(entry.content_id, from_cached_state(entry.state));
    }

    LibraryCache { paths, contents }
}

/// Grava o cache em disco. Escrita atômica via .tmp + rename.
pub fn save(cache: &LibraryCache) {
    let dir = match cache_dir() {
        Some(d) => d,
        None => return,
    };
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("cache: falha ao criar {}: {e}", dir.display());
        return;
    }

    // Coleta apenas entradas de conteúdo persistíveis (Parsed /
    // Unsupported). Em seguida, filtra `paths` para só referenciar
    // content_ids que sobreviveram. Isso impede uma situação onde um
    // path remanescente apontaria para um conteúdo "fantasma".
    let contents: Vec<DiskContentEntry> = cache
        .contents
        .iter()
        .filter_map(|(content_id, state)| {
            let cached_state = to_cached_state(state)?;
            Some(DiskContentEntry {
                content_id: *content_id,
                state: cached_state,
            })
        })
        .collect();
    let valid_ids: std::collections::HashSet<ContentId> =
        contents.iter().map(|e| e.content_id).collect();
    let paths: Vec<DiskPathFingerprint> = cache
        .paths
        .iter()
        .filter(|(_, fp)| valid_ids.contains(&fp.content_id))
        .map(|(path, fp)| {
            let (secs, nanos) = system_time_to_parts(fp.mtime);
            DiskPathFingerprint {
                path: path.to_string_lossy().into_owned(),
                size: fp.size,
                mtime_secs: secs,
                mtime_nanos: nanos,
                content_id: fp.content_id,
            }
        })
        .collect();

    let disk = DiskCache {
        version: CACHE_VERSION,
        paths,
        contents,
    };
    let bytes = match bitcode::serialize(&disk) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cache: falha ao serializar: {e}");
            return;
        }
    };
    let path = dir.join(CACHE_FILE);
    let tmp = dir.join(format!("{CACHE_FILE}.tmp"));
    if let Err(e) = fs::write(&tmp, &bytes) {
        eprintln!("cache: falha ao gravar {}: {e}", tmp.display());
        return;
    }
    if let Err(e) = fs::rename(&tmp, &path) {
        eprintln!("cache: falha ao renomear para {}: {e}", path.display());
    }
}
