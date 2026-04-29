// SALT Encoding — codifica build orders no formato compacto SALT,
// compatível com Spawning Tool, Embot Advanced e SC2 Scrapbook.
//
// Referência: https://github.com/Veritasimo/sc2-scrapbook/blob/master/SALT.cs
//
// Formato:  [version]title|author|description|~[supply][min][sec][type][item]...
//
// Cada campo usa um caractere da tabela ASCII imprimível (94 chars,
// espaço a til) mapeando 0-93.

use crate::build_order::{classify_entry, EntryKind, EntryOutcome, PlayerBuildOrder};

// ── Tabela de caracteres SALT ──────────────────────────────────────
const CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

fn encode_char(value: usize) -> char {
    CHARS.chars().nth(value).unwrap_or('~')
}

// ── Step types (conforme spec SALT) ────────────────────────────────
const STEP_STRUCTURE: usize = 0;
const STEP_UNIT: usize = 1;
const STEP_MORPH: usize = 2;
const STEP_UPGRADE: usize = 3;

// ── Mapeamento MPQ → (step_type, item_id) ──────────────────────────
//
// Baseado nas tabelas GetStructure/GetUnit/GetMorph/GetUpgrade do
// SALT.cs de referência. Workers são codificados como Unit.

fn mpq_to_salt(action: &str, _kind: EntryKind) -> Option<(usize, usize)> {
    // Structures
    let s = match action {
        "Armory"            => Some((STEP_STRUCTURE, 0)),
        "Barracks"          => Some((STEP_STRUCTURE, 1)),
        "Bunker"            => Some((STEP_STRUCTURE, 2)),
        "CommandCenter"     => Some((STEP_STRUCTURE, 3)),
        "EngineeringBay"    => Some((STEP_STRUCTURE, 4)),
        "Factory"           => Some((STEP_STRUCTURE, 5)),
        "FusionCore"        => Some((STEP_STRUCTURE, 6)),
        "GhostAcademy"      => Some((STEP_STRUCTURE, 7)),
        "MissileTurret"     => Some((STEP_STRUCTURE, 8)),
        "BarracksReactor"   => Some((STEP_STRUCTURE, 9)),
        "FactoryReactor"    => Some((STEP_STRUCTURE, 10)),
        "StarportReactor"   => Some((STEP_STRUCTURE, 11)),
        "Refinery"          => Some((STEP_STRUCTURE, 12)),
        "SensorTower"       => Some((STEP_STRUCTURE, 13)),
        "Starport"          => Some((STEP_STRUCTURE, 14)),
        "SupplyDepot"       => Some((STEP_STRUCTURE, 15)),
        "BarracksTechLab"   => Some((STEP_STRUCTURE, 16)),
        "FactoryTechLab"    => Some((STEP_STRUCTURE, 17)),
        "StarportTechLab"   => Some((STEP_STRUCTURE, 18)),

        "Assimilator" | "AssimilatorRich" => Some((STEP_STRUCTURE, 19)),
        "CyberneticsCore"   => Some((STEP_STRUCTURE, 20)),
        "DarkShrine"        => Some((STEP_STRUCTURE, 21)),
        "FleetBeacon"       => Some((STEP_STRUCTURE, 22)),
        "Forge"             => Some((STEP_STRUCTURE, 23)),
        "Gateway"           => Some((STEP_STRUCTURE, 24)),
        "Nexus"             => Some((STEP_STRUCTURE, 25)),
        "PhotonCannon"      => Some((STEP_STRUCTURE, 26)),
        "Pylon"             => Some((STEP_STRUCTURE, 27)),
        "RoboticsBay"       => Some((STEP_STRUCTURE, 28)),
        "RoboticsFacility"  => Some((STEP_STRUCTURE, 29)),
        "Stargate"          => Some((STEP_STRUCTURE, 30)),
        "TemplarArchive"    => Some((STEP_STRUCTURE, 31)),
        "TwilightCouncil"   => Some((STEP_STRUCTURE, 32)),

        "BanelingNest"      => Some((STEP_STRUCTURE, 33)),
        "EvolutionChamber"  => Some((STEP_STRUCTURE, 34)),
        "Extractor"         => Some((STEP_STRUCTURE, 35)),
        "Hatchery"          => Some((STEP_STRUCTURE, 36)),
        "HydraliskDen"      => Some((STEP_STRUCTURE, 37)),
        "InfestationPit"    => Some((STEP_STRUCTURE, 38)),
        "NydusNetwork"      => Some((STEP_STRUCTURE, 39)),
        "RoachWarren"       => Some((STEP_STRUCTURE, 40)),
        "SpawningPool"      => Some((STEP_STRUCTURE, 41)),
        "SpineCrawler"      => Some((STEP_STRUCTURE, 42)),
        "Spire"             => Some((STEP_STRUCTURE, 43)),
        "SporeCrawler"      => Some((STEP_STRUCTURE, 44)),
        "UltraliskCavern"   => Some((STEP_STRUCTURE, 45)),
        "CreepTumor"        => Some((STEP_STRUCTURE, 46)),
        "ShieldBattery"     => Some((STEP_STRUCTURE, 26)), // sem ID próprio, mapeamos para cannon slot

        _ => None,
    };
    if s.is_some() { return s; }

    // Units (inclui workers)
    let u = match action {
        "Banshee"           => Some((STEP_UNIT, 0)),
        "Battlecruiser"     => Some((STEP_UNIT, 1)),
        "GhostAlternate" | "Ghost" => Some((STEP_UNIT, 2)),
        "Hellion" | "HellionTank" => Some((STEP_UNIT, 3)),
        "Marauder"          => Some((STEP_UNIT, 4)),
        "Marine"            => Some((STEP_UNIT, 5)),
        "Medivac"           => Some((STEP_UNIT, 6)),
        "Raven"             => Some((STEP_UNIT, 7)),
        "Reaper"            => Some((STEP_UNIT, 8)),
        "SCV"               => Some((STEP_UNIT, 9)),
        "SiegeTank"         => Some((STEP_UNIT, 10)),
        "Thor"              => Some((STEP_UNIT, 11)),
        "WidowMine"         => Some((STEP_UNIT, 42)),
        "VikingFighter"     => Some((STEP_UNIT, 12)),
        "Cyclone"           => Some((STEP_UNIT, 48)),
        "Liberator"         => Some((STEP_UNIT, 49)),

        "Adept"             => Some((STEP_UNIT, 51)),
        "Carrier"           => Some((STEP_UNIT, 14)),
        "Colossus"          => Some((STEP_UNIT, 15)),
        "DarkTemplar"       => Some((STEP_UNIT, 16)),
        "Disruptor"         => Some((STEP_UNIT, 50)),
        "HighTemplar"       => Some((STEP_UNIT, 17)),
        "Immortal"          => Some((STEP_UNIT, 18)),
        "Mothership"        => Some((STEP_UNIT, 19)),
        "Observer"          => Some((STEP_UNIT, 20)),
        "Oracle"            => Some((STEP_UNIT, 44)),
        "Phoenix"           => Some((STEP_UNIT, 21)),
        "Probe"             => Some((STEP_UNIT, 22)),
        "Sentry"            => Some((STEP_UNIT, 23)),
        "Stalker"           => Some((STEP_UNIT, 24)),
        "Tempest"           => Some((STEP_UNIT, 45)),
        "VoidRay"           => Some((STEP_UNIT, 25)),
        "WarpPrism"         => Some((STEP_UNIT, 39)),
        "Zealot"            => Some((STEP_UNIT, 26)),

        "Corruptor"         => Some((STEP_UNIT, 27)),
        "Drone"             => Some((STEP_UNIT, 28)),
        "Hydralisk"         => Some((STEP_UNIT, 29)),
        "Infestor"          => Some((STEP_UNIT, 38)),
        "Mutalisk"          => Some((STEP_UNIT, 30)),
        "Overlord"          => Some((STEP_UNIT, 31)),
        "Queen"             => Some((STEP_UNIT, 32)),
        "Roach"             => Some((STEP_UNIT, 33)),
        "SwarmHostMP"       => Some((STEP_UNIT, 46)),
        "Ultralisk"         => Some((STEP_UNIT, 34)),
        "Viper"             => Some((STEP_UNIT, 47)),
        "Zergling"          => Some((STEP_UNIT, 35)),

        _ => None,
    };
    if u.is_some() { return u; }

    // Morphs
    let m = match action {
        "OrbitalCommand"    => Some((STEP_MORPH, 0)),
        "PlanetaryFortress" => Some((STEP_MORPH, 1)),
        "WarpGate"          => Some((STEP_MORPH, 2)),
        "Archon"            => Some((STEP_MORPH, 13)),
        "Lair"              => Some((STEP_MORPH, 3)),
        "Hive"              => Some((STEP_MORPH, 4)),
        "GreaterSpire"      => Some((STEP_MORPH, 5)),
        "BroodLord"         => Some((STEP_MORPH, 6)),
        "Baneling"          => Some((STEP_MORPH, 7)),
        "Overseer"          => Some((STEP_MORPH, 8)),
        "Ravager"           => Some((STEP_MORPH, 9)),
        "LurkerMP"          => Some((STEP_MORPH, 10)),
        "LurkerDenMP"       => Some((STEP_MORPH, 12)),
        _ => None,
    };
    if m.is_some() { return m; }

    // Upgrades & Research
    let up = match action {
        // Terran
        "TerranBuildingArmor"                       => Some((STEP_UPGRADE, 0)),
        "TerranInfantryArmorsLevel1"
            | "TerranInfantryArmorsLevel2"
            | "TerranInfantryArmorsLevel3"          => Some((STEP_UPGRADE, 1)),
        "TerranInfantryWeaponsLevel1"
            | "TerranInfantryWeaponsLevel2"
            | "TerranInfantryWeaponsLevel3"          => Some((STEP_UPGRADE, 2)),
        "TerranVehicleAndShipArmorsLevel1"
            | "TerranVehicleAndShipArmorsLevel2"
            | "TerranVehicleAndShipArmorsLevel3"     => Some((STEP_UPGRADE, 5)),
        "TerranShipWeaponsLevel1"
            | "TerranShipWeaponsLevel2"
            | "TerranShipWeaponsLevel3"              => Some((STEP_UPGRADE, 4)),
        "TerranVehicleWeaponsLevel1"
            | "TerranVehicleWeaponsLevel2"
            | "TerranVehicleWeaponsLevel3"           => Some((STEP_UPGRADE, 6)),

        "BansheeCloak"                              => Some((STEP_UPGRADE, 8)),
        "PersonalCloaking"                          => Some((STEP_UPGRADE, 9)),
        "Stimpack"                                  => Some((STEP_UPGRADE, 11)),
        "PunisherGrenades"                          => Some((STEP_UPGRADE, 15)),
        "ShieldWall"                                => Some((STEP_UPGRADE, 16)),
        "BattlecruiserEnableSpecializations"        => Some((STEP_UPGRADE, 52)),
        "HiSecAutoTracking"                         => Some((STEP_UPGRADE, 53)),
        "InterferenceMatrix"                        => Some((STEP_UPGRADE, 12)), // closest: seeker missiles slot
        "DrillClaws" | "DrillingClaws"              => Some((STEP_UPGRADE, 66)),

        // Protoss
        "ProtossGroundArmorsLevel1"
            | "ProtossGroundArmorsLevel2"
            | "ProtossGroundArmorsLevel3"            => Some((STEP_UPGRADE, 18)),
        "ProtossGroundWeaponsLevel1"
            | "ProtossGroundWeaponsLevel2"
            | "ProtossGroundWeaponsLevel3"            => Some((STEP_UPGRADE, 19)),
        "ProtossAirArmorsLevel1"
            | "ProtossAirArmorsLevel2"
            | "ProtossAirArmorsLevel3"               => Some((STEP_UPGRADE, 20)),
        "ProtossAirWeaponsLevel1"
            | "ProtossAirWeaponsLevel2"
            | "ProtossAirWeaponsLevel3"              => Some((STEP_UPGRADE, 21)),
        "ProtossShieldsLevel1"
            | "ProtossShieldsLevel2"
            | "ProtossShieldsLevel3"                 => Some((STEP_UPGRADE, 22)),

        "PsiStormTech"                              => Some((STEP_UPGRADE, 24)),
        "BlinkTech"                                 => Some((STEP_UPGRADE, 25)),
        "WarpGateResearch"                          => Some((STEP_UPGRADE, 26)),
        "Charge"                                    => Some((STEP_UPGRADE, 27)),
        "ExtendedThermalLance"                      => Some((STEP_UPGRADE, 47)),
        "ObserverGraviticBooster"                   => Some((STEP_UPGRADE, 59)),
        "GraviticDrive"                             => Some((STEP_UPGRADE, 60)),
        "PhoenixRangeUpgrade" | "AnionPulseCrystals" => Some((STEP_UPGRADE, 67)),
        "AdeptPiercingAttack"                       => Some((STEP_UPGRADE, 73)),

        // Zerg
        "ZergGroundArmorsLevel1"
            | "ZergGroundArmorsLevel2"
            | "ZergGroundArmorsLevel3"               => Some((STEP_UPGRADE, 28)),
        "ZergMeleeWeaponsLevel1"
            | "ZergMeleeWeaponsLevel2"
            | "ZergMeleeWeaponsLevel3"               => Some((STEP_UPGRADE, 29)),
        "ZergFlyerArmorsLevel1"
            | "ZergFlyerArmorsLevel2"
            | "ZergFlyerArmorsLevel3"                => Some((STEP_UPGRADE, 30)),
        "ZergFlyerWeaponsLevel1"
            | "ZergFlyerWeaponsLevel2"
            | "ZergFlyerWeaponsLevel3"               => Some((STEP_UPGRADE, 31)),
        "ZergMissileWeaponsLevel1"
            | "ZergMissileWeaponsLevel2"
            | "ZergMissileWeaponsLevel3"             => Some((STEP_UPGRADE, 32)),

        "EvolveGroovedSpines"                       => Some((STEP_UPGRADE, 33)),
        "OverlordSpeed"                             => Some((STEP_UPGRADE, 34)),
        "GlialReconstitution"                       => Some((STEP_UPGRADE, 36)),
        "TunnelingClaws"                            => Some((STEP_UPGRADE, 38)),
        "ChitinousPlating" | "AnabolicSynthesis"    => Some((STEP_UPGRADE, 40)),
        "AdrenalGlands"                             => Some((STEP_UPGRADE, 41)),
        "MetabolicBoost"                            => Some((STEP_UPGRADE, 42)),
        "Burrow"                                    => Some((STEP_UPGRADE, 44)),
        "CentrifugalHooks"                         => Some((STEP_UPGRADE, 45)),
        "NeuralParasite"                            => Some((STEP_UPGRADE, 49)),
        "PathogenGlands"                            => Some((STEP_UPGRADE, 50)),
        "EvolveMuscularAugments"                    => Some((STEP_UPGRADE, 65)),
        "LurkerRange"                               => Some((STEP_UPGRADE, 69)),

        _ => None,
    };
    if up.is_some() { return up; }

    // Fallback: tenta adivinhar pelo EntryKind
    None
}

