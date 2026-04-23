//! Orquestrador do parser single-pass.
//!
//! `parse_replay` abre o MPQ uma vez, lê header/details/init_data/
//! tracker events/game events/message events na ordem certa e monta
//! o `ReplayTimeline` final. Os dois helpers privados decodificam
//! pedaços específicos de `InitData`/`Details` que não cabem em um
//! submódulo próprio.

use std::collections::HashMap;
use std::path::Path;

use crate::utils::{extract_clan_and_name, game_speed_to_loops_per_second};

use super::types::{PlayerTimeline, ReplayTimeline, Toon};
use super::{finalize, game, message, tracker};

/// Faz o parsing single-pass do replay e devolve um `ReplayTimeline`.
///
/// `max_time_seconds == 0` significa sem limite. `max_time_seconds == 1`
/// é um fast-path usado pela biblioteca da GUI: o parser retorna logo
/// após carregar metadados, sem decodificar tracker/message events.
pub fn parse_replay(path: &Path, max_time_seconds: u32) -> Result<ReplayTimeline, String> {
    let path_str = path.to_str().unwrap_or_default();

    let (mpq, file_contents) =
        s2protocol::read_mpq(path_str).map_err(|e| format!("{:?}", e))?;
    let (_, header) =
        s2protocol::read_protocol_header(&mpq).map_err(|e| format!("{:?}", e))?;
    let details =
        s2protocol::read_details(path_str, &mpq, &file_contents).map_err(|e| format!("{:?}", e))?;
    let init_data = s2protocol::read_init_data(path_str, &mpq, &file_contents).ok();

    let active_count = details.player_list.iter().filter(|p| p.observe == 0).count();
    if active_count < 2 {
        return Err("menos de 2 jogadores".to_string());
    }

    // Blizzard grava `time_local_offset` em unidades FILETIME como
    // `local - UTC` (negativo para oeste de Greenwich — um replay
    // gravado em São Paulo tem offset = -3h). `transform_to_naivetime`
    // do s2protocol SUBTRAI o offset em vez de somá-lo, então o valor
    // exibido fica a 2×offset de distância do horário local correto
    // (6h no futuro, no caso BR). Negamos o offset na entrada para
    // cancelar a subtração interna e obter de fato o horário local.
    let datetime = s2protocol::transform_to_naivetime(details.time_utc, -details.time_local_offset)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| "0000-00-00T00:00:00".to_string());

    let game_loops = header.m_elapsed_game_loops as u32;
    let base_build = header.m_version.m_base_build;
    let version = format!(
        "{}.{}.{}.{}",
        header.m_version.m_major,
        header.m_version.m_minor,
        header.m_version.m_revision,
        header.m_version.m_build,
    );
    let loops_per_second = game_speed_to_loops_per_second(&details.game_speed);

    // player_id (1-indexado no player_list completo) → índice no
    // vec de jogadores ativos.
    let player_idx: HashMap<u8, usize> = details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .enumerate()
        .map(|(out_idx, (in_idx, _))| ((in_idx + 1) as u8, out_idx))
        .collect();

    let players: Vec<PlayerTimeline> = details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .map(|(in_idx, p)| {
            let (clan, name) = extract_clan_and_name(&p.name);
            let mmr = init_data
                .as_ref()
                .and_then(|id| find_mmr_for_slot(id, p.working_set_slot_id));
            // `Toon::new` devolve `None` para `id == 0` (AI/computer).
            let toon = Toon::new(
                p.toon.region,
                p.toon.program_id,
                p.toon.realm,
                p.toon.id,
            );
            PlayerTimeline {
                name,
                clan,
                race: p.race.clone(),
                mmr,
                // `player_id` 1-baseado, casando com `player_idx` acima
                // e com o `killer_player_id` dos tracker events.
                player_id: (in_idx + 1) as u8,
                result: Some(p.result.clone()),
                toon,
                stats: Vec::new(),
                upgrades: Vec::new(),
                entity_events: Vec::new(),
                production_cmds: Vec::new(),
                inject_cmds: Vec::new(),
                unit_positions: Vec::new(),
                camera_positions: Vec::new(),
                alive_count: HashMap::new(),
                worker_capacity: Vec::new(),
                worker_births: Vec::new(),
                army_capacity: Vec::new(),
                army_productions: Vec::new(),
                worker_capacity_cumulative: Vec::new(),
                army_capacity_cumulative: Vec::new(),
                upgrade_cumulative: Vec::new(),
                creep_index: Vec::new(),
            }
        })
        .collect();

    // user_id (0-indexado em player_list completo) → display name
    // para correlacionar message events.
    let user_names: HashMap<i64, String> = details
        .player_list
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (_, name) = extract_clan_and_name(&p.name);
            (i as i64, name)
        })
        .collect();

    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let map = details.title.clone();
    let duration_seconds = (game_loops as f64 / loops_per_second).round() as u32;

    let cache_handles = details.cache_handles.clone();

    // Dimensões do mapa vêm do init_data; sem ele caem pra zero e a
    // aba Timeline cai num fallback de aspect 1:1 (raro — `init_data`
    // só falta em replays muito antigos ou corrompidos).
    let (map_size_x, map_size_y) = init_data
        .as_ref()
        .map(|id| {
            let gd = &id.sync_lobby_state.game_description;
            (gd.map_size_x, gd.map_size_y)
        })
        .unwrap_or((0, 0));

    let mut timeline = ReplayTimeline {
        file,
        map,
        datetime,
        game_loops,
        duration_seconds,
        loops_per_second,
        base_build,
        version,
        max_time_seconds,
        players,
        chat: Vec::new(),
        cache_handles,
        map_size_x,
        map_size_y,
        resources: Vec::new(),
    };

    // Fast path para metadata-only (usado pela biblioteca da GUI):
    // não decodificamos tracker/message events.
    if max_time_seconds == 1 {
        return Ok(timeline);
    }

    let max_loops = if max_time_seconds == 0 {
        0
    } else {
        (max_time_seconds as f64 * loops_per_second).round() as u32
    };

    let mut index_owner: tracker::IndexOwnerMap = HashMap::new();
    tracker::process_tracker_events(
        path_str,
        &mpq,
        &file_contents,
        &player_idx,
        &mut timeline.players,
        &mut index_owner,
        &mut timeline.resources,
        max_loops,
    )?;

    // user_id (0-baseado, vem dos game events) → player_idx (índice em
    // `timeline.players`). Necessário pra `game::process_game_events`
    // saber em qual player empurrar `ProductionCmd`/`InjectCmd`/câmera.
    //
    // Cuidado: o `user_id` NÃO corresponde à posição em `details.player_list`
    // (que só lista jogadores ativos). Em replays com observers no lobby,
    // o `user_id` 1 pode ser um spectator e o player real estar em `user_id` 2.
    // A fonte de verdade é `init_data.lobby_state.slots[i].user_id`,
    // casado com o jogador via `working_set_slot_id`.
    let user_to_player_idx: HashMap<i64, usize> = build_user_to_player_idx(
        init_data.as_ref(),
        &details,
    );

    game::process_game_events(
        path_str,
        &mpq,
        &file_contents,
        &user_to_player_idx,
        &index_owner,
        base_build,
        &mut timeline.players,
        max_loops,
    )?;

    message::process_message_events(
        path_str,
        &mpq,
        &file_contents,
        &user_names,
        max_loops,
        &mut timeline.chat,
    )?;

    finalize::finalize_indices(&mut timeline.players);

    Ok(timeline)
}

