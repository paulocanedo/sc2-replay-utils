//! Payload do overlay — o contexto que o template Jinja2 do usuário vê.
//!
//! Os tipos da biblioteca (`ParsedMeta`, `PlayerMeta`, `LibraryStats`) não
//! derivam `Serialize`, então usamos structs-espelho aqui. É o mesmo padrão
//! que `gui/cache.rs` já aplica para o cache bincode: os tipos de domínio
//! ficam livres de atributos de serialização e cada consumidor define a
//! forma que precisa.
//!
//! **Tudo já vem pré-formatado.** `winrate_pct`, `duration_label`,
//! `race_letter` e `time` são computados em Rust de propósito — o conjunto
//! de filtros do minijinja é pequeno e o template é editado pelo usuário
//! final, que não deveria precisar escrever
//! `{{ (session.winrate * 100) | round | int }}`.
//!
//! A projeção que popula estes tipos vive em
//! `crate::library::overlay_snapshot` (precisa de helpers `pub(super)` de
//! dentro daquele módulo).

use serde::Serialize;

/// Raiz do contexto do template. Os nomes dos campos são API pública para
/// os templates dos usuários — renomear quebra overlays existentes.
#[derive(Serialize, Clone, Debug, Default)]
pub struct OverlayData {
    /// Revisão do `OverlayState` no instante do publish. O template padrão
    /// a emite como `window.__overlayRev` para que o `live.js` saiba com
    /// qual versão a página foi renderizada — sem isso, um publish que caia
    /// entre o render do HTML e o primeiro fetch do script seria perdido.
    pub revision: u64,
    /// Timestamp local do publish, `YYYY-MM-DDTHH:MM:SS`.
    pub generated_at: String,
    /// `false` quando o usuário ainda não cadastrou nickname nenhum. Nesse
    /// estado não dá para saber qual jogador é "ele", então `session` fica
    /// zerada e `last_game.me`/`opponent` ficam `None`. O template padrão
    /// ramifica nisso para mostrar uma dica em vez de zeros silenciosos.
    pub nicknames_configured: bool,
    pub session: SessionStats,
    /// Replay parseado mais recente da biblioteca — **não** necessariamente
    /// de hoje. Um streamer quer ver a última partida antes do primeiro
    /// jogo da sessão; `is_today` deixa o template distinguir os casos.
    ///
    /// É exatamente `recent_games[0]`, mantido como campo próprio porque é
    /// o caso de uso mais comum e evita `recent_games | first` no template.
    pub last_game: Option<Game>,
    /// Histórico recente, mais novo primeiro, no máximo
    /// [`RECENT_GAMES`](crate::library::overlay_snapshot::RECENT_GAMES).
    ///
    /// Escopo é a biblioteca inteira, **não** só hoje — é "forma recente",
    /// e uma faixa de resultados vazia toda manhã seria inútil. Cada
    /// entrada carrega `is_today` para o template distinguir.
    pub recent_games: Vec<Game>,
    /// Quem está jogando **agora**, lido do cliente do SC2 (ver
    /// [`crate::overlay::live`]). É o único campo que não vem da biblioteca:
    /// os outros descrevem partidas que já terminaram e viraram replay.
    pub live: LiveGame,
}

/// Recorte do cliente do SC2 no instante do último poll.
///
/// Deliberadamente raso: nome e raça de quem está na tela, mais dois
/// booleanos para o template escolher a mensagem. Sem placar, sem tempo de
/// jogo, sem estado de partida — o que precisa disso vem do replay depois.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct LiveGame {
    /// `true` quando o último poll falou com o cliente. `false` = SC2
    /// fechado, ainda carregando, ou API indisponível.
    pub connected: bool,
    /// `true` quando há um **1v1 entre dois humanos** na tela de jogo.
    ///
    /// Vale para custom 1v1 também: a API do cliente não diz se a partida é
    /// de ladder. Fora isso o recorte é o mesmo do resto do overlay — vs IA,
    /// FFA, 2v2 e replay não contam, e o template mostra "fora de jogo".
    pub in_game: bool,
    /// Os dois jogadores, na ordem em que o cliente os devolve. Vazio quando
    /// `in_game` é `false`.
    pub players: Vec<LivePlayer>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LivePlayer {
    pub name: String,
    /// Raça por extenso (`"Terran"`), normalizada a partir das abreviações
    /// do cliente (`"Terr"`, `"Prot"`, `"Zerg"`, `"random"`) para casar com
    /// `OverlayPlayer.race` e com os nomes dos SVGs em `race/`.
    pub race: String,
    /// Inicial da raça (`"T"`, `"P"`, `"Z"`, `"R"`).
    pub race_letter: String,
}

