//! Quem está jogando **agora**, lido do cliente do SC2.
//!
//! O jogo expõe uma API HTTP local (não documentada pela Blizzard, estável
//! há anos) em `127.0.0.1:6119`:
//!
//! ```text
//! GET /ui    → {"activeScreens":["ScreenHome/ScreenHome", ...]}
//! GET /game  → {"isReplay":false,"displayTime":12.3,
//!               "players":[{"id":1,"name":"X","type":"user","race":"Terr", ...}]}
//! ```
//!
//! `activeScreens` vazio é o único sinal de que a tela de jogo está no ar —
//! em qualquer menu a lista traz a pilha de telas abertas. Por isso o poll
//! começa por `/ui`: fora de jogo ele nem chega a pedir `/game`.
//!
//! **Não há máquina de estado aqui.** A pergunta é só "quem está jogando
//! agora?", respondida do zero a cada poll — sem histórico, sem eventos de
//! início/fim, sem cooldown de transição. Tudo que depende de partida
//! *terminada* já vem do replay, que é a fonte confiável; um cliente que
//! some no meio de uma partida não pode deixar estatística pela metade.
//!
//! **Thread própria, publicando direto no `OverlayState`.** Não é só para a
//! UI não esperar por I/O: o egui não repinta uma janela minimizada, e
//! `update()` é quem dispara todo o resto do app. Um streamer com o app
//! minimizado — o caso normal — teria o overlay congelado se este poll
//! dependesse da UI thread.
//!
//! O cliente HTTP é feito à mão pelo mesmo motivo que o percent-decode em
//! `assets.rs`: são duas requisições GET para um endereço fixo de loopback,
//! e uma crate de HTTP client não entraria em mais lugar nenhum do projeto.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::utils::race_letter;

use super::data::{LiveGame, LivePlayer};
use super::shared::OverlayState;

/// Endereço da API do cliente. A porta é fixa no jogo — não há opção nem
/// linha de comando que a mude, então também não há o que configurar aqui.
pub const SC2_API: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6119);

/// Intervalo entre polls. Dois segundos é o mesmo valor que o projeto
/// anterior usava em produção: rápido o bastante para o overlay virar junto
/// com a partida e devagar o bastante para ser invisível no jogo.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Timeouts do socket. Curtos de propósito: com o SC2 fechado o connect
/// falha na hora (loopback recusa), mas uma porta 6119 ocupada por outro
/// programa que aceite a conexão e não responda não pode segurar a thread
/// além de um ciclo de poll.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_secs(1);

/// Teto de resposta lida. O JSON real tem algumas centenas de bytes.
const MAX_RESPONSE: usize = 64 * 1024;

// ── Thread ──────────────────────────────────────────────────────────────

/// Thread de polling. Vive enquanto o servidor do overlay viver.
pub(super) struct LivePoller {
    /// Fechar o canal (soltando o `Sender`) acorda o `recv_timeout` na hora;
    /// sem isso o shutdown esperaria o intervalo de poll inteiro.
    stop: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl LivePoller {
    pub(super) fn start(state: Arc<OverlayState>) -> Self {
        Self::start_at(SC2_API, state)
    }

    /// Idem, contra um endereço arbitrário — é o que permite testar a thread
    /// inteira contra um servidor de mentira, sem o SC2 aberto.
    pub(super) fn start_at(addr: SocketAddr, state: Arc<OverlayState>) -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        let handle = thread::Builder::new()
            .name("overlay-sc2-poll".into())
            .spawn(move || {
                // Começa do estado que o `OverlayState` já publica por
                // default (desconectado, fora de jogo). Assim o caso comum de
                // "SC2 fechado" não gasta uma revisão logo no start.
                let mut last = LiveGame::default();
                loop {
                    let next = poll_once(addr);
                    if next != last {
                        state.publish_live(next.clone());
                        last = next;
                    }
                    match rx.recv_timeout(POLL_INTERVAL) {
                        Err(RecvTimeoutError::Timeout) => continue,
                        // Sinal explícito ou canal fechado: hora de sair.
                        _ => break,
                    }
                }
            })
            .ok();
        Self {
            stop: Some(tx),
            handle,
        }
    }
}

