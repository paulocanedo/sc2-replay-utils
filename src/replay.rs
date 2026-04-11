// Parser single-pass do replay.
//
// Abre o MPQ uma única vez, lê tracker events e message events em
// um único loop cada e produz um `ReplayTimeline` indexado por tempo
// que serve como fonte única de verdade para todos os extractors
// (build_order, army_value, supply_block, production_gap, chat) e
// para a GUI.
//
// O parser **traduz** os eventos crus do replay (UnitInit/UnitBorn/
// UnitDone/UnitDied/UnitTypeChange) para um vocabulário semântico do
// app — `EntityEvent { kind: ProductionStarted | ProductionFinished
// | ProductionCancelled | Died, … }`. Os consumers nunca tocam no
// formato bruto.

use std::collections::HashMap;
use std::path::Path;

use s2protocol::message_events::{GameEMessageRecipient, ReplayMessageEvent};
use s2protocol::tracker_events::{unit_tag, ReplayTrackerEvent};

use crate::utils::{extract_clan_and_name, game_speed_to_loops_per_second};

// ── Constantes de morph ─────────────────────────────────────────────

/// Tempo de morph CC → Orbital Command em game loops (~25s em Faster).
const ORBITAL_MORPH_TIME: u32 = 560;

/// Tempo de morph CC → Planetary Fortress em game loops (~36s em Faster).
const PF_MORPH_TIME: u32 = 806;

/// Sentinel usado em `EntityEvent.creator_ability` para marcar
/// `ProductionStarted` derivados de `UnitInit` (warp-ins / construções
/// iniciadas). O build_order usa esse marcador para distinguir
/// eventos de início de construção dos spawns "instantâneos" via
/// `UnitBorn`.
pub const UNIT_INIT_MARKER: &str = "__UnitInit__";

// ── Tipos de saída ───────────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct StatsSnapshot {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    pub minerals: i32,
    pub vespene: i32,
    pub minerals_rate: i32,
    pub vespene_rate: i32,
    pub workers: i32,
    pub supply_used: i32,
    pub supply_made: i32,
    pub army_value_minerals: i32,
    pub army_value_vespene: i32,
    pub minerals_lost_army: i32,
    pub vespene_lost_army: i32,
    pub minerals_killed_army: i32,
    pub vespene_killed_army: i32,
}

#[derive(Clone, serde::Serialize)]
pub struct UpgradeEntry {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    /// Sequência global do evento na stream do tracker — ver `EntityEvent::seq`.
    #[serde(skip)]
    pub seq: u32,
    pub name: String,
}

/// Tipo semântico de evento sobre unidades e estruturas.
///
/// O parser traduz os eventos crus do replay para uma destas variantes
/// — o resto do app só lida com este vocabulário, não com Born/Init/
/// Done/Died direto do MPQ.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityEventKind {
    /// Build/train/warp/morph foi iniciado.
    ProductionStarted,
    /// Build/train/warp/morph ficou pronto.
    ProductionFinished,
    /// Build iniciado mas nunca terminou (entidade morreu antes da
    /// conclusão). Não conta como "morte" para o contador de unidades
    /// vivas.
    ProductionCancelled,
    /// Entidade pronta foi destruída ou se transformou em outro tipo
    /// (morphs emitem `Died` para o tipo antigo).
    Died,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityCategory {
    Worker,
    Unit,
    Structure,
}

#[derive(Clone, serde::Serialize)]
pub struct EntityEvent {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    /// Sequência global do evento na stream do tracker, atribuída pelo
    /// parser. Permite reconstituir a ordem original entre `entity_events`
    /// e `upgrades` quando os dois ocorrem no mesmo `game_loop` (o
    /// build_order depende dessa interleavação).
    #[serde(skip)]
    pub seq: u32,
    pub kind: EntityEventKind,
    pub entity_type: String,
    pub category: EntityCategory,
    /// Tag interno do replay — para correlação entre eventos do mesmo
    /// objeto. Não serializado.
    #[serde(skip)]
    pub tag: i64,
    pub pos_x: u8,
    pub pos_y: u8,
    /// Habilidade que iniciou a produção. Só populado em
    /// `ProductionStarted`. Usado pelo build_order para distinguir
    /// trains/morphs de spawns iniciais.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_ability: Option<String>,
    /// Quem matou a entidade. None quando o evento é uma transformação
    /// (morph) ou quando o killer é desconhecido.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub killer_player_id: Option<u8>,
}

#[derive(Clone, serde::Serialize)]
pub struct ChatEntry {
    #[serde(rename = "loop")]
    pub game_loop: u32,
    pub player_name: String,
    pub recipient: String,
    pub message: String,
}

#[derive(serde::Serialize)]
pub struct PlayerTimeline {
    pub name: String,
    pub clan: String,
    pub race: String,
    pub mmr: Option<i32>,

