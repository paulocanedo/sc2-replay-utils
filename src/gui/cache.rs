// Cache persistente de metadados da biblioteca de replays.
//
// Grava em bincode um mapa (caminho → metadados + mtime) para que
// startups subsequentes não precisem re-parsear replays já conhecidos.
// O cache fica em {config_dir}/sc2-replay-utils/cache/library_meta.bin.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::library::{MetaState, ParsedMeta, PlayerMeta};

const CACHE_VERSION: u32 = 2;
const CACHE_FILE: &str = "library_meta.bin";

// ── Tipos serializáveis (desacoplados dos tipos da UI) ───────────────

#[derive(Serialize, Deserialize)]
struct DiskCache {
    version: u32,
    entries: Vec<CacheEntry>,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    path: String,
    mtime_secs: u64,
    mtime_nanos: u32,
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
    players: Vec<CachedPlayerMeta>,
}

#[derive(Serialize, Deserialize)]
struct CachedPlayerMeta {
    name: String,
    race: String,
    mmr: Option<i32>,
    result: String,
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

fn to_cached_meta(meta: &ParsedMeta) -> CachedParsedMeta {
    CachedParsedMeta {
        map: meta.map.clone(),
        datetime: meta.datetime.clone(),
        duration_seconds: meta.duration_seconds,
        game_loops: meta.game_loops,
        players: meta
            .players
            .iter()
            .map(|p| CachedPlayerMeta {
                name: p.name.clone(),
                race: p.race.clone(),
                mmr: p.mmr,
                result: p.result.clone(),
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
        players: c
            .players
            .into_iter()
            .map(|p| PlayerMeta {
                name: p.name,
                race: p.race,
                mmr: p.mmr,
                result: p.result,
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

// ── Diretório e caminho ──────────────────────────────────────────────

fn cache_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sc2-replay-utils").join("cache"))
}

fn cache_path() -> Option<PathBuf> {
    cache_dir().map(|d| d.join(CACHE_FILE))
}

// ── API pública ──────────────────────────────────────────────────────

/// Carrega o cache do disco. Retorna mapa vazio se o arquivo não existir,
/// estiver corrompido ou tiver versão incompatível.
pub fn load() -> HashMap<PathBuf, (SystemTime, MetaState)> {
    let path = match cache_path() {
        Some(p) if p.exists() => p,
        _ => return HashMap::new(),
    };
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cache: falha ao ler {}: {e}", path.display());
            return HashMap::new();
        }
    };
    let disk: DiskCache = match bitcode::deserialize(&bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cache: falha ao decodificar {}: {e}", path.display());
            return HashMap::new();
        }
    };
    if disk.version != CACHE_VERSION {
        eprintln!(
            "cache: versão incompatível ({} != {CACHE_VERSION}), descartando",
            disk.version
        );
        return HashMap::new();
    }
    let mut map = HashMap::with_capacity(disk.entries.len());
    for entry in disk.entries {
        let path = PathBuf::from(entry.path);
        let mtime = parts_to_system_time(entry.mtime_secs, entry.mtime_nanos);
        let state = from_cached_state(entry.state);
        map.insert(path, (mtime, state));
    }
    map
}

/// Grava o cache em disco. Escrita atômica via .tmp + rename.
pub fn save(cache: &HashMap<PathBuf, (SystemTime, MetaState)>) {
    let dir = match cache_dir() {
        Some(d) => d,
        None => return,
    };
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("cache: falha ao criar {}: {e}", dir.display());
        return;
    }
    let entries: Vec<CacheEntry> = cache
        .iter()
        .filter_map(|(path, (mtime, state))| {
            let cached_state = to_cached_state(state)?;
            let (secs, nanos) = system_time_to_parts(*mtime);
            Some(CacheEntry {
                path: path.to_string_lossy().into_owned(),
                mtime_secs: secs,
                mtime_nanos: nanos,
                state: cached_state,
            })
        })
        .collect();
    let disk = DiskCache {
        version: CACHE_VERSION,
        entries,
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
