//! Servidor HTTP do overlay: bind, pool de workers e tabela de rotas.
//!
//! **Só escuta em 127.0.0.1.** Isso resolve duas coisas de uma vez: o
//! overlay não fica exposto na LAN, e o Windows Firewall não mostra o
//! diálogo de "permitir este app" (ele só dispara em bind de `0.0.0.0`/`::`).
//!
//! **O pool de threads é obrigatório, não uma otimização.** O `tiny_http`
//! não spawna thread nenhuma — `Server::recv()` é sequencial. Como
//! `/_overlay/revision?wait=1` faz long-poll, uma única requisição parkada
//! travaria todo o resto, inclusive o carregamento da própria página.
//! [`OverlayState::wait_for_change`](super::shared::OverlayState::wait_for_change)
//! completa a defesa limitando quantas threads podem parkar ao mesmo tempo.

use std::io::Cursor;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tiny_http::{Header, Request, Response, Server, StatusCode};

use super::assets;
use super::live::LivePoller;
use super::render;
use super::shared::OverlayState;

/// Threads que atendem requisições. Ver a nota sobre long-poll no topo do
/// módulo — este número é o teto de conexões simultâneas, não um ajuste de
/// throughput.
pub(super) const WORKERS: usize = 4;

/// Quanto tempo um long-poll fica parkado antes de responder com a revisão
/// atual. Abaixo de todo idle timeout default da pilha (proxies, CEF,
/// `fetch`), e o cliente reconecta na hora.
const LONG_POLL_TIMEOUT: Duration = Duration::from_secs(25);

/// Prefixo reservado pelo app. Um arquivo do usuário em `overlay/_overlay/`
/// fica inacessível — documentado no cabeçalho do template padrão.
const RESERVED_PREFIX: &str = "/_overlay/";

/// Script de live reload, servido em `/_overlay/live.js`.
///
/// Aprende a revisão inicial de `window.__overlayRev`, que o template
/// escreve com o valor que o próprio render usou. Sem isso, um publish que
/// caísse entre o render do HTML e o primeiro fetch seria perdido até o
/// publish seguinte. Se o usuário apagar essa linha do template o script
/// ainda funciona — só perde a exatidão nessa borda.
const LIVE_JS: &str = r#"(function () {
  var rev = (typeof window.__overlayRev === 'number') ? window.__overlayRev : null;
  function poll() {
    var url = '/_overlay/revision' + (rev === null ? '' : '?since=' + rev + '&wait=1');
    fetch(url, { cache: 'no-store' })
      .then(function (r) { return r.text(); })
      .then(function (t) {
        var next = Number(t);
        if (!isFinite(next)) { setTimeout(poll, 2000); return; }
        if (rev === null) { rev = next; poll(); return; }
        if (next !== rev) { location.reload(); return; }
        poll();
      })
      .catch(function () { setTimeout(poll, 2000); });
  }
  poll();
})();
"#;

pub struct OverlayHandle {
    server: Arc<Server>,
    state: Arc<OverlayState>,
    workers: Vec<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    /// Poll do cliente do SC2. Amarrado ao servidor de propósito: ele existe
    /// só para alimentar o overlay, e um poll de rede rodando com o overlay
    /// desligado seria trabalho que ninguém lê. O `Drop` dele para a thread.
    _live: LivePoller,
    pub bound_port: u16,
    pub dir: PathBuf,
}

impl OverlayHandle {
    pub fn state(&self) -> &Arc<OverlayState> {
        &self.state
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.bound_port)
    }
}

impl Drop for OverlayHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // Ordem importa: primeiro soltamos quem está parkado num long-poll,
        // senão essas threads só sairiam no timeout de 25 s e o `join`
        // abaixo seguraria o fechamento do app.
        self.state.wake_all();
        // `unblock()` solta UM `recv()` por chamada — ele enfileira um único
        // token de controle ("If there are several such threads, only one is
        // unblocked", tiny_http 0.12). Com um pool, chamar uma vez só deixa
        // os outros workers presos para sempre e o `join` abaixo nunca
        // retorna. Os tokens ficam na fila até serem consumidos, então um
        // por worker cobre também quem estava ocupado num handler e só volta
        // ao `recv()` depois.
        for _ in 0..self.workers.len() {
            self.server.unblock();
        }
        for h in self.workers.drain(..) {
            let _ = h.join();
        }
    }
}