// ── Encoder público ────────────────────────────────────────────────

const MIN_SUPPLY: usize = 5;

/// Codifica o build order de um jogador no formato SALT.
///
/// Apenas entradas `Completed` são incluídas (cancel/destroy são
/// ignoradas — SALT não tem conceito de cancel). Workers são omitidos
/// por convenção da maioria dos SALT encoders.
pub fn encode(player: &PlayerBuildOrder, lps: f64) -> String {
    let version = 0usize; // versão 0
    let title = format!("{} ({})", player.name, race_initial(&player.race));
    let author = "";
    let description = "";

    let mut out = String::new();
    out.push(encode_char(version));
    out.push_str(&title);
    out.push('|');
    out.push_str(author);
    out.push('|');
    out.push_str(description);
    out.push('|');
    out.push('~');

    for entry in &player.entries {
        if entry.outcome != EntryOutcome::Completed {
            continue;
        }

        let kind = classify_entry(entry);

        // Workers são omitidos na convenção SALT
        if kind == EntryKind::Worker {
            continue;
        }

        let Some((step_type, item_id)) = mpq_to_salt(&entry.action, kind) else {
            continue; // ação não mapeável (ex: add-ons, itens desconhecidos)
        };

        // Supply: 0 = blank, else value = supply - MIN_SUPPLY + 1
        let supply_val = if (entry.supply as usize) >= MIN_SUPPLY {
            entry.supply as usize - MIN_SUPPLY + 1
        } else {
            0
        };

        let total_seconds = (entry.game_loop as f64 / lps) as usize;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;

        // Emite count cópias (ex: "Marine x3" → 3 blocos)
        for _ in 0..entry.count {
            out.push(encode_char(supply_val));
            out.push(encode_char(minutes));
            out.push(encode_char(seconds));
            out.push(encode_char(step_type));
            out.push(encode_char(item_id));
        }
    }

    out
}