impl Drop for LivePoller {
    fn drop(&mut self) {
        // Soltar o Sender fecha o canal e faz o `recv_timeout` devolver
        // `Disconnected` imediatamente.
        self.stop.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Um ciclo completo. **Nunca falha**: qualquer problema vira "desconectado",
/// que é exatamente o que o usuário precisa ver.
fn poll_once(addr: SocketAddr) -> LiveGame {
    let Ok(ui) = get_json::<UiResponse>(addr, "/ui") else {
        return LiveGame::default();
    };
    // Em menu não há o que perguntar — economiza metade das requisições no
    // estado em que o app passa a maior parte do tempo.
    if !ui.active_screens.is_empty() {
        return LiveGame {
            connected: true,
            ..LiveGame::default()
        };
    }
    let Ok(game) = get_json::<GameResponse>(addr, "/game") else {
        return LiveGame::default();
    };
    live_from(&game)
}

// ── Payloads da API ─────────────────────────────────────────────────────

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
struct UiResponse {
    #[serde(default)]
    active_screens: Vec<String>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
struct GameResponse {
    #[serde(default)]
    is_replay: bool,
    #[serde(default)]
    players: Vec<ApiPlayer>,
}

#[derive(Deserialize, Default, Debug)]
struct ApiPlayer {
    #[serde(default)]
    name: String,
    /// `"user"` ou `"computer"`. É o que separa ladder/custom de treino
    /// contra IA.
    #[serde(default, rename = "type")]
    kind: String,
    /// `"Terr"`, `"Prot"`, `"Zerg"` ou `"random"` (minúsculo mesmo).
    #[serde(default)]
    race: String,
}

/// Traduz a resposta de `/game` (já sabendo que a tela de jogo está no ar).
///
/// O recorte é o mesmo do resto do overlay, com uma ressalva: a API do
/// cliente **não diz se a partida é de ladder**, então "dois humanos, nenhuma
/// IA" é a aproximação mais próxima disponível — um custom 1v1 com um amigo
/// entra aqui. Vs IA, FFA, 2v2 e replay ficam de fora.
fn live_from(game: &GameResponse) -> LiveGame {
    let humans: Vec<&ApiPlayer> = game.players.iter().filter(|p| p.kind == "user").collect();
    let has_ai = game.players.iter().any(|p| p.kind == "computer");
    if game.is_replay || has_ai || humans.len() != 2 {
        return LiveGame {
            connected: true,
            ..LiveGame::default()
        };
    }
    LiveGame {
        connected: true,
        in_game: true,
        players: humans
            .into_iter()
            .map(|p| {
                let race = normalize_race(&p.race);
                LivePlayer {
                    name: p.name.clone(),
                    race: race.to_string(),
                    race_letter: race_letter(race).to_string(),
                }
            })
            .collect(),
    }
}

/// `"Terr"` → `"Terran"`. Casa com `OverlayPlayer.race` e com os nomes dos
/// SVGs em `race/`, para o template não precisar de dois vocabulários.
///
/// Vai pela inicial, igual a `utils::race_letter`: o cliente já usou formas
/// diferentes ao longo dos patches (`"Terr"`, `"Terran"`), e o que não
/// reconhecemos cai em Random — que é o que o cliente devolve enquanto a
/// raça de um jogador random ainda não foi revelada.
fn normalize_race(raw: &str) -> &'static str {
    match raw.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('t') => "Terran",
        Some('p') => "Protoss",
        Some('z') => "Zerg",
        _ => "Random",
    }
}

// ── Cliente HTTP ────────────────────────────────────────────────────────

fn get_json<T: DeserializeOwned>(addr: SocketAddr, path: &str) -> Result<T, String> {
    let body = get(addr, path)?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

fn get(addr: SocketAddr, path: &str) -> Result<Vec<u8>, String> {
    let mut stream =
        TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let _ = stream.flush();

    let mut raw = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break, // EOF
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            // Timeout de leitura: o que já chegou pode bastar (ver abaixo).
            Err(_) => break,
        }
        if raw.len() > MAX_RESPONSE {
            return Err("resposta grande demais".into());
        }
        // Sair assim que o corpo estiver completo importa: se o cliente
        // ignorar o `Connection: close` e mantiver a conexão viva, esperar o
        // EOF custaria o timeout de leitura inteiro em *todo* poll.
        if let Some(done) = parse_response(&raw, false) {
            return done;
        }
    }
    parse_response(&raw, true).unwrap_or_else(|| Err("resposta HTTP incompleta".into()))
}

/// Extrai o corpo de uma resposta HTTP.
///
/// **Função pura**, para ser testável sem o SC2 aberto — é o único pedaço
/// deste módulo que não dá para exercitar contra o jogo de verdade.
///
/// `None` = ainda faltam bytes. `eof` sinaliza que a conexão fechou, o que
/// torna "o resto é o corpo" uma leitura válida (é o que acontece quando a
/// resposta não traz `Content-Length` nem `chunked`).
fn parse_response(raw: &[u8], eof: bool) -> Option<Result<Vec<u8>, String>> {
    let split = find(raw, b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let body = &raw[split + 4..];

    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok());
    if status != Some(200) {
        return Some(Err(format!(
            "HTTP {}",
            status.map_or_else(|| "?".to_string(), |s| s.to_string())
        )));
    }

    if let Some(len) = header(head, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        if body.len() < len {
            return None;
        }
        return Some(Ok(body[..len].to_vec()));
    }
    if header(head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        return dechunk(body).map(Ok);
    }
    eof.then(|| Ok(body.to_vec()))
}