/// Sobe o servidor. O bind acontece na thread chamadora (a UI) de propósito:
/// assim `AddrInUse` vira `Err` **antes** de qualquer thread nascer, e a
/// mensagem chega na janela de configurações em vez de sumir num log.
///
/// Nunca dá panic.
pub fn start(port: u16, state: Arc<OverlayState>) -> Result<OverlayHandle, String> {
    let dir = assets::ensure_dir()?;
    let server = Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let bound_port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(port);
    let server = Arc::new(server);
    let shutdown = Arc::new(AtomicBool::new(false));

    let mut workers = Vec::with_capacity(WORKERS);
    for i in 0..WORKERS {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        let shutdown = Arc::clone(&shutdown);
        let dir = dir.clone();
        let handle = thread::Builder::new()
            .name(format!("overlay-http-{i}"))
            .spawn(move || {
                loop {
                    let Ok(req) = server.recv() else {
                        break; // unblock() ou socket fechado
                    };
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    // O minijinja devolve Result e não dá panic, mas um
                    // worker morrendo em silêncio se manifesta como "o
                    // overlay travou depois de três horas" e custa um dia
                    // pra achar. Seis linhas de seguro.
                    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
                        route(req, &state, &dir);
                    }));
                }
            })
            .map_err(|e| format!("spawn overlay worker: {e}"))?;
        workers.push(handle);
    }

    let _live = LivePoller::start(Arc::clone(&state));

    Ok(OverlayHandle {
        server,
        state,
        workers,
        shutdown,
        _live,
        bound_port,
        dir,
    })
}

// ── Roteamento ──────────────────────────────────────────────────────────

fn route(req: Request, state: &OverlayState, dir: &Path) {
    if !host_is_local(&req) {
        // Sem autenticação nenhuma, uma página no browser normal do usuário
        // poderia fazer DNS rebinding para esta porta e ler as estatísticas
        // dele. O OBS sempre manda o Host literal que você digitou.
        let _ = req.respond(text_response(403, "forbidden"));
        return;
    }

    let method = req.method().as_str().to_ascii_uppercase();
    if method != "GET" && method != "HEAD" {
        let _ = req.respond(text_response(405, "method not allowed"));
        return;
    }

    let url = req.url().to_string();
    let (path, query) = split_query(&url);

    if let Some(rest) = path.strip_prefix(RESERVED_PREFIX) {
        route_reserved(req, rest, query, state);
        return;
    }

    if assets::is_template(path) {
        let Some(name) = assets::template_name(path) else {
            let _ = req.respond(text_response(404, "not found"));
            return;
        };
        let (data, _) = state.snapshot();
        let html = render::render(dir, &name, &data);
        let _ = req.respond(html_response(html));
        return;
    }

    match assets::resolve_static(dir, path) {
        Some(file) => respond_file(req, &file),
        None => {
            let _ = req.respond(text_response(404, "not found"));
        }
    }
}

fn route_reserved(req: Request, rest: &str, query: &str, state: &OverlayState) {
    match rest {
        "live.js" => {
            let _ = req.respond(
                Response::from_string(LIVE_JS)
                    .with_header(header("Content-Type", "text/javascript; charset=utf-8"))
                    .with_header(header("Cache-Control", "no-store")),
            );
        }
        "revision" => {
            let since = query_param(query, "since").and_then(|v| v.parse::<u64>().ok());
            let wait = query_param(query, "wait").as_deref() == Some("1");
            let rev = match (since, wait) {
                (Some(s), true) => state.wait_for_change(s, LONG_POLL_TIMEOUT),
                _ => state.revision(),
            };
            let _ = req.respond(
                Response::from_string(rev.to_string())
                    .with_header(header("Content-Type", "text/plain; charset=utf-8"))
                    .with_header(header("Cache-Control", "no-store")),
            );
        }
        "data.json" => {
            // Nada consome esta rota — ela existe para o usuário descobrir
            // os nomes exatos das variáveis disponíveis ao template dele.
            let (data, _) = state.snapshot();
            let body = serde_json::to_string_pretty(&*data)
                .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
            let _ = req.respond(
                Response::from_string(body)
                    .with_header(header("Content-Type", "application/json; charset=utf-8"))
                    .with_header(header("Cache-Control", "no-store")),
            );
        }
        _ => {
            let _ = req.respond(text_response(404, "not found"));
        }
    }
}

fn respond_file(req: Request, file: &Path) {
    let Ok(handle) = std::fs::File::open(file) else {
        let _ = req.respond(text_response(404, "not found"));
        return;
    };
    // `from_file` streama e tira o Content-Length do metadata — um webm de
    // 5 MB não é slurpado pra RAM.
    let response = Response::from_file(handle)
        .with_header(header("Content-Type", assets::content_type(file)))
        // `no-cache` (revalida) e não `no-store`: o CEF do OBS cacheia
        // forte, e trocar um logo.png precisa aparecer no próximo reload.
        .with_header(header("Cache-Control", "no-cache"));
    let _ = req.respond(response);
}

