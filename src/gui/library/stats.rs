//! Derived aggregates over the library (winrate, MMR trend, matchup stats,
//! top maps). This is an explicit derived cache: the source of truth stays
//! in `ReplayLibrary::entries` — `compute_library_stats` is a pure O(n)
//! projection over those entries. The scanner owns invalidation via a
//! `stats_dirty` flag.

use std::collections::HashMap;

use crate::config::AppConfig;

use super::entry_row::{find_user_player, matchup_code};
use super::types::{LibraryEntry, MetaState};

#[derive(Clone, Debug, Default)]
pub struct MatchupStat {
    pub code: String,
    pub n: usize,
    pub wins: usize,
}

impl MatchupStat {
    pub fn winrate(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            self.wins as f32 / self.n as f32
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LibraryStats {
    pub total_parsed: usize,
    pub total_unsupported: usize,
    pub wins: usize,
    pub losses: usize,
    pub winrate_global: Option<f32>,
    pub mmr_latest: Option<i32>,
    pub mmr_trend_delta: Option<i32>,
    pub matchup_winrates: Vec<MatchupStat>,
    pub top_maps: Vec<(String, usize)>,
    pub best_matchup: Option<String>,
}

pub const MMR_TREND_WINDOW: usize = 7;
const BEST_MATCHUP_MIN_GAMES: usize = 5;
const TOP_MAPS_LIMIT: usize = 5;

pub fn compute_library_stats<'a, I>(entries: I, config: &AppConfig) -> LibraryStats
where
    I: IntoIterator<Item = &'a LibraryEntry>,
{
    let mut total_parsed = 0usize;
    let mut total_unsupported = 0usize;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut map_counts: HashMap<String, usize> = HashMap::new();
    let mut matchup_counts: HashMap<String, MatchupStat> = HashMap::new();
    let mut user_mmr_points: Vec<(String, i32)> = Vec::new();

    let has_nicknames = !config.user_nicknames.is_empty();

    for entry in entries {
        match &entry.meta {
            MetaState::Parsed(meta) => {
                total_parsed += 1;
                *map_counts.entry(meta.map.clone()).or_insert(0) += 1;

                if has_nicknames {
                    if let Some(user) = find_user_player(meta, config) {
                        match user.result.as_str() {
                            "Win" => wins += 1,
                            "Loss" => losses += 1,
                            _ => {}
                        }
                        let code = matchup_code(meta, config);
                        if !code.is_empty() {
                            let stat = matchup_counts
                                .entry(code.clone())
                                .or_insert_with(|| MatchupStat {
                                    code: code.clone(),
                                    n: 0,
                                    wins: 0,
                                });
                            stat.n += 1;
                            if user.result == "Win" {
                                stat.wins += 1;
                            }
                        }
                        if let Some(mmr) = user.mmr {
                            user_mmr_points.push((meta.datetime.clone(), mmr));
                        }
                    }
                }
            }
            MetaState::Unsupported(_) => total_unsupported += 1,
            MetaState::Pending | MetaState::Failed(_) => {}
        }
    }

    let decided = wins + losses;
    let winrate_global = if has_nicknames && decided > 0 {
        Some(wins as f32 / decided as f32)
    } else {
        None
    };

    // ISO 8601 datetimes are lexicographically orderable.
    user_mmr_points.sort_by(|a, b| b.0.cmp(&a.0));
    let mmr_latest = user_mmr_points.first().map(|(_, m)| *m);
    let mmr_trend_delta = if user_mmr_points.len() >= MMR_TREND_WINDOW {
        Some(user_mmr_points[0].1 - user_mmr_points[MMR_TREND_WINDOW - 1].1)
    } else {
        None
    };

    let mut maps_vec: Vec<(String, usize)> = map_counts.into_iter().collect();
    maps_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    maps_vec.truncate(TOP_MAPS_LIMIT);

    let mut matchups: Vec<MatchupStat> = matchup_counts.into_values().collect();
    matchups.sort_by(|a, b| a.code.cmp(&b.code));

    let best_matchup = matchups
        .iter()
        .filter(|m| m.n >= BEST_MATCHUP_MIN_GAMES)
        .max_by(|a, b| {
            a.winrate()
                .partial_cmp(&b.winrate())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|m| m.code.clone());

    LibraryStats {
        total_parsed,
        total_unsupported,
        wins,
        losses,
        winrate_global,
        mmr_latest,
        mmr_trend_delta,
        matchup_winrates: matchups,
        top_maps: maps_vec,
        best_matchup,
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{ParsedMeta, PlayerMeta};
    use super::*;
    use std::path::PathBuf;

    fn make_parsed(
        map: &str,
        datetime: &str,
        user_name: &str,
        user_race: &str,
        user_mmr: Option<i32>,
        user_result: &str,
        opp_race: &str,
        opp_mmr: Option<i32>,
    ) -> LibraryEntry {
        let opp_result = if user_result == "Win" { "Loss" } else { "Win" };
        LibraryEntry {
            path: PathBuf::from(format!("replay-{datetime}.SC2Replay")),
            filename: format!("replay-{datetime}.SC2Replay"),
            mtime: None,
            meta: MetaState::Parsed(ParsedMeta {
                map: map.into(),
                datetime: datetime.into(),
                duration_seconds: 600,
                game_loops: 10000,
                version: None,
                cache_handles: Vec::new(),
                players: vec![
                    PlayerMeta {
                        name: user_name.into(),
                        race: user_race.into(),
                        mmr: user_mmr,
                        result: user_result.into(),
                        opening: None,
                    },
                    PlayerMeta {
                        name: "opponent".into(),
                        race: opp_race.into(),
                        mmr: opp_mmr,
                        result: opp_result.into(),
                        opening: None,
                    },
                ],
            }),
        }
    }

    fn make_unsupported() -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from("u.SC2Replay"),
            filename: "u.SC2Replay".into(),
            mtime: None,
            meta: MetaState::Unsupported("not 1v1".into()),
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

    fn config_with_nick(nick: &str) -> AppConfig {
        let mut c = AppConfig::default();
        c.user_nicknames = vec![nick.into()];
        c
    }

    #[test]
    fn empty_entries_yields_zero_stats() {
        let cfg = config_with_nick("me");
        let s = compute_library_stats(&[], &cfg);
        assert_eq!(s.total_parsed, 0);
        assert_eq!(s.wins, 0);
        assert_eq!(s.losses, 0);
        assert_eq!(s.winrate_global, None);
        assert_eq!(s.mmr_latest, None);
        assert!(s.top_maps.is_empty());
        assert!(s.matchup_winrates.is_empty());
        assert_eq!(s.best_matchup, None);
    }

    #[test]
    fn wins_losses_and_winrate_from_user_perspective() {
        let cfg = config_with_nick("me");
        let entries = vec![
            make_parsed("Map1", "2026-04-10T10:00:00", "me", "Terran", Some(3900), "Win", "Zerg", Some(3800)),
            make_parsed("Map2", "2026-04-10T11:00:00", "me", "Terran", Some(3950), "Win", "Protoss", Some(3850)),
            make_parsed("Map3", "2026-04-10T12:00:00", "me", "Terran", Some(3900), "Loss", "Zerg", Some(4000)),
        ];
        let s = compute_library_stats(&entries, &cfg);
        assert_eq!(s.total_parsed, 3);
        assert_eq!(s.wins, 2);
        assert_eq!(s.losses, 1);
        assert!((s.winrate_global.unwrap() - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn unsupported_and_pending_are_separated_from_parsed() {
        let cfg = config_with_nick("me");
        let entries = vec![
            make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", None, "Win", "Zerg", None),
            make_unsupported(),
            make_unsupported(),
            make_pending(),
        ];
        let s = compute_library_stats(&entries, &cfg);
        assert_eq!(s.total_parsed, 1);
        assert_eq!(s.total_unsupported, 2);
        assert_eq!(s.wins, 1);
    }

    #[test]
    fn empty_nicknames_populates_only_totals_and_maps() {
        let cfg = AppConfig::default();
        let entries = vec![
            make_parsed("Map1", "2026-04-10T10:00:00", "a", "Terran", Some(3900), "Win", "Zerg", None),
            make_parsed("Map1", "2026-04-10T11:00:00", "a", "Terran", Some(3900), "Loss", "Zerg", None),
        ];
        let s = compute_library_stats(&entries, &cfg);
        assert_eq!(s.total_parsed, 2);
        assert_eq!(s.wins, 0);
        assert_eq!(s.losses, 0);
        assert_eq!(s.winrate_global, None);
        assert_eq!(s.mmr_latest, None);
        assert_eq!(s.top_maps.len(), 1);
        assert_eq!(s.top_maps[0], ("Map1".into(), 2));
        assert!(s.matchup_winrates.is_empty());
        assert_eq!(s.best_matchup, None);
    }

    #[test]
    fn mmr_trend_delta_requires_seven_points() {
        let cfg = config_with_nick("me");
        let mut entries = Vec::new();
        for i in 0..7 {
            entries.push(make_parsed(
                "M",
                &format!("2026-04-10T{:02}:00:00", i),
                "me",
                "Terran",
                Some(3900 + i as i32 * 10),
                "Win",
                "Zerg",
                None,
            ));
        }
        let s = compute_library_stats(&entries, &cfg);
        assert_eq!(s.mmr_latest, Some(3960));
        assert_eq!(s.mmr_trend_delta, Some(60));
    }

    #[test]
    fn mmr_trend_delta_absent_with_too_few_points() {
        let cfg = config_with_nick("me");
        let entries = vec![
            make_parsed("M", "2026-04-10T10:00:00", "me", "Terran", Some(3900), "Win", "Zerg", None),
            make_parsed("M", "2026-04-10T11:00:00", "me", "Terran", Some(3950), "Win", "Zerg", None),
        ];
        let s = compute_library_stats(&entries, &cfg);
        assert_eq!(s.mmr_latest, Some(3950));
        assert_eq!(s.mmr_trend_delta, None);
    }

    #[test]
    fn best_matchup_needs_five_games_minimum() {
        let cfg = config_with_nick("me");
        let mut entries = Vec::new();
        for i in 0..5 {
            entries.push(make_parsed(
                "M",
                &format!("2026-04-10T{:02}:00:00", i),
                "me",
                "Terran",
                None,
                "Win",
                "Zerg",
                None,
            ));
        }
        entries.push(make_parsed(
            "M",
            "2026-04-10T23:00:00",
            "me",
            "Terran",
            None,
            "Win",
            "Protoss",
            None,
        ));
        let s = compute_library_stats(&entries, &cfg);
        assert_eq!(s.best_matchup, Some("TvZ".into()));
    }

    #[test]
    fn accepts_pre_filtered_iterator() {
        let cfg = config_with_nick("me");
        let entries = vec![
            make_parsed("Map1", "2026-04-10T10:00:00", "me", "Terran", None, "Win", "Zerg", None),
            make_parsed("Map2", "2026-04-11T10:00:00", "me", "Terran", None, "Loss", "Zerg", None),
            make_parsed("Map3", "2026-04-12T10:00:00", "me", "Terran", None, "Win", "Zerg", None),
        ];
        // Simula um filtro outcome=Wins passando só vitórias à projeção.
        let filtered = entries.iter().filter(|e| match &e.meta {
            MetaState::Parsed(m) => m.players[0].result == "Win",
            _ => false,
        });
        let s = compute_library_stats(filtered, &cfg);
        assert_eq!(s.total_parsed, 2);
        assert_eq!(s.wins, 2);
        assert_eq!(s.losses, 0);
        assert!((s.winrate_global.unwrap() - 1.0).abs() < 1e-6);
        // Top maps só considera as duas vitórias.
        let names: Vec<&str> = s.top_maps.iter().map(|(m, _)| m.as_str()).collect();
        assert!(names.contains(&"Map1"));
        assert!(names.contains(&"Map3"));
        assert!(!names.contains(&"Map2"));
    }

    #[test]
    fn top_maps_sorted_by_count_desc_then_name_asc() {
        let cfg = config_with_nick("me");
        let mut entries = Vec::new();
        for _ in 0..3 {
            entries.push(make_parsed("MapB", "2026-04-10T10:00:00", "me", "Terran", None, "Win", "Zerg", None));
        }
        for _ in 0..3 {
            entries.push(make_parsed("MapA", "2026-04-10T10:00:00", "me", "Terran", None, "Win", "Zerg", None));
        }
        for _ in 0..1 {
            entries.push(make_parsed("MapC", "2026-04-10T10:00:00", "me", "Terran", None, "Win", "Zerg", None));
        }
        let s = compute_library_stats(&entries, &cfg);
        assert_eq!(s.top_maps.len(), 3);
        assert_eq!(s.top_maps[0], ("MapA".into(), 3));
        assert_eq!(s.top_maps[1], ("MapB".into(), 3));
        assert_eq!(s.top_maps[2], ("MapC".into(), 1));
    }
}