/// Valor de um header, case-insensitive no nome. A primeira linha (status)
/// é pulada.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Junta os chunks de um corpo `Transfer-Encoding: chunked`. `None` enquanto
/// o chunk final (tamanho zero) não chegou.
fn dechunk(body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let line_end = find(rest, b"\r\n")?;
        let size = std::str::from_utf8(&rest[..line_end])
            .ok()?
            // Extensões de chunk (`1a;algo=x`) são legais e ignoráveis.
            .split(';')
            .next()?
            .trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        let start = line_end + 2;
        let end = start.checked_add(size)?;
        // +2 pelo CRLF que fecha o chunk.
        if rest.len() < end + 2 {
            return None;
        }
        out.extend_from_slice(&rest[start..end]);
        rest = &rest[end + 2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    fn game(json: &str) -> GameResponse {
        serde_json::from_str(json).expect("payload de exemplo deve parsear")
    }

    /// Payload de um 1v1 real, no formato que o cliente devolve.
    const LADDER_1V1: &str = r#"{
        "isReplay": false,
        "displayTime": 137.5,
        "players": [
            {"id":1,"name":"Kerrigan","type":"user","race":"Zerg","result":"Undecided"},
            {"id":2,"name":"Raynor","type":"user","race":"Terr","result":"Undecided"}
        ]
    }"#;

    #[test]
    fn a_1v1_between_humans_reports_both_players() {
        let live = live_from(&game(LADDER_1V1));
        assert!(live.connected && live.in_game);
        assert_eq!(live.players.len(), 2);
        assert_eq!(live.players[0].name, "Kerrigan");
        assert_eq!(live.players[0].race, "Zerg");
        assert_eq!(live.players[0].race_letter, "Z");
        // A abreviação do cliente vira o nome por extenso que o resto do
        // overlay usa — é o que faz `/race/{{ race | lower }}.svg` funcionar.
        assert_eq!(live.players[1].race, "Terran");
        assert_eq!(live.players[1].race_letter, "T");
    }

    #[test]
    fn games_outside_the_1v1_cut_report_connected_but_not_in_game() {
        let cases = [
            // Contra a IA.
            r#"{"players":[{"name":"Me","type":"user","race":"Zerg"},
                           {"name":"CPU","type":"computer","race":"Terr"}]}"#,
            // FFA / 2v2.
            r#"{"players":[{"name":"A","type":"user","race":"Zerg"},
                           {"name":"B","type":"user","race":"Terr"},
                           {"name":"C","type":"user","race":"Prot"}]}"#,
            // Assistindo replay: a tela de jogo está no ar, mas ninguém está
            // jogando.
            r#"{"isReplay":true,"players":[{"name":"A","type":"user","race":"Zerg"},
                                           {"name":"B","type":"user","race":"Terr"}]}"#,
            // Sem jogadores (transição de carregamento).
            r#"{"players":[]}"#,
        ];
        for json in cases {
            let live = live_from(&game(json));
            assert!(live.connected, "{json}");
            assert!(!live.in_game, "não deveria contar: {json}");
            assert!(live.players.is_empty(), "{json}");
        }
    }

    #[test]
    fn an_unrevealed_random_player_falls_back_to_random() {
        let live = live_from(&game(
            r#"{"players":[{"name":"A","type":"user","race":"random"},
                           {"name":"B","type":"user","race":""}]}"#,
        ));
        assert_eq!(live.players[0].race, "Random");
        assert_eq!(live.players[1].race, "Random");
        assert_eq!(live.players[1].race_letter, "R");
    }

    #[test]
    fn unknown_fields_and_missing_fields_do_not_break_the_parse() {
        // A API muda entre patches; um campo novo não pode derrubar o poll,
        // e um ausente não pode virar erro.
        let live = live_from(&game(
            r#"{"isReplay":false,"someNewField":42,
                "players":[{"id":1,"name":"A","type":"user","race":"Zerg","result":"Undecided","apm":210},
                           {"id":2,"name":"B","type":"user","race":"Prot"}]}"#,
        ));
        assert!(live.in_game);
        assert_eq!(live.players[1].race, "Protoss");
    }

    #[test]
    fn ui_response_parses_both_shapes() {
        let menu: UiResponse =
            serde_json::from_str(r#"{"activeScreens":["ScreenHome/ScreenHome"]}"#).unwrap();
        assert!(!menu.active_screens.is_empty());
        let in_game: UiResponse = serde_json::from_str(r#"{"activeScreens":[]}"#).unwrap();
        assert!(in_game.active_screens.is_empty());
    }

    // ── Cliente HTTP ────────────────────────────────────────────────────

    #[test]
    fn parses_a_content_length_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_response(raw, false).unwrap().unwrap(), b"{}");
    }

    #[test]
    fn waits_for_the_rest_of_the_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n{}";
        assert!(parse_response(raw, false).is_none());
        // Nem o EOF inventa os bytes que faltam.
        assert!(parse_response(b"HTTP/1.1 200 OK\r\n", true).is_none());
    }

    #[test]
    fn parses_a_chunked_response() {
        // `{"a"` = 4 bytes, `:1}` = 3 — os tamanhos vão em hexadecimal.
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw, false).unwrap().unwrap(), br#"{"a":1}"#);
        // Sem o chunk final ainda falta dado.
        let partial = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n";
        assert!(parse_response(partial, false).is_none());
        // Um tamanho declarado maior que o que chegou também é "falta dado",
        // e não um corpo cortado no meio.
        let short = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n{\"a\"\r\n";
        assert!(parse_response(short, false).is_none());
    }

    #[test]
    fn a_body_without_length_needs_the_eof_to_be_complete() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true}";
        assert!(parse_response(raw, false).is_none());
        assert_eq!(parse_response(raw, true).unwrap().unwrap(), br#"{"ok":true}"#);
    }

    #[test]
    fn a_non_200_is_an_error_and_not_an_empty_body() {
        let raw = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        assert!(parse_response(raw, false).unwrap().is_err());
    }

    #[test]
    fn header_lookup_is_case_insensitive_and_skips_the_status_line() {
        let head = "HTTP/1.1 200 OK\r\nCONTENT-LENGTH: 7\r\nX-Other: 1";
        assert_eq!(header(head, "content-length"), Some("7"));
        assert_eq!(header(head, "missing"), None);
        // "HTTP/1.1 200 OK" tem `:`? Não — mas a linha de status precisa ser
        // pulada de qualquer forma para um header chamado "HTTP/1.1" não
        // existir.
        assert_eq!(header(head, "HTTP/1.1"), None);
    }

    // ── Thread ponta a ponta ────────────────────────────────────────────

    /// Servidor de mentira no lugar do SC2: `/ui` diz que a tela de jogo
    /// está no ar e `/game` devolve um 1v1.
    ///
    /// Exercita o caminho inteiro — thread, socket, parse HTTP, JSON e
    /// publish — que é a única forma de testar isto sem o jogo aberto.
    #[test]
    fn the_poller_publishes_what_the_client_reports() {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let addr = server.server_addr().to_ip().unwrap();
        let stop = Arc::new(AtomicBool::new(false));

        let srv = Arc::clone(&server);
        let flag = Arc::clone(&stop);
        let fake = thread::spawn(move || {
            while let Ok(req) = srv.recv() {
                if flag.load(Ordering::Acquire) {
                    break;
                }
                let body = match req.url() {
                    "/ui" => r#"{"activeScreens":[]}"#.to_string(),
                    _ => LADDER_1V1.to_string(),
                };
                let _ = req.respond(tiny_http::Response::from_string(body));
            }
        });

        let state = OverlayState::new();
        let poller = LivePoller::start_at(addr.into(), Arc::clone(&state));

        // O primeiro poll sai na hora; a margem cobre uma máquina de CI lenta.
        let deadline = Instant::now() + Duration::from_secs(5);
        let live = loop {
            let (data, _) = state.snapshot();
            if data.live.in_game || Instant::now() > deadline {
                break data.live.clone();
            }
            thread::sleep(Duration::from_millis(25));
        };
        assert!(live.connected, "deveria ter falado com o servidor");
        assert!(live.in_game, "deveria estar em jogo: {live:?}");
        assert_eq!(live.players.len(), 2);
        assert_eq!(live.players[1].race, "Terran");

        // O shutdown não pode esperar o intervalo de poll.
        let t0 = Instant::now();
        drop(poller);
        assert!(
            t0.elapsed() < POLL_INTERVAL,
            "shutdown deveria ser imediato, levou {:?}",
            t0.elapsed()
        );

        stop.store(true, Ordering::Release);
        server.unblock();
        let _ = fake.join();
    }

    #[test]
    fn a_client_that_is_not_there_reports_disconnected() {
        // Porta efêmera fechada: é o estado "SC2 fechado", que precisa ser
        // silencioso e barato.
        let addr: SocketAddr = ([127, 0, 0, 1], 1).into();
        let t0 = Instant::now();
        let live = poll_once(addr);
        assert!(!live.connected);
        assert!(!live.in_game);
        assert!(
            t0.elapsed() < CONNECT_TIMEOUT + Duration::from_secs(1),
            "connect recusado deveria voltar rápido, levou {:?}",
            t0.elapsed()
        );
    }
}
