use std::collections::HashMap;
use std::path::Path;

use s2protocol::message_events::{GameEMessageRecipient, ReplayMessageEvent};

use crate::utils::{extract_clan_and_name, game_speed_to_loops_per_second};

pub struct ChatEntry {
    pub game_loop: u32,
    pub player_name: String,
    pub recipient: String,
    pub message: String,
}

pub struct ChatResult {
    pub datetime: String,
    pub map_name: String,
    pub loops_per_second: f64,
    pub entries: Vec<ChatEntry>,
}

pub fn extract_chat(path: &Path, max_time_seconds: u32) -> Result<ChatResult, String> {
    let path_str = path.to_str().unwrap_or_default();

    let (mpq, file_contents) =
        s2protocol::read_mpq(path_str).map_err(|e| format!("{:?}", e))?;
    let details =
        s2protocol::read_details(path_str, &mpq, &file_contents).map_err(|e| format!("{:?}", e))?;

    let loops_per_second = game_speed_to_loops_per_second(&details.game_speed);

    let datetime =
        s2protocol::transform_to_naivetime(details.time_utc, details.time_local_offset)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
            .unwrap_or_else(|| "0000-00-00T00:00:00".to_string());

    // user_id (1-based index in player_list) → display name
    let user_names: HashMap<i64, String> = details
        .player_list
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (_, name) = extract_clan_and_name(&p.name);
            ((i + 1) as i64, name)
        })
        .collect();

    let max_loops = if max_time_seconds == 0 {
        0u32
    } else {
        (max_time_seconds as f64 * loops_per_second).round() as u32
    };

    let message_events =
        s2protocol::read_message_events(path_str, &mpq, &file_contents)
            .map_err(|e| format!("{:?}", e))?;

    let mut game_loop: i64 = 0;
    let mut entries: Vec<ChatEntry> = Vec::new();

    for ev in message_events {
        game_loop += ev.delta;
        let loop_u32 = game_loop.max(0) as u32;
        if max_loops != 0 && loop_u32 > max_loops {
            break;
        }

        let ReplayMessageEvent::EChat(msg) = ev.event;
        let player_name = user_names
            .get(&ev.user_id)
            .cloned()
            .unwrap_or_else(|| format!("Player{}", ev.user_id));

        let recipient = match msg.m_recipient {
            GameEMessageRecipient::EAll => "all",
            GameEMessageRecipient::EAllies => "allies",
            GameEMessageRecipient::EIndividual => "individual",
            GameEMessageRecipient::EBattlenet => "battlenet",
            GameEMessageRecipient::EObservers => "observers",
        }
        .to_string();

        entries.push(ChatEntry {
            game_loop: loop_u32,
            player_name,
            recipient,
            message: msg.m_string,
        });
    }

    Ok(ChatResult {
        datetime,
        map_name: details.title,
        loops_per_second,
        entries,
    })
}

pub fn to_chat_txt(result: &ChatResult, player_names: (&str, &str)) -> String {
    let mut lines: Vec<String> = Vec::new();

    let date = &result.datetime[..10]; // "YYYY-MM-DD"
    lines.push(format!(
        "# Chat – {} {} – {} vs {}",
        date, result.map_name, player_names.0, player_names.1
    ));
    lines.push(String::new());

    if result.entries.is_empty() {
        lines.push("(sem mensagens de chat)".to_string());
    } else {
        for entry in &result.entries {
            let secs = (entry.game_loop as f64 / result.loops_per_second).round() as u32;
            let mm = secs / 60;
            let ss = secs % 60;
            lines.push(format!(
                "[{:02}:{:02}] {} ({}): {}",
                mm, ss, entry.player_name, entry.recipient, entry.message
            ));
        }
    }

    lines.join("\n") + "\n"
}
