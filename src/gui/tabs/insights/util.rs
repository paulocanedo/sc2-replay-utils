// Helpers compartilhados entre cards da aba Insights.

use crate::replay_state::LoadedReplay;

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

/// Verifica se a partida é 1v1 com as duas raças iguais (TvT/PvP/ZvZ).
/// Retorna `Some((opp_idx, race_letter))` quando sim, `None` quando
/// não. `race_letter` é a primeira letra da raça em maiúscula ('T',
/// 'P' ou 'Z').
pub fn mirror_match(loaded: &LoadedReplay, player_idx: usize) -> Option<(usize, char)> {
    if loaded.timeline.players.len() != 2 {
        return None;
    }
    let pov = loaded.timeline.players.get(player_idx)?;
    let opp_idx = if player_idx == 0 { 1 } else { 0 };
    let opp = loaded.timeline.players.get(opp_idx)?;
    let pov_r = pov.race.chars().next()?.to_ascii_uppercase();
    let opp_r = opp.race.chars().next()?.to_ascii_uppercase();
    if pov_r != opp_r {
        return None;
    }
    if !matches!(pov_r, 'T' | 'P' | 'Z') {
        return None;
    }
    Some((opp_idx, pov_r))
}

/// Formata diff de loops como tempo sinalizado (+0:10, -0:05, 0:00).
pub fn format_signed_time(delta_loops: i64, lps: f64) -> String {
    let sign = if delta_loops > 0 {
        "+"
    } else if delta_loops < 0 {
        "-"
    } else {
        ""
    };
    let secs = (delta_loops.unsigned_abs() as f64 / lps).round() as u32;
    format!("{sign}{}:{:02}", secs / 60, secs % 60)
}
