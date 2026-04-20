//! Coleta — numa única passada por `player.entries` — os fatos
//! relevantes para rotular a abertura. O resultado (`WindowFacts`) é
//! consumido pelos classificadores por raça e pelo fallback de
//! assinatura.

use crate::build_order::types::BuildOrderEntry;

/// Fatos sobre o início do replay de um jogador. Tudo privado ao
/// módulo `opening` — o único "produto" público é `OpeningLabel`.
#[derive(Default)]
pub(super) struct WindowFacts {
    /// Ao menos uma entry com `game_loop <= T_OPENING_END`.
    pub(super) has_any_before_opening_end: bool,

    // ── Tempos de primeira aparição (game_loop). `None` = não vi ──
    // Estruturas-chave
    pub(super) first_expansion_loop: Option<u32>, // Hatchery/CC/Nexus além do inicial
    pub(super) first_gas_loop: Option<u32>,       // Extractor/Refinery/Assimilator
    pub(super) spawning_pool_loop: Option<u32>,
    pub(super) first_barracks_loop: Option<u32>,
    pub(super) second_barracks_loop: Option<u32>,
    pub(super) third_barracks_loop: Option<u32>,
    pub(super) factory_loop: Option<u32>,
    pub(super) starport_loop: Option<u32>,
    pub(super) first_gateway_loop: Option<u32>,
    pub(super) second_gateway_loop: Option<u32>,
    pub(super) third_gateway_loop: Option<u32>,
    pub(super) fourth_gateway_loop: Option<u32>,
    pub(super) cybernetics_loop: Option<u32>,
    pub(super) forge_loop: Option<u32>,
    pub(super) stargate_loop: Option<u32>,
    pub(super) robotics_loop: Option<u32>,
    pub(super) twilight_loop: Option<u32>,
    pub(super) dark_shrine_loop: Option<u32>,
    pub(super) roach_warren_loop: Option<u32>,
    pub(super) baneling_nest_loop: Option<u32>,
    pub(super) lair_loop: Option<u32>,
    pub(super) reactor_loop: Option<u32>, // primeiro BarracksReactor (addon)
    pub(super) bunker_loop: Option<u32>,
    pub(super) photon_cannon_loop: Option<u32>,

    // Upgrades (start time)
    pub(super) metabolic_boost_loop: Option<u32>, // Zergling speed
    pub(super) stimpack_loop: Option<u32>,
    pub(super) warpgate_loop: Option<u32>,
    pub(super) blink_loop: Option<u32>,

    // Contagens de unidades produzidas no intervalo [0, follow_end]
    pub(super) marines: u32,
    pub(super) marauders: u32,
    pub(super) reapers: u32,
    pub(super) hellions: u32,
    pub(super) banshees: u32,
    pub(super) zerglings: u32,
    pub(super) banelings: u32,
    pub(super) roaches: u32,
    pub(super) ravagers: u32,
    pub(super) stalkers: u32,
    pub(super) sentries: u32,
    pub(super) phoenixes: u32,
    pub(super) immortals: u32,
    pub(super) void_rays: u32,
    pub(super) dark_templars: u32,

    // Supply no instante do SpawningPool (para rotular "14 Pool").
    pub(super) supply_at_pool: Option<u16>,

    // Supply signature: primeiros marcos chave para fallback honesto.
    pub(super) signature: Vec<(u16, String)>,
}

