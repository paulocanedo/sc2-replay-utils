//! Medição de tempo do parse de replay por estágio. Usado pelo bin
//! `profile_replay` para diagnosticar gargalos sem subir a GUI.

use std::path::Path;
use std::time::Instant;

use crate::load_progress::LoadStage;
use crate::replay::parse_replay_from_bytes_with_progress;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProfileResult {
    pub read_ms: u128,
    pub header_ms: u128,
    pub tracker_ms: u128,
    pub game_ms: u128,
    pub message_ms: u128,
    pub total_ms: u128,
}

pub fn profile_parse(path: &Path) -> Result<ProfileResult, String> {
    let t_start = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let t_after_read = Instant::now();

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut t_header_end: Option<Instant> = None;
    let mut t_tracker_end: Option<Instant> = None;
    let mut t_game_end: Option<Instant> = None;

    let mut on_stage = |stage: LoadStage| {
        let now = Instant::now();
        match stage {
            LoadStage::DecodingTracker => t_header_end = Some(now),
            LoadStage::DecodingGame => t_tracker_end = Some(now),
            LoadStage::DecodingMessages => t_game_end = Some(now),
            _ => {}
        }
    };

    parse_replay_from_bytes_with_progress(&file_name, &bytes, 0, &mut on_stage)?;
    let t_end = Instant::now();

    let read_ms = (t_after_read - t_start).as_millis();
    let header_ms = t_header_end
        .map(|t| (t - t_after_read).as_millis())
        .unwrap_or(0);
    let tracker_ms = match (t_header_end, t_tracker_end) {
        (Some(a), Some(b)) => (b - a).as_millis(),
        _ => 0,
    };
    let game_ms = match (t_tracker_end, t_game_end) {
        (Some(a), Some(b)) => (b - a).as_millis(),
        _ => 0,
    };
    let message_ms = t_game_end
        .map(|t| (t_end - t).as_millis())
        .unwrap_or(0);
    let total_ms = (t_end - t_start).as_millis();

    Ok(ProfileResult {
        read_ms,
        header_ms,
        tracker_ms,
        game_ms,
        message_ms,
        total_ms,
    })
}
