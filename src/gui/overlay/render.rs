//! Render do template Jinja2 do usuário.
//!
//! Duas decisões que parecem erradas e não são:
//!
//! **1. Um `Environment` novo a cada request.** O `path_loader` memoiza os
//! templates dentro do `Environment`, então um de vida longa nunca veria as
//! edições do usuário — ele teria que reiniciar o app pra ver uma mudança
//! de CSS. O custo do rebuild é alguns mapas vazios, um `read_to_string` de
//! ~4 KB e o parse de um template pequeno: bem abaixo de 1 ms, e só
//! acontece em reload de página (≈ uma por partida). Um cache por mtime
//! aqui seria estritamente pior que a versão ingênua.
//!
//! **2. A página de erro sai com HTTP 200.** Ela *é* o conteúdo do overlay:
//! servi-la com 2xx é o que faz o usuário ler a mensagem no preview da
//! stream, em vez de a fonte Browser do OBS mostrar uma tela de erro
//! genérica do CEF. Um erro de sintaxe não pode ser um retângulo em branco
//! sem explicação.

use std::error::Error;
use std::path::Path;

use minijinja::{Environment, Value};

use super::assets;
use super::data::OverlayData;

/// Renderiza `name` a partir de `dir`. **Nunca falha** — erro de template
/// vira uma página de erro legível.
pub fn render(dir: &Path, name: &str, data: &OverlayData) -> String {
    let env = environment(dir);
    let ctx = Value::from_serialize(data);
    match env.get_template(name).and_then(|t| t.render(&ctx)) {
        Ok(html) => html,
        Err(e) => error_page(name, &e),
    }
}

fn environment(dir: &Path) -> Environment<'static> {
    let mut env = Environment::new();
    // Disco primeiro, cópia embutida depois — para *qualquer* nome, não só
    // o `index.html`.
    //
    // As páginas são montadas a partir de `base.html` e de meia dúzia de
    // peças em `partials/`; apagar uma delas por engano não pode virar
    // overlay em branco no meio da transmissão. O fallback vale só para
    // arquivo **ausente**: um erro de sintaxe continua chegando à página de
    // erro, senão o usuário veria o padrão funcionando e nunca saberia que
    // suas edições foram ignoradas.
    let disk = minijinja::path_loader(dir);
    env.set_loader(move |name| match disk(name)? {
        Some(source) => Ok(Some(source)),
        None => Ok(assets::embedded(name).map(str::to_string)),
    });
    // Explícito de propósito: nome de jogador vem de arquivo de replay e
    // pode conter `<`. O callback default escolhe por extensão, que é
    // exatamente o que queremos (.html escapa, .txt não).
    env.set_auto_escape_callback(minijinja::default_auto_escape_callback);
    env
}

/// Página de erro: bloco vermelho com arquivo, linha, tipo, cadeia de
/// causas e o trecho ofensor da fonte (é para isso que a feature `debug`
/// do minijinja está ligada).
///
/// O auto-reload de 2 s existe porque editar um template **não** bumpa a
/// revisão do `OverlayState` — o `live.js` sozinho nunca perceberia que o
/// usuário consertou o arquivo. Com ele, salvar o arquivo faz o overlay
/// voltar sozinho.
fn error_page(name: &str, err: &minijinja::Error) -> String {
    let mut detail = format!("{} ({:?})", err, err.kind());
    let mut source: Option<&dyn Error> = err.source();
    while let Some(e) = source {
        detail.push_str(&format!("\n  caused by: {e}"));
        source = e.source();
    }

    let line = err.line();
    let snippet = err
        .template_source()
        .zip(line)
        .map(|(src, line)| numbered_context(src, line))
        .unwrap_or_default();

    let location = match line {
        Some(l) => format!("{name}:{l}"),
        None => name.to_string(),
    };

    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\">\
<title>Overlay template error</title>\
<style>\
body{{margin:0;background:#1a0c0e;color:#ffd9dc;\
font:14px/1.5 'Cascadia Code','JetBrains Mono',Consolas,monospace}}\
.box{{margin:16px;padding:16px;border-left:4px solid #e0616b;\
background:rgba(224,97,107,.10);border-radius:6px}}\
h1{{margin:0 0 8px;font-size:15px;color:#ff8b93;letter-spacing:.04em}}\
.loc{{color:#ffb3b8;margin-bottom:10px}}\
pre{{margin:0;white-space:pre-wrap;word-break:break-word}}\
.src{{margin-top:12px;padding-top:12px;border-top:1px solid rgba(255,255,255,.10);color:#e8c9cc}}\
.foot{{margin-top:12px;color:#a77c80;font-size:12px}}\
</style></head><body><div class=\"box\">\
<h1>OVERLAY TEMPLATE ERROR</h1>\
<div class=\"loc\">{location}</div>\
<pre>{detail}</pre>{snippet}\
<div class=\"foot\">Fix the file and save — this page reloads by itself.</div>\
</div>\
<script>setTimeout(function(){{location.reload()}},2000)</script>\
</body></html>",
        location = escape(&location),
        detail = escape(&detail),
        snippet = snippet,
    )
}

