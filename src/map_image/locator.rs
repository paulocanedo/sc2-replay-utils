// Localização de arquivos `.SC2Map` / `.s2ma` no disco.
//
// Match estratégia v1: pelo *stem* do filename. Funciona perfeitamente
// para mapas instalados em pastas como `StarCraft II/Maps/<Title>.SC2Map`.
// Mapas baixados para o Battle.net Cache têm nome em hash (e.g.
// `b1f...c9.s2ma`) e por isso não casam por stem — ficam como limitação
// conhecida desta versão. Quando precisarmos resolver maps do cache,
// estendemos para abrir e ler o `DocumentHeader` interno.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Lista os caminhos padrão onde o StarCraft II guarda mapas no Windows,
/// filtrando os que existem no sistema atual. A ordem reflete prioridade:
/// instalações oficiais primeiro.
///
/// **Battle.net Cache não está aqui** propositalmente: contém milhares de
/// `.s2ma` cujo filename é hash hexadecimal, então o matching por stem
/// (estratégia da v1) nunca casa com eles. Varrê-lo só queima ~segundos
/// no thread da UI sem ganho. Será adicionado quando o locator suportar
/// matching por conteúdo (lendo `DocumentHeader` interno).
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
    fn empty_title_returns_none() {
        let dir = tmp_dir("empty");
        File::create(dir.join("Foo.SC2Map")).unwrap();
        assert!(resolve_map_file("", &[dir.clone()]).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
