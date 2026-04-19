//! Classificação de abertura (opening label) para o build order.
//!
//! Produz uma string curta e legível no vocabulário da comunidade SC2
//! (`"Hatch First — Ling/Queen"`, `"3 Rax Reaper Expand"`,
//! `"Gate Expand — Stalker/Sentry"`) a partir do `PlayerBuildOrder` já
//! extraído pelo módulo irmão `extract.rs`.
//!
//! # Princípios de design
//!
//! - **Fonte única de verdade**: varre apenas `player.entries` — o
//!   mesmo stream canônico que alimenta a GUI, o CSV golden e os
//!   charts. Nada de re-decodificar tracker events.
//! - **Janela temporal curta**: `T_FOLLOW_UP_END = 5 min` de game
//!   time. Compatível com o `max_time_seconds = 300` que a biblioteca
//!   passa ao parser (scanner.rs) — assim a classificação roda sem
//!   parsear o replay inteiro.
//! - **Fallback honesto**: quando nenhuma heurística casa, devolvemos
//!   uma *assinatura de supply* feita dos dados reais
//!   (`"13 Pool, 15 Hatch"`) em vez de inventar um rótulo errado.
//! - **Nomenclatura em inglês** tanto em en quanto em pt-BR.
//!   Jogadores brasileiros também falam "Hatch First", "3 Rax",
//!   "Gate Expand". Só o fallback genérico é traduzido.

use super::types::{BuildOrderEntry, PlayerBuildOrder};

/// Nível de confiança do rótulo. `Named` = casou uma heurística
/// nomeada; `Signature` = fallback honesto baseado nos primeiros
/// marcos de supply; `Insufficient` = replay curto demais para
/// classificar (< 3 min de game time).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confidence {
    Named,
    Signature,
    Insufficient,
}

/// Rótulo de abertura pronto para display.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OpeningLabel {
    pub opening: String,
    pub follow_up: Option<String>,
    pub confidence: Confidence,
}

impl OpeningLabel {
    /// Monta a string de display: `"{opening} — {follow_up}"` ou só
    /// `"{opening}"`.
    pub fn to_display_string(&self) -> String {
        match &self.follow_up {
            Some(f) if !f.is_empty() => format!("{} — {}", self.opening, f),
            _ => self.opening.clone(),
        }
    }
}

/// Ponto de corte que separa **abertura** (escolha de pool/rax/gateway
/// vs. expansão vs. gás) de **follow-up** (primeiras unidades, upgrades
/// e tech). 3 min game time (normal speed: 3×60×22.4 ≈ 4032 loops).
/// Computado em loops a partir do `lps` real do replay.
const T_OPENING_END_SECS: u32 = 180;

/// Ponto de corte do follow-up. 5 min de game time — pega Stim
/// timing, primeira leva de unidades, primeiro upgrade de speed, etc.
const T_FOLLOW_UP_END_SECS: u32 = 300;

/// Se o replay sequer tem algum evento de build order nos primeiros
/// `T_OPENING_END_SECS`, devolvemos `Insufficient` com a string genérica
/// configurável pelo caller via i18n. Internamente usamos "Too short"
/// como placeholder — o caller que integra o label à UI (scanner.rs)
/// decide se traduz para `"Muito curto"`.
const INSUFFICIENT_PLACEHOLDER: &str = "Too short";

/// Fallback textual em inglês para signature vazia (replay sem
/// nenhum evento antes da janela — muito raro, normalmente só ocorre
/// em replays de 30s).
const SIGNATURE_FALLBACK: &str = "Standard opening";

// ── API pública ──────────────────────────────────────────────────────

