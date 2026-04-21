//! Enums e struct de filtro/ordenação da biblioteca.

use crate::config::AppConfig;

use super::date::matches_date_range;
use super::entry_row::{find_user_player, matchup_code, race_letter};
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
        }
    }
}

impl LibraryFilter {
    /// Inicializa o filtro restaurando preferências salvas no config.
    pub fn from_config(config: &crate::config::AppConfig) -> Self {
        Self {
            date_range: config.library_date_range,
            ..Self::default()
        }
    }
}

/// Chave de invalidação do cache de stats. Ignora ordenação porque
/// `sort`/`sort_ascending` não afetam os agregados exibidos no hero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsFilterKey {
    pub search: String,
    pub race: Option<char>,
    pub outcome: OutcomeFilter,
    pub date_range: DateRange,
}

impl From<&LibraryFilter> for StatsFilterKey {
    fn from(f: &LibraryFilter) -> Self {
        Self {
            search: f.search.clone(),
            race: f.race,
            outcome: f.outcome,
            date_range: f.date_range,
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
        || filter.date_range != DateRange::All;

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
}
