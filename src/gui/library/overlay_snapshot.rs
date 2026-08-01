//! Projeção `&[LibraryEntry]` → `OverlayData` para o overlay de transmissão.
//!
//! Mora dentro de `library` (e não em `overlay`) pelo mesmo motivo que
//! `stats.rs`: precisa dos helpers `pub(super)` de `entry_row` e `date`.
//! Alargá-los para `pub` vazaria as entranhas de um módulo de *render de
//! linha* para o crate inteiro. A direção de dependência fica
//! `library → overlay::data`; `overlay` nunca importa `library`.
//!
//! É um cache derivado puro, exatamente como `compute_library_stats` — a
//! fonte da verdade continua sendo `ReplayLibrary::entries`. Roda em
//! O(entries) e só é chamada em evento (replay novo, fim de scan, mudança
//! de nickname), nunca por frame.

use crate::config::AppConfig;
use crate::overlay::data::{Game, OverlayData, OverlayPlayer, RaceRecord, SessionStats};
use crate::utils::race_letter;

use super::date::{matches_date_range, now_local_str};
use super::entry_row::{find_user_player, matchup_code};
use super::filter::DateRange;
use super::stats::compute_library_stats;
use super::types::{LibraryEntry, MetaState, ParsedMeta};

/// Quantas partidas entram em `OverlayData::recent_games` — o suficiente
/// para a faixa de "forma recente" dos templates sem inflar o JSON.
pub const RECENT_GAMES: usize = 10;

/// Raças de oponente reportadas em `session.by_race`, em ordem fixa. A
/// lista é sempre completa (mesmo zerada) para o template poder desenhar a
/// grade inteira sem checar ausências.
const TRACKED_RACES: [&str; 4] = ["Zerg", "Terran", "Protoss", "Random"];

/// Constrói o snapshot do overlay.
///
/// Recebe slice (e não `&ReplayLibrary`) para ser testável do mesmo jeito
/// que `compute_library_stats`.
pub fn build(entries: &[LibraryEntry], config: &AppConfig, today: &str) -> OverlayData {
    let recent_games = build_recent_games(entries, config, today);
    OverlayData {
        // Preenchida por `OverlayState::publish` — aqui ainda não sabemos
        // qual será.
        revision: 0,
        generated_at: now_local_str(),
        nicknames_configured: !config.user_nicknames.is_empty(),
        session: build_session(entries, config, today),
        last_game: recent_games.first().cloned(),
        recent_games,
    }
}

fn build_session(entries: &[LibraryEntry], config: &AppConfig, today: &str) -> SessionStats {
    // Reusa a projeção existente sobre um iterador filtrado por hoje: a
    // assinatura de `compute_library_stats` já aceita `IntoIterator`, então
    // a classificação win/loss não é duplicada aqui.
    let stats = compute_library_stats(entries.iter().filter(|e| is_today(e, today)), config);

    // `LibraryStats::mmr_trend_delta` NÃO serve aqui: é uma janela de 7
    // jogos, que aplicada só a hoje daria `None` em praticamente toda
    // sessão real. O que o streamer quer é o saldo do dia — último MMR
    // menos o primeiro. Não "simplifique" isto de volta para
    // `mmr_trend_delta`; há um teste de regressão logo abaixo.
    let mut mmr_points: Vec<(&str, i32)> = entries
        .iter()
        .filter_map(|e| parsed_today(e, today))
        .filter_map(|m| Some((m.datetime.as_str(), find_user_player(m, config)?.mmr?)))
        .collect();
    mmr_points.sort_by(|a, b| a.0.cmp(b.0)); // ISO 8601 ordena lexicograficamente

    let mmr_latest = mmr_points.last().map(|(_, m)| *m);
    let mmr_delta = match (mmr_points.first(), mmr_points.last()) {
        (Some((_, first)), Some((_, last))) if mmr_points.len() >= 2 => Some(last - first),
        _ => None,
    };

    SessionStats {
        date: today.to_string(),
        games: stats.wins + stats.losses,
        wins: stats.wins,
        losses: stats.losses,
        winrate: stats.winrate_global,
        winrate_pct: stats.winrate_global.map(|w| (w * 100.0).round() as u32),
        mmr_latest,
        mmr_delta,
        by_race: build_by_race(entries, config, today),
    }
}

