// Classificação de unidades, estruturas e upgrades.
//
// Tudo aqui é privado ao módulo `replay` — o resto do app só vê o
// `EntityCategory` já anotado em cada `EntityEvent`.

use super::types::EntityCategory;

/// Workers (coletores de recursos).
pub(super) fn is_worker_name(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone" | "MULE")
}

/// Estruturas que produzem workers (consideradas para `production_gap`).
pub(super) fn is_worker_producer(name: &str) -> bool {
    matches!(
        name,
        "CommandCenter" | "OrbitalCommand" | "PlanetaryFortress" | "Nexus"
    )
}

/// Lista hard-coded de estruturas conhecidas. Usada para classificar
/// `EntityCategory::Structure` no momento do parser, evitando que
/// consumers precisem reclassificar.
pub(super) fn is_structure_name(name: &str) -> bool {
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

pub(super) fn classify_entity(name: &str) -> EntityCategory {
    if is_worker_name(name) {
        EntityCategory::Worker
    } else if is_structure_name(name) {
        EntityCategory::Structure
    } else {
        EntityCategory::Unit
    }
}

// ── Upgrades ────────────────────────────────────────────────────────

pub(super) fn upgrade_level(name: &str) -> u8 {
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

pub(super) fn is_attack_upgrade(name: &str) -> bool {
    name.contains("Weapons")
        || name.contains("Attacks")
        || name.contains("MeleeAttacks")
        || name.contains("RangedAttacks")
        || name.contains("AirAttacks")
        || name.contains("GroundWeapons")
        || name.contains("AirWeapons")
        || name.contains("FlierAttacks")
}

pub(super) fn is_armor_upgrade(name: &str) -> bool {
    name.contains("Armor")
        || name.contains("Carapace")
        || name.contains("Shields")
        || name.contains("GroundArmor")
        || name.contains("AirArmor")
        || name.contains("Plating")
        || name.contains("Chitinous")
}