    pub stats: Vec<StatsSnapshot>,
    pub upgrades: Vec<UpgradeEntry>,
    pub entity_events: Vec<EntityEvent>,

    /// Diff cumulativo de "entidades vivas" por tipo. Para cada
    /// `entity_type`, um vetor ordenado de
    /// `(game_loop, alive_count_apos_o_evento)`. Construído no
    /// pós-processamento a partir de `entity_events`.
    #[serde(skip)]
    pub alive_count: HashMap<String, Vec<(u32, i32)>>,

    /// Capacidade de produção de workers (CC/Orbital/PF/Nexus). Cada
    /// par é `(game_loop, capacity_apos_o_evento)`, ordenado.
    #[serde(skip)]
    pub worker_capacity: Vec<(u32, i32)>,

    /// game_loops em que workers (SCV/Probe) nasceram, ordenado.
    /// Usado por `production_gap` para detectar slots ociosos.
    #[serde(skip)]
    pub worker_births: Vec<u32>,

    /// `(game_loop, attack_level_apos, armor_level_apos)` cumulativo
    /// para queries de scrubbing.
    #[serde(skip)]
    pub upgrade_cumulative: Vec<(u32, u8, u8)>,
}

#[derive(serde::Serialize)]
pub struct ReplayTimeline {
    pub file: String,
    pub map: String,
    pub datetime: String,
    pub game_loops: u32,
    pub duration_seconds: u32,
    pub loops_per_second: f64,
    /// Limite de coleta de eventos em segundos. 0 indica sem limite.
    pub max_time_seconds: u32,
    pub players: Vec<PlayerTimeline>,
    pub chat: Vec<ChatEntry>,
}

// ── API de scrubbing ────────────────────────────────────────────────

impl ReplayTimeline {
    /// Converte segundos para game loops usando o `loops_per_second`
    /// do replay.
    #[allow(dead_code)]
    pub fn loop_at_seconds(&self, secs: f64) -> u32 {
        (secs * self.loops_per_second).max(0.0).round() as u32
    }
}

impl PlayerTimeline {
    /// Último `StatsSnapshot` cujo game_loop é ≤ `game_loop`.
    /// Binary search → O(log n).
    pub fn stats_at(&self, game_loop: u32) -> Option<&StatsSnapshot> {
        let i = self.stats.partition_point(|s| s.game_loop <= game_loop);
        if i == 0 {
            None
        } else {
            Some(&self.stats[i - 1])
        }
    }

    /// Slice de upgrades pesquisados até `game_loop` (inclusivo).
    #[allow(dead_code)]
    pub fn upgrades_until(&self, game_loop: u32) -> &[UpgradeEntry] {
        let i = self.upgrades.partition_point(|u| u.game_loop <= game_loop);
        &self.upgrades[..i]
    }

    /// Nível de attack acumulado até `game_loop` (inclusivo).
    #[allow(dead_code)]
    pub fn attack_level_at(&self, game_loop: u32) -> u8 {
        let i = self
            .upgrade_cumulative
            .partition_point(|(l, _, _)| *l <= game_loop);
        if i == 0 {
            0
        } else {
            self.upgrade_cumulative[i - 1].1
        }
    }

    /// Nível de armor acumulado até `game_loop` (inclusivo).
    #[allow(dead_code)]
    pub fn armor_level_at(&self, game_loop: u32) -> u8 {
        let i = self
            .upgrade_cumulative
            .partition_point(|(l, _, _)| *l <= game_loop);
        if i == 0 {
            0
        } else {
            self.upgrade_cumulative[i - 1].2
        }
    }

    /// Quantas entidades de `entity_type` estão vivas em `game_loop`.
    #[allow(dead_code)]
    pub fn alive_count_at(&self, entity_type: &str, game_loop: u32) -> i32 {
        let Some(v) = self.alive_count.get(entity_type) else {
            return 0;
        };
        let i = v.partition_point(|(l, _)| *l <= game_loop);
        if i == 0 {
            0
        } else {
            v[i - 1].1
        }
    }

    /// Capacidade de produção de workers em `game_loop`.
    /// As entradas em `worker_capacity` são deltas (+1/-1); aqui acumulamos
    /// até o ponto pedido. O custo é O(n) sobre uma lista pequena (poucas
    /// dezenas de eventos por jogador), aceitável dado que esta API serve
    /// scrubbing pontual da GUI, não loops quentes.
    #[allow(dead_code)]
    pub fn worker_capacity_at(&self, game_loop: u32) -> i32 {
        let i = self
            .worker_capacity
            .partition_point(|(l, _)| *l <= game_loop);
        self.worker_capacity[..i]
            .iter()
            .map(|(_, d)| *d)
            .sum::<i32>()
            .max(0)
    }
}

// ── Helpers de classificação ────────────────────────────────────────

/// Workers (coletores de recursos).
pub fn is_worker_name(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone" | "MULE")
}