/// Recorde de hoje por raça do **oponente**.
///
/// Percorre as entries uma vez acumulando num array de tamanho fixo em vez
/// de um `HashMap` — são quatro raças e a ordem de saída é fixa de qualquer
/// jeito.
fn build_by_race(entries: &[LibraryEntry], config: &AppConfig, today: &str) -> Vec<RaceRecord> {
    let mut tally = [(0usize, 0usize); TRACKED_RACES.len()];
    for meta in entries.iter().filter_map(|e| parsed_today(e, today)) {
        let Some(me) = find_user_player(meta, config) else {
            continue;
        };
        let Some(opponent) = meta
            .players
            .iter()
            .find(|p| !p.name.eq_ignore_ascii_case(&me.name))
        else {
            continue;
        };
        let Some(slot) = TRACKED_RACES
            .iter()
            .position(|r| race_letter(r) == race_letter(&opponent.race))
        else {
            continue;
        };
        match me.result.as_str() {
            "Win" => tally[slot].0 += 1,
            "Loss" => tally[slot].1 += 1,
            _ => {}
        }
    }

    TRACKED_RACES
        .iter()
        .zip(tally)
        .map(|(race, (wins, losses))| {
            let games = wins + losses;
            RaceRecord {
                race: (*race).to_string(),
                race_letter: race_letter(race).to_string(),
                games,
                wins,
                losses,
                winrate_pct: (games > 0)
                    .then(|| ((wins as f32 / games as f32) * 100.0).round() as u32),
            }
        })
        .collect()
}

/// As `RECENT_GAMES` partidas mais recentes, mais nova primeiro.
///
/// Ordena por `datetime` em vez de confiar na ordem do vec: `entries` só
/// fica ordenado por mtime depois que o scan termina, e `ingest_pending`
/// insere no índice 0 uma entry ainda sem metadata.
fn build_recent_games(entries: &[LibraryEntry], config: &AppConfig, today: &str) -> Vec<Game> {
    let mut parsed: Vec<&ParsedMeta> = entries
        .iter()
        .filter_map(|e| match &e.meta {
            MetaState::Parsed(m) => Some(m),
            _ => None,
        })
        .collect();
    // ISO 8601 ordena lexicograficamente; desc.
    parsed.sort_by(|a, b| b.datetime.cmp(&a.datetime));
    parsed
        .into_iter()
        .take(RECENT_GAMES)
        .map(|m| to_game(m, config, today))
        .collect()
}

fn to_game(meta: &ParsedMeta, config: &AppConfig, today: &str) -> Game {
    let players: Vec<OverlayPlayer> = meta.players.iter().map(to_overlay_player).collect();
    let me = find_user_player(meta, config).map(to_overlay_player);
    // O oponente é o jogador que não é o usuário. Sem nickname cadastrado
    // não há como decidir, e os dois ficam `None` — o template ramifica em
    // `nicknames_configured`.
    let opponent = me.as_ref().and_then(|me| {
        meta.players
            .iter()
            .find(|p| !p.name.eq_ignore_ascii_case(&me.name))
            .map(to_overlay_player)
    });

    let (date, time) = split_datetime(&meta.datetime);
    Game {
        map: meta.map.clone(),
        datetime: meta.datetime.clone(),
        is_today: date == today,
        date,
        time,
        matchup: matchup_code(meta, config),
        result: me.as_ref().map(|p| p.result.clone()).unwrap_or_default(),
        duration_seconds: meta.duration_seconds,
        duration_label: duration_label(meta.duration_seconds),
        me,
        opponent,
        players,
    }
}

fn to_overlay_player(p: &super::types::PlayerMeta) -> OverlayPlayer {
    OverlayPlayer {
        name: p.name.clone(),
        race: p.race.clone(),
        race_letter: race_letter(&p.race).to_string(),
        mmr: p.mmr,
        result: p.result.clone(),
    }
}

fn is_today(entry: &LibraryEntry, today: &str) -> bool {
    parsed_today(entry, today).is_some()
}

fn parsed_today<'a>(entry: &'a LibraryEntry, today: &str) -> Option<&'a ParsedMeta> {
    match &entry.meta {
        MetaState::Parsed(m) if matches_date_range(&m.datetime, DateRange::Today, today) => Some(m),
        _ => None,
    }
}

/// `"2026-07-31T20:02:11"` → `("2026-07-31", "20:02")`.
fn split_datetime(datetime: &str) -> (String, String) {
    let mut parts = datetime.splitn(2, 'T');
    let date = parts.next().unwrap_or_default().to_string();
    let time = parts
        .next()
        .map(|t| t.chars().take(5).collect::<String>())
        .unwrap_or_default();
    (date, time)
}

