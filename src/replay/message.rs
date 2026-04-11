// Decodificação dos message events (apenas chat, no momento).

use std::collections::HashMap;

use s2protocol::message_events::{GameEMessageRecipient, ReplayMessageEvent};

use super::types::ChatEntry;

pub(super) fn process_message_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    user_names: &HashMap<i64, String>,
    max_loops: u32,
    chat: &mut Vec<ChatEntry>,
) -> Result<(), String> {
    let message_events = s2protocol::read_message_events(path_str, mpq, file_contents)
        .map_err(|e| format!("{:?}", e))?;

    let mut game_loop: i64 = 0;
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

        chat.push(ChatEntry {
            game_loop: loop_u32,
            player_name,
            recipient,
            message: msg.m_string,
        });
    }

    Ok(())
}
