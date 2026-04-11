use std::fs;
use std::path::{Path, PathBuf};

// ── Helpers de tempo ────────────────────────────────────────────────────────

/// Converte a velocidade do jogo SC2 para game loops por segundo real.
///
/// SC2 roda a 16 loops/segundo de jogo. Na velocidade Faster (padrão competitivo),
/// a simulação avança 1.4× mais rápido, resultando em 22.4 loops por segundo real.
pub fn game_speed_to_loops_per_second(game_speed: &str) -> f64 {
    match game_speed {
        "Slower" => 9.6,
        "Slow"   => 12.8,
        "Fast"   => 19.2,
        "Faster" => 22.4,
        _        => 16.0, // "Normal" e fallback
    }
}

// ── Helpers de string ────────────────────────────────────────────────────────

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

// ── Filesystem ──────────────────────────────────────────────────────────────

/// Lista todos os arquivos `.SC2Replay` em `base` **recursivamente**, ordenados
/// por nome. Usada pela biblioteca da GUI ao apontar para pastas no estilo do
/// SC2, onde os replays ficam em subpastas tipo
/// `Accounts/<id>/<região>/Replays/Multiplayer`. Retorna `Vec` vazio em caso
/// de erro em qualquer subdiretório — não aborta o processo.
pub fn list_replays_recursive(base: &Path) -> Vec<PathBuf> {
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
            } else if path
                .extension()
                .map_or(false, |ext| ext.eq_ignore_ascii_case("SC2Replay"))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
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