/// Três linhas de contexto ao redor de `line`, numeradas, com a ofensora
/// destacada.
fn numbered_context(source: &str, line: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let first = line.saturating_sub(2).max(1);
    let last = (line + 1).min(lines.len());
    let mut out = String::from("<pre class=\"src\">");
    for n in first..=last {
        let Some(text) = lines.get(n - 1) else { continue };
        let marker = if n == line { "&gt;" } else { " " };
        out.push_str(&format!("{marker} {n:>4} | {}\n", escape(text)));
    }
    out.push_str("</pre>");
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Escreve **todos** os arquivos padrão num tempdir e devolve o caminho.
    ///
    /// Percorre `assets::DEFAULTS` em vez de listar os arquivos à mão: com a
    /// página quebrada em peças, uma lista manual esqueceria a próxima peça
    /// nova e o teste passaria pelo fallback embutido sem exercitar o que
    /// está no disco.
    fn default_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sc2ru-overlay-render-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, contents) in assets::DEFAULTS {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, contents).unwrap();
        }
        dir
    }

    #[test]
    fn default_template_renders_populated_data() {
        let dir = default_dir("populated");
        let html = render(&dir, "index.html", &assets::fixture());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("Site Delta"));
        assert!(html.contains("ZvT"));
        assert!(html.contains("12:34"));
        assert!(html.contains("Raynor"));
        assert!(html.contains("60%"));
        assert!(html.contains("+42"));
        // A revisão precisa chegar no JS — é o que fecha a corrida entre o
        // render do HTML e o primeiro fetch do live.js.
        assert!(html.contains("window.__overlayRev = 7"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_template_renders_empty_state() {
        // Biblioteca vazia / sem nickname: o primeiro estado que todo
        // usuário novo vê. Não pode explodir nem mostrar zeros sem contexto.
        let dir = default_dir("empty");
        let html = render(&dir, "index.html", &OverlayData::default());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("Waiting for your first game"));
        assert!(html.contains("Add your nickname"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn player_names_are_html_escaped() {
        let dir = default_dir("escape");
        let mut data = assets::fixture();
        if let Some(g) = data.last_game.as_mut() {
            if let Some(o) = g.opponent.as_mut() {
                o.name = "<script>x</script>".into();
            }
        }
        let html = render(&dir, "index.html", &data);
        assert!(!html.contains("<script>x</script>"));
        assert!(html.contains("&lt;script&gt;"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_index_falls_back_to_the_builtin() {
        let dir = default_dir("missing");
        fs::remove_file(dir.join("index.html")).unwrap();
        let html = render(&dir, "index.html", &assets::fixture());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("Site Delta"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn syntax_error_renders_a_readable_error_page() {
        let dir = default_dir("broken");
        fs::write(
            dir.join("broken.html"),
            "<p>ok</p>\n<p>{{ session.wins </p>\n<p>tail</p>\n",
        )
        .unwrap();
        let html = render(&dir, "broken.html", &assets::fixture());
        assert!(html.contains("OVERLAY TEMPLATE ERROR"));
        // Localização (arquivo:linha) e o trecho ofensor.
        assert!(html.contains("broken.html:2"), "{html}");
        assert!(html.contains("session.wins"));
        // Auto-recuperação: editar o template não bumpa a revisão, então
        // sem este reload o overlay ficaria preso na página de erro.
        assert!(html.contains("location.reload()"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_template_does_not_poison_the_next_request() {
        // Consequência direta do Environment por request.
        let dir = default_dir("recover");
        let path = dir.join("index.html");
        fs::write(&path, "{{ session.wins ").unwrap();
        assert!(render(&dir, "index.html", &assets::fixture()).contains("OVERLAY TEMPLATE ERROR"));
        fs::write(&path, "wins={{ session.wins }}").unwrap();
        assert_eq!(render(&dir, "index.html", &assets::fixture()), "wins=3");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dashboard_template_renders_populated_data() {
        let dir = default_dir("dash-populated");
        let html = render(&dir, "stats-dashboard.html", &assets::fixture());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("60%"), "donut de winrate");
        assert!(html.contains("Site Delta"));
        assert!(html.contains("Raynor"));
        assert!(html.contains("/race/terran.svg"), "ícone da raça do oponente");
        // A grade de raças é sempre completa, mesmo com raças sem jogos.
        for race in ["Zerg", "Terran", "Protoss", "Random"] {
            assert!(html.contains(race), "falta a raça {race}");
        }
        // Faixa de forma: 1 jogo real + 9 slots vazios.
        assert_eq!(html.matches("result-block empty").count(), 9, "{html}");
        assert_eq!(html.matches("result-block win").count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dashboard_template_renders_empty_state() {
        let dir = default_dir("dash-empty");
        let html = render(&dir, "stats-dashboard.html", &OverlayData::default());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("No games recorded yet"));
        assert!(html.contains("Add your nickname"));
        // Sem `by_race` no default, a grade some — mas a faixa continua
        // com os 10 slots, que é o que segura a largura do painel.
        assert_eq!(html.matches("result-block empty").count(), 10);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_template_renders_the_players_on_screen() {
        let dir = default_dir("live-in-game");
        let html = render(&dir, "live-players.html", &assets::fixture());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("Kerrigan"));
        assert!(html.contains("Raynor"));
        assert!(html.contains("/race/zerg.svg"));
        assert!(html.contains("/race/terran.svg"));
        // Um "VS" só, entre os dois — `loop.last` segurando o separador.
        assert_eq!(html.matches(r#"class="vs""#).count(), 1);
        assert!(!html.contains("Not in a game"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_template_separates_out_of_game_from_sc2_closed() {
        let dir = default_dir("live-idle");
        // SC2 aberto, mas em menu.
        let mut data = OverlayData::default();
        data.live.connected = true;
        let html = render(&dir, "live-players.html", &data);
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("Not in a game"), "{html}");

        // SC2 fechado: o default, que é também o estado do primeiro frame.
        let html = render(&dir, "live-players.html", &OverlayData::default());
        assert!(html.contains("StarCraft II is not running"), "{html}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_written_by_the_user_gets_skeleton_pieces_and_reload_for_free() {
        // É a promessa da estrutura: cinco linhas viram um overlay completo.
        // Se este teste quebrar, o README está mentindo.
        let dir = default_dir("assembly");
        fs::write(
            dir.join("mine.html"),
            r#"{% extends "base.html" %}
{% block title %}Mine{% endblock %}
{% block content %}
{% include "partials/session-inline.html" %}
{% include "partials/live-versus.html" %}
{% endblock %}"#,
        )
        .unwrap();
        let html = render(&dir, "mine.html", &assets::fixture());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("<title>Mine</title>"));
        // Estilos das peças, sem o autor ter linkado nada.
        assert!(html.contains("/tokens.css") && html.contains("/components.css"));
        // Live reload embutido no esqueleto — o erro clássico de esquecer as
        // duas linhas deixou de ser possível.
        assert!(html.contains("window.__overlayRev = 7"));
        assert!(html.contains("/_overlay/live.js"));
        // E o conteúdo das duas peças.
        assert!(html.contains("TODAY"));
        assert!(html.contains("Kerrigan"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_deleted_piece_falls_back_to_the_embedded_copy() {
        // Uma página montada tem muitos arquivos; perder um por engano não
        // pode virar tela em branco no meio da transmissão.
        let dir = default_dir("missing-piece");
        fs::remove_file(dir.join("partials").join("session-inline.html")).unwrap();
        fs::remove_file(dir.join("base.html")).unwrap();
        fs::remove_file(dir.join("macros.html")).unwrap();
        let html = render(&dir, "index.html", &assets::fixture());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("TODAY"), "a peça embutida entrou no lugar");
        assert!(html.contains("window.__overlayRev = 7"), "o esqueleto também");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_piece_renders_on_its_own_as_a_page() {
        // Cada peça é um template válido sozinha, então dá para apontar uma
        // fonte do OBS direto para /partials/<nome>.html.
        let dir = default_dir("piece-alone");
        let html = render(&dir, "partials/race-grid.html", &assets::fixture());
        assert!(!html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("/race/terran.svg"));
        assert!(!html.contains("<html"), "peça não traz esqueleto");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_piece_reports_the_error_with_the_offending_file() {
        // O fallback embutido vale só para arquivo ausente. Um erro de
        // sintaxe numa peça precisa apontar a peça — e não a página que a
        // incluiu, nem o padrão servido em silêncio.
        let dir = default_dir("broken-piece");
        fs::write(
            dir.join("partials").join("session-inline.html"),
            "<p>{{ session.wins </p>",
        )
        .unwrap();
        let html = render(&dir, "index.html", &assets::fixture());
        assert!(html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        assert!(html.contains("session-inline.html"), "{html}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_index_reports_the_error_instead_of_silently_serving_the_builtin() {
        // O fallback embutido existe para arquivo *ausente*. Se ele também
        // cobrisse erro de sintaxe, o usuário veria o overlay padrão
        // funcionando e nunca saberia que suas edições foram ignoradas.
        let dir = default_dir("broken-index");
        fs::write(dir.join("index.html"), "{{ session.wins ").unwrap();
        let html = render(&dir, "index.html", &assets::fixture());
        assert!(html.contains("OVERLAY TEMPLATE ERROR"), "{html}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_non_index_template_reports_the_error() {
        // O fallback embutido vale só pra `/`; um `/alt.html` inexistente
        // tem que dizer que não existe, não servir o index no lugar.
        let dir = default_dir("notfound");
        let html = render(&dir, "alt.html", &assets::fixture());
        assert!(html.contains("OVERLAY TEMPLATE ERROR"));
        let _ = fs::remove_dir_all(&dir);
    }
}
