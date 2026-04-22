// Walker do `replay.game.events` (stream que carrega Cmd events e
// SelectionDelta/ControlGroupUpdate). O propósito é colher os Cmd
// events de produção (Train/Build/Research/Morph) com o produtor
// real associado, alimentando `PlayerTimeline.production_cmds` que
// o `build_order` usa para corrigir tempos sob Chrono Boost.
//
// Como o Cmd não carrega o tag do produtor, reconstruímos a seleção
// ativa por user com a mesma lógica do s2protocol
// (`game_events::state::handle_selection_delta`/`update_control_group`):
// `SelectionDelta` para o `ACTIVE_UNITS_GROUP_IDX` substitui a seleção,
// `ControlGroupUpdate::ERecall` recupera um grupo salvo. Ignoramos
// outras variantes porque a heurística "primeiro tag selecionado é o
// produtor" basta para casar com a fila do tracker.
//
// O nome bruto da ação ("Marine", "Stimpack", …) é resolvido via
// `balance_data::resolve_ability_command`, que consulta a tabela
// `(producer, ability_id, cmd_index) → action_id` gerada em build
// time. Cmds que não casam com nenhuma ação de produção (movimentos,
// stop, attack, …) são silenciosamente descartados.

use std::collections::HashMap;

use s2protocol::game_events::{
    GameEControlGroupUpdate, GameSCmdData, GameSSelectionMask, ReplayGameEvent,
};
use s2protocol::tracker_events::unit_tag_index;

use crate::balance_data::resolve_ability_command;

use super::tracker::IndexOwnerMap;
use super::types::{CameraPosition, InjectCmd, PlayerTimeline, ProductionCmd};

/// Fator de escala das coordenadas "full" de game events → tile coords.
/// `GameSMapCoord3D` / `GameSPoint3D` usam fixed-point com 12 bits
/// fracionários (20 bits totais).
const POS_RATIO: i64 = 4096;

/// Fator de escala das coordenadas "mini" de game events → tile coords.
/// `GameSPointMini` (usado em CameraUpdate) usa fixed-point com 8 bits
/// fracionários (16 bits totais).
const POS_RATIO_MINI: i64 = 256;

/// Mesma constante usada por `s2protocol::state::ACTIVE_UNITS_GROUP_IDX`
/// — o slot 10 do array de control groups guarda a seleção ativa do
/// jogador (separada dos 0..9 hotkey-able).
const ACTIVE_UNITS_GROUP_IDX: usize = 10;

/// Estado da seleção por usuário. Cada índice de 0 a 10 representa um
/// control group; o slot 10 é a seleção ativa. Cada entrada é uma lista
/// de **tags completos** de unidades (`u32`, embora o SC2 use o nome
/// `unit_tag` para o valor `u32` que vem nos game events — diferente
/// do `i64` do tracker; conversão via `unit_tag_index`).
#[derive(Default)]
struct UserSelection {
    control_groups: Vec<Vec<u32>>,
}

impl UserSelection {
    fn new() -> Self {
        Self {
            control_groups: vec![Vec::new(); 11],
        }
    }

    fn active(&self) -> &[u32] {
        &self.control_groups[ACTIVE_UNITS_GROUP_IDX]
    }
}