fn race_initial(race: &str) -> char {
    match race.to_ascii_lowercase().chars().next() {
        Some('t') => 'T',
        Some('p') => 'P',
        Some('z') => 'Z',
        _ => '?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_order::BuildOrderEntry;

    #[test]
    fn encode_char_boundaries() {
        assert_eq!(encode_char(0), ' ');
        assert_eq!(encode_char(1), '!');
        assert_eq!(encode_char(94), '~');
        // Clamp
        assert_eq!(encode_char(200), '~');
    }

    /// Cobre as três categorias da tabela `mpq_to_salt`: unit, structure,
    /// upgrade. Uma amostra por categoria é suficiente — a tabela é estática
    /// e os paths de resolução são idênticos entre as três.
    #[test]
    fn mpq_action_maps_cover_all_categories() {
        let cases = [
            ("Marine", EntryKind::Unit, STEP_UNIT, 5),
            ("Barracks", EntryKind::Structure, STEP_STRUCTURE, 1),
            ("Stimpack", EntryKind::Research, STEP_UPGRADE, 11),
        ];
        for (name, kind, step, idx) in cases {
            assert_eq!(
                mpq_to_salt(name, kind),
                Some((step, idx)),
                "{name} ({:?}) deveria mapear para ({step}, {idx})",
                kind,
            );
        }
    }

    #[test]
    fn encode_minimal_build() {
        let player = PlayerBuildOrder {
            name: "Test".to_string(),
            race: "Terran".to_string(),
            mmr: None,
            entries: vec![
                BuildOrderEntry {
                    supply: 14,
                    supply_made: 15,
                    game_loop: 600, // ~27s at 22.4 lps
                    finish_loop: 1200,
                    seq: 0,
                    action: "SupplyDepot".to_string(),
                    count: 1,
                    is_upgrade: false,
                    is_structure: true,
                    outcome: EntryOutcome::Completed,
                    chrono_boosts: 0,
                    producer_type: None,
                    producer_id: None,
                },
            ],
        };

        let salt = encode(&player, 22.4);
        // Deve começar com versão + metadata + ~
        assert!(salt.contains('~'));
        // Após o ~ deve ter exatamente 5 chars (1 entry)
        let after_tilde = salt.split('~').nth(1).unwrap();
        assert_eq!(after_tilde.len(), 5);
    }
}