/// Estruturas que produzem workers (consideradas para `production_gap`).
pub fn is_worker_producer(name: &str) -> bool {
    matches!(
        name,
        "CommandCenter" | "OrbitalCommand" | "PlanetaryFortress" | "Nexus"
    )
}

/// Lista hard-coded de estruturas conhecidas. Usada para classificar
/// `EntityCategory::Structure` no momento do parser, evitando que
/// consumers precisem reclassificar.
pub fn is_structure_name(name: &str) -> bool {
    matches!(
        name,
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

fn classify_entity(name: &str) -> EntityCategory {
    if is_worker_name(name) {
        EntityCategory::Worker
    } else if is_structure_name(name) {
        EntityCategory::Structure
    } else {
        EntityCategory::Unit
    }
}

fn morph_build_time(unit_type: &str) -> u32 {
    match unit_type {
        "OrbitalCommand" => ORBITAL_MORPH_TIME,
        "PlanetaryFortress" => PF_MORPH_TIME,
        _ => 0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    InProgress,
    Finished,
}

struct TagState {
    entity_type: String,
    player_id: u8,
    pos_x: u8,
    pos_y: u8,
    lifecycle: Lifecycle,
}

// ── Parser principal ────────────────────────────────────────────────

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

    let datetime = s2protocol::transform_to_naivetime(details.time_utc, details.time_local_offset)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| "0000-00-00T00:00:00".to_string());

    let game_loops = header.m_elapsed_game_loops as u32;
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
        .filter(|p| p.observe == 0)
        .map(|p| {
            let (clan, name) = extract_clan_and_name(&p.name);
            let mmr = init_data
                .as_ref()
                .and_then(|id| find_mmr_for_slot(id, p.working_set_slot_id));
            PlayerTimeline {
                name,
                clan,
                race: p.race.clone(),
                mmr,
                stats: Vec::new(),
                upgrades: Vec::new(),
                entity_events: Vec::new(),
                alive_count: HashMap::new(),
                worker_capacity: Vec::new(),
                worker_births: Vec::new(),
                upgrade_cumulative: Vec::new(),
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

    let mut timeline = ReplayTimeline {
        file,
        map,
        datetime,
        game_loops,
        duration_seconds,
        loops_per_second,
        max_time_seconds,
        players,
        chat: Vec::new(),
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

    process_tracker_events(
        path_str,
        &mpq,
        &file_contents,
        &player_idx,
        &mut timeline.players,
        max_loops,
    )?;

    process_message_events(
        path_str,
        &mpq,
        &file_contents,
        &user_names,
        max_loops,
        &mut timeline.chat,
    )?;

    finalize_indices(&mut timeline.players);

    Ok(timeline)
}

// ── Tracker events ──────────────────────────────────────────────────

fn process_tracker_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    player_idx: &HashMap<u8, usize>,
    players: &mut [PlayerTimeline],
    max_loops: u32,
) -> Result<(), String> {
    let tracker_events = s2protocol::read_tracker_events(path_str, mpq, file_contents)
        .map_err(|e| format!("{:?}", e))?;

    let mut tag_map: HashMap<i64, TagState> = HashMap::new();
    let mut cur_attack: Vec<u8> = vec![0; players.len()];
    let mut cur_armor: Vec<u8> = vec![0; players.len()];

    let mut game_loop: u32 = 0;
    // Sequência monotônica usada como tiebreaker entre eventos do
    // mesmo `game_loop`. Compartilhada entre `entity_events` e
    // `upgrades` para preservar a interleavação original do tracker.
    let mut seq: u32 = 0;

    for ev in tracker_events {
        game_loop += ev.delta;
        if max_loops != 0 && game_loop > max_loops {
            break;
        }
        seq = seq.wrapping_add(1);

        match ev.event {
            ReplayTrackerEvent::PlayerStats(e) => {
                let Some(&idx) = player_idx.get(&e.player_id) else { continue };
                let s = &e.stats;
                players[idx].stats.push(StatsSnapshot {
                    game_loop,
                    minerals: s.minerals_current,
                    vespene: s.vespene_current,
                    minerals_rate: s.minerals_collection_rate,
                    vespene_rate: s.vespene_collection_rate,
                    workers: s.workers_active_count,
                    supply_used: s.food_used,
                    supply_made: s.food_made,
                    army_value_minerals: s.minerals_used_active_forces,
                    army_value_vespene: s.vespene_used_active_forces,
                    minerals_lost_army: s.minerals_lost_army,
                    vespene_lost_army: s.vespene_lost_army,
                    minerals_killed_army: s.minerals_killed_army,
                    vespene_killed_army: s.vespene_killed_army,
                });
            }

            ReplayTrackerEvent::Upgrade(e) => {
                let Some(&idx) = player_idx.get(&e.player_id) else { continue };
                if e.upgrade_type_name.contains("Spray") {
                    continue;
                }
                let level = upgrade_level(&e.upgrade_type_name);
                if is_attack_upgrade(&e.upgrade_type_name) && level > 0 {
                    cur_attack[idx] = cur_attack[idx].max(level);
                }
                if is_armor_upgrade(&e.upgrade_type_name) && level > 0 {
                    cur_armor[idx] = cur_armor[idx].max(level);
                }
                players[idx].upgrades.push(UpgradeEntry {
                    game_loop,
                    seq,
                    name: e.upgrade_type_name,
                });
                players[idx]
                    .upgrade_cumulative
                    .push((game_loop, cur_attack[idx], cur_armor[idx]));
            }

            ReplayTrackerEvent::UnitBorn(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(&idx) = player_idx.get(&e.control_player_id) else {
                    // Insere mesmo assim para resolução posterior se vier UnitDied.
                    tag_map.insert(
                        tag,
                        TagState {
                            entity_type: e.unit_type_name.clone(),
                            player_id: e.control_player_id,
                            pos_x: e.x,
                            pos_y: e.y,
                            lifecycle: Lifecycle::Finished,
                        },
                    );
                    continue;
                };

                let category = classify_entity(&e.unit_type_name);
                let creator_ability = e.creator_ability_name.clone().filter(|s| !s.is_empty());

                // Decisão baseada no estado atual da tag.
                let prev_lifecycle = tag_map.get(&tag).map(|s| (s.entity_type.clone(), s.lifecycle));

                match prev_lifecycle {
                    None => {
                        // Tag novo: spawn instantâneo. Started+Finished
                        // no mesmo loop.
                        push_event(
                            &mut players[idx],
                            EntityEvent {
                                game_loop,
                                seq,
                                kind: EntityEventKind::ProductionStarted,
                                entity_type: e.unit_type_name.clone(),
                                category,
                                tag,
                                pos_x: e.x,
                                pos_y: e.y,
                                creator_ability: creator_ability.clone(),
                                killer_player_id: None,
                            },
                        );
                        push_event(
                            &mut players[idx],
                            EntityEvent {
                                game_loop,
                                seq,
                                kind: EntityEventKind::ProductionFinished,
                                entity_type: e.unit_type_name.clone(),
                                category,
                                tag,
                                pos_x: e.x,
                                pos_y: e.y,
                                creator_ability: None,
                                killer_player_id: None,
                            },
                        );

                        // Worker nascido via Train (SCV/Probe).
                        if matches!(category, EntityCategory::Worker)
                            && creator_ability.as_deref().unwrap_or("").contains("Train")
                            && matches!(e.unit_type_name.as_str(), "SCV" | "Probe")
                        {
                            players[idx].worker_births.push(game_loop);
                        }

                        // Estrutura produtora aparecendo (ex: CC inicial).
                        if is_worker_producer(&e.unit_type_name) {
                            players[idx].worker_capacity.push((game_loop, 1));
                        }

                        tag_map.insert(
                            tag,
                            TagState {
                                entity_type: e.unit_type_name.clone(),
                                player_id: e.control_player_id,
                                pos_x: e.x,
                                pos_y: e.y,
                                lifecycle: Lifecycle::Finished,
                            },
                        );
                    }
                    Some((prev_type, _)) if prev_type != e.unit_type_name => {
                        // Tag existente, type diferente: morph completion
                        // (UnitTypeChange pode ou não ter chegado antes).
                        apply_type_change(
                            &mut players[idx],
                            game_loop,
                            seq,
                            tag,
                            &prev_type,
                            &e.unit_type_name,
                            e.x,
                            e.y,
                            creator_ability,
                        );
                        tag_map.insert(
                            tag,
                            TagState {
                                entity_type: e.unit_type_name.clone(),
                                player_id: e.control_player_id,
                                pos_x: e.x,
                                pos_y: e.y,
                                lifecycle: Lifecycle::Finished,
                            },
                        );
                    }
                    Some((_, Lifecycle::InProgress)) => {
                        // Init prévio + Born agora = produção concluída.
                        push_event(
                            &mut players[idx],
                            EntityEvent {
                                game_loop,
                                seq,
                                kind: EntityEventKind::ProductionFinished,
                                entity_type: e.unit_type_name.clone(),
                                category,
                                tag,
                                pos_x: e.x,
                                pos_y: e.y,
                                creator_ability: None,
                                killer_player_id: None,
                            },
                        );
                        if is_worker_producer(&e.unit_type_name) {
                            players[idx].worker_capacity.push((game_loop, 1));
                        }
                        if let Some(state) = tag_map.get_mut(&tag) {
                            state.lifecycle = Lifecycle::Finished;
                        }
                    }
                    Some((_, Lifecycle::Finished)) => {
                        // Tag re-emitido, mesmo tipo, já Finished — duplicata, ignorar.
                    }
                }
            }

            ReplayTrackerEvent::UnitInit(e) => {
                // Cosméticos / placements táticos não são build order.
                if e.unit_type_name.contains("Tumor") || e.unit_type_name.contains("Spray") {
                    continue;
                }
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(&idx) = player_idx.get(&e.control_player_id) else { continue };
                let category = classify_entity(&e.unit_type_name);

                push_event(
                    &mut players[idx],
                    EntityEvent {
                        game_loop,
                        seq,
                        kind: EntityEventKind::ProductionStarted,
                        entity_type: e.unit_type_name.clone(),
                        category,
                        tag,
                        pos_x: e.x,
                        pos_y: e.y,
                        // Sentinel: marca eventos vindos de UnitInit para
                        // que o build_order possa distinguir warp-ins/
                        // construções de spawns iniciais.
                        creator_ability: Some(UNIT_INIT_MARKER.to_string()),
                        killer_player_id: None,
                    },
                );

                tag_map.insert(
                    tag,
                    TagState {
                        entity_type: e.unit_type_name.clone(),
                        player_id: e.control_player_id,
                        pos_x: e.x,
                        pos_y: e.y,
                        lifecycle: Lifecycle::InProgress,
                    },
                );
            }

            ReplayTrackerEvent::UnitDone(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(state) = tag_map.get_mut(&tag) else { continue };
                let Some(&idx) = player_idx.get(&state.player_id) else { continue };
                let category = classify_entity(&state.entity_type);
                let entity_type = state.entity_type.clone();
                let pos_x = state.pos_x;
                let pos_y = state.pos_y;

                push_event(
                    &mut players[idx],
                    EntityEvent {
                        game_loop,
                        seq,
                        kind: EntityEventKind::ProductionFinished,
                        entity_type: entity_type.clone(),
                        category,
                        tag,
                        pos_x,
                        pos_y,
                        creator_ability: None,
                        killer_player_id: None,
                    },
                );
                if is_worker_producer(&entity_type) {
                    players[idx].worker_capacity.push((game_loop, 1));
                }
                if let Some(state) = tag_map.get_mut(&tag) {
                    state.lifecycle = Lifecycle::Finished;
                }
            }

            ReplayTrackerEvent::UnitDied(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(state) = tag_map.remove(&tag) else { continue };
                let Some(&idx) = player_idx.get(&state.player_id) else { continue };
                let category = classify_entity(&state.entity_type);

                let kind = if state.lifecycle == Lifecycle::InProgress {
                    EntityEventKind::ProductionCancelled
                } else {
                    if is_worker_producer(&state.entity_type) {
                        players[idx].worker_capacity.push((game_loop, -1));
                    }
                    EntityEventKind::Died
                };

                push_event(
                    &mut players[idx],
                    EntityEvent {
                        game_loop,
                        seq,
                        kind,
                        entity_type: state.entity_type,
                        category,
                        tag,
                        pos_x: e.x,
                        pos_y: e.y,
                        creator_ability: None,
                        killer_player_id: e.killer_player_id,
                    },
                );
            }

            ReplayTrackerEvent::UnitTypeChange(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let Some(state) = tag_map.get(&tag) else { continue };
                if state.entity_type == e.unit_type_name {
                    continue; // dedupe vs UnitBorn de morph
                }
                let pid = state.player_id;
                let pos_x = state.pos_x;
                let pos_y = state.pos_y;
                let old_type = state.entity_type.clone();
                let Some(&idx) = player_idx.get(&pid) else { continue };

                apply_type_change(
                    &mut players[idx],
                    game_loop,
                    seq,
                    tag,
                    &old_type,
                    &e.unit_type_name,
                    pos_x,
                    pos_y,
                    None,
                );

                tag_map.insert(
                    tag,
                    TagState {
                        entity_type: e.unit_type_name.clone(),
                        player_id: pid,
                        pos_x,
                        pos_y,
                        lifecycle: Lifecycle::Finished,
                    },
                );
            }

            _ => {}
        }
    }

    Ok(())
}

fn push_event(player: &mut PlayerTimeline, ev: EntityEvent) {
    player.entity_events.push(ev);
}

/// Aplica uma transição de tipo (UnitTypeChange ou UnitBorn de morph).
///
/// Emite `Died` para o tipo antigo e `ProductionStarted`+`ProductionFinished`
/// para o novo tipo, ambos no `game_loop` atual (a back-data do `Started`
/// é evitada porque o build_order da GUI já faz o ajuste de start-time
/// via `build_time_seconds`). A lógica de capacidade de workers, que
/// precisa do morph downtime real, ainda usa o instante back-dated.
fn apply_type_change(
    player: &mut PlayerTimeline,
    game_loop: u32,
    seq: u32,
    tag: i64,
    old_type: &str,
    new_type: &str,
    pos_x: u8,
    pos_y: u8,
    creator_ability: Option<String>,
) {
    let old_category = classify_entity(old_type);
    let new_category = classify_entity(new_type);

    push_event(
        player,
        EntityEvent {
            game_loop,
            seq,
            kind: EntityEventKind::Died,
            entity_type: old_type.to_string(),
            category: old_category,
            tag,
            pos_x,
            pos_y,
            creator_ability: None,
            killer_player_id: None,
        },
    );

    // creator_ability passa-pelo como veio: Some("MorphTo*") quando o
    // evento veio de um UnitBorn de morph real (player issued), None
    // quando veio de UnitTypeChange (siege, lift, lower depot, etc).
    // O build_order só inclui o caso Some("MorphTo*"); transformações
    // mecânicas (None) não devem aparecer no build order.
    push_event(
        player,
        EntityEvent {
            game_loop,
            seq,
            kind: EntityEventKind::ProductionStarted,
            entity_type: new_type.to_string(),
            category: new_category,
            tag,
            pos_x,
            pos_y,
            creator_ability,
            killer_player_id: None,
        },
    );
    push_event(
        player,
        EntityEvent {
            game_loop,
            seq,
            kind: EntityEventKind::ProductionFinished,
            entity_type: new_type.to_string(),
            category: new_category,
            tag,
            pos_x,
            pos_y,
            creator_ability: None,
            killer_player_id: None,
        },
    );

    // Worker capacity: replica a lógica de production_gap.rs, com o
    // backfill de morph downtime para CC→Orbital/PF.
    let old_is_prod = is_worker_producer(old_type);
    let new_is_prod = is_worker_producer(new_type);
    match (old_is_prod, new_is_prod) {
        (true, true) => {
            let mt = morph_build_time(new_type);
            if mt > 0 {
                let morph_start = game_loop.saturating_sub(mt);
                player.worker_capacity.push((morph_start, -1));
                player.worker_capacity.push((game_loop, 1));
            }
        }
        (true, false) => {
            player.worker_capacity.push((game_loop, -1));
        }
        (false, true) => {
            player.worker_capacity.push((game_loop, 1));
        }
        (false, false) => {}
    }
}

// ── Message events ──────────────────────────────────────────────────

fn process_message_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    user_names: &HashMap<i64, String>,
    max_loops: u32,
    chat: &mut Vec<ChatEntry>,
) -> Result<(), String> {
    let message_events = s2protocol::read_message_events(path_str, mpq, file_contents)
        .map_err(|e| format!("{:?}", e))?;

    let mut game_loop: i64 = 0;
    for ev in message_events {
        game_loop += ev.delta;
        let loop_u32 = game_loop.max(0) as u32;
        if max_loops != 0 && loop_u32 > max_loops {
            break;
        }

        let ReplayMessageEvent::EChat(msg) = ev.event;
        let player_name = user_names
            .get(&ev.user_id)
            .cloned()
            .unwrap_or_else(|| format!("Player{}", ev.user_id));

        let recipient = match msg.m_recipient {
            GameEMessageRecipient::EAll => "all",
            GameEMessageRecipient::EAllies => "allies",
            GameEMessageRecipient::EIndividual => "individual",
            GameEMessageRecipient::EBattlenet => "battlenet",
            GameEMessageRecipient::EObservers => "observers",
        }
        .to_string();

        chat.push(ChatEntry {
            game_loop: loop_u32,
            player_name,
            recipient,
            message: msg.m_string,
        });
    }

    Ok(())
}

// ── Pós-processamento ───────────────────────────────────────────────

fn finalize_indices(players: &mut [PlayerTimeline]) {
    for player in players.iter_mut() {
        // Eventos podem ter sido emitidos fora de ordem por morphs
        // (apply_type_change empilha múltiplos no mesmo loop). A
        // ordenação é estável, então a ordem relativa entre eventos
        // do mesmo loop é preservada.
        player.entity_events.sort_by_key(|e| e.game_loop);
        player.worker_capacity.sort_by_key(|(l, _)| *l);
        player.worker_births.sort_unstable();

        // alive_count: ProductionFinished ++; Died --; ignora
        // Started/Cancelled.
        let mut counts: HashMap<String, i32> = HashMap::new();
        for ev in &player.entity_events {
            match ev.kind {
                EntityEventKind::ProductionFinished => {
                    let c = counts.entry(ev.entity_type.clone()).or_insert(0);
                    *c += 1;
                    let v = player
                        .alive_count
                        .entry(ev.entity_type.clone())
                        .or_default();
                    v.push((ev.game_loop, *c));
                }
                EntityEventKind::Died => {
                    let c = counts.entry(ev.entity_type.clone()).or_insert(0);
                    *c -= 1;
                    let v = player
                        .alive_count
                        .entry(ev.entity_type.clone())
                        .or_default();
                    v.push((ev.game_loop, *c));
                }
                _ => {}
            }
        }
    }
}

// ── Helpers de upgrade ──────────────────────────────────────────────

fn upgrade_level(name: &str) -> u8 {
    if name.ends_with("Level3") || (name.ends_with('3') && name.contains("Level")) {
        3
    } else if name.ends_with("Level2") || (name.ends_with('2') && name.contains("Level")) {
        2
    } else if name.ends_with("Level1") || (name.ends_with('1') && name.contains("Level")) {
        1
    } else {
        0
    }
}

fn is_attack_upgrade(name: &str) -> bool {
    name.contains("Weapons")
        || name.contains("Attacks")
        || name.contains("MeleeAttacks")
        || name.contains("RangedAttacks")
        || name.contains("AirAttacks")
        || name.contains("GroundWeapons")
        || name.contains("AirWeapons")
        || name.contains("FlierAttacks")
}

fn is_armor_upgrade(name: &str) -> bool {
    name.contains("Armor")
        || name.contains("Carapace")
        || name.contains("Shields")
        || name.contains("GroundArmor")
        || name.contains("AirArmor")
        || name.contains("Plating")
        || name.contains("Chitinous")
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

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Caminho para o replay de exemplo (Terran vs Protoss, com morphs
    /// CC→Orbital e uma estrutura cancelada). Usado como golden em
    /// vários testes.
    fn example_replay() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/replay1.SC2Replay")
    }

    fn load() -> ReplayTimeline {
        parse_replay(&example_replay(), 0).expect("parse_replay")
    }

    #[test]
    fn timeline_loads() {
        let t = load();
        assert_eq!(t.players.len(), 2);
        assert!(t.game_loops > 0);
        assert!(t.loops_per_second > 0.0);
        assert!(!t.players[0].name.is_empty());
        assert!(!t.players[1].name.is_empty());
    }

    #[test]
    fn metadata_only_fast_path_skips_events() {
        let t = parse_replay(&example_replay(), 1).expect("parse_replay fast");
        assert_eq!(t.players.len(), 2);
        // Fast path: nada de tracker/message events.
        for p in &t.players {
            assert!(p.stats.is_empty(), "stats deveria estar vazio no fast path");
            assert!(
                p.entity_events.is_empty(),
                "entity_events deveria estar vazio no fast path",
            );
            assert!(p.upgrades.is_empty());
        }
        assert!(t.chat.is_empty());
    }

    #[test]
    fn stats_at_returns_latest_le() {
        let t = load();
        let p = &t.players[0];
        assert!(!p.stats.is_empty());

        // Antes do primeiro snapshot → None.
        assert!(p.stats_at(0).is_none() || p.stats[0].game_loop == 0);

        // No próprio loop do primeiro snapshot, deve devolvê-lo.
        let first = &p.stats[0];
        let s = p.stats_at(first.game_loop).unwrap();
        assert_eq!(s.game_loop, first.game_loop);

        // No meio do replay, devolve o snapshot mais recente <= alvo.
        let mid = p.stats[p.stats.len() / 2].game_loop;
        let s = p.stats_at(mid + 1).unwrap();
        assert!(s.game_loop <= mid + 1);

        // Depois do último snapshot, devolve o último.
        let last = p.stats.last().unwrap().game_loop;
        let s = p.stats_at(last + 1_000_000).unwrap();
        assert_eq!(s.game_loop, last);
    }

    #[test]
    fn upgrades_until_is_prefix() {
        let t = load();
        let p = &t.players[0];
        // 0 → vazio.
        assert!(p.upgrades_until(0).is_empty() || p.upgrades[0].game_loop == 0);
        // ∞ → todos.
        let all = p.upgrades_until(u32::MAX);
        assert_eq!(all.len(), p.upgrades.len());
        // Monotônico em loop.
        for w in p.upgrades.windows(2) {
            assert!(w[0].game_loop <= w[1].game_loop);
        }
    }

    #[test]
    fn alive_count_monotonic_for_morphs() {
        let t = load();
        // O contador acumulado por tipo nunca pode ficar negativo.
        for p in &t.players {
            for (kind, series) in &p.alive_count {
                for (loop_, count) in series {
                    assert!(
                        *count >= 0,
                        "alive_count negativo para {} no loop {}: {}",
                        kind, loop_, count
                    );
                }
            }
        }
    }

    #[test]
    fn cancellation_emitted_when_in_progress_dies() {
        let t = load();
        // O replay de exemplo tem um CommandCenter (tag 95682561) que
        // teve UnitInit no loop 7953 e UnitDied no loop 8450 — esse
        // par precisa virar ProductionCancelled, não Died.
        let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
        let cancellations: Vec<_> = terran
            .entity_events
            .iter()
            .filter(|e| e.kind == EntityEventKind::ProductionCancelled)
            .collect();
        assert!(
            !cancellations.is_empty(),
            "esperava ao menos um ProductionCancelled (CC interrompido)",
        );
        assert!(
            cancellations
                .iter()
                .any(|e| e.entity_type == "CommandCenter" && e.game_loop == 8450),
            "esperava o ProductionCancelled específico do CC tag=95682561 no loop 8450",
        );
    }

    #[test]
    fn instant_units_emit_started_and_finished_same_loop() {
        let t = load();
        // SCVs treinados a partir do CC nascem instantaneamente do
        // ponto de vista do tracker (UnitBorn cru). O parser deve
        // emitir Started+Finished no MESMO game_loop para essas tags.
        let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
        let mut by_tag: HashMap<i64, Vec<(u32, EntityEventKind)>> = HashMap::new();
        for ev in &terran.entity_events {
            if ev.entity_type == "SCV" {
                by_tag.entry(ev.tag).or_default().push((ev.game_loop, ev.kind));
            }
        }
        let mut found = 0;
        for (_, evs) in &by_tag {
            let started = evs.iter().find(|(_, k)| *k == EntityEventKind::ProductionStarted);
            let finished = evs.iter().find(|(_, k)| *k == EntityEventKind::ProductionFinished);
            if let (Some(s), Some(f)) = (started, finished) {
                assert_eq!(s.0, f.0, "Started/Finished deveriam estar no mesmo loop para SCV");
                found += 1;
            }
        }
        assert!(found > 0, "esperava ao menos um SCV com Started+Finished no mesmo loop");
    }

    #[test]
    fn morph_emits_started_and_finished_for_new_type() {
        let t = load();
        // O replay tem CC→OrbitalCommand (apply_type_change). Para os
        // morphs, esperamos Died do tipo antigo + Started + Finished do
        // tipo novo no mesmo game_loop e mesmo tag. Filtramos os
        // ProductionStarted de Orbital que vieram de morph (i.e., têm um
        // Died de CC no mesmo loop+tag) e checamos o Finished pareado.
        let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
        let morph_starts: Vec<_> = terran
            .entity_events
            .iter()
            .filter(|e| {
                e.entity_type == "OrbitalCommand"
                    && e.kind == EntityEventKind::ProductionStarted
                    && terran.entity_events.iter().any(|d| {
                        d.tag == e.tag
                            && d.game_loop == e.game_loop
                            && d.kind == EntityEventKind::Died
                            && d.entity_type == "CommandCenter"
                    })
            })
            .collect();
        assert!(
            !morph_starts.is_empty(),
            "esperava ao menos um morph CC→OrbitalCommand (Died+Started no mesmo loop+tag)",
        );
        for s in &morph_starts {
            let finished = terran.entity_events.iter().any(|e| {
                e.tag == s.tag
                    && e.kind == EntityEventKind::ProductionFinished
                    && e.game_loop == s.game_loop
                    && e.entity_type == "OrbitalCommand"
            });
            assert!(
                finished,
                "morph sem ProductionFinished de OrbitalCommand pareado em {}",
                s.game_loop,
            );
        }
    }

    #[test]
    fn worker_capacity_never_negative() {
        // O parser pode empurrar -1 mesmo quando a capacidade
        // observada estaria 0; o consumer (production_gap) clampa.
        // Mas a soma cumulativa correta nunca deveria ficar negativa
        // se a parser-side ignorar Cancelled (estrutura nunca +1'd).
        let t = load();
        for p in &t.players {
            let mut cum: i32 = 0;
            let mut events = p.worker_capacity.clone();
            events.sort_by_key(|(l, _)| *l);
            for (_, delta) in &events {
                cum += delta;
                assert!(
                    cum >= 0,
                    "worker_capacity acumulado ficou negativo em {}: {:?}",
                    p.name, events,
                );
            }
        }
    }

    #[test]
    fn state_at_loop_zero_returns_no_stats() {
        let t = load();
        let p = &t.players[0];
        // Stats começam após o loop 0 (snapshot inicial); stats_at(0)
        // pode devolver Some se o primeiro snapshot é exatamente em
        // loop 0, ou None caso contrário. Em ambos os casos não deve
        // panicar.
        let _ = p.stats_at(0);
        let _ = p.upgrades_until(0);
        let _ = p.worker_capacity_at(0);
    }

    #[test]
    fn state_at_loop_past_end_returns_last() {
        let t = load();
        let p = &t.players[0];
        let last_stat = p.stats.last().unwrap();
        let s = p.stats_at(u32::MAX).unwrap();
        assert_eq!(s.game_loop, last_stat.game_loop);

        // worker_capacity após o fim deve devolver o último valor
        // acumulado (não 0).
        let last_cap = p
            .worker_capacity
            .iter()
            .fold(0i32, |acc, (_, d)| acc + d);
        assert_eq!(p.worker_capacity_at(u32::MAX), last_cap);
    }

    #[test]
    fn entity_events_sorted_by_loop() {
        let t = load();
        for p in &t.players {
            for w in p.entity_events.windows(2) {
                assert!(
                    w[0].game_loop <= w[1].game_loop,
                    "entity_events fora de ordem em {}: {} > {}",
                    p.name, w[0].game_loop, w[1].game_loop,
                );
            }
        }
    }
}
