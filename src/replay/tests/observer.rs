use super::*;

/// Regressão pra `build_user_to_player_idx`: o replay
/// `replay_observed.SC2Replay` foi gravado por um observador, então
/// `user_id` 1 e 3 são spectators e os jogadores reais ficam em
/// `user_id` 0 e 2. O parser antigo mapeava o spectator (user_id=1)
/// pro player_idx=1, e os ProductionCmds/InjectCmds/CameraPositions
/// do Terran (LiquidClem) eram perdidos.
#[test]
fn user_id_mapping_skips_observer_slots() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/replay_observed.SC2Replay");
    let t = parse_replay(&path, 0).expect("parse_replay");
    assert_eq!(t.players.len(), 2);
    let clem = t
        .players
        .iter()
        .find(|p| p.name.contains("Clem"))
        .expect("LiquidClem deve estar entre os jogadores");
    assert!(
        clem.camera_positions.len() > 500,
        "esperava centenas de amostras de câmera, achei {}",
        clem.camera_positions.len()
    );
    let first = clem.camera_positions.first().unwrap();
    assert!(
        first.game_loop < 100,
        "primeira amostra de câmera deve estar perto do início (loop < 100), achei {}",
        first.game_loop
    );
}
