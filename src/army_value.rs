use std::path::Path;

use crate::replay::parse_replay;

// ── Structs de saída ──────────────────────────────────────────────────────────

pub enum UpgradeKind {
    Attack,
    Armor,
    Other,
}

pub struct ArmySnapshot {
    pub game_loop: u32,
    pub army_total: i32,
    pub army_gas: i32,
    pub attack_level: u8,
    pub armor_level: u8,
}

pub struct ArmyUpgradeEvent {
    pub game_loop: u32,
    pub name: String,
    pub raw_name: String,
    pub kind: UpgradeKind,
    pub level: u8,
}

pub struct PlayerArmyValue {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub snapshots: Vec<ArmySnapshot>,
    pub upgrade_events: Vec<ArmyUpgradeEvent>,
}

pub struct ArmyValueResult {
    pub players: Vec<PlayerArmyValue>,
    pub game_loops: u32,
    pub loops_per_second: f64,
    pub map_name: String,
    pub datetime: String,
}

// ── Extração ──────────────────────────────────────────────────────────────────

pub fn extract_army_value(path: &Path, max_time_seconds: u32) -> Result<ArmyValueResult, String> {
    let data = parse_replay(path, max_time_seconds, false)?;

    let players = data
        .players
        .iter()
        .map(|player| {
            // Classifica upgrades e determina nível
            let mut upgrade_events: Vec<ArmyUpgradeEvent> = player
                .upgrades
                .iter()
                .filter(|u| !u.name.contains("Spray") && u.game_loop > 0)
                .map(|u| {
                    let (kind, level) = classify_upgrade(&u.name);
                    let name = abbreviate_upgrade(&u.name);
                    ArmyUpgradeEvent {
                        game_loop: u.game_loop,
                        name,
                        raw_name: u.name.clone(),
                        kind,
                        level,
                    }
                })
                .collect();

            // Ordena por game_loop para processamento correto
            upgrade_events.sort_by_key(|e| e.game_loop);

            // Calcula attack_level e armor_level acumulado para cada snapshot
            let mut cur_attack: u8 = 0;
            let mut cur_armor: u8 = 0;
            let mut upg_idx = 0;

            let snapshots = player
                .stats_snapshots
                .iter()
                .map(|s| {
                    // Avança upgrades até o game_loop atual
                    while upg_idx < upgrade_events.len()
                        && upgrade_events[upg_idx].game_loop <= s.game_loop
                    {
                        match upgrade_events[upg_idx].kind {
                            UpgradeKind::Attack => {
                                cur_attack = cur_attack.max(upgrade_events[upg_idx].level)
                            }
                            UpgradeKind::Armor => {
                                cur_armor = cur_armor.max(upgrade_events[upg_idx].level)
                            }
                            UpgradeKind::Other => {}
                        }
                        upg_idx += 1;
                    }
                    ArmySnapshot {
                        game_loop: s.game_loop,
                        army_total: s.army_value_minerals + s.army_value_vespene,
                        army_gas: s.army_value_vespene,
                        attack_level: cur_attack,
                        armor_level: cur_armor,
                    }
                })
                .collect();

            PlayerArmyValue {
                name: player.name.clone(),
                race: player.race.clone(),
                mmr: player.mmr,
                snapshots,
                upgrade_events,
            }
        })
        .collect();

    Ok(ArmyValueResult {
        players,
        game_loops: data.game_loops,
        loops_per_second: data.loops_per_second,
        map_name: data.map.clone(),
        datetime: data.datetime.clone(),
    })
}

// ── Classificação de upgrades ─────────────────────────────────────────────────

fn classify_upgrade(name: &str) -> (UpgradeKind, u8) {
    let level = if name.ends_with("Level3") || name.ends_with("3") && name.contains("Level") {
        3
    } else if name.ends_with("Level2") || name.ends_with("2") && name.contains("Level") {
        2
    } else if name.ends_with("Level1") || name.ends_with("1") && name.contains("Level") {
        1
    } else {
        0
    };

    let is_attack = name.contains("Weapons")
        || name.contains("Attacks")
        || name.contains("MeleeAttacks")
        || name.contains("RangedAttacks")
        || name.contains("AirAttacks")
        || name.contains("GroundWeapons")
        || name.contains("AirWeapons")
        || name.contains("FlierAttacks");

    let is_armor = name.contains("Armor")
        || name.contains("Carapace")
        || name.contains("Shields")
        || name.contains("GroundArmor")
        || name.contains("AirArmor")
        || name.contains("Plating")
        || name.contains("Chitinous");

    if is_attack {
        (UpgradeKind::Attack, level)
    } else if is_armor {
        (UpgradeKind::Armor, level)
    } else {
        (UpgradeKind::Other, level)
    }
}

/// Abrevia nomes longos de upgrades para exibição no gráfico.
fn abbreviate_upgrade(name: &str) -> String {
    // Remove prefixos de raça
    let stripped = name
        .trim_start_matches("Terran")
        .trim_start_matches("Zerg")
        .trim_start_matches("Protoss");

    // Substitui sufixos de nível
    let base = if stripped.ends_with("Level1") {
        format!("{} +1", &stripped[..stripped.len() - 6])
    } else if stripped.ends_with("Level2") {
        format!("{} +2", &stripped[..stripped.len() - 6])
    } else if stripped.ends_with("Level3") {
        format!("{} +3", &stripped[..stripped.len() - 6])
    } else {
        stripped.to_string()
    };

    base
}

// ── CSV ───────────────────────────────────────────────────────────────────────

pub fn to_army_value_csv(snapshots: &[ArmySnapshot], lps: f64) -> String {
    let mut out = String::new();
    out.push_str("time, army_total, army_gas, attack_level, armor_level\n");
    for s in snapshots {
        let total_secs = (s.game_loop as f64 / lps).round() as u32;
        let time = format!("{:02}:{:02}", total_secs / 60, total_secs % 60);
        out.push_str(&format!(
            "{}, {}, {}, {}, {}\n",
            time, s.army_total, s.army_gas, s.attack_level, s.armor_level
        ));
    }
    out
}
