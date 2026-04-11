use std::collections::HashMap;
use std::path::Path;

use s2protocol::tracker_events::{unit_tag, ReplayTrackerEvent};

use crate::build_order::format_time;
use crate::utils::{extract_clan_and_name, game_speed_to_loops_per_second};

// ── Constantes ───────────────────────────────────────────────────────────────

/// Tempo de produção de um worker em game loops (~12s em Faster).
const WORKER_BUILD_TIME: u32 = 272;

/// Mínimo de game loops ociosos para registrar um gap.
const MIN_IDLE_LOOPS: u32 = 20;

/// Tempo de morph CC → Orbital Command em game loops (~25s em Faster).
const ORBITAL_MORPH_TIME: u32 = 560;

/// Tempo de morph CC → Planetary Fortress em game loops (~36s em Faster).
const PF_MORPH_TIME: u32 = 806;

// ── Structs de saída ─────────────────────────────────────────────────────────

pub struct ProductionGapEntry {
    pub start_loop: u32,
    pub end_loop: u32,
    pub capacity: u32,
    pub active: u32,
    pub idle_slots: u32,
}

pub struct PlayerProductionGap {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub entries: Vec<ProductionGapEntry>,
    pub is_zerg: bool,
    pub total_idle_loops: u32,
    pub efficiency_pct: f64,
}

pub struct ProductionGapResult {
    pub players: Vec<PlayerProductionGap>,
    pub game_loops: u32,
    pub loops_per_second: f64,
    pub datetime: String,
    pub map_name: String,
}

// ── Classificação ────────────────────────────────────────────────────────────

fn is_worker_producer(unit_type: &str) -> bool {
    matches!(
        unit_type,
        "CommandCenter" | "OrbitalCommand" | "PlanetaryFortress" | "Nexus"
    )
}

fn is_worker_type(unit_type: &str) -> bool {
    matches!(unit_type, "SCV" | "Probe")
}

fn is_zerg_race(race: &str) -> bool {
    race.starts_with('Z') || race.starts_with('z')
}

/// Retorna o tempo de morph em game loops para estruturas produtoras de workers.
fn morph_build_time(unit_type: &str) -> u32 {
    match unit_type {
        "OrbitalCommand" => ORBITAL_MORPH_TIME,
        "PlanetaryFortress" => PF_MORPH_TIME,
        _ => 0,
    }
}

// ── Extração ─────────────────────────────────────────────────────────────────

