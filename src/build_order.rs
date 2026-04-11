use std::collections::HashMap;
use std::path::Path;

use s2protocol::tracker_events::ReplayTrackerEvent;

use crate::utils::{extract_clan_and_name, game_speed_to_loops_per_second};

// ── Structs de saída ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct BuildOrderEntry {
    pub supply: u8,
    pub game_loop: u32,
    pub action: String,
    pub count: u32,
    pub is_upgrade: bool,
    pub is_structure: bool,
}

pub struct PlayerBuildOrder {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub entries: Vec<BuildOrderEntry>,
}

pub struct BuildOrderResult {
    pub players: Vec<PlayerBuildOrder>,
    pub datetime: String,
    pub map_name: String,
    pub loops_per_second: f64,
}

// ── Extração ──────────────────────────────────────────────────────────────────

/// Extrai a Build Order de cada jogador ativo.
pub fn extract_build_order(
    path: &Path,
    max_time_seconds: u32,
) -> Result<BuildOrderResult, String> {
    let path_str = path.to_str().unwrap_or_default();

    let (mpq, file_contents) =
        s2protocol::read_mpq(path_str).map_err(|e| format!("{:?}", e))?;
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

    // player_id (1-indexado no player_list completo) → índice no vec de jogadores ativos
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

    let max_loops = if max_time_seconds == 0 { 0 } else { (max_time_seconds as f64 * loops_per_second).round() as u32 };

    // supply atual por player_id (atualizado em cada PlayerStats)
    let mut supply_now: HashMap<u8, u8> = HashMap::new();
    // entradas brutas por jogador, antes da deduplicação
    let mut raw: Vec<Vec<BuildOrderEntry>> = (0..active_count).map(|_| Vec::new()).collect();

    let mut game_loop: u32 = 0;

    for ev in tracker_events {
        game_loop += ev.delta;
        if max_loops != 0 && game_loop > max_loops {
            break;
        }
        if game_loop == 0 {
            continue;
        }

        match ev.event {
            ReplayTrackerEvent::PlayerStats(e) => {
                supply_now.insert(e.player_id, e.stats.food_used as u8);
            }

            ReplayTrackerEvent::UnitBorn(e) => {
                let ability = match &e.creator_ability_name {
                    Some(a) if !a.is_empty() => a,
                    _ => continue,
                };
                // Keep only real training (ability contains "Train") or morphs ("MorphTo*")
                if !ability.contains("Train") && !ability.starts_with("MorphTo") {
                    continue;
                }
                let Some(&idx) = player_idx.get(&e.control_player_id) else { continue };
                let supply = *supply_now.get(&e.control_player_id).unwrap_or(&0);
                // MorphTo pode produzir estruturas (ex: OrbitalCommand) ou unidades (ex: Hellbat)
                let is_structure = ability.starts_with("MorphTo") && is_structure_name(&e.unit_type_name);
                raw[idx].push(BuildOrderEntry {
                    supply,
                    game_loop,
                    action: e.unit_type_name,
                    count: 1,
                    is_upgrade: false,
                    is_structure,
                });
            }

            ReplayTrackerEvent::UnitInit(e) => {
                // Exclude cosmetics and tactical placements, not build order
                if e.unit_type_name.contains("Tumor") || e.unit_type_name.contains("Spray") {
                    continue;
                }
                let Some(&idx) = player_idx.get(&e.control_player_id) else { continue };
                let supply = *supply_now.get(&e.control_player_id).unwrap_or(&0);
                // UnitInit é sempre uma construção sendo iniciada
                raw[idx].push(BuildOrderEntry {
                    supply,
                    game_loop,
                    action: e.unit_type_name,
                    count: 1,
                    is_upgrade: false,
                    is_structure: true,
                });
            }

            ReplayTrackerEvent::Upgrade(e) => {
                if e.upgrade_type_name.contains("Spray") {
                    continue;
                }
                let Some(&idx) = player_idx.get(&e.player_id) else { continue };
                let supply = *supply_now.get(&e.player_id).unwrap_or(&0);
                raw[idx].push(BuildOrderEntry {
                    supply,
                    game_loop,
                    action: e.upgrade_type_name,
                    count: 1,
                    is_upgrade: true,
                    is_structure: false,
                });
            }

            _ => {}
        }
    }

    let player_meta: Vec<(String, String, Option<i32>)> = details
        .player_list
        .iter()
        .filter(|p| p.observe == 0)
        .map(|p| {
            let (_, name) = extract_clan_and_name(&p.name);
            let mmr = init_data.as_ref()
                .and_then(|id| find_mmr_for_slot(id, p.working_set_slot_id));
            (name, p.race.clone(), mmr)
        })
        .collect();

    let players = raw
        .into_iter()
        .map(deduplicate)
        .enumerate()
        .map(|(i, entries)| {
            let (name, race, mmr) = player_meta.get(i).cloned().unwrap_or_default();
            PlayerBuildOrder { name, race, mmr, entries }
        })
        .collect();

    Ok(BuildOrderResult { players, datetime, map_name, loops_per_second })
}