// ── Helpers de resposta ─────────────────────────────────────────────────

fn html_response(body: String) -> Response<Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"))
}

fn text_response(status: u16, body: &str) -> Response<Cursor<Vec<u8>>> {
    Response::from_string(body.to_string())
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"))
}

/// `Header::from_bytes` só falha com nome/valor inválidos — os nossos são
/// literais, então o `expect` documenta uma invariante e não um risco.
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("header literal válido")
}

// ── Parsing de URL ──────────────────────────────────────────────────────

fn split_query(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// Aceita apenas Host de loopback. A porta é ignorada; `Host` ausente é
/// rejeitado (HTTP/1.1 exige o header, e todo cliente real manda).
fn host_is_local(req: &Request) -> bool {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("Host"))
        .map(|h| {
            let value = h.value.as_str();
            // IPv6 literal chega como `[::1]:8722`; cortar no último `:`
            // preserva os colons de dentro dos colchetes.
            let host = match value.rsplit_once(':') {
                Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
                _ => value,
            };
            host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host == "::1"
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    #[test]
    fn query_parsing() {
        assert_eq!(split_query("/a/b?x=1"), ("/a/b", "x=1"));
        assert_eq!(split_query("/a/b"), ("/a/b", ""));
        assert_eq!(query_param("since=4&wait=1", "since").as_deref(), Some("4"));
        assert_eq!(query_param("since=4&wait=1", "wait").as_deref(), Some("1"));
        assert_eq!(query_param("since=4", "wait"), None);
        assert_eq!(query_param("", "wait"), None);
    }

    /// Faz um GET cru e devolve `(status, body)`.
    ///
    /// Lê exatamente `Content-Length` bytes em vez de até EOF: o tiny_http
    /// mantém a conexão viva, então ler até EOF pendura o teste para sempre.
    /// Um timeout no socket é a segunda linha de defesa — sem ele, uma
    /// resposta que nunca chegue travaria a suíte inteira em vez de falhar.
    fn get(port: u16, path: &str, host: &str) -> (u16, String) {
        use std::io::Read;

        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut stream = stream;
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        stream.flush().unwrap();

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).unwrap();
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let mut length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().unwrap_or(0);
                }
            }
        }

        let mut body = vec![0u8; length];
        if length > 0 {
            reader.read_exact(&mut body).unwrap();
        }
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[test]
    fn serves_requests_and_shuts_down_cleanly() {
        // Porta 0 = efêmera, então este teste não conflita em CI nem com
        // um app rodando na máquina do dev.
        let state = OverlayState::new();
        let handle = start(0, Arc::clone(&state)).expect("bind na porta efêmera");
        let port = handle.bound_port;
        assert!(port > 0);

        let (status, body) = get(port, "/_overlay/revision", "127.0.0.1");
        assert_eq!(status, 200);
        assert!(body.trim().parse::<u64>().is_ok(), "corpo: {body:?}");

        let (status, body) = get(port, "/_overlay/live.js", "127.0.0.1");
        assert_eq!(status, 200);
        assert!(body.contains("location.reload()"));

        let (status, body) = get(port, "/_overlay/data.json", "localhost");
        assert_eq!(status, 200);
        assert!(body.contains("\"session\""));

        // Guard de rebinding.
        let (status, _) = get(port, "/", "evil.test");
        assert_eq!(status, 403);

        // Path traversal — inclusive percent-encoded.
        for path in ["/../config.yaml", "/..%2fconfig.yaml", "/%2e%2e/config.yaml"] {
            let (status, _) = get(port, path, "127.0.0.1");
            assert_eq!(status, 404, "deveria rejeitar {path}");
        }

        // `/` sempre renderiza, mesmo se o usuário não tiver template
        // (fallback embutido).
        let (status, body) = get(port, "/", "127.0.0.1");
        assert_eq!(status, 200);
        assert!(body.contains("__overlayRev"));

        // Asset estático: o `ensure_dir` do start escreveu o style.css padrão.
        let (status, body) = get(port, "/style.css", "127.0.0.1");
        assert_eq!(status, 200);
        assert!(body.contains("background: transparent"));

        // Deixa um long-poll parkado e então desmonta. Se o `Drop` não
        // soltasse os waiters antes do `join`, o fechamento do app ficaria
        // preso até o timeout de 25 s — este assert é o guarda disso.
        let poller = thread::spawn(move || get(port, "/_overlay/revision?since=0&wait=1", "127.0.0.1"));
        thread::sleep(Duration::from_millis(200));

        let t0 = std::time::Instant::now();
        drop(handle);
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "shutdown deveria soltar o long-poll, levou {:?}",
            t0.elapsed()
        );
        let _ = poller.join();
    }
}
