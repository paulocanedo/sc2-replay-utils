//! Enums e struct de filtro/ordenação da biblioteca.

use crate::config::AppConfig;

use super::date::matches_date_range;
use super::entry_row::{find_user_index, find_user_player, matchup_code, race_letter};
use super::types::{LibraryEntry, MetaState};

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub enum OutcomeFilter {
    #[default]
    All,
    Wins,
    Losses,
}

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum DateRange {
    All,
    Today,
    #[default]
    ThisWeek,
    ThisMonth,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum SortOrder {
    #[default]
    Date,
    Duration,
    Mmr,
    Map,
}

#[derive(Clone, PartialEq, Eq)]
pub struct LibraryFilter {
    pub search: String,
    pub race: Option<char>,
    pub outcome: OutcomeFilter,
    pub date_range: DateRange,
    pub sort: SortOrder,
    pub sort_ascending: bool,
    /// Filtros de "relacionados" acionados via menu de contexto em
    /// `entry_row`. Todos compõem AND com os filtros acima. Usam
    /// matching exato (case-insensitive em nomes e mapas) — `search`
    /// fuzzy continua orthogonal.
    pub opponent_name: Option<String>,
    pub matchup_code: Option<String>,
    pub map_name: Option<String>,
    pub opening: Option<String>,
}

impl Default for LibraryFilter {
    fn default() -> Self {
        Self {
            search: String::new(),
            race: None,
            outcome: OutcomeFilter::All,
            date_range: DateRange::default(),
            sort: SortOrder::Date,
            sort_ascending: false,
            opponent_name: None,
            matchup_code: None,
            map_name: None,
            opening: None,
        }
    }
}

impl LibraryFilter {
    /// Inicializa o filtro restaurando preferências salvas no config.
    /// Quando `library_date_range` ainda não foi persistido (`None`),
    /// caímos no default (`ThisWeek`) apenas como placeholder — o
    /// trigger de auto-detect em `app/mod.rs` reescreve o valor assim
    /// que o scan termina.
    pub fn from_config(config: &crate::config::AppConfig) -> Self {
        Self {
            date_range: config.library_date_range.unwrap_or_default(),
            race: config.library_race,
            ..Self::default()
        }
    }
}

/// Varre as janelas temporais da mais restrita (`Today`) para a mais
/// permissiva (`All`) e retorna a primeira em que existe ao menos um
/// replay já parseado. `None` indica biblioteca sem nenhuma entrada
/// `Parsed` — o caller decide se persiste algo ou tenta de novo depois.
///
/// Usada no startup: quando `config.library_date_range == None`, o app
/// espera o scan terminar e persiste a primeira janela não-vazia para
/// evitar que o primeiro launch abra a biblioteca num filtro vazio.
pub fn detect_best_date_range(entries: &[LibraryEntry], today: &str) -> Option<DateRange> {
    const CANDIDATES: [DateRange; 4] = [
        DateRange::Today,
        DateRange::ThisWeek,
        DateRange::ThisMonth,
        DateRange::All,
    ];
    for range in CANDIDATES {
        let has_match = entries.iter().any(|e| match &e.meta {
            MetaState::Parsed(m) => matches_date_range(&m.datetime, range, today),
            _ => false,
        });
        if has_match {
            return Some(range);
        }
    }
    None
}

/// Chave de invalidação do cache de stats. Ignora ordenação porque
/// `sort`/`sort_ascending` não afetam os agregados exibidos no hero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsFilterKey {
    pub search: String,
    pub race: Option<char>,
    pub outcome: OutcomeFilter,
    pub date_range: DateRange,
    pub opponent_name: Option<String>,
    pub matchup_code: Option<String>,
    pub map_name: Option<String>,
    pub opening: Option<String>,
}

impl From<&LibraryFilter> for StatsFilterKey {
    fn from(f: &LibraryFilter) -> Self {
        Self {
            search: f.search.clone(),
            race: f.race,
            outcome: f.outcome,
            date_range: f.date_range,
            opponent_name: f.opponent_name.clone(),
            matchup_code: f.matchup_code.clone(),
            map_name: f.map_name.clone(),
            opening: f.opening.clone(),
        }
    }
}

