use image::DynamicImage;

/// Tamanho fixo (px) para todos os ícones exibidos nas imagens.
pub const ICON_SIZE: u32 = 40;

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
        "AutoTurret"          => include_bytes!("images/terran/auto_turret.png"),
        "PointDefenseDrone"   => include_bytes!("images/terran/point_defense_drone.png"),

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

        _ => return None,
    })
}
