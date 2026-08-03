//! Pasta de templates do overlay: bootstrap, guard de path traversal e
//! mapa de content-type.
//!
//! A pasta é `dirs::config_dir()/sc2-replay-utils/overlay/` — mesma
//! convenção do `config.yaml` e do cache da biblioteca. Ela é criada no
//! **start do servidor**, não no start do app: quem nunca liga a feature
//! nunca ganha um diretório órfão.
//!
//! Tudo que o usuário colocar lá é servido por HTTP. O guard aqui impede
//! *sair* da pasta; ele deliberadamente não restringe o que é servido
//! *dentro* dela.

use std::fs;
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_README: &str = include_str!("../../../data/overlay/README.md");
pub const DEFAULT_INDEX_HTML: &str = include_str!("../../../data/overlay/index.html");
pub const DEFAULT_STYLE_CSS: &str = include_str!("../../../data/overlay/style.css");
pub const DEFAULT_DASHBOARD_HTML: &str =
    include_str!("../../../data/overlay/stats-dashboard.html");
pub const DEFAULT_DASHBOARD_CSS: &str =
    include_str!("../../../data/overlay/stats-dashboard.css");
pub const DEFAULT_LIVE_HTML: &str = include_str!("../../../data/overlay/live-players.html");
pub const DEFAULT_LIVE_CSS: &str = include_str!("../../../data/overlay/live-players.css");

/// Arquivos escritos no bootstrap e reescritos pelo "restaurar padrões".
///
/// Os nomes usam `/` como separador; `ensure_dir` cria os subdiretórios.
/// Os SVGs de raça são os mesmos que a UI nativa usa (`assets/race/`), para
/// o overlay e o app falarem a mesma língua visual.
///
/// O `README.md` vem primeiro de propósito: é a primeira coisa que o usuário
/// vê ao abrir a pasta, e é ele que documenta o contrato de dados sem exigir
/// que ninguém leia o Rust. Entra no `restore_defaults` como os demais —
/// senão um README editado (ou de uma versão antiga) sobreviveria a um
/// "restaurar padrões" e passaria a descrever um template que não existe
/// mais.
const DEFAULTS: &[(&str, &str)] = &[
    ("README.md", DEFAULT_README),
    ("index.html", DEFAULT_INDEX_HTML),
    ("style.css", DEFAULT_STYLE_CSS),
    ("stats-dashboard.html", DEFAULT_DASHBOARD_HTML),
    ("stats-dashboard.css", DEFAULT_DASHBOARD_CSS),
    ("live-players.html", DEFAULT_LIVE_HTML),
    ("live-players.css", DEFAULT_LIVE_CSS),
    ("race/terran.svg", include_str!("../../../assets/race/terran.svg")),
    ("race/protoss.svg", include_str!("../../../assets/race/protoss.svg")),
    ("race/zerg.svg", include_str!("../../../assets/race/zerg.svg")),
    ("race/random.svg", include_str!("../../../assets/race/random.svg")),
];

/// Template servido em `/` quando o usuário não tem um `index.html`.
pub const INDEX_TEMPLATE: &str = "index.html";

/// Teto de tamanho para asset estático. Não é uma medida de segurança —
/// é para um usuário que largue uma captura de tela crua de 4 GB na pasta
/// não pendurar a fonte Browser do OBS até o disco acabar.
const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;

/// Nomes de dispositivo reservados do Windows. Abrir um destes não lê um
/// arquivo — conversa com um device driver.
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Caminho da pasta de templates. `None` quando `dirs::config_dir()` não
/// resolve — mesmo modo de falha que `AppConfig::config_path()`.
pub fn overlay_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sc2-replay-utils").join("overlay"))
}

