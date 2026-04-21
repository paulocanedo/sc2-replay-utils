// Classificação de unidades, estruturas e upgrades.
//
// Tudo aqui é privado ao módulo `replay` — o resto do app só vê o
// `EntityCategory` já anotado em cada `EntityEvent`.

use super::types::{EntityCategory, ResourceKind};

/// Classifica um `unit_type_name` como nó de recurso neutro. `None`
/// para qualquer outra unidade (incluindo prédios, tropas, etc). Usado
/// durante o parse do tracker para capturar `MineralField*` e
/// `*Geyser*` em `ReplayTimeline.resources`.
///
/// Os nomes cobrem todas as variantes ladder + campaign observadas
/// em replays modernos. O match por `starts_with` em vez de lista
/// explícita reduz o risco de perder variantes novas (ex.:
/// `MineralField450` apareceu em patches recentes).
pub(super) fn resource_kind(name: &str) -> Option<ResourceKind> {
    if name.starts_with("RichMineralField") {
        Some(ResourceKind::RichMineral)
    } else if name.starts_with("MineralField")
        || name.starts_with("LabMineralField")
        || name.starts_with("BattleStationMineralField")
        || name.starts_with("PurifierMineralField")
    {
        Some(ResourceKind::Mineral)
    } else if name.starts_with("RichVespeneGeyser") {
        Some(ResourceKind::RichVespene)
    } else if name.contains("VespeneGeyser") || name.ends_with("Geyser") {
        Some(ResourceKind::Vespene)
    } else {
        None
    }
}

/// Workers (coletores de recursos).
pub fn is_worker_name(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone" | "MULE")
}

/// Estruturas que produzem workers (consideradas para `production_gap`).
pub(super) fn is_worker_producer(name: &str) -> bool {
    matches!(
        name,
        "CommandCenter" | "OrbitalCommand" | "PlanetaryFortress" | "Nexus"
    )
}

/// Estruturas que produzem unidades army (Terran e Protoss). Zerg usa
/// larvas em Hatchery/Lair/Hive — tratamento específico fica fora
/// dessa lista, consumers pulam a raça inteira.
pub(super) fn is_army_producer(name: &str) -> bool {
    matches!(
        name,
        // Terran
        "Barracks" | "Factory" | "Starport"
        // Protoss
        | "Gateway" | "WarpGate" | "RoboticsFacility" | "Stargate"
    )
}

/// Produtores army de todas as raças — inclui Hatchery/Lair/Hive como
/// "1 slot de larva bandwidth" para modelar idle de produção Zerg. Usado
/// por `derive_capacity_indices` para alimentar `army_capacity`.
pub(super) fn is_army_producer_all(name: &str) -> bool {
    is_army_producer(name) || is_zerg_hatch(name)
}

/// Hatcheries do Zerg (inclui morphs Lair/Hive) — cada uma contribui
/// com 1 slot de larva bandwidth na série de eficiência Zerg
/// (`production_efficiency::compute_series_zerg`). Valor 1 (e não 3)
/// porque o throughput sustentável é limitado pela regen de larva
/// (~11s por larva), não pelo cap visual de 3 larvae.
pub fn is_zerg_hatch(name: &str) -> bool {
    matches!(name, "Hatchery" | "Lair" | "Hive")
}

/// Unidades Zerg que nascem diretamente de larva (consomem 1 slot
/// na série de eficiência Zerg — vertente "Army"). Overlord entra
/// porque **usa slot de larva**, mesmo não sendo army no sentido
/// clássico — modelar como army reflete a decisão macro de gastar
/// uma larva que poderia ter virado Drone/Zergling.
///
/// Fora: Queen (morph do prédio, não usa larva), Baneling/Ravager/
/// Lurker/BroodLord/Overseer (morph de unidade existente).
pub fn is_larva_born_army(name: &str) -> bool {
    matches!(
        name,
        "Zergling" | "Roach" | "Hydralisk" | "Infestor"
        | "SwarmHost" | "SwarmHostMP"
        | "Mutalisk" | "Corruptor" | "Viper"
        | "Ultralisk" | "Overlord"
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

pub(super) fn classify_entity(name: &str) -> EntityCategory {
    if is_worker_name(name) {
        EntityCategory::Worker
    } else if is_structure_name(name) || is_creep_tumor_name(name) {
        // Tumors são "estruturas" semânticas: 0 supply, ficam paradas,
        // têm lifecycle de build/born/die. Classificar como Unit faria
        // o `supply_block` tratá-las como tropas — inofensivo (cost=0)
        // mas polui o merge stream sem necessidade.
        EntityCategory::Structure
    } else {
        EntityCategory::Unit
    }
}

/// Reconhece todas as variantes de creep tumor que o replay emite:
/// `CreepTumor` (planta inicial), `CreepTumorBurrowed` (após burrow),
/// `CreepTumorQueen` (planta da queen) e `CreepTumorMissile`
/// (projétil intermediário). Usado por `classify_entity` e por
/// `finalize.rs` ao construir `creep_index`.
pub(super) fn is_creep_tumor_name(name: &str) -> bool {
    name.starts_with("CreepTumor")
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
