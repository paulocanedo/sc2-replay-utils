use std::fs;
use std::path::{Path, PathBuf};
use std::process;

pub const DEFAULT_DIR_NAME: &str = "sc2replays-pack";

// ── Helpers de string ────────────────────────────────────────────────────────

/// Converte o nome da raça para a letra inicial maiúscula (T/P/Z/R).
pub fn race_letter(race: &str) -> char {
    match race.chars().next().unwrap_or('?') {
        'T' | 't' => 'T',
        'P' | 'p' => 'P',
        'Z' | 'z' => 'Z',
        'R' | 'r' => 'R',
        other => other,
    }
}

/// Separa clan tag e nome limpo de um nome bruto como "&lt;TAG&gt;<sp/>PlayerName".
/// Retorna `(clan, name)`. Clan é string vazia se não houver tag.
pub fn extract_clan_and_name(raw: &str) -> (String, String) {
    if let Some(sp_pos) = raw.find("<sp/>") {
        let name = raw[sp_pos + 5..].to_string();
        let prefix = &raw[..sp_pos];
        let clan = match (prefix.find("&lt;"), prefix.find("&gt;")) {
            (Some(start), Some(end)) => prefix[start + 4..end].to_string(),
            _ => String::new(),
        };
        (clan, name)
    } else {
        (String::new(), raw.to_string())
    }
}

/// Substitui caracteres que não são alfanuméricos, `-` ou `_` por `_`.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

// ── Helpers de filesystem ────────────────────────────────────────────────────

/// Resolve um caminho (arquivo ou diretório): usa `opt` se fornecido, senão busca `DEFAULT_DIR_NAME`.
pub fn resolve_path(opt: Option<PathBuf>) -> PathBuf {
    if let Some(p) = opt {
        if !p.exists() {
            eprintln!("Erro: '{}' não encontrado", p.display());
            process::exit(1);
        }
        return p;
    }
    let candidate = Path::new(DEFAULT_DIR_NAME);
    if candidate.is_dir() {
        println!("Usando diretório encontrado: {}", candidate.display());
        return candidate.to_path_buf();
    }
    eprintln!(
        "Nenhum argumento fornecido e '{}' não encontrado no diretório atual.",
        DEFAULT_DIR_NAME
    );
    process::exit(1);
}

/// Resolve o diretório de entrada: usa `opt` se fornecido, senão busca `DEFAULT_DIR_NAME`.
pub fn resolve_dir(opt: Option<PathBuf>) -> PathBuf {
    if let Some(p) = opt {
        if !p.is_dir() {
            eprintln!("Erro: '{}' não é um diretório", p.display());
            process::exit(1);
        }
        return p;
    }
    let candidate = Path::new(DEFAULT_DIR_NAME);
    if candidate.is_dir() {
        println!("Usando diretório encontrado: {}", candidate.display());
        return candidate.to_path_buf();
    }
    eprintln!(
        "Nenhum argumento fornecido e '{}' não encontrado no diretório atual.",
        DEFAULT_DIR_NAME
    );
    process::exit(1);
}

/// Lista todos os arquivos `.SC2Replay` em `dir`, ordenados por nome.
pub fn list_replays(dir: &Path) -> Vec<PathBuf> {
    let mut replays: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| {
            eprintln!("Erro ao ler diretório '{}': {}", dir.display(), e);
            process::exit(1);
        })
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.is_file()
                && path
                    .extension()
                    .map_or(false, |ext| ext.eq_ignore_ascii_case("SC2Replay")))
            .then_some(path)
        })
        .collect();
    replays.sort();
    replays
}

// ── Descoberta de replay mais recente ────────────────────────────────────────

/// Retorna o diretório padrão de replays do StarCraft II, se existir.
/// Usa a pasta de Documentos real do sistema (resolve corretamente nomes
/// localizados como "Documentos" no Windows em português).
pub fn sc2_default_dir() -> Option<PathBuf> {
    let p = dirs::document_dir()?.join("StarCraft II");
    p.is_dir().then_some(p)
}

/// Busca recursivamente em `base` o arquivo `.SC2Replay` modificado mais recentemente.
pub fn find_latest_replay(base: &Path) -> Option<PathBuf> {
    let mut latest: Option<(PathBuf, std::time::SystemTime)> = None;
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
            } else if path
                .extension()
                .map_or(false, |e| e.eq_ignore_ascii_case("SC2Replay"))
            {
                let modified = entry.metadata().ok()?.modified().ok()?;
                let is_newer = latest.as_ref().map_or(true, |(_, t)| modified > *t);
                if is_newer {
                    latest = Some((path, modified));
                }
            }
        }
    }

    latest.map(|(p, _)| p)
}

/// Garante que `out_dir` existe e está vazio. Cria se necessário, aborta se tiver arquivos.
pub fn prepare_out_dir(out_dir: &Path) {
    if out_dir.exists() {
        let not_empty = fs::read_dir(out_dir)
            .unwrap_or_else(|e| {
                eprintln!("Erro ao ler '{}': {}", out_dir.display(), e);
                process::exit(1);
            })
            .next()
            .is_some();
        if not_empty {
            eprintln!(
                "Erro: diretório '{}' já existe e não está vazio",
                out_dir.display()
            );
            process::exit(1);
        }
    } else {
        fs::create_dir_all(out_dir).unwrap_or_else(|e| {
            eprintln!("Erro ao criar '{}': {}", out_dir.display(), e);
            process::exit(1);
        });
    }
}
