// Helpers compartilhados entre cards da aba Insights.

/// Formata uma duração em segundos para leitura humana:
/// `<60s` → `"Ns"`; `≥60s` → `"mm:ss"`. Não cobre horas — replays
/// SC2 raramente passam de 60 min e `mm:ss` segura bem.
pub fn human_duration(total_secs: u32) -> String {
    if total_secs < 60 {
        format!("{total_secs}s")
    } else {
        format!("{}:{:02}", total_secs / 60, total_secs % 60)
    }
}