/// Predicado único usado pela lista virtualizada e pelo cálculo de stats,
/// garantindo que "o que aparece na lista" e "o que o hero mede" sejam o
/// mesmo conjunto de entradas. Entradas não-`Parsed` só passam quando
/// nenhum filtro está ativo (legado da lista, para não esconder Pending
/// durante o scan inicial); como `compute_library_stats` ignora estados
/// não-`Parsed` internamente, o comportamento para stats é equivalente.
pub fn matches_filter(
    entry: &LibraryEntry,
    filter: &LibraryFilter,
    config: &AppConfig,
    today: &str,
) -> bool {
    let needle = filter.search.trim().to_ascii_lowercase();
    let search_active = !needle.is_empty();
    let any_filter_active = search_active
        || filter.race.is_some()
        || filter.outcome != OutcomeFilter::All
        || filter.date_range != DateRange::All
        || filter.opponent_name.is_some()
        || filter.matchup_code.is_some()
        || filter.map_name.is_some()
        || filter.opening.is_some();

    match &entry.meta {
        MetaState::Parsed(meta) => {
            if search_active {
                let name_match = meta
                    .players
                    .iter()
                    .any(|p| p.name.to_ascii_lowercase().contains(&needle));
                let map_match = meta.map.to_ascii_lowercase().contains(&needle);
                let mc = matchup_code(meta, config);
                let matchup_match = mc.to_ascii_lowercase().contains(&needle);
                let opening_match = meta.players.iter().any(|p| {
                    p.opening
                        .as_ref()
                        .map_or(false, |o| o.to_ascii_lowercase().contains(&needle))
                });
                if !(name_match || map_match || matchup_match || opening_match) {
                    return false;
                }
            }
            if let Some(wanted) = &filter.opponent_name {
                let wanted_lc = wanted.to_ascii_lowercase();
                let user_idx = find_user_index(meta, config);
                let opp_match = match user_idx {
                    Some(i) if meta.players.len() == 2 => {
                        meta.players[1 - i].name.to_ascii_lowercase() == wanted_lc
                    }
                    _ => meta
                        .players
                        .iter()
                        .any(|p| p.name.to_ascii_lowercase() == wanted_lc),
                };
                if !opp_match {
                    return false;
                }
            }
            if let Some(wanted) = &filter.matchup_code {
                if matchup_code(meta, config) != *wanted {
                    return false;
                }
            }
            if let Some(wanted) = &filter.map_name {
                if meta.map.to_ascii_lowercase() != wanted.to_ascii_lowercase() {
                    return false;
                }
            }
            if let Some(wanted) = &filter.opening {
                let any = meta
                    .players
                    .iter()
                    .any(|p| p.opening.as_deref() == Some(wanted.as_str()));
                if !any {
                    return false;
                }
            }
            if let Some(race_ch) = filter.race {
                let user = find_user_player(meta, config);
                let matches = user.map_or(false, |p| race_letter(&p.race) == race_ch);
                if !matches {
                    return false;
                }
            }
            match filter.outcome {
                OutcomeFilter::All => {}
                OutcomeFilter::Wins => {
                    let won = find_user_player(meta, config)
                        .map_or(false, |p| p.result == "Win");
                    if !won {
                        return false;
                    }
                }
                OutcomeFilter::Losses => {
                    let lost = find_user_player(meta, config)
                        .map_or(false, |p| p.result == "Loss");
                    if !lost {
                        return false;
                    }
                }
            }
            if !matches_date_range(&meta.datetime, filter.date_range, today) {
                return false;
            }
            true
        }
        _ => !any_filter_active,
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{LibraryEntry, MetaState, ParsedMeta, PlayerMeta};
    use super::*;
    use std::path::PathBuf;

    fn make_parsed(
        map: &str,
        datetime: &str,
        user_name: &str,
        user_race: &str,
        user_result: &str,
        opp_race: &str,
    ) -> LibraryEntry {
        let opp_result = if user_result == "Win" { "Loss" } else { "Win" };
        LibraryEntry {
            path: PathBuf::from(format!("{datetime}.SC2Replay")),
            filename: format!("{datetime}.SC2Replay"),
            mtime: None,
            meta: MetaState::Parsed(ParsedMeta {
                map: map.into(),
                datetime: datetime.into(),
                duration_seconds: 600,
                game_loops: 10_000,
                version: None,
                cache_handles: Vec::new(),
                players: vec![
                    PlayerMeta {
                        name: user_name.into(),
                        race: user_race.into(),
                        mmr: None,
                        result: user_result.into(),
                        opening: None,
                    },
                    PlayerMeta {
                        name: "opp".into(),
                        race: opp_race.into(),
                        mmr: None,
                        result: opp_result.into(),
                        opening: None,
                    },
                ],
            }),
        }
    }

    fn make_pending() -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from("p.SC2Replay"),
            filename: "p.SC2Replay".into(),
            mtime: None,
            meta: MetaState::Pending,
        }
    }

    fn cfg_with(nick: &str) -> AppConfig {
        let mut c = AppConfig::default();
        c.user_nicknames = vec![nick.into()];
        c
    }

    #[test]
    fn no_filters_passes_everything() {
        let cfg = cfg_with("me");
        let f = LibraryFilter {
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        let parsed = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let pending = make_pending();
        assert!(matches_filter(&parsed, &f, &cfg, "2026-04-20"));
        assert!(matches_filter(&pending, &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn pending_drops_when_any_filter_active() {
        let cfg = cfg_with("me");
        let f = LibraryFilter {
            outcome: OutcomeFilter::Wins,
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&make_pending(), &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn outcome_wins_keeps_only_user_wins() {
        let cfg = cfg_with("me");
        let f = LibraryFilter {
            outcome: OutcomeFilter::Wins,
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        let win = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let loss = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Loss", "Zerg");
        assert!(matches_filter(&win, &f, &cfg, "2026-04-20"));
        assert!(!matches_filter(&loss, &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn race_filter_matches_user_race() {
        let cfg = cfg_with("me");
        let f = LibraryFilter {
            race: Some('T'),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        let t = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let z = make_parsed("M", "2026-04-10T10:00:00", "me", "Zerg", "Win", "Zerg");
        assert!(matches_filter(&t, &f, &cfg, "2026-04-20"));
        assert!(!matches_filter(&z, &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn search_matches_player_map_or_matchup() {
        let cfg = cfg_with("me");
        let parsed = make_parsed("Ancient Cistern", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        for q in ["ancient", "ME", "TvZ"] {
            let f = LibraryFilter {
                search: q.into(),
                date_range: DateRange::All,
                ..LibraryFilter::default()
            };
            assert!(matches_filter(&parsed, &f, &cfg, "2026-04-20"), "query={q}");
        }
        let miss = LibraryFilter {
            search: "nope".into(),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&parsed, &miss, &cfg, "2026-04-20"));
    }

    #[test]
    fn search_matches_opening_label() {
        let cfg = cfg_with("me");
        let mut parsed = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        if let MetaState::Parsed(meta) = &mut parsed.meta {
            meta.players[0].opening = Some("3 Rax Reaper — Stim Timing".into());
            meta.players[1].opening = Some("Hatch First — Ling/Queen".into());
        }
        for q in ["reaper", "HATCH", "ling/queen", "stim"] {
            let f = LibraryFilter {
                search: q.into(),
                date_range: DateRange::All,
                ..LibraryFilter::default()
            };
            assert!(matches_filter(&parsed, &f, &cfg, "2026-04-20"), "query={q}");
        }
        let miss = LibraryFilter {
            search: "roach".into(),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&parsed, &miss, &cfg, "2026-04-20"));
    }

    #[test]
    fn search_ignores_missing_opening() {
        let cfg = cfg_with("me");
        let parsed = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let f = LibraryFilter {
            search: "hatch".into(),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&parsed, &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn stats_filter_key_ignores_sort() {
        let mut a = LibraryFilter::default();
        let mut b = LibraryFilter::default();
        a.sort = SortOrder::Date;
        b.sort = SortOrder::Mmr;
        a.sort_ascending = true;
        b.sort_ascending = false;
        assert_eq!(StatsFilterKey::from(&a), StatsFilterKey::from(&b));
        b.outcome = OutcomeFilter::Wins;
        assert_ne!(StatsFilterKey::from(&a), StatsFilterKey::from(&b));
    }

    #[test]
    fn stats_filter_key_diverges_for_related_fields() {
        let base = LibraryFilter::default();
        let mut with_opponent = base.clone();
        with_opponent.opponent_name = Some("opp".into());
        assert_ne!(
            StatsFilterKey::from(&base),
            StatsFilterKey::from(&with_opponent)
        );
        let mut with_map = base.clone();
        with_map.map_name = Some("Ancient Cistern".into());
        assert_ne!(StatsFilterKey::from(&base), StatsFilterKey::from(&with_map));
    }

    #[test]
    fn opponent_filter_matches_non_user_player() {
        let cfg = cfg_with("me");
        let parsed = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let f = LibraryFilter {
            opponent_name: Some("opp".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(matches_filter(&parsed, &f, &cfg, "2026-04-20"));

        // Matching contra o próprio usuário não deve passar.
        let f_self = LibraryFilter {
            opponent_name: Some("me".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&parsed, &f_self, &cfg, "2026-04-20"));
    }

    #[test]
    fn opponent_filter_is_case_insensitive() {
        let cfg = cfg_with("me");
        let parsed = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let f = LibraryFilter {
            opponent_name: Some("OPP".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(matches_filter(&parsed, &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn matchup_filter_is_exact_not_substring() {
        let cfg = cfg_with("me");
        let parsed = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let f_hit = LibraryFilter {
            matchup_code: Some("TvZ".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(matches_filter(&parsed, &f_hit, &cfg, "2026-04-20"));
        let f_miss = LibraryFilter {
            matchup_code: Some("TvP".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&parsed, &f_miss, &cfg, "2026-04-20"));
    }

    #[test]
    fn map_filter_case_insensitive_exact() {
        let cfg = cfg_with("me");
        let parsed = make_parsed(
            "Ancient Cistern",
            "2026-04-10T10:00:00",
            "me",
            "Terran",
            "Win",
            "Zerg",
        );
        let f = LibraryFilter {
            map_name: Some("ancient cistern".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(matches_filter(&parsed, &f, &cfg, "2026-04-20"));
        let f_miss = LibraryFilter {
            map_name: Some("Ancient".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&parsed, &f_miss, &cfg, "2026-04-20"));
    }

    #[test]
    fn opening_filter_matches_any_player() {
        let cfg = cfg_with("me");
        let mut parsed = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        if let MetaState::Parsed(meta) = &mut parsed.meta {
            meta.players[0].opening = Some("1 Rax FE — Stim Timing".into());
            meta.players[1].opening = Some("Hatch First — Ling/Queen".into());
        }
        let f_user = LibraryFilter {
            opening: Some("1 Rax FE — Stim Timing".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(matches_filter(&parsed, &f_user, &cfg, "2026-04-20"));
        let f_opp = LibraryFilter {
            opening: Some("Hatch First — Ling/Queen".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(matches_filter(&parsed, &f_opp, &cfg, "2026-04-20"));
        let f_miss = LibraryFilter {
            opening: Some("Hatch".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&parsed, &f_miss, &cfg, "2026-04-20"));
    }

    #[test]
    fn related_filters_combine_and_with_outcome() {
        let cfg = cfg_with("me");
        let win = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Win", "Zerg");
        let loss = make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", "Loss", "Zerg");
        let f = LibraryFilter {
            opponent_name: Some("opp".into()),
            outcome: OutcomeFilter::Wins,
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(matches_filter(&win, &f, &cfg, "2026-04-20"));
        assert!(!matches_filter(&loss, &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn pending_drops_when_related_filter_active() {
        let cfg = cfg_with("me");
        let f = LibraryFilter {
            map_name: Some("M".into()),
            date_range: DateRange::All,
            ..LibraryFilter::default()
        };
        assert!(!matches_filter(&make_pending(), &f, &cfg, "2026-04-20"));
    }

    #[test]
    fn detect_picks_today_when_today_has_entries() {
        // Mix de hoje, ontem e mês passado: o mais restrito (`Today`) bate.
        let entries = vec![
            make_parsed("M", "2026-04-23T09:00:00", "me", "Terran", "Win", "Zerg"),
            make_parsed("M", "2026-04-22T10:00:00", "me", "Terran", "Win", "Zerg"),
            make_parsed("M", "2026-03-10T10:00:00", "me", "Terran", "Win", "Zerg"),
        ];
        assert_eq!(
            detect_best_date_range(&entries, "2026-04-23"),
            Some(DateRange::Today),
        );
    }

    #[test]
    fn detect_falls_through_to_week_when_today_empty() {
        // 2026-04-23 é uma quinta; 2026-04-21 (terça) está na mesma semana.
        let entries = vec![
            make_parsed("M", "2026-04-21T10:00:00", "me", "Terran", "Win", "Zerg"),
            make_parsed("M", "2026-03-10T10:00:00", "me", "Terran", "Win", "Zerg"),
        ];
        assert_eq!(
            detect_best_date_range(&entries, "2026-04-23"),
            Some(DateRange::ThisWeek),
        );
    }

    #[test]
    fn detect_falls_through_to_month() {
        // Só entries do mês corrente mas fora da semana (dia 1).
        let entries = vec![
            make_parsed("M", "2026-04-01T10:00:00", "me", "Terran", "Win", "Zerg"),
            make_parsed("M", "2025-12-15T10:00:00", "me", "Terran", "Win", "Zerg"),
        ];
        assert_eq!(
            detect_best_date_range(&entries, "2026-04-23"),
            Some(DateRange::ThisMonth),
        );
    }

    #[test]
    fn detect_falls_through_to_all() {
        // Apenas entries antigos — cai no mais permissivo.
        let entries = vec![
            make_parsed("M", "2025-06-10T10:00:00", "me", "Terran", "Win", "Zerg"),
            make_parsed("M", "2024-11-02T10:00:00", "me", "Terran", "Win", "Zerg"),
        ];
        assert_eq!(
            detect_best_date_range(&entries, "2026-04-23"),
            Some(DateRange::All),
        );
    }

    #[test]
    fn detect_returns_none_when_library_empty() {
        let entries: Vec<LibraryEntry> = Vec::new();
        assert_eq!(detect_best_date_range(&entries, "2026-04-23"), None);
    }

    #[test]
    fn detect_ignores_pending_entries() {
        // Entries ainda não-parseados não contam: só `Pending` ⇒ `None`.
        let entries = vec![make_pending()];
        assert_eq!(detect_best_date_range(&entries, "2026-04-23"), None);
    }
}
