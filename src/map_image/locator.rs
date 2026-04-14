// Localização de arquivos `.SC2Map` / `.s2ma` no disco.
//
// Duas estratégias, em ordem de preferência:
//
// 1. **Por cache handle do replay** — mais preciso e instantâneo.
//    `m_cacheHandles` no header de cada `.SC2Replay` lista exatamente
//    quais arquivos `.s2ma` foram usados, com SHA-256 hex. O Battle.net
//    armazena cada um em `Cache\<hash[0..2]>\<hash[2..4]>\<hash>.s2ma`,
//    então um cache_handle vira um caminho exato sem scan algum.
//    Cobre 100% dos mapas de ladder.
//
// 2. **Por título do mapa, matching por stem do filename** — fallback
//    para mapas instalados em `Documents\StarCraft II\Maps` cujo
//    filename é o título humano (ex.: `Pylon Pasture LE.SC2Map`).
//    Útil para mapas custom ou de testes.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Lista os caminhos padrão de pastas de Maps **instaladas** do
/// StarCraft II no Windows, filtrando as que existem no sistema atual.
/// É o universo do fallback por título (`resolve_map_file_default`),
/// não cobre o Battle.net Cache — esse é resolvido por cache handle
/// (ver [`resolve_from_cache_handles`]).
pub fn default_search_paths() -> Vec<PathBuf> {
    [
        r"C:\Program Files\StarCraft II\Maps",
        r"C:\Program Files (x86)\StarCraft II\Maps",
        r"C:\ProgramData\Blizzard Entertainment\StarCraft II\Maps",
    ]
    .into_iter()
    .map(PathBuf::from)
    .filter(|p| p.is_dir())
    .collect()
}

/// Roots do Battle.net Cache no Windows. O cliente Battle.net guarda os
/// `.s2ma` baixados em `Cache\<XX>\<YY>\<hash>.s2ma`. Há duas localizações
/// observadas na prática (system-wide e per-user) — verificamos ambas.
pub fn battlenet_cache_roots() -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from(
        r"C:\ProgramData\Blizzard Entertainment\Battle.net\Cache",
    )];
    if let Some(local) = dirs::data_local_dir() {
        out.push(local.join("Battle.net").join("Cache"));
    }
    out.into_iter().filter(|p| p.is_dir()).collect()
}

/// Resolve cache handles de replay para os caminhos dos `.s2ma`
/// correspondentes no Battle.net Cache que existem no disco.
///
/// **Devolve uma lista**, não um único path: o `m_cacheHandles` de um
/// replay tipicamente contém o mapa real **e** vários stubs de
/// dependência (mods como `Core.SC2Mod`, `Liberty.SC2Mod`, etc) — todos
/// como `.s2ma`. Os stubs de mod não são MPQs, são arquivos texto tipo
/// `"Standard Data: Core.SC2Mod"`. O caller (`load_for_replay`) abre
/// cada um e fica com o primeiro que for um MPQ válido com Minimap.tga.
///
/// Esta é a forma preferida de resolver mapas de ladder: é exata
/// (hash do replay → filename direto) e instantânea (sem scan).
pub fn resolve_from_cache_handles(handles: &[String]) -> Vec<PathBuf> {
    let roots = battlenet_cache_roots();
    if roots.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for handle in handles {
        let Some((ext, hash)) = parse_cache_handle(handle) else {
            continue;
        };
        if ext != "s2ma" {
            continue;
        }
        for root in &roots {
            let path = cache_handle_to_path(root, &hash);
            if path.is_file() {
                out.push(path);
                break;
            }
        }
    }
    out
}

/// Faz o parsing do cache handle hex de 80 chars do replay (formato
/// `s2protocol::Details::cache_handles`) extraindo extensão (4 bytes
/// ASCII como hex) e hash (64 chars hex de SHA-256). Region/delimiter
/// são ignorados — o filename do cache só usa o hash.
fn parse_cache_handle(handle: &str) -> Option<(String, String)> {
    // Layout do hex: 8(ext) + 4(delim "0000") + 4(region ASCII) + 64(hash) = 80
    if handle.len() != 80 {
        return None;
    }
    let ext = decode_hex_ascii(&handle[0..8])?;
    let hash = handle[16..].to_ascii_lowercase();
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((ext, hash))
}

/// Converte uma string de chars hex pares em bytes ASCII (e.g. `"73326d61"`
/// → `"s2ma"`). Para nossa utilidade — extrair a extensão de um cache
/// handle — basta isso. Devolve `None` se a string não for hex válida ou
/// se algum byte não for ASCII imprimível.
fn decode_hex_ascii(hex: &str) -> Option<String> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut out = String::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks_exact(2) {
        let s = std::str::from_utf8(chunk).ok()?;
        let byte = u8::from_str_radix(s, 16).ok()?;
        if !(0x20..=0x7e).contains(&byte) && byte != 0 {
            return None;
        }
        if byte != 0 {
            out.push(byte as char);
        }
    }
    Some(out)
}

fn cache_handle_to_path(root: &Path, hash: &str) -> PathBuf {
    // O Battle.net Cache shardiza por 2/2 chars do hash.
    root.join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.s2ma"))
}

