use std::collections::HashMap;
use std::path::Path;

use s2protocol::tracker_events::{unit_tag, ReplayTrackerEvent};

use crate::utils::extract_clan_and_name;

// ── Structs de saída ─────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct StatsSnapshot {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    pub minerals: i32,
    pub vespene: i32,
    pub minerals_rate: i32,
    pub vespene_rate: i32,
    pub workers: i32,
    pub supply_used: i32,
    pub supply_made: i32,
    pub army_value_minerals: i32,
    pub army_value_vespene: i32,
    pub minerals_lost_army: i32,
    pub vespene_lost_army: i32,
    pub minerals_killed_army: i32,
    pub vespene_killed_army: i32,
}

#[derive(serde::Serialize)]
pub struct UpgradeEntry {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct UnitEntry {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    pub event: &'static str, // "born" | "init" | "done"
    pub unit_type: String,
    pub x: u8,
    pub y: u8,
}

#[derive(serde::Serialize)]
pub struct UnitLossEntry {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    pub unit_type: String,
    pub x: u8,
    pub y: u8,
    pub killer_player_id: Option<u8>,
}

#[derive(serde::Serialize)]
pub struct PlayerData {
    pub name: String,
    pub clan: String,
    pub race: String,
    pub stats_snapshots: Vec<StatsSnapshot>,
    pub upgrades: Vec<UpgradeEntry>,
    pub units: Vec<UnitEntry>,
    pub unit_losses: Vec<UnitLossEntry>,
}

#[derive(serde::Serialize)]
pub struct ReplayData {
    pub file: String,
    pub map: String,
    pub datetime: String,
    pub game_loops: u32,
    pub duration_seconds: u32,
    pub players: Vec<PlayerData>,
}

// ── Parser principal ─────────────────────────────────────────────────────────

pub fn parse_replay(path: &Path) -> Result<ReplayData, String> {
    let path_str = path.to_str().unwrap_or_default();

    let (mpq, file_contents) =
        s2protocol::read_mpq(path_str).map_err(|e| format!("{:?}", e))?;
    let (_, header) =
        s2protocol::read_protocol_header(&mpq).map_err(|e| format!("{:?}", e))?;
    let details =
        s2protocol::read_details(path_str, &mpq, &file_contents).map_err(|e| format!("{:?}", e))?;

    let active_players: Vec<_> = details.player_list.iter().filter(|p| p.observe == 0).collect();
    if active_players.len() < 2 {
        return Err("menos de 2 jogadores".to_string());
    }

    let datetime =
        s2protocol::transform_to_naivetime(details.time_utc, details.time_local_offset)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
            .unwrap_or_else(|| "0000-00-00T00:00:00".to_string());

    let game_loops = header.m_elapsed_game_loops as u32;

    // Mapeia player_id do tracker (posição 1-indexada no player_list completo)
    // → índice no Vec<PlayerData> (somente jogadores ativos)
    let player_idx: HashMap<u8, usize> = details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .enumerate()
        .map(|(out_idx, (in_idx, _))| ((in_idx + 1) as u8, out_idx))
        .collect();

    let mut players: Vec<PlayerData> = active_players
        .iter()
        .map(|p| {
            let (clan, name) = extract_clan_and_name(&p.name);
            PlayerData {
                name,
                clan,
                race: p.race.clone(),
                stats_snapshots: Vec::new(),
                upgrades: Vec::new(),
                units: Vec::new(),
                unit_losses: Vec::new(),
            }
        })
        .collect();

    process_tracker_events(path_str, &mpq, &file_contents, &player_idx, &mut players)?;

    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(ReplayData {
        file,
        map: details.title.clone(),
        datetime,
        game_loops,
        duration_seconds: game_loops / 16,
        players,
    })
}

// ── Processamento de tracker events ─────────────────────────────────────────

fn process_tracker_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    player_idx: &HashMap<u8, usize>,
    players: &mut Vec<PlayerData>,
) -> Result<(), String> {
    let tracker_events = s2protocol::read_tracker_events(path_str, mpq, file_contents)
        .map_err(|e| format!("{:?}", e))?;

    // tag → (unit_type_name, control_player_id, x, y)
    // necessário para resolver UnitDone e UnitDied, que não carregam o tipo
    let mut tag_map: HashMap<i64, (String, u8, u8, u8)> = HashMap::new();

    let mut game_loop: u32 = 0;

    for ev in tracker_events {
        game_loop += ev.delta;

        match ev.event {
            ReplayTrackerEvent::PlayerStats(e) => {
                let Some(&idx) = player_idx.get(&e.player_id) else { continue };
                let s = &e.stats;
                players[idx].stats_snapshots.push(StatsSnapshot {
                    game_loop,
                    minerals: s.minerals_current,
                    vespene: s.vespene_current,
                    minerals_rate: s.minerals_collection_rate,
                    vespene_rate: s.vespene_collection_rate,
                    workers: s.workers_active_count,
                    supply_used: s.food_used,
                    supply_made: s.food_made,
                    army_value_minerals: s.minerals_used_active_forces,
                    army_value_vespene: s.vespene_used_active_forces,
                    minerals_lost_army: s.minerals_lost_army,
                    vespene_lost_army: s.vespene_lost_army,
                    minerals_killed_army: s.minerals_killed_army,
                    vespene_killed_army: s.vespene_killed_army,
                });
            }

            ReplayTrackerEvent::Upgrade(e) => {
                let Some(&idx) = player_idx.get(&e.player_id) else { continue };
                players[idx].upgrades.push(UpgradeEntry {
                    game_loop,
                    name: e.upgrade_type_name,
                });
            }

            ReplayTrackerEvent::UnitBorn(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                tag_map.insert(tag, (e.unit_type_name.clone(), e.control_player_id, e.x, e.y));
                let Some(&idx) = player_idx.get(&e.control_player_id) else { continue };
                players[idx].units.push(UnitEntry {
                    game_loop,
                    event: "born",
                    unit_type: e.unit_type_name,
                    x: e.x,
                    y: e.y,
                });
            }

            ReplayTrackerEvent::UnitInit(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                tag_map.insert(tag, (e.unit_type_name.clone(), e.control_player_id, e.x, e.y));
                let Some(&idx) = player_idx.get(&e.control_player_id) else { continue };
                players[idx].units.push(UnitEntry {
                    game_loop,
                    event: "init",
                    unit_type: e.unit_type_name,
                    x: e.x,
                    y: e.y,
                });
            }

            ReplayTrackerEvent::UnitDone(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some((unit_type, pid, x, y)) = tag_map.get(&tag) else { continue };
                let Some(&idx) = player_idx.get(pid) else { continue };
                players[idx].units.push(UnitEntry {
                    game_loop,
                    event: "done",
                    unit_type: unit_type.clone(),
                    x: *x,
                    y: *y,
                });
            }

            ReplayTrackerEvent::UnitDied(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some((unit_type, pid, _, _)) = tag_map.get(&tag) else { continue };
                let Some(&idx) = player_idx.get(pid) else { continue };
                players[idx].unit_losses.push(UnitLossEntry {
                    game_loop,
                    unit_type: unit_type.clone(),
                    x: e.x,
                    y: e.y,
                    killer_player_id: e.killer_player_id,
                });
            }

            _ => {}
        }
    }

    Ok(())
}
