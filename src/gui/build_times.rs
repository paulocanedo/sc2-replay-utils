// Tempos de construção / pesquisa de SC2 LotV em velocidade Faster,
// expressos em "segundos" na mesma escala usada por `fmt_time`
// (loops brutos dividos por `loops_per_second` do replay). Valores
// aproximados, baseados em Liquipedia; a tabela é intencionalmente
// incompleta — ações desconhecidas retornam 0, o que preserva o
// tempo de conclusão como fallback seguro.
//
// Usado apenas pelo binário GUI, para converter o `game_loop` bruto
// (que reflete o instante de conclusão nos eventos `UnitBorn` e
// `Upgrade`) para o instante de início da ação, que é o que
// interessa num build order.

#![allow(dead_code)] // consumido apenas pelo binário GUI

/// Tempo de construção/pesquisa de uma ação em segundos. Retorna 0
/// quando não conhecido, caso em que o consumidor deve manter o
/// tempo original sem ajuste.
pub fn build_time_seconds(name: &str) -> u32 {
    match name {
        // ── Workers ──────────────────────────────────────────────
        "SCV" | "Probe" | "Drone" => 12,

        // ── Terran units ─────────────────────────────────────────
        "Marine" => 18,
        "Marauder" => 21,
        "Reaper" => 32,
        "Ghost" => 29,
        "Hellion" | "Hellbat" => 21,
        "WidowMine" => 21,
        "SiegeTank" => 32,
        "Cyclone" => 32,
        "Thor" => 43,
        "VikingFighter" => 30,
        "Medivac" => 30,
        "Liberator" => 43,
        "Raven" => 43,
        "Banshee" => 43,
        "Battlecruiser" => 64,

        // ── Protoss units ────────────────────────────────────────
        "Zealot" => 27,
        "Stalker" => 30,
        "Sentry" => 26,
        "Adept" => 27,
        "HighTemplar" => 39,
        "DarkTemplar" => 39,
        "Archon" => 9,
        "Immortal" => 39,
        "Colossus" => 54,
        "Disruptor" => 36,
        "Observer" => 21,
        "WarpPrism" => 36,
        "Phoenix" => 25,
        "VoidRay" => 43,
        "Oracle" => 37,
        "Tempest" => 43,
        "Carrier" => 64,
        "Mothership" => 89,

        // ── Zerg units (inclui tempo de morph da larva) ──────────
        "Zergling" => 17,
        "Queen" => 36,
        "Baneling" => 14,
        "Roach" => 19,
        "Ravager" => 9,
        "Hydralisk" => 24,
        "Lurker" => 18,
        "Mutalisk" => 24,
        "Corruptor" => 29,
        "BroodLord" => 24,
        "Infestor" => 36,
        "SwarmHost" => 29,
        "Ultralisk" => 39,
        "Viper" => 29,
        "Overlord" => 18,
        "Overseer" => 12,

        // ── Estruturas via morph (UnitBorn + MorphTo*) ───────────
        "OrbitalCommand" => 25,
        "PlanetaryFortress" => 36,
        "Lair" => 57,
        "Hive" => 71,
        "WarpGate" => 7,
        "GreaterSpire" => 71,

        // ── Terran: research / upgrades ──────────────────────────
        "Stimpack" => 121,
        "CombatShield" | "ShieldWall" => 79,
        "PunisherGrenades" | "ConcussiveShells" => 60,
        "HiSecAutoTracking" => 57,
        "TerranBuildingArmor" | "StructureArmorUpgrade" => 100,
        "TerranInfantryWeaponsLevel1" => 114,
        "TerranInfantryWeaponsLevel2" => 136,
        "TerranInfantryWeaponsLevel3" => 157,
        "TerranInfantryArmorsLevel1" => 114,
        "TerranInfantryArmorsLevel2" => 136,
        "TerranInfantryArmorsLevel3" => 157,
        "TerranVehicleWeaponsLevel1" => 114,
        "TerranVehicleWeaponsLevel2" => 136,
        "TerranVehicleWeaponsLevel3" => 157,
        "TerranVehicleAndShipArmorsLevel1" => 114,
        "TerranVehicleAndShipArmorsLevel2" => 136,
        "TerranVehicleAndShipArmorsLevel3" => 157,
        "TerranShipWeaponsLevel1" => 114,
        "TerranShipWeaponsLevel2" => 136,
        "TerranShipWeaponsLevel3" => 157,

        // ── Protoss: research / upgrades ─────────────────────────
        "WarpGateResearch" => 100,
        "BlinkTech" | "Blink" => 121,
        "Charge" => 100,
        "PsiStormTech" => 79,
        "ExtendedThermalLance" => 100,
        "GraviticBoosters" | "ObserverGraviticBooster" => 57,
        "GraviticDrive" => 57,
        "AnionPulseCrystals" => 64,
        "ProtossGroundWeaponsLevel1" => 129,
        "ProtossGroundWeaponsLevel2" => 154,
        "ProtossGroundWeaponsLevel3" => 179,
        "ProtossGroundArmorsLevel1" => 129,
        "ProtossGroundArmorsLevel2" => 154,
        "ProtossGroundArmorsLevel3" => 179,
        "ProtossShieldsLevel1" => 129,
        "ProtossShieldsLevel2" => 154,
        "ProtossShieldsLevel3" => 179,
        "ProtossAirWeaponsLevel1" => 129,
        "ProtossAirWeaponsLevel2" => 154,
        "ProtossAirWeaponsLevel3" => 179,
        "ProtossAirArmorsLevel1" => 129,
        "ProtossAirArmorsLevel2" => 154,
        "ProtossAirArmorsLevel3" => 179,

        // ── Zerg: research / upgrades ────────────────────────────
        "Burrow" => 79,
        "ZerglingMovementSpeed" | "MetabolicBoost" => 79,
        "ZerglingAttackSpeed" | "AdrenalGlands" => 93,
        "CentrifugalHooks" => 79,
        "GlialReconstitution" | "GlialRegeneration" => 79,
        "TunnelingClaws" => 79,
        "EvolveGroovedSpines" => 57,
        "EvolveMuscularAugments" => 57,
        "OverlordSpeed" | "PneumatizedCarapace" => 43,
        "OverlordTransport" | "VentralSacs" => 43,
        "NeuralParasite" => 79,
        "PathogenGlands" => 79,
        "ChitinousPlating" => 79,
        "AnabolicSynthesis" => 43,
        "ZergMeleeWeaponsLevel1" => 114,
        "ZergMeleeWeaponsLevel2" => 136,
        "ZergMeleeWeaponsLevel3" => 157,
        "ZergGroundArmorsLevel1" => 114,
        "ZergGroundArmorsLevel2" => 136,
        "ZergGroundArmorsLevel3" => 157,
        "ZergMissileWeaponsLevel1" => 114,
        "ZergMissileWeaponsLevel2" => 136,
        "ZergMissileWeaponsLevel3" => 157,
        "ZergFlyerWeaponsLevel1" => 114,
        "ZergFlyerWeaponsLevel2" => 136,
        "ZergFlyerWeaponsLevel3" => 157,
        "ZergFlyerArmorsLevel1" => 114,
        "ZergFlyerArmorsLevel2" => 136,
        "ZergFlyerArmorsLevel3" => 157,

        _ => 0,
    }
}
