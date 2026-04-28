//! Classificação genérica (independente de raça): mapeia nomes de
//! entidades para tipos canônicos de lane e filtra unidades-alvo por
//! `LaneMode`. Também contém a tabela `intern_unit_name` usada para
//! garantir um `&'static str` em `ProductionBlock::produced_type`.

use crate::replay::{is_incapacitating_addon, is_larva_born_army, is_worker_name};

use super::types::LaneMode;

/// Tipos de townhall (modo Workers).
pub(super) fn townhall_canonical(name: &str) -> Option<&'static str> {
    match name {
        "Nexus" => Some("Nexus"),
        "CommandCenter" => Some("CommandCenter"),
        "OrbitalCommand" => Some("OrbitalCommand"),
        "PlanetaryFortress" => Some("PlanetaryFortress"),
        "Hatchery" => Some("Hatchery"),
        "Lair" => Some("Lair"),
        "Hive" => Some("Hive"),
        _ => None,
    }
}

/// Estruturas produtoras de army (modo Army). Inclui Hatch/Lair/Hive
/// como produtoras Zerg.
pub(super) fn army_producer_canonical(name: &str) -> Option<&'static str> {
    match name {
        "Barracks" => Some("Barracks"),
        "Factory" => Some("Factory"),
        "Starport" => Some("Starport"),
        "Gateway" => Some("Gateway"),
        "WarpGate" => Some("WarpGate"),
        "RoboticsFacility" => Some("RoboticsFacility"),
        "Stargate" => Some("Stargate"),
        "Hatchery" => Some("Hatchery"),
        "Lair" => Some("Lair"),
        "Hive" => Some("Hive"),
        _ => None,
    }
}

pub(super) fn lane_canonical(name: &str, mode: LaneMode) -> Option<&'static str> {
    match mode {
        LaneMode::Workers => townhall_canonical(name),
        LaneMode::Army => army_producer_canonical(name),
    }
}

pub(super) fn is_target_unit(name: &str, mode: LaneMode, is_zerg: bool) -> bool {
    match mode {
        LaneMode::Workers => matches!(name, "SCV" | "Probe" | "Drone"),
        LaneMode::Army => {
            if is_worker_name(name) {
                return false;
            }
            if is_zerg {
                is_larva_born_army(name)
            } else {
                // Terran/Protoss: whitelist via `intern_unit_name`.
                // Filtra estruturas (SupplyDepot, Refinery,
                // EngineeringBay, GhostAcademy, CommandCenter, Bunker,
                // …), summons que vêm com `creator_unit_tag` apontando
                // pra unidade-mãe e não pra um produtor-estrutura
                // (KD8Charge granada de Reaper, AutoTurret de Raven,
                // Broodling, Locust, Changeling, Interceptor) e
                // qualquer tipo desconhecido. Sem essa whitelist o
                // bloco resultante caía no fallback de proximidade do
                // `resolve_producer` e era atribuído à
                // Barracks/Factory/Starport mais próxima — gerando
                // blocos `Producing` fantasmas com `produced_type=None`.
                //
                // Addons (Reactor/TechLab) tecnicamente estão em
                // `intern_unit_name` mas vão pelo fluxo dedicado de
                // `pending_addon` (bloco `Impeded`), então excluímos
                // explicitamente aqui.
                intern_unit_name(name).is_some() && !is_incapacitating_addon(name)
            }
        }
    }
}

/// Captura o nome estaticamente embutido pra unidades-alvo. Como
/// `EntityEvent.entity_type` é `String`, precisamos de uma tabela de
/// nomes-com-ciclo-de-vida-`'static` para colocar em `produced_type`.
/// Cobre todas as unidades army (T/P/Z), workers e os addons Terran.
pub(super) fn intern_unit_name(name: &str) -> Option<&'static str> {
    Some(match name {
        // Terran
        "SCV" => "SCV",
        "MULE" => "MULE",
        "Marine" => "Marine",
        "Marauder" => "Marauder",
        "Reaper" => "Reaper",
        "Ghost" => "Ghost",
        "Hellion" => "Hellion",
        "Hellbat" => "Hellbat",
        "WidowMine" => "WidowMine",
        "SiegeTank" => "SiegeTank",
        "Cyclone" => "Cyclone",
        "Thor" => "Thor",
        "VikingFighter" => "VikingFighter",
        "Medivac" => "Medivac",
        "Liberator" => "Liberator",
        "Banshee" => "Banshee",
        "Raven" => "Raven",
        "Battlecruiser" => "Battlecruiser",
        // Terran addons (modo Army Terran — Impeded)
        "BarracksReactor" => "BarracksReactor",
        "BarracksTechLab" => "BarracksTechLab",
        "FactoryReactor" => "FactoryReactor",
        "FactoryTechLab" => "FactoryTechLab",
        "StarportReactor" => "StarportReactor",
        "StarportTechLab" => "StarportTechLab",
        // Protoss
        "Probe" => "Probe",
        "Zealot" => "Zealot",
        "Stalker" => "Stalker",
        "Sentry" => "Sentry",
        "Adept" => "Adept",
        "HighTemplar" => "HighTemplar",
        "DarkTemplar" => "DarkTemplar",
        "Immortal" => "Immortal",
        "Colossus" => "Colossus",
        "Disruptor" => "Disruptor",
        "Observer" => "Observer",
        "WarpPrism" => "WarpPrism",
        "Phoenix" => "Phoenix",
        "VoidRay" => "VoidRay",
        "Oracle" => "Oracle",
        "Tempest" => "Tempest",
        "Carrier" => "Carrier",
        "Mothership" => "Mothership",
        // Zerg
        "Drone" => "Drone",
        "Overlord" => "Overlord",
        "Zergling" => "Zergling",
        "Queen" => "Queen",
        "Roach" => "Roach",
        "Hydralisk" => "Hydralisk",
        "Infestor" => "Infestor",
        "SwarmHost" => "SwarmHost",
        "SwarmHostMP" => "SwarmHostMP",
        "Mutalisk" => "Mutalisk",
        "Corruptor" => "Corruptor",
        "Viper" => "Viper",
        "Ultralisk" => "Ultralisk",
        _ => return None,
    })
}