pub fn extract_production_gaps(
    path: &Path,
    max_time_seconds: u32,
) -> Result<ProductionGapResult, String> {
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

    let datetime = s2protocol::transform_to_naivetime(details.time_utc, details.time_local_offset)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| "0000-00-00T00:00:00".to_string());
    let map_name = details.title.clone();
    let loops_per_second = game_speed_to_loops_per_second(&details.game_speed);
    let game_loops = header.m_elapsed_game_loops as u32;

    let player_idx: HashMap<u8, usize> = details
        .player_list
        .iter()
        .enumerate()
        .filter(|(_, p)| p.observe == 0)
        .enumerate()
        .map(|(out_idx, (in_idx, _))| ((in_idx + 1) as u8, out_idx))
        .collect();

    let tracker_events = s2protocol::read_tracker_events(path_str, &mpq, &file_contents)
        .map_err(|e| format!("{:?}", e))?;

    let max_loops = if max_time_seconds == 0 {
        0
    } else {
        (max_time_seconds as f64 * loops_per_second).round() as u32
    };

    // tag → (unit_type_name, control_player_id)
    let mut tag_map: HashMap<i64, (String, u8)> = HashMap::new();

    // Por jogador: timestamps de nascimento de workers e eventos de capacidade
    let mut worker_births: Vec<Vec<u32>> = (0..active_count).map(|_| Vec::new()).collect();
    let mut capacity_events: Vec<Vec<(u32, i32)>> = (0..active_count).map(|_| Vec::new()).collect();

    // Tags de estruturas que foram iniciadas (UnitInit) mas ainda não concluídas (UnitDone)
    let mut pending_structures: HashMap<i64, usize> = HashMap::new();

    let mut game_loop: u32 = 0;

    for ev in tracker_events {
        game_loop += ev.delta;
        if max_loops != 0 && game_loop > max_loops {
            break;
        }

        match ev.event {
            ReplayTrackerEvent::UnitBorn(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                tag_map.insert(tag, (e.unit_type_name.clone(), e.control_player_id));

                let Some(&idx) = player_idx.get(&e.control_player_id) else {
                    continue;
                };

                // Worker nasceu (produção concluída)
                if is_worker_type(&e.unit_type_name) {
                    let ability = e.creator_ability_name.as_deref().unwrap_or("");
                    if ability.contains("Train") {
                        worker_births[idx].push(game_loop);
                    }
                }

                // Estrutura produtora apareceu (inicial ou construída via UnitInit→UnitDone)
                // Morphs (CC→Orbital) são tratados em UnitTypeChange, não aqui.
                if is_worker_producer(&e.unit_type_name) {
                    capacity_events[idx].push((game_loop, 1));
                }
            }

            ReplayTrackerEvent::UnitInit(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                tag_map.insert(tag, (e.unit_type_name.clone(), e.control_player_id));

                // Estrutura sendo construída — só conta quando UnitDone
                if is_worker_producer(&e.unit_type_name) {
                    if let Some(&idx) = player_idx.get(&e.control_player_id) {
                        pending_structures.insert(tag, idx);
                    }
                }
            }

            ReplayTrackerEvent::UnitDone(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                // Estrutura concluída — agora pode produzir
                if let Some(idx) = pending_structures.remove(&tag) {
                    capacity_events[idx].push((game_loop, 1));
                }
            }

            ReplayTrackerEvent::UnitDied(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                // Estrutura pendente destruída antes de ficar pronta
                pending_structures.remove(&tag);

                let Some((unit_type, pid)) = tag_map.get(&tag) else {
                    continue;
                };
                if is_worker_producer(unit_type) {
                    let Some(&idx) = player_idx.get(pid) else {
                        continue;
                    };
                    capacity_events[idx].push((game_loop, -1));
                }
            }

            ReplayTrackerEvent::UnitTypeChange(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let old_type = tag_map.get(&tag).cloned();
                tag_map.insert(tag, (e.unit_type_name.clone(),
                    old_type.as_ref().map(|(_, pid)| *pid).unwrap_or(0)));

                let Some((ref old_name, pid)) = old_type else { continue };
                let Some(&idx) = player_idx.get(&pid) else { continue };

                let old_is_prod = is_worker_producer(old_name);
                let new_is_prod = is_worker_producer(&e.unit_type_name);

                match (old_is_prod, new_is_prod) {
                    (true, true) => {
                        // CC → Orbital/PF: mesma capacidade, mas downtime durante o morph.
                        let mt = morph_build_time(&e.unit_type_name);
                        if mt > 0 {
                            let morph_start = game_loop.saturating_sub(mt);
                            capacity_events[idx].push((morph_start, -1));
                            capacity_events[idx].push((game_loop, 1));
                        }
                    }
                    (true, false) => {
                        // Produtora → não-produtora (ex: OrbitalCommand → OrbitalCommandFlying)
                        capacity_events[idx].push((game_loop, -1));
                    }
                    (false, true) => {
                        // Não-produtora → produtora (ex: OrbitalCommandFlying → OrbitalCommand)
                        capacity_events[idx].push((game_loop, 1));
                    }
                    (false, false) => {}
                }
            }

            _ => {}
        }
    }

    // Limite efetivo do jogo
    let effective_end = if max_loops == 0 {
        game_loops
    } else {
        game_loops.min(max_loops)
    };

    // Construir resultado por jogador
    let player_meta: Vec<(String, String, Option<i32>)> = details
        .player_list
        .iter()
        .filter(|p| p.observe == 0)
        .map(|p| {
            let (_, name) = extract_clan_and_name(&p.name);
            let mmr = init_data
                .as_ref()
                .and_then(|id| find_mmr_for_slot(id, p.working_set_slot_id));
            (name, p.race.clone(), mmr)
        })
        .collect();

    let players = player_meta
        .into_iter()
        .enumerate()
        .map(|(i, (name, race, mmr))| {
            if is_zerg_race(&race) {
                return PlayerProductionGap {
                    name,
                    race,
                    mmr,
                    entries: Vec::new(),
                    is_zerg: true,
                    total_idle_loops: 0,
                    efficiency_pct: 0.0,
                };
            }

            let (entries, total_idle, efficiency) = compute_idle_periods(
                &worker_births[i],
                &capacity_events[i],
                effective_end,
            );

            PlayerProductionGap {
                name,
                race,
                mmr,
                entries,
                is_zerg: false,
                total_idle_loops: total_idle,
                efficiency_pct: efficiency,
            }
        })
        .collect();

    Ok(ProductionGapResult {
        players,
        game_loops,
        loops_per_second,
        datetime,
        map_name,
    })
}