/// Placar da sessão de hoje.
#[derive(Serialize, Clone, Debug, Default)]
pub struct SessionStats {
    /// Dia coberto por estes números, `YYYY-MM-DD`.
    pub date: String,
    /// Partidas decididas hoje (`wins + losses`). Replays sem resultado
    /// atribuível ao usuário não entram.
    pub games: usize,
    pub wins: usize,
    pub losses: usize,
    /// Fração `0.0..=1.0`. `None` sem partidas decididas hoje.
    pub winrate: Option<f32>,
    /// Mesma coisa arredondada para `0..=100`, para o template não fazer conta.
    pub winrate_pct: Option<u32>,
    /// MMR do usuário na partida mais recente de hoje.
    pub mmr_latest: Option<i32>,
    /// Ganho/perda de MMR no dia: último MMR de hoje menos o primeiro.
    /// `None` com menos de dois pontos de MMR hoje.
    pub mmr_delta: Option<i32>,
    /// Recorde de hoje quebrado pela raça do **oponente**. Sempre as
    /// quatro entradas, em ordem fixa (Zerg, Terran, Protoss, Random),
    /// mesmo zeradas — assim o template pode desenhar a grade completa sem
    /// se preocupar com ausências.
    pub by_race: Vec<RaceRecord>,
}

/// Recorde contra uma raça específica dentro da sessão.
#[derive(Serialize, Clone, Debug)]
pub struct RaceRecord {
    /// Raça por extenso, em inglês (`"Zerg"`) — casa com `OverlayPlayer.race`.
    pub race: String,
    /// Inicial (`"Z"`), útil para ícones e classes CSS.
    pub race_letter: String,
    /// Partidas decididas contra esta raça hoje.
    pub games: usize,
    pub wins: usize,
    pub losses: usize,
    /// `0..=100`. `None` sem partidas decididas contra esta raça.
    pub winrate_pct: Option<u32>,
}

/// Uma partida. Mesma forma para `last_game` e para cada item de
/// `recent_games` — de propósito: é uma estrutura só para o autor do
/// template aprender.
#[derive(Serialize, Clone, Debug)]
pub struct Game {
    pub map: String,
    /// `YYYY-MM-DDTHH:MM:SS` como veio do replay.
    pub datetime: String,
    /// Parte da data, `YYYY-MM-DD`.
    pub date: String,
    /// Hora de início `HH:MM`.
    pub time: String,
    /// `true` quando esta partida entra no placar de `session`.
    pub is_today: bool,
    /// Código tipo `"TvZ"`, raça do usuário primeiro. `""` quando não há
    /// nickname cadastrado ou o replay não é 1v1.
    pub matchup: String,
    /// Resultado do ponto de vista do usuário, como vem do replay:
    /// `"Win"`, `"Loss"`, `"Undecided"` (acontece de verdade — partidas
    /// contra IA e desconexões) ou `""` quando não há nickname cadastrado.
    /// Só `Win`/`Loss` entram no placar de `session`. Um template que use
    /// este valor como classe CSS precisa cobrir os quatro casos.
    pub result: String,
    pub duration_seconds: u32,
    /// `"mm:ss"` (minutos não são zero-padded: `"7:03"`, `"61:01"`).
    pub duration_label: String,
    /// O usuário, quando algum nickname bate com um dos jogadores.
    pub me: Option<OverlayPlayer>,
    /// O outro jogador. `None` pelo mesmo motivo que `me`.
    pub opponent: Option<OverlayPlayer>,
    /// Os dois jogadores na ordem do replay, independente de nickname.
    /// É o que um template usa quando quer renderizar o placar "cru".
    pub players: Vec<OverlayPlayer>,
}

#[derive(Serialize, Clone, Debug)]
pub struct OverlayPlayer {
    pub name: String,
    /// Raça por extenso como vem do replay (`"Terran"`, `"Protoss"`, `"Zerg"`).
    pub race: String,
    /// Inicial da raça (`"T"`, `"P"`, `"Z"`, `"R"`), útil para ícones.
    pub race_letter: String,
    pub mmr: Option<i32>,
    /// `"Win"`, `"Loss"`, `"Undecided"` ou `""` — ver `Game::result`.
    pub result: String,
}
