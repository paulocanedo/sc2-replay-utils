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