// ── Cálculo de períodos ociosos ──────────────────────────────────────────────

/// Tipos de evento na timeline unificada.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EvKind {
    CapacityUp,
    ProdStart,
    ProdEnd,
    CapacityDown,
}

impl EvKind {
    fn order(self) -> u8 {
        match self {
            EvKind::CapacityUp => 0,
            EvKind::ProdStart => 1,
            EvKind::ProdEnd => 2,
            EvKind::CapacityDown => 3,
        }
    }
}

fn compute_idle_periods(
    worker_births: &[u32],
    capacity_events: &[(u32, i32)],
    game_end: u32,
) -> (Vec<ProductionGapEntry>, u32, f64) {
    // Montar timeline de eventos
    let mut timeline: Vec<(u32, EvKind)> = Vec::new();

    for &(gl, delta) in capacity_events {
        if delta > 0 {
            timeline.push((gl, EvKind::CapacityUp));
        } else {
            timeline.push((gl, EvKind::CapacityDown));
        }
    }

    for &birth in worker_births {
        let start = birth.saturating_sub(WORKER_BUILD_TIME);
        timeline.push((start, EvKind::ProdStart));
        timeline.push((birth, EvKind::ProdEnd));
    }

    // Ordenar: por game_loop, desempate por ordem do tipo
    timeline.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.order().cmp(&b.1.order())));

    let mut capacity: i32 = 0;
    let mut active: i32 = 0;
    let mut entries: Vec<ProductionGapEntry> = Vec::new();

    // Estado do gap aberto
    let mut gap_start: Option<(u32, u32, u32)> = None; // (start_loop, capacity, active)

    // Para cálculo de eficiência: acumular (capacity_slot_time, idle_slot_time)
    let mut sum_capacity_time: u64 = 0;
    let mut sum_idle_time: u64 = 0;
    let mut prev_loop: u32 = 0;

    for &(gl, kind) in &timeline {
        if gl > game_end {
            break;
        }

        // Acumular tempos do segmento anterior
        let dt = (gl.min(game_end) - prev_loop) as u64;
        if capacity > 0 && dt > 0 {
            sum_capacity_time += capacity as u64 * dt;
            let idle = (capacity - active).max(0) as u64;
            sum_idle_time += idle * dt;
        }

        // Atualizar contadores
        match kind {
            EvKind::CapacityUp => capacity += 1,
            EvKind::CapacityDown => capacity = (capacity - 1).max(0),
            EvKind::ProdStart => active += 1,
            EvKind::ProdEnd => active = (active - 1).max(0),
        }

        let idle_now = (capacity - active.min(capacity)).max(0);

        // Gerenciar gaps
        if idle_now > 0 && capacity > 0 {
            if gap_start.is_none() {
                gap_start = Some((gl, capacity as u32, active.max(0) as u32));
            }
        } else if let Some((start, cap, act)) = gap_start.take() {
            if gl.saturating_sub(start) >= MIN_IDLE_LOOPS {
                entries.push(ProductionGapEntry {
                    start_loop: start,
                    end_loop: gl,
                    capacity: cap,
                    active: act,
                    idle_slots: cap.saturating_sub(act),
                });
            }
        }

        prev_loop = gl;
    }

    // Segmento final até game_end
    let dt = game_end.saturating_sub(prev_loop) as u64;
    if capacity > 0 && dt > 0 {
        sum_capacity_time += capacity as u64 * dt;
        let idle = (capacity - active).max(0) as u64;
        sum_idle_time += idle * dt;
    }

    // Fechar gap aberto
    if let Some((start, cap, act)) = gap_start.take() {
        if game_end.saturating_sub(start) >= MIN_IDLE_LOOPS {
            entries.push(ProductionGapEntry {
                start_loop: start,
                end_loop: game_end,
                capacity: cap,
                active: act,
                idle_slots: cap.saturating_sub(act),
            });
        }
    }

    let total_idle: u32 = entries
        .iter()
        .map(|e| e.end_loop.saturating_sub(e.start_loop))
        .sum();

    let efficiency = if sum_capacity_time > 0 {
        100.0 * (1.0 - sum_idle_time as f64 / sum_capacity_time as f64)
    } else {
        100.0
    };

    (entries, total_idle, efficiency)
}