/// Cria a pasta (com pais) e escreve os arquivos padrão **apenas se
/// ausentes** — nunca sobrescreve o que o usuário editou.
pub fn ensure_dir() -> Result<PathBuf, String> {
    let dir = overlay_dir().ok_or_else(|| "config dir indisponível".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    for (name, contents) in DEFAULTS {
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        fs::write(&path, contents).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(dir)
}

/// Reescreve os arquivos padrão com os originais embutidos, movendo para
/// `<nome>.bak` os que o usuário tinha editado.
///
/// Não há modal de confirmação — o `.bak` é a rede de segurança, e é
/// coerente com o "Restaurar padrões" do config, que também não confirma.
/// Arquivos idênticos ao padrão são reescritos sem gerar `.bak`, senão
/// cada clique encheria a pasta de backups de coisas que ninguém tocou
/// (os SVGs de raça, por exemplo). Arquivos que o usuário adicionou por
/// conta própria ficam intactos.
pub fn restore_defaults() -> Result<PathBuf, String> {
    let dir = ensure_dir()?;
    for (name, contents) in DEFAULTS {
        let path = dir.join(name);
        let modified = match fs::read_to_string(&path) {
            Ok(current) => current != *contents,
            // Ilegível ou binário: trate como modificado e preserve.
            Err(_) => path.exists(),
        };
        if modified {
            let backup = path.with_extension(format!(
                "{}.bak",
                path.extension().and_then(|e| e.to_str()).unwrap_or("")
            ));
            fs::rename(&path, &backup)
                .map_err(|e| format!("backup {}: {e}", backup.display()))?;
        }
        fs::write(&path, contents).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(dir)
}

/// `true` quando a rota deve ser renderizada como template em vez de
/// servida como arquivo. Só `.html` — CSS/JS/imagens saem verbatim.
pub fn is_template(url_path: &str) -> bool {
    url_path == "/" || url_path.to_ascii_lowercase().ends_with(".html")
}

/// Nome do template para uma rota. `/` vira `index.html`.
pub fn template_name(url_path: &str) -> Option<String> {
    if url_path == "/" {
        return Some(INDEX_TEMPLATE.to_string());
    }
    sanitize_rel_path(url_path).map(|p| {
        // minijinja usa `/` como separador em nome de template, inclusive
        // no Windows.
        p.components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/")
    })
}

/// Converte a parte de path de uma URL num caminho relativo seguro.
///
/// **Função pura** — não toca no filesystem, o que a torna exaustivamente
/// testável. Devolve `None` para qualquer coisa que possa escapar da pasta
/// ou alcançar algo que não seja um arquivo comum.
///
/// A ordem dos passos importa: o percent-decode vem **antes** das
/// verificações, senão `%2e%2e%2f` passa direto. `tiny_http` entrega
/// `request.url()` sem decodificar.
pub fn sanitize_rel_path(url_path: &str) -> Option<PathBuf> {
    let raw = url_path.strip_prefix('/').unwrap_or(url_path);
    if raw.is_empty() {
        return None;
    }
    let decoded = percent_decode(raw);
    if decoded.is_empty() || decoded.contains('\0') {
        return None;
    }

    let path = Path::new(&decoded);
    let mut out = PathBuf::new();
    let mut any = false;
    for component in path.components() {
        // Um único check que rejeita `..`, `.`, RootDir (`/etc/passwd`) e
        // Prefix do Windows (`C:\`, `\\server\share`).
        let Component::Normal(os) = component else {
            return None;
        };
        let name = os.to_str()?;
        if !is_safe_component(name) {
            return None;
        }
        out.push(name);
        any = true;
    }
    if !any { None } else { Some(out) }
}

/// Regras por componente. Aplicadas em todas as plataformas, não só no
/// Windows: o servidor precisa aceitar/rejeitar as mesmas URLs em todo
/// lugar, e um `\` num nome de arquivo — inofensivo no Linux, separador de
/// path no Windows — não é algo que valha a pena servir.
fn is_safe_component(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // Separador do Windows. No Linux, `Path` trataria `\..\..\x` como um
    // nome de arquivo só e o `Component::Normal` acima deixaria passar.
    if name.contains('\\') {
        return false;
    }
    // NTFS alternate data stream (`arquivo.txt:oculto`), e também mata
    // `C:` quando o `Path` do Linux não reconhece o prefixo de drive.
    if name.contains(':') {
        return false;
    }
    // O Windows remove espaço/ponto no fim silenciosamente, o que dá pra
    // usar pra driblar checagem por sufixo.
    if name.ends_with(' ') || name.ends_with('.') {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        return false;
    }
    true
}

/// Decodifica `%XX`. Sequências malformadas ficam literais — é o
/// comportamento que não perde informação, e o guard de path roda depois
/// de qualquer jeito.
///
/// Feito na mão em vez de trazer `percent-encoding`: são vinte linhas e a
/// crate não entra em mais lugar nenhum do projeto.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolve uma rota para um arquivo real dentro de `dir`, ou `None`.
///
/// Depois do guard puro ainda canonicalizamos e reconferimos containment:
/// isso pega symlink/junction que o usuário tenha criado dentro da pasta
/// apontando pra fora dela.
pub fn resolve_static(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let rel = sanitize_rel_path(url_path)?;
    // `dunce` evita o prefixo `\\?\` que o `fs::canonicalize` do Windows
    // adiciona — sem ele o `starts_with` abaixo falharia contra uma base
    // não-canonicalizada. Já é dependência do projeto.
    let base = dunce::canonicalize(dir).ok()?;
    let full = dunce::canonicalize(base.join(rel)).ok()?;
    if !full.starts_with(&base) {
        return None;
    }
    let meta = fs::metadata(&full).ok()?;
    // Nunca listamos diretório.
    if !meta.is_file() || meta.len() > MAX_ASSET_BYTES {
        return None;
    }
    Some(full)
}

/// Content-type pela extensão. Fallback `application/octet-stream`.
pub fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "txt" => "text/plain; charset=utf-8",
        // `text/markdown` seria mais correto, mas o browser baixa em vez de
        // exibir. O único .md aqui é o README de customização, e o ponto dele
        // é ser lido — inclusive em `http://127.0.0.1:<porta>/README.md`.
        "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Fixture usada nos testes de render do template padrão.
#[cfg(test)]
pub fn fixture() -> super::data::OverlayData {
    use super::data::{
        Game, LiveGame, LivePlayer, OverlayData, OverlayPlayer, RaceRecord, SessionStats,
    };

    fn live(name: &str, race: &str, letter: &str) -> LivePlayer {
        LivePlayer {
            name: name.into(),
            race: race.into(),
            race_letter: letter.into(),
        }
    }

    fn race(race: &str, letter: &str, wins: usize, losses: usize) -> RaceRecord {
        let games = wins + losses;
        RaceRecord {
            race: race.into(),
            race_letter: letter.into(),
            games,
            wins,
            losses,
            winrate_pct: (games > 0)
                .then(|| ((wins as f32 / games as f32) * 100.0).round() as u32),
        }
    }

    let me = OverlayPlayer {
        name: "Kerrigan".into(),
        race: "Zerg".into(),
        race_letter: "Z".into(),
        mmr: Some(4210),
        result: "Win".into(),
    };
    let opp = OverlayPlayer {
        name: "Raynor".into(),
        race: "Terran".into(),
        race_letter: "T".into(),
        mmr: Some(4180),
        result: "Loss".into(),
    };
    let last = Game {
        map: "Site Delta".into(),
        datetime: "2026-07-31T20:02:11".into(),
        date: "2026-07-31".into(),
        time: "20:02".into(),
        is_today: true,
        matchup: "ZvT".into(),
        result: "Win".into(),
        duration_seconds: 754,
        duration_label: "12:34".into(),
        me: Some(me.clone()),
        opponent: Some(opp.clone()),
        players: vec![me, opp],
    };
    OverlayData {
        revision: 7,
        generated_at: "2026-07-31T20:15:00".into(),
        nicknames_configured: true,
        session: SessionStats {
            date: "2026-07-31".into(),
            games: 5,
            wins: 3,
            losses: 2,
            winrate: Some(0.6),
            winrate_pct: Some(60),
            mmr_latest: Some(4210),
            mmr_delta: Some(42),
            by_race: vec![
                race("Zerg", "Z", 1, 0),
                race("Terran", "T", 2, 1),
                race("Protoss", "P", 0, 1),
                race("Random", "R", 0, 0),
            ],
        },
        last_game: Some(last.clone()),
        recent_games: vec![last],
        // Partida em andamento, para os testes de render exercitarem o
        // caminho "em jogo" do template ao vivo.
        live: LiveGame {
            connected: true,
            in_game: true,
            players: vec![
                live("Kerrigan", "Zerg", "Z"),
                live("Raynor", "Terran", "T"),
            ],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_files_and_subdirectories() {
        for ok in ["/style.css", "/index.html", "/img/logo.png", "/a/b/c/d.woff2"] {
            assert!(sanitize_rel_path(ok).is_some(), "deveria aceitar {ok}");
        }
        assert_eq!(sanitize_rel_path("/img/logo.png").unwrap(), Path::new("img").join("logo.png"));
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let bad = [
            "/",
            "",
            "/../config.yaml",
            "/..%2fconfig.yaml",
            "/%2e%2e/%2e%2e/config.yaml",
            "/a/../../b",
            "/./x.png",
            "/C:/Windows/win.ini",
            "/%43%3a/Windows/win.ini",
            r"/\..\..\config.yaml",
            "//server/share/x",
        ];
        for b in bad {
            assert!(sanitize_rel_path(b).is_none(), "deveria rejeitar {b:?}");
        }
    }

    #[test]
    fn rejects_windows_specific_hazards() {
        let bad = [
            "/CON",
            "/nul.txt",
            "/COM1",
            "/lpt9.css",
            "/x.txt:stream",
            "/trailing.",
            "/trailing ",
            "/a%00b",
        ];
        for b in bad {
            assert!(sanitize_rel_path(b).is_none(), "deveria rejeitar {b:?}");
        }
    }

    #[test]
    fn percent_decode_handles_malformed_sequences() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("%2f"), "/");
        // Truncado e inválido ficam literais.
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%ZZ"), "%ZZ");
        assert_eq!(percent_decode("%"), "%");
        // Multibyte UTF-8 round-trip ("é").
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }

    #[test]
    fn content_type_is_case_insensitive_with_octet_stream_fallback() {
        assert_eq!(content_type(Path::new("a.png")), "image/png");
        assert_eq!(content_type(Path::new("a.PNG")), "image/png");
        assert_eq!(content_type(Path::new("a.woff2")), "font/woff2");
        assert_eq!(content_type(Path::new("a.css")), "text/css; charset=utf-8");
        // O README de customização precisa abrir no browser, não baixar.
        assert_eq!(content_type(Path::new("README.md")), "text/plain; charset=utf-8");
        assert_eq!(content_type(Path::new("noext")), "application/octet-stream");
        assert_eq!(content_type(Path::new("a.zzz")), "application/octet-stream");
    }

    #[test]
    fn template_routing() {
        assert!(is_template("/"));
        assert!(is_template("/alt.html"));
        assert!(is_template("/ALT.HTML"));
        assert!(!is_template("/style.css"));
        assert_eq!(template_name("/").unwrap(), "index.html");
        assert_eq!(template_name("/alt.html").unwrap(), "alt.html");
        // Sempre `/` como separador, mesmo no Windows.
        assert_eq!(template_name("/sub/alt.html").unwrap(), "sub/alt.html");
        assert!(template_name("/../secret.html").is_none());
    }
}