pub(super) fn process_game_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    user_to_player_idx: &HashMap<i64, usize>,
    index_owner: &IndexOwnerMap,
    base_build: u32,
    players: &mut [PlayerTimeline],
    max_loops: u32,
) -> Result<(), String> {
    let game_events = match s2protocol::read_game_events(path_str, mpq, file_contents) {
        Ok(events) => events,
        // Replays muito antigos / corrompidos podem falhar aqui. Não é
        // crítico — só impacta a precisão de timing pra Protoss; o
        // resto do parse continua válido.
        Err(_) => return Ok(()),
    };

    let mut selections: HashMap<i64, UserSelection> = HashMap::new();
    let mut game_loop: i64 = 0;

    for ev in game_events {
        game_loop += ev.delta;
        if max_loops != 0 && game_loop as u32 > max_loops {
            break;
        }
        let user_id = ev.user_id;
        let selection = selections.entry(user_id).or_insert_with(UserSelection::new);

        match ev.event {
            ReplayGameEvent::SelectionDelta(delta) => {
                if delta.m_control_group_id as usize == ACTIVE_UNITS_GROUP_IDX {
                    // Atalho do s2protocol: substitui a seleção ativa
                    // pelos `m_add_unit_tags`. Não aplica `m_remove_mask`
                    // — o jogo só costuma emitir um set completo aqui,
                    // então casamento do produtor seguinte funciona.
                    selection.control_groups[ACTIVE_UNITS_GROUP_IDX] =
                        delta.m_delta.m_add_unit_tags;
                }
            }

            ReplayGameEvent::ControlGroupUpdate(cg) => {
                let idx = cg.m_control_group_index as usize;
                if idx >= selection.control_groups.len() {
                    continue;
                }
                match cg.m_control_group_update {
                    GameEControlGroupUpdate::ESet
                    | GameEControlGroupUpdate::ESetAndSteal => {
                        selection.control_groups[idx] =
                            selection.control_groups[ACTIVE_UNITS_GROUP_IDX].clone();
                    }
                    GameEControlGroupUpdate::EAppend
                    | GameEControlGroupUpdate::EAppendAndSteal => {
                        let mut current =
                            selection.control_groups[ACTIVE_UNITS_GROUP_IDX].clone();
                        selection.control_groups[idx].append(&mut current);
                        selection.control_groups[idx].sort_unstable();
                        selection.control_groups[idx].dedup();
                    }
                    GameEControlGroupUpdate::EClear => {
                        selection.control_groups[idx].clear();
                    }
                    GameEControlGroupUpdate::ERecall => {
                        // O `m_mask` aqui é uma deselect mask, mas para
                        // produção raramente importa — uma recall típica
                        // recupera o grupo inteiro. Aceitamos a
                        // imprecisão.
                        let _ = GameSSelectionMask::None;
                        selection.control_groups[ACTIVE_UNITS_GROUP_IDX] =
                            selection.control_groups[idx].clone();
                    }
                }
            }

            ReplayGameEvent::Cmd(cmd) => {
                let Some(abil) = cmd.m_abil else { continue };
                let Some(&player_idx) = user_to_player_idx.get(&user_id) else {
                    continue;
                };
                if selection.active().is_empty() {
                    continue;
                }
                // Pega o primeiro produtor selecionado — heurística do
                // SC2: o cmd vai pro primeiro prédio idle ou similar. A
                // imprecisão é absorvida pelo encadeamento por
                // finish_loop no build_order.
                let producer_tag = selection.active()[0] as i64;
                let Some(producer) = index_owner.get(&producer_tag) else {
                    continue;
                };
                if producer.player_idx != player_idx {
                    // Sanity: a unidade selecionada pertence ao mesmo
                    // jogador que emitiu o cmd. Caso contrário ignora —
                    // pode ser um spectate / shared control corner case.
                    continue;
                }
                let action = match resolve_ability_command(
                    &producer.unit_type,
                    abil.m_abil_link,
                    abil.m_abil_cmd_index,
                    base_build,
                ) {
                    Some(name) => name,
                    None => continue,
                };

                // SpawnLarva (Inject Larva) — captura separada com info
                // do alvo (Hatchery/Lair/Hive) extraída de m_data.
                if action == "SpawnLarva" {
                    if let GameSCmdData::TargetUnit(ref tu) = cmd.m_data {
                        let target_tag = tu.m_tag as i64;
                        let target_idx = unit_tag_index(target_tag);
                        let target_type = index_owner
                            .get(&target_tag)
                            .map(|e| e.unit_type.clone())
                            .unwrap_or_default();
                        let target_x = (tu.m_snapshot_point.x / POS_RATIO) as u8;
                        let target_y = (tu.m_snapshot_point.y / POS_RATIO) as u8;
                        players[player_idx].inject_cmds.push(InjectCmd {
                            game_loop: game_loop as u32,
                            target_tag_index: target_idx,
                            target_type,
                            target_x,
                            target_y,
                        });
                    }
                    continue;
                }

                players[player_idx].production_cmds.push(ProductionCmd {
                    game_loop: game_loop as u32,
                    ability: action.to_string(),
                    producer_tags: vec![producer_tag],
                    consumed: false,
                });
            }

            ReplayGameEvent::CameraUpdate(cam) => {
                let Some(&player_idx) = user_to_player_idx.get(&user_id) else {
                    continue;
                };
                let Some(ref target) = cam.m_target else {
                    continue;
                };
                let x = (target.x / POS_RATIO_MINI) as u8;
                let y = (target.y / POS_RATIO_MINI) as u8;
                // Dedup: skip se mesma posição do último sample.
                if let Some(last) = players[player_idx].camera_positions.last() {
                    if last.x == x && last.y == y {
                        continue;
                    }
                }
                players[player_idx].camera_positions.push(CameraPosition {
                    game_loop: game_loop as u32,
                    x,
                    y,
                });
            }

            _ => {}
        }
    }

    Ok(())
}