// ── Formatação CSV ───────────────────────────────────────────────────────────

pub fn to_production_gap_csv(
    player: &PlayerProductionGap,
    lps: f64,
) -> String {
    if player.is_zerg {
        return "[Zerg] Análise de produção de workers não suportada — mecânica de larvas requer implementação futura.\n"
            .to_string();
    }

    let entries = &player.entries;

    let rows: Vec<(String, String, String, String, String, String)> = entries
        .iter()
        .map(|e| {
            let duration = e.end_loop.saturating_sub(e.start_loop);
            (
                format_time(e.start_loop, lps),
                format_time(e.end_loop, lps),
                format_time(duration, lps),
                e.capacity.to_string(),
                e.active.to_string(),
                e.idle_slots.to_string(),
            )
        })
        .collect();

    let w_start = rows.iter().map(|r| r.0.len()).max().unwrap_or(0).max("start".len());
    let w_end = rows.iter().map(|r| r.1.len()).max().unwrap_or(0).max("end".len());
    let w_dur = rows.iter().map(|r| r.2.len()).max().unwrap_or(0).max("duration".len());
    let w_cap = rows.iter().map(|r| r.3.len()).max().unwrap_or(0).max("capacity".len());
    let w_act = rows.iter().map(|r| r.4.len()).max().unwrap_or(0).max("active".len());
    let w_idle = rows.iter().map(|r| r.5.len()).max().unwrap_or(0).max("idle_slots".len());

    let mut out = String::new();
    out.push_str(&format!(
        "{:<w_start$}, {:<w_end$}, {:<w_dur$}, {:<w_cap$}, {:<w_act$}, {:<w_idle$}\n",
        "start", "end", "duration", "capacity", "active", "idle_slots",
        w_start = w_start, w_end = w_end, w_dur = w_dur,
        w_cap = w_cap, w_act = w_act, w_idle = w_idle,
    ));
    for (start, end, dur, cap, act, idle) in &rows {
        out.push_str(&format!(
            "{:<w_start$}, {:<w_end$}, {:<w_dur$}, {:<w_cap$}, {:<w_act$}, {:<w_idle$}\n",
            start, end, dur, cap, act, idle,
            w_start = w_start, w_end = w_end, w_dur = w_dur,
            w_cap = w_cap, w_act = w_act, w_idle = w_idle,
        ));
    }

    out.push_str(&format!(
        "\nTotal idle: {} | Avg efficiency: {:.1}%\n",
        format_time(player.total_idle_loops, lps),
        player.efficiency_pct,
    ));

    out
}

// ── MMR lookup ───────────────────────────────────────────────────────────────

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