/// `"mm:ss"`. Minutos não são zero-padded (`"7:03"`, `"61:01"`) — é como
/// tempo de partida é lido em qualquer lugar do SC2.
fn duration_label(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{OpeningLabel, PlayerMeta};
    use std::path::PathBuf;

    const TODAY: &str = "2026-07-31";

    fn entry(
        datetime: &str,
        map: &str,
        user_name: &str,
        user_race: &str,
        user_mmr: Option<i32>,
        user_result: &str,
        opp_race: &str,
        duration: u32,
    ) -> LibraryEntry {
        let opp_result = if user_result == "Win" { "Loss" } else { "Win" };
        LibraryEntry {
            path: PathBuf::from(format!("{datetime}.SC2Replay")),
            filename: format!("{datetime}.SC2Replay"),
            mtime: None,
            meta: MetaState::Parsed(ParsedMeta {
                map: map.into(),
                datetime: datetime.into(),
                duration_seconds: duration,
                game_loops: duration * 22,
                version: None,
                cache_handles: Vec::new(),
                players: vec![
                    PlayerMeta {
                        name: user_name.into(),
                        race: user_race.into(),
                        mmr: user_mmr,
                        result: user_result.into(),
                        opening: OpeningLabel::Pending,
                    },
                    PlayerMeta {
                        name: "Raynor".into(),
                        race: opp_race.into(),
                        mmr: Some(4000),
                        result: opp_result.into(),
                        opening: OpeningLabel::Pending,
                    },
                ],
            }),
        }
    }

    fn cfg(nick: &str) -> AppConfig {
        let mut c = AppConfig::default();
        if !nick.is_empty() {
            c.user_nicknames = vec![nick.into()];
        }
        c
    }

    fn session_today() -> Vec<LibraryEntry> {
        vec![
            entry("2026-07-31T18:00:00", "Alpha", "Kerrigan", "Zerg", Some(4100), "Win", "Terran", 600),
            entry("2026-07-31T19:00:00", "Beta", "Kerrigan", "Zerg", Some(4150), "Loss", "Protoss", 754),
            entry("2026-07-31T20:00:00", "Gamma", "Kerrigan", "Zerg", Some(4142), "Win", "Terran", 421),
        ]
    }

    #[test]
    fn session_counts_only_today() {
        let mut entries = session_today();
        entries.push(entry(
            "2026-07-30T21:00:00", "Old", "Kerrigan", "Zerg", Some(3900), "Loss", "Terran", 300,
        ));
        let s = build(&entries, &cfg("Kerrigan"), TODAY).session;
        assert_eq!(s.date, TODAY);
        assert_eq!(s.games, 3);
        assert_eq!(s.wins, 2);
        assert_eq!(s.losses, 1);
        assert_eq!(s.winrate_pct, Some(67));
    }

    #[test]
    fn session_mmr_delta_is_last_minus_first_of_the_day() {
        // REGRESSÃO: se alguém trocar isto por `LibraryStats::mmr_trend_delta`
        // (janela de 7 jogos), este assert vira None e o teste quebra —
        // que é exatamente o ponto.
        let s = build(&session_today(), &cfg("Kerrigan"), TODAY).session;
        assert_eq!(s.mmr_latest, Some(4142));
        assert_eq!(s.mmr_delta, Some(42));
    }

    #[test]
    fn session_mmr_delta_needs_two_points() {
        let entries = vec![entry(
            "2026-07-31T18:00:00", "Alpha", "Kerrigan", "Zerg", Some(4100), "Win", "Terran", 600,
        )];
        let s = build(&entries, &cfg("Kerrigan"), TODAY).session;
        assert_eq!(s.mmr_latest, Some(4100));
        assert_eq!(s.mmr_delta, None);
    }

    #[test]
    fn last_game_is_the_max_datetime_not_the_first_entry() {
        // Vec deliberadamente fora de ordem: `entries` só fica ordenado por
        // mtime depois do scan, e `ingest_pending` insere no índice 0.
        let mut entries = session_today();
        entries.reverse();
        entries.insert(
            0,
            entry("2026-07-29T10:00:00", "Stale", "Kerrigan", "Zerg", None, "Loss", "Terran", 120),
        );
        let g = build(&entries, &cfg("Kerrigan"), TODAY).last_game.unwrap();
        assert_eq!(g.map, "Gamma");
        assert_eq!(g.datetime, "2026-07-31T20:00:00");
        assert_eq!(g.date, TODAY);
        assert_eq!(g.time, "20:00");
        assert!(g.is_today);
    }

    #[test]
    fn last_game_may_predate_today() {
        // Antes do primeiro jogo da sessão o streamer ainda quer ver a
        // última partida — só que marcada como não sendo de hoje.
        let entries = vec![entry(
            "2026-07-28T21:00:00", "Old", "Kerrigan", "Zerg", Some(3900), "Loss", "Terran", 300,
        )];
        let data = build(&entries, &cfg("Kerrigan"), TODAY);
        let g = data.last_game.unwrap();
        assert!(!g.is_today);
        assert_eq!(data.session.games, 0);
    }

    #[test]
    fn me_opponent_and_matchup_resolve_from_nicknames() {
        let g = build(&session_today(), &cfg("Kerrigan"), TODAY).last_game.unwrap();
        // Raça do usuário primeiro.
        assert_eq!(g.matchup, "ZvT");
        assert_eq!(g.result, "Win");
        assert_eq!(g.me.as_ref().unwrap().name, "Kerrigan");
        assert_eq!(g.me.as_ref().unwrap().race_letter, "Z");
        assert_eq!(g.opponent.as_ref().unwrap().name, "Raynor");
        assert_eq!(g.opponent.as_ref().unwrap().race_letter, "T");
        // `players` mantém a ordem do replay, independente de nickname.
        assert_eq!(g.players.len(), 2);
        assert_eq!(g.players[0].name, "Kerrigan");
    }

    #[test]
    fn nickname_match_is_case_insensitive() {
        let g = build(&session_today(), &cfg("kErRiGaN"), TODAY).last_game.unwrap();
        assert_eq!(g.me.unwrap().name, "Kerrigan");
    }

    #[test]
    fn without_nicknames_the_session_is_blank_but_the_game_still_shows() {
        let data = build(&session_today(), &cfg(""), TODAY);
        assert!(!data.nicknames_configured);
        assert_eq!(data.session.games, 0);
        assert_eq!(data.session.winrate, None);
        assert_eq!(data.session.mmr_latest, None);
        let g = data.last_game.unwrap();
        assert!(g.me.is_none());
        assert!(g.opponent.is_none());
        assert_eq!(g.result, "");
        // Sem usuário identificado, o matchup cai na ordem do replay.
        assert_eq!(g.matchup, "ZvT");
        assert_eq!(g.players.len(), 2);
    }

    #[test]
    fn non_parsed_entries_are_ignored() {
        let mut entries = session_today();
        entries.push(LibraryEntry {
            path: PathBuf::from("p.SC2Replay"),
            filename: "p.SC2Replay".into(),
            mtime: None,
            meta: MetaState::Pending,
        });
        entries.push(LibraryEntry {
            path: PathBuf::from("u.SC2Replay"),
            filename: "u.SC2Replay".into(),
            mtime: None,
            meta: MetaState::Unsupported("não 1v1".into()),
        });
        let data = build(&entries, &cfg("Kerrigan"), TODAY);
        assert_eq!(data.session.games, 3);
        assert_eq!(data.last_game.unwrap().map, "Gamma");
    }

    #[test]
    fn by_race_groups_by_opponent_race_and_always_has_four_entries() {
        let mut entries = session_today();
        // session_today: 2 vitórias vs Terran, 1 derrota vs Protoss.
        entries.push(entry(
            "2026-07-31T21:00:00", "Delta", "Kerrigan", "Zerg", Some(4160), "Loss", "Terran", 300,
        ));
        let by_race = build(&entries, &cfg("Kerrigan"), TODAY).session.by_race;

        let names: Vec<&str> = by_race.iter().map(|r| r.race.as_str()).collect();
        assert_eq!(names, ["Zerg", "Terran", "Protoss", "Random"], "ordem fixa");

        let terran = &by_race[1];
        assert_eq!((terran.wins, terran.losses, terran.games), (2, 1, 3));
        assert_eq!(terran.winrate_pct, Some(67));

        let protoss = &by_race[2];
        assert_eq!((protoss.wins, protoss.losses), (0, 1));
        assert_eq!(protoss.winrate_pct, Some(0));

        // Raça não enfrentada continua presente e zerada — o template
        // desenha a grade completa sem checar ausências.
        let zerg = &by_race[0];
        assert_eq!((zerg.games, zerg.winrate_pct), (0, None));
    }

    #[test]
    fn by_race_ignores_yesterday_and_unidentified_players() {
        let mut entries = session_today();
        entries.push(entry(
            "2026-07-30T21:00:00", "Old", "Kerrigan", "Zerg", None, "Win", "Terran", 300,
        ));
        // Sem nickname não há como saber quem é o oponente.
        assert!(
            build(&entries, &cfg(""), TODAY)
                .session
                .by_race
                .iter()
                .all(|r| r.games == 0)
        );
        // Com nickname, o jogo de ontem fica de fora: 2 vs Terran, não 3.
        let by_race = build(&entries, &cfg("Kerrigan"), TODAY).session.by_race;
        assert_eq!(by_race[1].games, 2);
    }

    #[test]
    fn undecided_games_appear_in_history_but_score_nowhere() {
        // Replays contra IA e desconexões trazem `result: "Undecided"` —
        // visto em replay real. Ele ocupa um slot na faixa de forma, mas
        // não pode entrar em winrate nem no recorde por raça.
        let mut entries = session_today();
        entries.push(entry(
            "2026-07-31T22:00:00", "Vs AI", "Kerrigan", "Zerg", None, "Undecided", "Protoss", 200,
        ));
        let data = build(&entries, &cfg("Kerrigan"), TODAY);
        assert_eq!(data.session.games, 3, "continua 2W-1L, sem o Undecided");
        assert_eq!(data.session.winrate_pct, Some(67));
        assert_eq!(data.session.by_race[2].games, 1, "Protoss segue só com a derrota");
        // Mas está no histórico, e é o jogo mais recente.
        assert_eq!(data.recent_games.len(), 4);
        assert_eq!(data.recent_games[0].result, "Undecided");
    }

    #[test]
    fn recent_games_are_newest_first_and_capped() {
        let mut entries: Vec<LibraryEntry> = (0..14)
            .map(|i| {
                entry(
                    &format!("2026-07-{:02}T12:00:00", i + 10),
                    &format!("Map{i}"),
                    "Kerrigan", "Zerg", Some(4000), "Win", "Terran", 300,
                )
            })
            .collect();
        entries.reverse();
        let data = build(&entries, &cfg("Kerrigan"), TODAY);
        assert_eq!(data.recent_games.len(), RECENT_GAMES);
        // Mais novo primeiro: 2026-07-23 é o maior datetime (i = 13).
        assert_eq!(data.recent_games[0].map, "Map13");
        assert_eq!(data.recent_games[9].map, "Map4");
        // `last_game` é exatamente `recent_games[0]` — uma forma só.
        assert_eq!(data.last_game.unwrap().map, "Map13");
    }

    #[test]
    fn recent_games_span_days_and_flag_which_are_today() {
        let mut entries = session_today();
        entries.push(entry(
            "2026-07-29T10:00:00", "Old", "Kerrigan", "Zerg", None, "Loss", "Terran", 120,
        ));
        let games = build(&entries, &cfg("Kerrigan"), TODAY).recent_games;
        assert_eq!(games.len(), 4);
        assert!(games[..3].iter().all(|g| g.is_today));
        assert!(!games[3].is_today, "o de 29/07 não é de hoje");
    }

    #[test]
    fn empty_library_yields_the_empty_state() {
        let data = build(&[], &cfg("Kerrigan"), TODAY);
        assert!(data.nicknames_configured);
        assert_eq!(data.session.games, 0);
        assert_eq!(data.session.winrate_pct, None);
        assert!(data.last_game.is_none());
        assert!(data.recent_games.is_empty());
        // A grade de raças existe mesmo sem nenhum jogo.
        assert_eq!(data.session.by_race.len(), 4);
    }

    #[test]
    fn duration_label_formats_without_padding_the_minutes() {
        assert_eq!(duration_label(0), "0:00");
        assert_eq!(duration_label(59), "0:59");
        assert_eq!(duration_label(423), "7:03");
        assert_eq!(duration_label(3661), "61:01");
    }

    #[test]
    fn split_datetime_tolerates_malformed_input() {
        assert_eq!(
            split_datetime("2026-07-31T20:02:11"),
            ("2026-07-31".into(), "20:02".into())
        );
        assert_eq!(split_datetime("2026-07-31"), ("2026-07-31".into(), "".into()));
        assert_eq!(split_datetime(""), ("".into(), "".into()));
    }
}