pub(super) fn collect_window_facts(
    entries: &[BuildOrderEntry],
    race: &str,
    open_end: u32,
    follow_end: u32,
) -> WindowFacts {
    let mut f = WindowFacts::default();
    let mut first_base_of_own_race_seen = false;

    // Nome canônico da "base" desse jogador — Hatchery para Zerg,
    // CommandCenter para Terran, Nexus para Protoss. O build order
    // não inclui a base inicial (ela não tem `creator_ability`),
    // então a primeira ocorrência já é a 2ª base (expansão).
    let expansion_name: &str = match race {
        "Zerg" => "Hatchery",
        "Terran" => "CommandCenter",
        "Protoss" => "Nexus",
        _ => "",
    };
    let gas_name: &str = match race {
        "Zerg" => "Extractor",
        "Terran" => "Refinery",
        "Protoss" => "Assimilator",
        _ => "",
    };

    for e in entries {
        if e.game_loop <= open_end {
            f.has_any_before_opening_end = true;
        }
        if e.game_loop > follow_end {
            break; // entries vêm ordenadas por game_loop (ver extract.rs)
        }

        let action = e.action.as_str();

        // Signature: primeiro gás, primeira expansão, primeira
        // estrutura de produção. Limitado a ~4 marcos.
        if f.signature.len() < 4 {
            let is_signature_worthy = action == expansion_name
                || action == gas_name
                || action == "SpawningPool"
                || action == "Barracks"
                || action == "Gateway";
            if is_signature_worthy {
                f.signature.push((e.supply, signature_name_for(action)));
            }
        }

        if action == expansion_name && !first_base_of_own_race_seen {
            first_base_of_own_race_seen = true;
            f.first_expansion_loop = Some(e.game_loop);
        }
        if action == gas_name && f.first_gas_loop.is_none() {
            f.first_gas_loop = Some(e.game_loop);
        }

        match action {
            "SpawningPool" => {
                if f.spawning_pool_loop.is_none() {
                    f.spawning_pool_loop = Some(e.game_loop);
                    f.supply_at_pool = Some(e.supply);
                }
            }
            "RoachWarren" => {
                if f.roach_warren_loop.is_none() {
                    f.roach_warren_loop = Some(e.game_loop);
                }
            }
            "BanelingNest" => {
                if f.baneling_nest_loop.is_none() {
                    f.baneling_nest_loop = Some(e.game_loop);
                }
            }
            "Lair" => {
                if f.lair_loop.is_none() {
                    f.lair_loop = Some(e.game_loop);
                }
            }
            "Barracks" => {
                if f.first_barracks_loop.is_none() {
                    f.first_barracks_loop = Some(e.game_loop);
                } else if f.second_barracks_loop.is_none() {
                    f.second_barracks_loop = Some(e.game_loop);
                } else if f.third_barracks_loop.is_none() {
                    f.third_barracks_loop = Some(e.game_loop);
                }
            }
            "Factory" => {
                if f.factory_loop.is_none() {
                    f.factory_loop = Some(e.game_loop);
                }
            }
            "Starport" => {
                if f.starport_loop.is_none() {
                    f.starport_loop = Some(e.game_loop);
                }
            }
            "BarracksReactor" => {
                if f.reactor_loop.is_none() {
                    f.reactor_loop = Some(e.game_loop);
                }
            }
            "Bunker" => {
                if f.bunker_loop.is_none() {
                    f.bunker_loop = Some(e.game_loop);
                }
            }
            "Gateway" => {
                if f.first_gateway_loop.is_none() {
                    f.first_gateway_loop = Some(e.game_loop);
                } else if f.second_gateway_loop.is_none() {
                    f.second_gateway_loop = Some(e.game_loop);
                } else if f.third_gateway_loop.is_none() {
                    f.third_gateway_loop = Some(e.game_loop);
                } else if f.fourth_gateway_loop.is_none() {
                    f.fourth_gateway_loop = Some(e.game_loop);
                }
            }
            "CyberneticsCore" => {
                if f.cybernetics_loop.is_none() {
                    f.cybernetics_loop = Some(e.game_loop);
                }
            }
            "Forge" => {
                if f.forge_loop.is_none() {
                    f.forge_loop = Some(e.game_loop);
                }
            }
            "Stargate" => {
                if f.stargate_loop.is_none() {
                    f.stargate_loop = Some(e.game_loop);
                }
            }
            "RoboticsFacility" => {
                if f.robotics_loop.is_none() {
                    f.robotics_loop = Some(e.game_loop);
                }
            }
            "TwilightCouncil" => {
                if f.twilight_loop.is_none() {
                    f.twilight_loop = Some(e.game_loop);
                }
            }
            "DarkShrine" => {
                if f.dark_shrine_loop.is_none() {
                    f.dark_shrine_loop = Some(e.game_loop);
                }
            }
            "PhotonCannon" => {
                if f.photon_cannon_loop.is_none() {
                    f.photon_cannon_loop = Some(e.game_loop);
                }
            }

            // Upgrades (start_loop já é o início do research)
            "zerglingmovementspeed" | "ZerglingMovementSpeed" => {
                if f.metabolic_boost_loop.is_none() {
                    f.metabolic_boost_loop = Some(e.game_loop);
                }
            }
            "Stimpack" => {
                if f.stimpack_loop.is_none() {
                    f.stimpack_loop = Some(e.game_loop);
                }
            }
            "WarpGate" | "WarpGateResearch" => {
                if f.warpgate_loop.is_none() {
                    f.warpgate_loop = Some(e.game_loop);
                }
            }
            "BlinkTech" | "Blink" | "blinktech" => {
                if f.blink_loop.is_none() {
                    f.blink_loop = Some(e.game_loop);
                }
            }

            // Contagens de unidades (somamos count porque entries
            // podem ser deduplicadas em grupos — ver deduplicate em
            // extract.rs).
            "Marine" => f.marines += e.count,
            "Marauder" => f.marauders += e.count,
            "Reaper" => f.reapers += e.count,
            "Hellion" | "HellionTank" => f.hellions += e.count,
            "Banshee" => f.banshees += e.count,
            "Zergling" => f.zerglings += e.count,
            "Baneling" => f.banelings += e.count,
            "Roach" => f.roaches += e.count,
            "Ravager" => f.ravagers += e.count,
            "Stalker" => f.stalkers += e.count,
            "Sentry" => f.sentries += e.count,
            "Phoenix" => f.phoenixes += e.count,
            "Immortal" => f.immortals += e.count,
            "VoidRay" => f.void_rays += e.count,
            "DarkTemplar" => f.dark_templars += e.count,
            _ => {}
        }
    }

    f
}

/// Nome curto e legível de uma estrutura para uso na assinatura de
/// supply. Encurta "CommandCenter" → "CC", "SpawningPool" → "Pool"
/// etc., seguindo o vocabulário usado pela comunidade.
pub(super) fn signature_name_for(action: &str) -> String {
    match action {
        "SpawningPool" => "Pool",
        "Hatchery" => "Hatch",
        "Extractor" => "Gas",
        "Refinery" => "Gas",
        "Assimilator" => "Gas",
        "CommandCenter" => "CC",
        "Barracks" => "Rax",
        "Nexus" => "Nexus",
        "Gateway" => "Gate",
        other => other,
    }
    .to_string()
}