/// Funde entradas consecutivas com a mesma ação em uma única com `count` incrementado.
fn deduplicate(entries: Vec<BuildOrderEntry>) -> Vec<BuildOrderEntry> {
    let mut out: Vec<BuildOrderEntry> = Vec::new();
    for entry in entries {
        match out.last_mut() {
            Some(last) if last.action == entry.action => last.count += 1,
            _ => out.push(entry),
        }
    }
    out
}

// ── Classificação de entradas ─────────────────────────────────────────────────

/// Categoria de uma entrada do build order. `Worker` é um subtipo
/// especial de `Unit` para SCV/Probe/Drone/MULE — útil pra filtros de
/// UI que querem esconder spam de workers sem sumir com o resto das
/// unidades. `Research` vs `Upgrade` distingue pesquisas pontuais
/// (Stimpack, Blink, WarpGate…) de upgrades com níveis
/// (InfantryWeaponsLevel1/2/3, Armor…).
#[allow(dead_code)] // consumido apenas pelo binário GUI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Worker,
    Unit,
    Structure,
    Research,
    Upgrade,
}

#[allow(dead_code)] // consumido apenas pelo binário GUI
impl EntryKind {
    /// Letra curta usada em UIs compactas (coluna "tipo" na GUI).
    /// `U` colide entre Unit e Upgrade — escolhemos `U` para Unit e
    /// `P` (de u**p**grade) para o segundo, já que Unit é mais comum.
    pub fn short_letter(self) -> &'static str {
        match self {
            EntryKind::Worker => "W",
            EntryKind::Unit => "U",
            EntryKind::Structure => "S",
            EntryKind::Research => "R",
            EntryKind::Upgrade => "P",
        }
    }

    /// Nome completo em inglês — útil como tooltip.
    pub fn full_name(self) -> &'static str {
        match self {
            EntryKind::Worker => "Worker",
            EntryKind::Unit => "Unit",
            EntryKind::Structure => "Structure",
            EntryKind::Research => "Research",
            EntryKind::Upgrade => "Upgrade",
        }
    }
}

/// Classifica uma entrada do build order em uma `EntryKind`. A decisão
/// usa os flags já armazenados (`is_upgrade`/`is_structure`) e o nome
/// bruto da ação para distinguir worker/unit e research/upgrade.
#[allow(dead_code)] // consumido apenas pelo binário GUI
pub fn classify_entry(entry: &BuildOrderEntry) -> EntryKind {
    if entry.is_upgrade {
        if is_leveled_upgrade(&entry.action) {
            EntryKind::Upgrade
        } else {
            EntryKind::Research
        }
    } else if entry.is_structure {
        EntryKind::Structure
    } else if is_worker_name(&entry.action) {
        EntryKind::Worker
    } else {
        EntryKind::Unit
    }
}

/// Retorna `true` se o nome da unidade é um worker (coletor de
/// recursos). Inclui MULE por gerar recurso como os demais, ainda
/// que seja invocado pela Orbital Command em vez de treinado.
#[allow(dead_code)] // consumido apenas pelo binário GUI
pub fn is_worker_name(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone" | "MULE")
}

