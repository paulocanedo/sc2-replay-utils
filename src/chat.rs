// Camada fina sobre `ReplayTimeline`. O parser single-pass já carrega
// e popula `timeline.chat`; aqui só montamos o `ChatResult` com a
// metadata extra que os consumers (CSV/GUI) esperam. `ChatEntry` em
// si vive em `replay.rs` para evitar dependência cíclica.

pub use crate::replay::ChatEntry;
use crate::replay::ReplayTimeline;

pub struct ChatResult {
    pub datetime: String,
    pub map_name: String,
    pub loops_per_second: f64,
    pub entries: Vec<ChatEntry>,
}

pub fn extract_chat(timeline: &ReplayTimeline) -> Result<ChatResult, String> {
    Ok(ChatResult {
        datetime: timeline.datetime.clone(),
        map_name: timeline.map.clone(),
        loops_per_second: timeline.loops_per_second,
        entries: timeline.chat.clone(),
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