/// Resolve um título de mapa (ex.: vindo de `ReplayTimeline.map`) para o
/// caminho de um arquivo MPQ candidato. Devolve `None` se nada bater.
///
/// Procura recursivamente em todos os `search_paths` por arquivos com
/// extensão `.SC2Map` ou `.s2ma`, comparando o stem do arquivo com o
/// `map_title` (case-insensitive). Para o `.s2ma` do cache do Battle.net,
/// o stem é um hash hexadecimal e nunca casa — limitação conhecida.
///
/// **Versão sem cache.** Cada chamada refaz a varredura — útil para
/// testes e usos pontuais. Para o caso quente do GUI use
/// [`resolve_map_file_default`], que cacheia o índice por sessão.
pub fn resolve_map_file(map_title: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    if map_title.is_empty() {
        return None;
    }
    for base in search_paths {
        for candidate in collect_candidates(base) {
            if stem_matches(&candidate, map_title) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Versão cacheada de [`resolve_map_file`] que usa [`default_search_paths`]
/// e mantém um índice `stem_lowercase → PathBuf` num `OnceLock` global.
///
/// A varredura recursiva acontece **uma única vez por processo**, na
/// primeira chamada. Lookups subsequentes são `HashMap::get`. Isso é
/// crítico para o thread da UI: hoje `LoadedReplay::load` roda síncrono
/// e qualquer varredura por replay carregado trava o frame.
///
/// Limitação: como o índice é construído na primeira chamada, mapas
/// instalados durante a sessão não aparecem até reiniciar.
pub fn resolve_map_file_default(map_title: &str) -> Option<PathBuf> {
    if map_title.is_empty() {
        return None;
    }
    let index = INDEX.get_or_init(|| build_index(&default_search_paths()));
    index.get(&map_title.to_ascii_lowercase()).cloned()
}

static INDEX: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();

fn build_index(search_paths: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut index: HashMap<String, PathBuf> = HashMap::new();
    for base in search_paths {
        for candidate in collect_candidates(base) {
            if let Some(stem) = candidate.file_stem().and_then(|s| s.to_str()) {
                // entry().or_insert: o primeiro path encontrado para um
                // dado stem ganha. A ordem de `default_search_paths`
                // (oficiais primeiro) é deliberada.
                index
                    .entry(stem.to_ascii_lowercase())
                    .or_insert(candidate);
            }
        }
    }
    index
}

/// Varredura recursiva coletando arquivos com extensão `.SC2Map` ou
/// `.s2ma`. Mesma forma de `crate::utils::list_replays_recursive`,
/// trocando o filtro de extensão. Erros em subdiretórios são ignorados
/// (continuamos com o que conseguimos ler).
fn collect_candidates(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut queue = vec![base.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push(path);
            } else if is_map_file(&path) {
                out.push(path);
            }
        }
    }
    out
}

fn is_map_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("SC2Map") || e.eq_ignore_ascii_case("s2ma"))
        .unwrap_or(false)
}

fn stem_matches(path: &Path, target: &str) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case(target))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs::{self, File};

    fn tmp_dir(name: &str) -> PathBuf {
        let p = env::temp_dir().join(format!(
            "sc2_locator_test_{}_{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn resolves_by_stem_case_insensitive() {
        let dir = tmp_dir("stem");
        let target = dir.join("Pylon Pasture LE.SC2Map");
        File::create(&target).unwrap();
        // Outro arquivo qualquer pra garantir que filtra.
        File::create(dir.join("noise.txt")).unwrap();

        let found = resolve_map_file("pylon pasture le", &[dir.clone()]);
        assert_eq!(found.as_deref(), Some(target.as_path()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolves_recursively_in_subdirs() {
        let dir = tmp_dir("recursive");
        let sub = dir.join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        let target = sub.join("Goldenaura LE.s2ma");
        File::create(&target).unwrap();

        let found = resolve_map_file("Goldenaura LE", &[dir.clone()]);
        assert_eq!(found.as_deref(), Some(target.as_path()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn returns_none_when_no_match() {
        let dir = tmp_dir("nomatch");
        File::create(dir.join("Other.SC2Map")).unwrap();
        assert!(resolve_map_file("Missing Map", &[dir.clone()]).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_real_cache_handle() {
        // Cache handle real montado: ext "s2ma" (73326d61) + delim "0000" +
        // region "EU" (4555) + hash de 64 chars (zeros pra simplicidade).
        let handle = format!(
            "{ext}{delim}{region}{hash}",
            ext = "73326d61",
            delim = "0000",
            region = "4555",
            hash = "abcdef0123456789".repeat(4),
        );
        assert_eq!(handle.len(), 80);
        let (ext, hash) = parse_cache_handle(&handle).expect("parse");
        assert_eq!(ext, "s2ma");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn rejects_handle_wrong_length() {
        assert!(parse_cache_handle("abc").is_none());
        assert!(parse_cache_handle(&"a".repeat(79)).is_none());
        assert!(parse_cache_handle(&"a".repeat(81)).is_none());
    }

    #[test]
    fn rejects_handle_with_non_hex_hash() {
        let handle = format!("73326d6100004555{}", "z".repeat(64));
        assert!(parse_cache_handle(&handle).is_none());
    }

    #[test]
    fn cache_handle_to_path_shards_correctly() {
        let root = PathBuf::from(r"C:\Cache");
        let hash = "abcdef0123456789".repeat(4);
        let path = cache_handle_to_path(&root, &hash);
        assert_eq!(
            path,
            root.join("ab").join("cd").join(format!("{hash}.s2ma"))
        );
    }

    #[test]
    fn decode_hex_ascii_works() {
        // "s2ma" em hex
        assert_eq!(decode_hex_ascii("73326d61").as_deref(), Some("s2ma"));
        // Bytes nulos são ignorados (formato region "EU\0\0" → "EU")
        assert_eq!(decode_hex_ascii("45550000").as_deref(), Some("EU"));
        // Hex inválido
        assert!(decode_hex_ascii("zz").is_none());
        // Comprimento ímpar
        assert!(decode_hex_ascii("733").is_none());
    }
}