// ── user_id mapping ──────────────────────────────────────────────────

/// Constrói o mapa `user_id → player_idx` (índice em `timeline.players`).
///
/// Estratégia: cada jogador ativo em `details.player_list` tem um
/// `working_set_slot_id`; esse mesmo id aparece em
/// `init_data.lobby_state.slots[i].working_set_slot_id`, e o slot
/// guarda o `user_id` real que vai aparecer nos game events
/// (`ev.user_id`). A junção pelos dois lados produz o mapeamento
/// correto mesmo quando há observers no lobby (que ocupam slots e
/// "puxam" o user_id dos jogadores reais para fora do range 0..N).
///
/// Fallback (sem `init_data`): assume `user_id == in_idx` em
/// `details.player_list`. É o que o parser fazia antes — funciona
/// para a maioria dos replays sem observers, mas erra em replays
/// observadados (LiquidClem em torneios, p.ex.).
fn build_user_to_player_idx(
    init_data: Option<&s2protocol::InitData>,
    details: &s2protocol::details::Details,
) -> HashMap<i64, usize> {
    if let Some(init) = init_data {
        // working_set_slot_id → player_idx em `timeline.players`.
        let slot_to_player: HashMap<u8, usize> = details
            .player_list
            .iter()
            .filter(|p| p.observe == 0)
            .enumerate()
            .filter_map(|(out_idx, p)| p.working_set_slot_id.map(|s| (s, out_idx)))
            .collect();

        let map: HashMap<i64, usize> = init
            .sync_lobby_state
            .lobby_state
            .slots
            .iter()
            .filter_map(|s| {
                let uid = s.user_id?;
                let wsid = s.working_set_slot_id?;
                let &player_idx = slot_to_player.get(&wsid)?;
                Some((uid as i64, player_idx))
            })
            .collect();

        if !map.is_empty() {
            return map;
        }
    }

    // Fallback: comportamento antigo. Só roda se init_data faltou ou
    // se o cruzamento por slot não produziu nenhuma entrada (replays
    // muito antigos / corrompidos).
    details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .enumerate()
        .map(|(out_idx, (in_idx, _))| (in_idx as i64, out_idx))
        .collect()
}

// ── MMR lookup ──────────────────────────────────────────────────────

/// Encontra o `scaled_rating` de um jogador no InitData usando
/// `working_set_slot_id`. O índice em `user_initial_data` é a posição
/// do slot em `lobby_state.slots` cujo `working_set_slot_id` bate.
fn find_mmr_for_slot(
    init: &s2protocol::InitData,
    working_set_slot_id: Option<u8>,
) -> Option<i32> {
    let wsid = working_set_slot_id?;
    let slot_idx = init
        .sync_lobby_state
        .lobby_state
        .slots
        .iter()
        .position(|s| s.working_set_slot_id == Some(wsid))?;
    init.sync_lobby_state
        .user_initial_data
        .get(slot_idx)?
        .scaled_rating
}
