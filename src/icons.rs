use image::DynamicImage;

/// Tamanho fixo (px) para todos os ícones exibidos nas imagens.
pub const ICON_SIZE: u32 = 32;

/// Retorna o ícone redimensionado para `action` na `race`, se disponível.
///
/// `action` é o nome bruto do tipo de unidade ou upgrade vindo do replay
/// (ex: "Marine", "SiegeTank", "TerranInfantryWeaponsLevel1").
pub fn lookup(race: &str, action: &str) -> Option<DynamicImage> {
    let bytes = if race.eq_ignore_ascii_case("Terran") {
        terran_bytes(action)?
    } else {
        return None;
    };
    let img = image::load_from_memory(bytes).ok()?;
    Some(img.resize_exact(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3))
}

fn terran_bytes(action: &str) -> Option<&'static [u8]> {
    Some(match action {
        // ── Unidades ─────────────────────────────────────────────────────────
        "Marine"              => include_bytes!("images/terran/marine.png"),
        "Marauder"            => include_bytes!("images/terran/marauder.png"),
        "Reaper"              => include_bytes!("images/terran/reaper.png"),
        "Ghost"               => include_bytes!("images/terran/ghost.png"),
        "Hellion"             => include_bytes!("images/terran/hellion.png"),
        "Hellbat"
        | "HellionTank"       => include_bytes!("images/terran/hellbat.png"),
        "SiegeTank"
        | "SiegeTankSieged"   => include_bytes!("images/terran/siege_tank.png"),
        "Thor"
        | "ThorAP"            => include_bytes!("images/terran/thor.png"),
        "Battlecruiser"       => include_bytes!("images/terran/battlecruiser.png"),
        "Banshee"             => include_bytes!("images/terran/banshee.png"),
        "Raven"               => include_bytes!("images/terran/raven.png"),
        "Liberator"
        | "LiberatorAG"       => include_bytes!("images/terran/liberator.png"),
        "VikingFighter"
        | "VikingAssault"     => include_bytes!("images/terran/viking.png"),
        "Medivac"             => include_bytes!("images/terran/medivac.png"),
        "Cyclone"             => include_bytes!("images/terran/cyclone.png"),
        "WidowMine"
        | "WidowMineBurrowed" => include_bytes!("images/terran/widow_mine.png"),
        "SCV"                 => include_bytes!("images/terran/scv.png"),
        "MULE" | "Mule"       => include_bytes!("images/terran/mule.png"),

        // ── Construções — base ────────────────────────────────────────────────
        "CommandCenter"                   => include_bytes!("images/structures/command_center.png"),
        "OrbitalCommand"                  => include_bytes!("images/structures/orbital_command.png"),
        "PlanetaryFortress"               => include_bytes!("images/structures/planetary_fortress.png"),
        "SupplyDepot" | "SupplyDepotLowered" => include_bytes!("images/structures/supply_depot.png"),
        "Refinery"                        => include_bytes!("images/structures/refinery.png"),

        // ── Construções — produção ────────────────────────────────────────────
        "Barracks"  => include_bytes!("images/structures/barracks.png"),
        "Factory"   => include_bytes!("images/structures/factory.png"),
        "Starport"  => include_bytes!("images/structures/starport.png"),

        // ── Construções — tecnologia ──────────────────────────────────────────
        "EngineeringBay" => include_bytes!("images/structures/engineering_bay.png"),
        "Armory"         => include_bytes!("images/structures/armory.png"),
        "FusionCore"     => include_bytes!("images/structures/fusion_core.png"),
        "GhostAcademy"   => include_bytes!("images/structures/ghost_academy.png"),

        // ── Construções — defesa ──────────────────────────────────────────────
        "Bunker"       => include_bytes!("images/structures/bunker.png"),
        "MissileTurret"=> include_bytes!("images/structures/missile_turret.png"),
        "SensorTower"  => include_bytes!("images/structures/sensor_tower.png"),

        // ── Construções — add-ons ─────────────────────────────────────────────
        "BarracksTechLab"
        | "FactoryTechLab"
        | "StarportTechLab" => include_bytes!("images/structures/tech_lab.png"),
        "BarracksReactor"
        | "FactoryReactor"
        | "StarportReactor" => include_bytes!("images/structures/reactor.png"),

        // ── Upgrades — armas de infantaria ───────────────────────────────────
        "TerranInfantryWeaponsLevel1" => include_bytes!("images/terran/infantry_weapons_1.png"),
        "TerranInfantryWeaponsLevel2" => include_bytes!("images/terran/infantry_weapons_2.png"),
        "TerranInfantryWeaponsLevel3" => include_bytes!("images/terran/infantry_weapons_3.png"),

        // ── Upgrades — armadura de infantaria ────────────────────────────────
        "TerranInfantryArmorsLevel1" => include_bytes!("images/terran/infantry_armor_1.png"),
        "TerranInfantryArmorsLevel2" => include_bytes!("images/terran/infantry_armor_2.png"),
        "TerranInfantryArmorsLevel3" => include_bytes!("images/terran/infantry_armor_3.png"),

        // ── Upgrades — armas de veículos ─────────────────────────────────────
        "TerranVehicleWeaponsLevel1" => include_bytes!("images/terran/vehicle_weapons_1.png"),
        "TerranVehicleWeaponsLevel2" => include_bytes!("images/terran/vehicle_weapons_2.png"),
        "TerranVehicleWeaponsLevel3" => include_bytes!("images/terran/vehicle_weapons_3.png"),

        // ── Upgrades — blindagem de veículos e naves ─────────────────────────
        "TerranVehicleAndShipArmorsLevel1"
        | "TerranVehicleAndShipPlatingLevel1" => include_bytes!("images/terran/vehicle_and_ship_plating_1.png"),
        "TerranVehicleAndShipArmorsLevel2"
        | "TerranVehicleAndShipPlatingLevel2" => include_bytes!("images/terran/vehicle_and_ship_plating_2.png"),
        "TerranVehicleAndShipArmorsLevel3"
        | "TerranVehicleAndShipPlatingLevel3" => include_bytes!("images/terran/vehicle_and_ship_plating_3.png"),

        // ── Upgrades — armas de naves ─────────────────────────────────────────
        "TerranShipWeaponsLevel1" => include_bytes!("images/terran/ship_weapons_1.png"),
        "TerranShipWeaponsLevel2" => include_bytes!("images/terran/ship_weapons_2.png"),
        "TerranShipWeaponsLevel3" => include_bytes!("images/terran/ship_weapons_3.png"),

        // ── Upgrades — habilidades de infantaria ─────────────────────────────
        "Stimpack"         => include_bytes!("images/terran/stimpack.png"),
        "ShieldWall"       => include_bytes!("images/terran/combat_shield.png"),
        "ConcussiveShells" => include_bytes!("images/terran/concussive_shells.png"),
        "PersonalCloaking"
        | "BansheeCloak"   => include_bytes!("images/terran/cloaking.png"),

        // ── Upgrades — habilidades de veículos ───────────────────────────────
        "InfernalPreIgniter"  => include_bytes!("images/terran/infernal_pre_igniter.png"),
        "MagFieldAccelerator" => include_bytes!("images/terran/mag_field_accelerator.png"),

        // ── Upgrades — estruturas ─────────────────────────────────────────────
        "HiSecAutoTracking"               => include_bytes!("images/terran/hi_sec_auto_tracking.png"),
        "NeosteelFrame" | "NeosteelArmor" => include_bytes!("images/terran/neosteel_armor.png"),

        _ => return None,
    })
}
