use std::path::Path;

use crate::utils::extract_clan_and_name;

#[derive(serde::Serialize)]
pub struct PlayerData {
    pub name: String,
    pub clan: String,
    pub race: String,
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

pub fn parse_replay(path: &Path) -> Result<ReplayData, String> {
    let path_str = path.to_str().unwrap_or_default();

    let (mpq, file_contents) = s2protocol::read_mpq(path_str)
        .map_err(|e| format!("{:?}", e))?;

    let (_, header) = s2protocol::read_protocol_header(&mpq)
        .map_err(|e| format!("{:?}", e))?;

    let details = s2protocol::read_details(path_str, &mpq, &file_contents)
        .map_err(|e| format!("{:?}", e))?;

    let active_players: Vec<_> = details
        .player_list
        .iter()
        .filter(|p| p.observe == 0)
        .collect();

    if active_players.len() < 2 {
        return Err("menos de 2 jogadores".to_string());
    }

    let datetime =
        s2protocol::transform_to_naivetime(details.time_utc, details.time_local_offset)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
            .unwrap_or_else(|| "0000-00-00T00:00:00".to_string());

    let game_loops = header.m_elapsed_game_loops as u32;

    let players = active_players
        .iter()
        .map(|p| {
            let (clan, name) = extract_clan_and_name(&p.name);
            PlayerData { name, clan, race: p.race.clone() }
        })
        .collect();

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