/// Heurística para separar upgrades com níveis (Weapons/Armor 1-3)
/// de pesquisas pontuais. SC2 sufixa os níveis com "Level1/2/3".
#[allow(dead_code)] // consumido apenas pelo binário GUI
fn is_leveled_upgrade(name: &str) -> bool {
    name.ends_with("Level1") || name.ends_with("Level2") || name.ends_with("Level3")
}

/// Retorna `true` se o nome da unidade corresponde a uma estrutura conhecida.
/// Usado para classificar eventos `UnitBorn` com habilidade `MorphTo*` que produzem
/// construções em vez de unidades (ex: OrbitalCommand, Lair).
fn is_structure_name(name: &str) -> bool {
    matches!(name,
        // Terran — base
        "CommandCenter" | "OrbitalCommand" | "PlanetaryFortress" |
        "SupplyDepot" | "SupplyDepotLowered" | "Refinery" |
        // Terran — produção
        "Barracks" | "Factory" | "Starport" |
        // Terran — tecnologia
        "EngineeringBay" | "Armory" | "FusionCore" | "GhostAcademy" |
        // Terran — defesa
        "Bunker" | "MissileTurret" | "SensorTower" |
        // Terran — add-ons
        "BarracksTechLab" | "FactoryTechLab" | "StarportTechLab" |
        "BarracksReactor" | "FactoryReactor" | "StarportReactor" |
        // Zerg — base
        "Hatchery" | "Lair" | "Hive" | "Extractor" |
        // Zerg — produção/tecnologia
        "SpawningPool" | "RoachWarren" | "HydraliskDen" | "BanelingNest" |
        "EvolutionChamber" | "Spire" | "GreaterSpire" |
        "InfestationPit" | "UltraliskCavern" | "NydusNetwork" | "NydusCanal" |
        "LurkerDen" |
        // Zerg — defesa
        "SpineCrawler" | "SporeCrawler" |
        // Protoss — base
        "Nexus" | "Pylon" | "Assimilator" |
        // Protoss — produção/tecnologia
        "Gateway" | "WarpGate" | "Forge" | "CyberneticsCore" |
        "TwilightCouncil" | "Stargate" | "RoboticsFacility" |
        "TemplarArchive" | "DarkShrine" | "RoboticsBay" | "FleetBeacon" |
        // Protoss — defesa
        "PhotonCannon" | "ShieldBattery"
    )
}

// ── Formatação CSV de largura fixada ─────────────────────────────────────────

pub fn format_time(game_loop: u32, lps: f64) -> String {
    let total_secs = (game_loop as f64 / lps).round() as u32;
    format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
}

/// Serializa entradas como CSV de largura fixada.
/// Colunas: supply, time, action (largura calculada a partir dos dados).
pub fn to_fixed_csv(entries: &[BuildOrderEntry], lps: f64) -> String {
    // Constrói as strings de cada coluna para calcular larguras
    let rows: Vec<(String, String, String)> = entries
        .iter()
        .map(|e| {
            let action = if e.count > 1 {
                format!("{} x{}", e.action, e.count)
            } else {
                e.action.clone()
            };
            (e.supply.to_string(), format_time(e.game_loop, lps), action)
        })
        .collect();

    let w_supply = rows.iter().map(|(s, _, _)| s.len()).max().unwrap_or(0).max("supply".len());
    let w_time = rows.iter().map(|(_, t, _)| t.len()).max().unwrap_or(0).max("time".len());
    let w_action = rows.iter().map(|(_, _, a)| a.len()).max().unwrap_or(0).max("action".len());

    let mut out = String::new();
    // Cabeçalho
    out.push_str(&format!(
        "{:<w_supply$}, {:<w_time$}, {:<w_action$}\n",
        "supply", "time", "action",
        w_supply = w_supply, w_time = w_time, w_action = w_action,
    ));
    // Dados
    for (supply, time, action) in &rows {
        out.push_str(&format!(
            "{:<w_supply$}, {:<w_time$}, {:<w_action$}\n",
            supply, time, action,
            w_supply = w_supply, w_time = w_time, w_action = w_action,
        ));
    }
    out
}

// ── MMR lookup ────────────────────────────────────────────────────────────────

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