/// Classifica a abertura de um jogador. Não toca disk/network — só
/// lê `player.entries`, já ordenado cronologicamente por start_loop.
///
/// `lps` é o `loops_per_second` do replay (normal 22.4 no LotV, outros
/// valores em replays legados). Usado para converter os cortes de
/// tempo em loops.
pub fn classify_opening(player: &PlayerBuildOrder, lps: f64) -> OpeningLabel {
    let open_end = (T_OPENING_END_SECS as f64 * lps).round() as u32;
    let follow_end = (T_FOLLOW_UP_END_SECS as f64 * lps).round() as u32;

    let facts = collect_window_facts(&player.entries, &player.race, open_end, follow_end);

    // Sem qualquer evento relevante até o fim da janela da abertura?
    // O replay provavelmente parou cedo (GG em 30s, replay corrompido).
    if !facts.has_any_before_opening_end {
        return OpeningLabel {
            opening: INSUFFICIENT_PLACEHOLDER.to_string(),
            follow_up: None,
            confidence: Confidence::Insufficient,
        };
    }

    let named = match player.race.as_str() {
        "Zerg" => classify_zerg(&facts),
        "Terran" => classify_terran(&facts),
        "Protoss" => classify_protoss(&facts),
        _ => None,
    };

    if let Some(label) = named {
        return label;
    }

    signature_fallback(&facts)
}

// ── Janela de observação ─────────────────────────────────────────────

/// Fatos sobre o início do replay de um jogador. Preenchido numa
/// única passada em `player.entries`. Tudo privado — o único
/// "produto" público deste módulo é `OpeningLabel`.
#[derive(Default)]
struct WindowFacts {
    /// Ao menos uma entry com `game_loop <= T_OPENING_END`.
    has_any_before_opening_end: bool,

    // ── Tempos de primeira aparição (game_loop). `None` = não vi ──
    // Estruturas-chave
    first_expansion_loop: Option<u32>, // Hatchery/CommandCenter/Nexus além do inicial
    first_gas_loop: Option<u32>,       // Extractor/Refinery/Assimilator
    spawning_pool_loop: Option<u32>,
    first_barracks_loop: Option<u32>,
    second_barracks_loop: Option<u32>,
    third_barracks_loop: Option<u32>,
    factory_loop: Option<u32>,
    starport_loop: Option<u32>,
    first_gateway_loop: Option<u32>,
    second_gateway_loop: Option<u32>,
    third_gateway_loop: Option<u32>,
    fourth_gateway_loop: Option<u32>,
    cybernetics_loop: Option<u32>,
    forge_loop: Option<u32>,
    stargate_loop: Option<u32>,
    robotics_loop: Option<u32>,
    twilight_loop: Option<u32>,
    dark_shrine_loop: Option<u32>,
    roach_warren_loop: Option<u32>,
    baneling_nest_loop: Option<u32>,
    lair_loop: Option<u32>,
    reactor_loop: Option<u32>, // primeiro BarracksReactor (addon)
    bunker_loop: Option<u32>,
    photon_cannon_loop: Option<u32>,

    // Upgrades (start time)
    metabolic_boost_loop: Option<u32>, // Zergling speed
    stimpack_loop: Option<u32>,
    warpgate_loop: Option<u32>,
    blink_loop: Option<u32>,

    // Contagens de unidades produzidas no intervalo [0, follow_end]
    marines: u32,
    marauders: u32,
    reapers: u32,
    hellions: u32,
    banshees: u32,
    zerglings: u32,
    banelings: u32,
    roaches: u32,
    ravagers: u32,
    stalkers: u32,
    sentries: u32,
    phoenixes: u32,
    immortals: u32,
    void_rays: u32,
    dark_templars: u32,

    // Supply no instante do SpawningPool (para rotular "14 Pool").
    supply_at_pool: Option<u16>,

    // Supply signature: primeiros marcos chave para fallback honesto.
    signature: Vec<(u16, String)>,
}

fn collect_window_facts(
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

        // Tempos de primeira aparição (structures + key upgrades)
        if action == expansion_name {
            if !first_base_of_own_race_seen {
                first_base_of_own_race_seen = true;
                f.first_expansion_loop = Some(e.game_loop);
            }
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
fn signature_name_for(action: &str) -> String {
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

// ── Classificadores por raça ─────────────────────────────────────────

fn classify_zerg(f: &WindowFacts) -> Option<OpeningLabel> {
    let pool = f.spawning_pool_loop?;
    let hatch = f.first_expansion_loop;
    let gas = f.first_gas_loop;

    let opening = match hatch {
        // Sem expansão até 5 min: extremamente aggressive; rotula
        // pela posição do pool.
        None => format!("{} Pool", f.supply_at_pool.unwrap_or(0)),
        Some(h) if pool < h => {
            // Pool antes da 2ª Hatch.
            match gas {
                Some(g) if g < pool => "Gas First".to_string(),
                _ => format!("{} Pool", f.supply_at_pool.unwrap_or(0)),
            }
        }
        Some(_) => {
            // Hatch antes do Pool.
            match gas {
                Some(g) if g < pool => "Hatch Gas Pool".to_string(),
                _ => "Hatch First".to_string(),
            }
        }
    };

    let follow_up = zerg_follow_up(f);
    Some(OpeningLabel {
        opening,
        follow_up,
        confidence: Confidence::Named,
    })
}

fn zerg_follow_up(f: &WindowFacts) -> Option<String> {
    // Ordem de prioridade: tech mais "assinante" primeiro.
    if f.baneling_nest_loop.is_some() && f.banelings >= 4 {
        return Some("Baneling Bust".to_string());
    }
    if f.ravagers >= 1 {
        return Some("Roach/Ravager".to_string());
    }
    if f.roach_warren_loop.is_some() && f.roaches >= 3 {
        return Some("Roach".to_string());
    }
    if f.lair_loop.is_some() {
        return Some("Fast Lair".to_string());
    }
    if f.metabolic_boost_loop.is_some() && f.zerglings >= 8 {
        return Some("Speedling".to_string());
    }
    if f.zerglings >= 2 {
        return Some("Ling/Queen".to_string());
    }
    if f.first_expansion_loop.is_some() {
        return Some("Macro".to_string());
    }
    None
}

fn classify_terran(f: &WindowFacts) -> Option<OpeningLabel> {
    let rax = f.first_barracks_loop?; // precisa ter pelo menos 1 rax
    let cc = f.first_expansion_loop;

    let opening = if let Some(cc_loop) = cc {
        if cc_loop < rax {
            // CC antes do Rax — "CC First" (super greedy, raro).
            "CC First".to_string()
        } else if f.third_barracks_loop.is_some()
            && f.third_barracks_loop.unwrap() < cc_loop
        {
            "3 Rax".to_string()
        } else if f.second_barracks_loop.is_some()
            && f.second_barracks_loop.unwrap() < cc_loop
        {
            "2 Rax".to_string()
        } else if f.factory_loop.is_some()
            && f.starport_loop.is_some()
            && f.factory_loop.unwrap() <= cc_loop.saturating_add(1)
        {
            // Ordem clássica: Rax → Factory → Starport antes ou perto
            // da expansão. Se os três existem até 5 min, rotulamos.
            "1-1-1".to_string()
        } else if f.reactor_loop.is_some()
            && f.reactor_loop.unwrap() < cc_loop
            && f.reapers >= 1
        {
            "Reaper Expand".to_string()
        } else {
            "1 Rax FE".to_string()
        }
    } else {
        // Sem expansão até 5 min — pressure/all-in.
        if f.third_barracks_loop.is_some() {
            "3 Rax".to_string()
        } else if f.second_barracks_loop.is_some() {
            "2 Rax".to_string()
        } else if f.factory_loop.is_some() && f.starport_loop.is_some() {
            "1-1-1".to_string()
        } else {
            "1 Rax".to_string()
        }
    };

    let follow_up = terran_follow_up(f);
    Some(OpeningLabel {
        opening,
        follow_up,
        confidence: Confidence::Named,
    })
}

fn terran_follow_up(f: &WindowFacts) -> Option<String> {
    if f.banshees >= 1 {
        return Some("Banshee".to_string());
    }
    if f.hellions >= 2 && f.reactor_loop.is_some() {
        return Some("Reactor Hellion".to_string());
    }
    if f.factory_loop.is_some() && f.marines < 4 {
        return Some("Mech".to_string());
    }
    if f.stimpack_loop.is_some() {
        return Some("Stim Timing".to_string());
    }
    if f.marauders >= 2 && f.marines >= 4 {
        return Some("Bio + Marauder".to_string());
    }
    if f.marines >= 6 {
        return Some("Bio".to_string());
    }
    if f.reapers >= 1 && f.reactor_loop.is_some() {
        return Some("Reaper Pressure".to_string());
    }
    None
}

fn classify_protoss(f: &WindowFacts) -> Option<OpeningLabel> {
    let gateway = f.first_gateway_loop;
    let nexus = f.first_expansion_loop;

    // "Cannon Rush" precisa preceder a pressão normal: Forge antes
    // do Gateway + pelo menos 1 cannon construído cedo.
    if let Some(forge) = f.forge_loop {
        if let Some(gate) = gateway {
            if forge < gate && f.photon_cannon_loop.is_some() {
                return Some(OpeningLabel {
                    opening: "Cannon Rush".to_string(),
                    follow_up: None,
                    confidence: Confidence::Named,
                });
            }
        } else {
            // Sem Gateway até 5 min + Forge cedo + Cannon → Cannon Rush.
            if f.photon_cannon_loop.is_some() {
                return Some(OpeningLabel {
                    opening: "Cannon Rush".to_string(),
                    follow_up: None,
                    confidence: Confidence::Named,
                });
            }
        }
    }

    let opening = match (nexus, gateway) {
        // Nexus antes do Gateway → FFE (quando há Forge cedo) ou
        // Nexus First (quando não há).
        (Some(n), Some(g)) if n < g => {
            if f.forge_loop.map_or(false, |fg| fg < n) {
                "Nexus First (FFE)".to_string()
            } else {
                "Nexus First".to_string()
            }
        }
        (Some(n), Some(_)) => {
            // Gateway antes do Nexus.
            if f.fourth_gateway_loop.is_some() && f.fourth_gateway_loop.unwrap() < n {
                "4 Gate".to_string()
            } else if f.third_gateway_loop.is_some() && f.third_gateway_loop.unwrap() < n {
                "3 Gate Expand".to_string()
            } else {
                "Gate Expand".to_string()
            }
        }
        (None, Some(_)) => {
            // Sem expansão até 5 min.
            if f.fourth_gateway_loop.is_some() {
                "4 Gate".to_string()
            } else if f.third_gateway_loop.is_some() {
                "3 Gate".to_string()
            } else {
                "1 Gate Tech".to_string()
            }
        }
        // Sem Gateway até a janela — não classificamos.
        _ => return None,
    };

    let follow_up = protoss_follow_up(f);
    Some(OpeningLabel {
        opening,
        follow_up,
        confidence: Confidence::Named,
    })
}

fn protoss_follow_up(f: &WindowFacts) -> Option<String> {
    if f.dark_shrine_loop.is_some() || f.dark_templars >= 1 {
        return Some("DT".to_string());
    }
    if f.blink_loop.is_some() {
        return Some("Blink".to_string());
    }
    if f.immortals >= 1 || f.robotics_loop.is_some() {
        return Some("Immortal".to_string());
    }
    if f.phoenixes >= 1 {
        return Some("Phoenix".to_string());
    }
    if f.void_rays >= 1 || f.stargate_loop.is_some() {
        return Some("Void Ray".to_string());
    }
    if f.stalkers >= 3 && f.sentries >= 1 {
        return Some("Stalker/Sentry".to_string());
    }
    if f.stalkers >= 3 {
        return Some("Stalker".to_string());
    }
    None
}

// ── Fallback de assinatura de supply ─────────────────────────────────

fn signature_fallback(f: &WindowFacts) -> OpeningLabel {
    if f.signature.is_empty() {
        return OpeningLabel {
            opening: SIGNATURE_FALLBACK.to_string(),
            follow_up: None,
            confidence: Confidence::Signature,
        };
    }

    // Até 3 marcos — mais que isso deixa de parecer "resumo".
    let parts: Vec<String> = f
        .signature
        .iter()
        .take(3)
        .map(|(supply, name)| format!("{} {}", supply, name))
        .collect();
    OpeningLabel {
        opening: parts.join(", "),
        follow_up: None,
        confidence: Confidence::Signature,
    }
}

// ── Testes sintéticos ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_order::types::EntryOutcome;

    /// Constrói uma `BuildOrderEntry` mínima para teste. `loop_secs` é o
    /// tempo de início em segundos (será convertido pra game loops por
    /// game_loop = secs * 22.4 arredondado); `supply` é o supply usado
    /// no início; `action` é o nome canônico.
    fn entry(action: &str, loop_secs: f64, supply: u16) -> BuildOrderEntry {
        let gl = (loop_secs * 22.4).round() as u32;
        BuildOrderEntry {
            supply,
            supply_made: supply + 2,
            game_loop: gl,
            finish_loop: gl + 100,
            seq: 0,
            action: action.to_string(),
            count: 1,
            is_upgrade: matches!(
                action,
                "Stimpack"
                    | "zerglingmovementspeed"
                    | "WarpGate"
                    | "WarpGateResearch"
                    | "BlinkTech"
                    | "Blink"
                    | "blinktech"
                    | "ZerglingMovementSpeed"
            ),
            is_structure: matches!(
                action,
                "SpawningPool"
                    | "Hatchery"
                    | "Extractor"
                    | "Refinery"
                    | "Assimilator"
                    | "CommandCenter"
                    | "Barracks"
                    | "Factory"
                    | "Starport"
                    | "BarracksReactor"
                    | "Gateway"
                    | "Nexus"
                    | "CyberneticsCore"
                    | "Forge"
                    | "Stargate"
                    | "RoboticsFacility"
                    | "TwilightCouncil"
                    | "DarkShrine"
                    | "RoachWarren"
                    | "BanelingNest"
                    | "Lair"
                    | "PhotonCannon"
                    | "Bunker"
            ),
            outcome: EntryOutcome::Completed,
            chrono_boosts: 0,
        }
    }

    fn player_with(race: &str, entries: Vec<BuildOrderEntry>) -> PlayerBuildOrder {
        PlayerBuildOrder {
            name: "Test".to_string(),
            race: race.to_string(),
            mmr: None,
            entries,
        }
    }

    // ── Zerg ────────────────────────────────────────────────────

    #[test]
    fn zerg_14_pool_detects_supply_and_labels() {
        let p = player_with(
            "Zerg",
            vec![
                entry("SpawningPool", 55.0, 14),
                entry("Extractor", 75.0, 15),
                entry("Hatchery", 110.0, 16),
                entry("Zergling", 140.0, 18),
                entry("Zergling", 142.0, 20),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "14 Pool");
        assert_eq!(lbl.confidence, Confidence::Named);
    }

    #[test]
    fn zerg_12_pool_differs_from_14_pool_by_supply() {
        let p = player_with(
            "Zerg",
            vec![
                entry("SpawningPool", 45.0, 12),
                entry("Hatchery", 140.0, 14),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "12 Pool");
    }

    #[test]
    fn zerg_hatch_first_labeled_correctly() {
        let p = player_with(
            "Zerg",
            vec![
                entry("Hatchery", 50.0, 17),
                entry("SpawningPool", 80.0, 17),
                entry("Extractor", 100.0, 18),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "Hatch First");
    }

    #[test]
    fn zerg_hatch_gas_pool_when_gas_before_pool() {
        let p = player_with(
            "Zerg",
            vec![
                entry("Hatchery", 40.0, 17),
                entry("Extractor", 55.0, 17),
                entry("SpawningPool", 80.0, 18),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "Hatch Gas Pool");
    }

    #[test]
    fn zerg_speedling_requires_metabolic_boost_and_lings() {
        let mut entries = vec![
            entry("SpawningPool", 55.0, 14),
            entry("Hatchery", 110.0, 16),
            entry("zerglingmovementspeed", 180.0, 22),
        ];
        for s in 0..9 {
            entries.push(entry("Zergling", 140.0 + s as f64, 18 + s as u16));
        }
        let p = player_with("Zerg", entries);
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.follow_up.as_deref(), Some("Speedling"));
    }

    #[test]
    fn zerg_baneling_bust_needs_nest_and_four_banes() {
        let mut entries = vec![
            entry("SpawningPool", 55.0, 14),
            entry("Hatchery", 110.0, 16),
            entry("BanelingNest", 180.0, 22),
        ];
        for _ in 0..4 {
            entries.push(entry("Baneling", 220.0, 28));
        }
        let p = player_with("Zerg", entries);
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.follow_up.as_deref(), Some("Baneling Bust"));
    }

    // ── Terran ──────────────────────────────────────────────────

    #[test]
    fn terran_1_rax_fe_named_correctly() {
        let p = player_with(
            "Terran",
            vec![
                entry("Barracks", 70.0, 15),
                entry("Refinery", 95.0, 16),
                entry("CommandCenter", 180.0, 19),
                entry("Marine", 200.0, 20),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "1 Rax FE");
        assert_eq!(lbl.confidence, Confidence::Named);
    }

    #[test]
    fn terran_3_rax_pressure_labeled() {
        let p = player_with(
            "Terran",
            vec![
                entry("Barracks", 70.0, 15),
                entry("Barracks", 100.0, 17),
                entry("Barracks", 140.0, 19),
                entry("Marine", 200.0, 21),
                entry("Marine", 210.0, 22),
                entry("Marine", 220.0, 23),
                entry("Marine", 230.0, 24),
                entry("Marine", 240.0, 25),
                entry("Marine", 250.0, 26),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "3 Rax");
    }

    #[test]
    fn terran_reaper_expand_requires_reactor_and_reaper() {
        let p = player_with(
            "Terran",
            vec![
                entry("Barracks", 70.0, 15),
                entry("BarracksReactor", 100.0, 16),
                entry("Reaper", 130.0, 17),
                entry("CommandCenter", 200.0, 20),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "Reaper Expand");
    }

    #[test]
    fn terran_cc_first_when_cc_before_rax() {
        let p = player_with(
            "Terran",
            vec![
                entry("CommandCenter", 40.0, 14),
                entry("Barracks", 80.0, 16),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "CC First");
    }

    #[test]
    fn terran_stim_timing_follow_up() {
        let p = player_with(
            "Terran",
            vec![
                entry("Barracks", 70.0, 15),
                entry("Refinery", 95.0, 16),
                entry("CommandCenter", 180.0, 19),
                entry("Stimpack", 250.0, 24),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.follow_up.as_deref(), Some("Stim Timing"));
    }

    // ── Protoss ─────────────────────────────────────────────────

    #[test]
    fn protoss_gate_expand_named() {
        let p = player_with(
            "Protoss",
            vec![
                entry("Gateway", 60.0, 14),
                entry("Assimilator", 85.0, 15),
                entry("CyberneticsCore", 100.0, 16),
                entry("Nexus", 180.0, 19),
                entry("Stalker", 210.0, 22),
                entry("Stalker", 220.0, 23),
                entry("Stalker", 225.0, 24),
                entry("Sentry", 230.0, 25),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "Gate Expand");
        assert_eq!(lbl.follow_up.as_deref(), Some("Stalker/Sentry"));
    }

    #[test]
    fn protoss_ffe_detected_when_forge_before_nexus() {
        let p = player_with(
            "Protoss",
            vec![
                entry("Forge", 50.0, 13),
                entry("Nexus", 90.0, 15),
                entry("Gateway", 140.0, 18),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "Nexus First (FFE)");
    }

    #[test]
    fn protoss_nexus_first_without_forge() {
        let p = player_with(
            "Protoss",
            vec![
                entry("Nexus", 70.0, 14),
                entry("Gateway", 150.0, 18),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "Nexus First");
    }

    #[test]
    fn protoss_4_gate_labeled_when_four_gateways() {
        let p = player_with(
            "Protoss",
            vec![
                entry("Gateway", 60.0, 14),
                entry("Gateway", 110.0, 17),
                entry("Gateway", 150.0, 19),
                entry("Gateway", 200.0, 21),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "4 Gate");
    }

    #[test]
    fn protoss_cannon_rush_when_forge_before_gateway_and_cannon_built() {
        let p = player_with(
            "Protoss",
            vec![
                entry("Forge", 30.0, 11),
                entry("PhotonCannon", 80.0, 13),
                entry("Gateway", 140.0, 15),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.opening, "Cannon Rush");
    }

    // ── Fallback ────────────────────────────────────────────────

    #[test]
    fn unknown_race_falls_back_to_signature() {
        let p = player_with(
            "Random",
            vec![
                entry("Barracks", 70.0, 15),
                entry("Refinery", 95.0, 16),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.confidence, Confidence::Signature);
        assert!(lbl.opening.contains("Rax"));
    }

    #[test]
    fn signature_fallback_has_at_most_three_marks() {
        let p = player_with(
            "Random",
            vec![
                entry("Barracks", 50.0, 14),
                entry("Refinery", 60.0, 15),
                entry("CommandCenter", 90.0, 17),
                entry("Barracks", 100.0, 18),
                entry("Barracks", 120.0, 19),
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        // 3 marcos separados por ", ": 2 vírgulas
        assert_eq!(lbl.opening.matches(',').count(), 2);
    }

    #[test]
    fn replay_with_no_entries_before_window_is_insufficient() {
        // Entries vazio → has_any_before_opening_end = false.
        let p = player_with("Zerg", vec![]);
        let lbl = classify_opening(&p, 22.4);
        assert_eq!(lbl.confidence, Confidence::Insufficient);
    }

    #[test]
    fn entries_beyond_follow_up_end_are_ignored() {
        // Pool às 05:30 (tardíssimo), fora da janela de 5 min.
        let p = player_with(
            "Zerg",
            vec![
                entry("Extractor", 60.0, 15),       // dentro da janela (supply marcado)
                entry("SpawningPool", 330.0, 40),   // 5:30 — fora da janela
            ],
        );
        let lbl = classify_opening(&p, 22.4);
        // Sem pool dentro da janela, cai no fallback de signature.
        assert_eq!(lbl.confidence, Confidence::Signature);
    }

    // ── Formatação ──────────────────────────────────────────────

    #[test]
    fn display_string_combines_opening_and_follow_up() {
        let lbl = OpeningLabel {
            opening: "1 Rax FE".to_string(),
            follow_up: Some("Stim Timing".to_string()),
            confidence: Confidence::Named,
        };
        assert_eq!(lbl.to_display_string(), "1 Rax FE — Stim Timing");
    }

    #[test]
    fn display_string_uses_opening_alone_when_no_follow_up() {
        let lbl = OpeningLabel {
            opening: "CC First".to_string(),
            follow_up: None,
            confidence: Confidence::Named,
        };
        assert_eq!(lbl.to_display_string(), "CC First");
    }

    // ── Smoke sobre replays reais ───────────────────────────────

    /// Sanity check: os replays em `examples/` devem gerar rótulos
    /// com confidence `Named` para os dois jogadores. Não validamos
    /// o conteúdo exato do rótulo aqui (isso fica para os golden
    /// tests); só garantimos que a classificação não degenera em
    /// signature/insufficient para replays bem-formados.
    #[test]
    fn smoke_golden_replay_produces_named_labels() {
        use crate::build_order::extract_build_order;
        use crate::replay::parse_replay;
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/old_republic_50.SC2Replay");
        let timeline = parse_replay(&path, 0).expect("parse");
        let bo = extract_build_order(&timeline).expect("extract");
        let lps = bo.loops_per_second;
        assert!(!bo.players.is_empty());
        for player in &bo.players {
            let lbl = classify_opening(player, lps);
            eprintln!(
                "  {:>10} ({:>7}): {}  [{:?}]",
                player.name,
                player.race,
                lbl.to_display_string(),
                lbl.confidence,
            );
            assert_eq!(
                lbl.confidence,
                Confidence::Named,
                "esperava Named para {} em replay golden, veio {:?} ({})",
                player.name, lbl.confidence, lbl.to_display_string(),
            );
        }
    }
}
