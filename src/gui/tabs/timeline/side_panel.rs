//! Painel lateral — stats de um jogador no instante de scrubbing.
//!
//! Organizado em blocos separados por dividers hairline:
//! 1. Identidade — nome + chip da raça.
//! 2. Supply — barra de progresso com destaque quando blocked.
//! 3. Economia — minerals/gas (com ícones desenhados) + rates,
//!    workers com barra de capacidade.
//! 4. Army — valor total + split mineral/gas + pips de attack/armor.
//! 5. Composição — chips com abreviação de 3 letras por unidade viva.
//! 6. Estruturas — chips com abreviação de 3 letras por estrutura viva.
//! 7. Pesquisas — chips com upgrades pontuais concluídos (não-leveled).
//! 8. Eficiência — building focus, idle time e supply block count.

use std::collections::HashMap;

use egui::{
    epaint::Shape, pos2, vec2, Align, Color32, Layout, ProgressBar, Rect, RichText, Sense, Stroke,
    Ui,
};

use crate::colors::{player_slot_color_bright, ACCENT_WARNING, LABEL_DIM, LABEL_SOFT};
use crate::config::AppConfig;
use crate::locale::{localize, tf, Language};
use crate::production_gap::{compute_idle_periods, compute_idle_periods_ranges, is_zerg_race};
use crate::replay::{is_structure_name, PlayerTimeline, StatsSnapshot, UpgradeEntry};
use crate::replay_state::LoadedReplay;
use crate::supply_block::SupplyBlockEntry;
use crate::tokens::{size_body, size_caption, size_subtitle, SPACE_S, SPACE_XS};
use crate::widgets::{chip, player_identity, NameDensity};

use super::entities::structure_attention_at;

/// Cor do diamante de minerals. Azul-claro próximo do cristal in-game.
const MINERAL_COLOR: Color32 = Color32::from_rgb(100, 180, 230);
/// Cor do círculo de gas. Verde-claro próximo do geyser Vespene.
const GAS_COLOR: Color32 = Color32::from_rgb(90, 200, 150);
/// Workers por base em saturação ideal (≈ 3 por patch + 3 por geyser).
const WORKERS_PER_BASE_IDEAL: i32 = 22;
/// Teto do denominador de saturação. Acima disso, `👷 N/M` repete o
/// numerador (barra cheia) pra não exibir razões tipo `90/80` que
/// saturam visualmente mas não dizem nada acionável.
const WORKER_SATURATION_CAP: i32 = 80;

/// Renderiza o painel lateral de um jogador. Faz lookup de todos os
/// dados derivados (`production`, `supply_blocks`) diretamente no
/// `LoadedReplay` pra evitar um fan-out de argumentos.
pub(super) fn player_side_panel(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    idx: usize,
    game_loop: u32,
    cfg: &AppConfig,
) {
    let Some(p) = loaded.timeline.players.get(idx) else {
        return;
    };
    let supply_blocks: &[SupplyBlockEntry] = loaded
        .supply_blocks_per_player
        .get(idx)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let loops_per_second = loaded.timeline.loops_per_second;
    let lang = cfg.language;
    let slot_color = player_slot_color_bright(idx);

    ui.add_space(SPACE_S);
    header(ui, p, idx, cfg, lang);
    ui.add_space(SPACE_XS);
    ui.separator();

    match p.stats_at(game_loop) {
        Some(s) => {
            ui.add_space(SPACE_XS);
            supply_bar(ui, s, slot_color, lang);
            ui.add_space(SPACE_S);
            economy_block(ui, p, s, game_loop, cfg, lang);
            ui.add_space(SPACE_S);
            ui.separator();
            ui.add_space(SPACE_XS);
            army_block(ui, p, s, game_loop, slot_color, cfg, lang);
        }
        None => {
            ui.add_space(SPACE_S);
            ui.weak("—");
        }
    }

    ui.add_space(SPACE_XS);
    ui.separator();
    ui.add_space(SPACE_XS);
    units_block(ui, p, game_loop, lang);

    ui.add_space(SPACE_XS);
    ui.separator();
    ui.add_space(SPACE_XS);
    structures_block(ui, p, game_loop, lang);

    ui.add_space(SPACE_XS);
    ui.separator();
    ui.add_space(SPACE_XS);
    researches_block(ui, p, game_loop, loops_per_second, lang);

    ui.add_space(SPACE_XS);
    ui.separator();
    ui.add_space(SPACE_XS);
    efficiency_block(ui, p, game_loop, supply_blocks, loops_per_second, lang);
}

// ── Header ─────────────────────────────────────────────────────────────

fn header(ui: &mut Ui, p: &PlayerTimeline, idx: usize, cfg: &AppConfig, lang: Language) {
    let is_user = cfg.is_user(&p.name);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        player_identity(
            ui,
            &p.name,
            &p.race,
            idx,
            is_user,
            NameDensity::Normal,
            cfg,
            lang,
        );
    });
}

// ── Supply bar ─────────────────────────────────────────────────────────

fn supply_bar(ui: &mut Ui, s: &StatsSnapshot, slot_color: Color32, lang: Language) {
    let cap = s.supply_made.min(200);
    let frac = if cap > 0 {
        (s.supply_used as f32 / cap as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let blocked = s.supply_made > 0 && s.supply_used >= s.supply_made;
    let bar_color = if blocked { ACCENT_WARNING } else { slot_color };
    let label = if blocked {
        format!("⚠ {}/{}", s.supply_used, cap)
    } else {
        format!("{}/{}", s.supply_used, cap)
    };
    let tt_key = if blocked {
        "timeline.tt.supply_blocked"
    } else {
        "timeline.tt.supply"
    };
    ui.add(
        ProgressBar::new(frac)
            .fill(bar_color)
            .text(RichText::new(label).small().strong()),
    )
    .on_hover_text(tf(tt_key, lang, &[]));
}

// ── Economy block ──────────────────────────────────────────────────────

fn economy_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    s: &StatsSnapshot,
    game_loop: u32,
    cfg: &AppConfig,
    lang: Language,
) {
    resource_row(ui, MINERAL_COLOR, s.minerals, s.minerals_rate, true, cfg, lang);
    resource_row(ui, GAS_COLOR, s.vespene, s.vespene_rate, false, cfg, lang);
    worker_row(ui, p, s, game_loop, lang);
}

fn resource_row(
    ui: &mut Ui,
    icon_color: Color32,
    value: i32,
    rate: i32,
    is_mineral: bool,
    cfg: &AppConfig,
    lang: Language,
) {
    let tt_key = if is_mineral {
        "timeline.tt.minerals"
    } else {
        "timeline.tt.vespene"
    };
    ui.horizontal(|ui| {
        let size = size_body(cfg);
        let (resp, painter) = ui.allocate_painter(vec2(size, size), Sense::hover());
        if is_mineral {
            paint_mineral_icon(&painter, resp.rect, icon_color);
        } else {
            paint_gas_icon(&painter, resp.rect, icon_color);
        }
        ui.monospace(value.to_string());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("+{}/m", rate))
                    .size(size_caption(cfg))
                    .color(LABEL_DIM),
            );
        });
    })
    .response
    .on_hover_text(tf(tt_key, lang, &[]));
}

fn worker_row(ui: &mut Ui, p: &PlayerTimeline, s: &StatsSnapshot, game_loop: u32, lang: Language) {
    // `worker_capacity_at` devolve o número de town halls vivos (cada
    // base = 1 slot de treinamento). Para exibição no painel, porém,
    // queremos mostrar saturação (workers / ideal) e não slot count.
    let bases = p.worker_capacity_at(game_loop);
    let saturation = (bases * WORKERS_PER_BASE_IDEAL).min(WORKER_SATURATION_CAP);
    let (denom, frac) = if s.workers > WORKER_SATURATION_CAP {
        (s.workers, 1.0)
    } else if saturation > 0 {
        (
            saturation,
            (s.workers as f32 / saturation as f32).clamp(0.0, 1.0),
        )
    } else {
        // Nenhuma base registrada ainda (fração inicial do replay).
        (s.workers.max(1), 1.0)
    };
    ui.horizontal(|ui| {
        ui.monospace(format!("👷 {}/{}", s.workers, denom));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            draw_mini_bar(ui, 42.0, 4.0, frac, LABEL_SOFT);
        });
    })
    .response
    .on_hover_text(tf("timeline.tt.workers", lang, &[]));
}

// ── Army block ─────────────────────────────────────────────────────────

fn army_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    s: &StatsSnapshot,
    game_loop: u32,
    slot_color: Color32,
    cfg: &AppConfig,
    lang: Language,
) {
    let total = s.army_value_minerals + s.army_value_vespene;
    ui.label(
        RichText::new(format!("⚔ {total}"))
            .size(size_subtitle(cfg))
            .strong()
            .color(slot_color),
    )
    .on_hover_text(tf("timeline.tt.army_value", lang, &[]));
    let caption = size_caption(cfg);
    ui.horizontal(|ui| {
        let (resp, painter) = ui.allocate_painter(vec2(caption, caption), Sense::hover());
        paint_mineral_icon(&painter, resp.rect, MINERAL_COLOR);
        ui.label(
            RichText::new(s.army_value_minerals.to_string())
                .size(caption)
                .color(LABEL_DIM),
        );
        ui.label(RichText::new("·").size(caption).color(LABEL_DIM));
        let (resp, painter) = ui.allocate_painter(vec2(caption, caption), Sense::hover());
        paint_gas_icon(&painter, resp.rect, GAS_COLOR);
        ui.label(
            RichText::new(s.army_value_vespene.to_string())
                .size(caption)
                .color(LABEL_DIM),
        );
    })
    .response
    .on_hover_text(tf("timeline.tt.army_split", lang, &[]));

    let atk = p.attack_level_at(game_loop);
    let arm = p.armor_level_at(game_loop);
    if atk > 0 || arm > 0 {
        ui.add_space(SPACE_XS);
        ui.horizontal(|ui| {
            if atk > 0 {
                chip(ui, &format!("⚔+{atk}"), true, Some(slot_color))
                    .on_hover_text(tf("timeline.tt.atk_upgrade", lang, &[]));
            }
            if arm > 0 {
                chip(ui, &format!("🛡+{arm}"), true, Some(slot_color))
                    .on_hover_text(tf("timeline.tt.arm_upgrade", lang, &[]));
            }
        });
    }
}

// ── Units block ────────────────────────────────────────────────────────
//
// Chips `ABR N` com uma abreviação de 3 letras por tipo de unidade
// viva no instante. Placeholder até o sprite sheet entrar — o ABR vai
// virar imagem, o N continua. Estruturas ficam de fora (já estão
// implícitas em Supply/Economy/Army).

fn units_block(ui: &mut Ui, p: &PlayerTimeline, game_loop: u32, lang: Language) {
    let mut entries: Vec<(&str, i32)> = p
        .alive_count
        .iter()
        .filter_map(|(ty, _)| {
            // `Beacon*` são pings de minimapa (attack/defend/rally/custom),
            // não unidades controláveis — filtramos como ruído.
            if is_structure_name(ty) || ty.starts_with("Beacon") {
                return None;
            }
            let count = p.alive_count_at(ty, game_loop);
            if count > 0 {
                Some((ty.as_str(), count))
            } else {
                None
            }
        })
        .collect();
    if entries.is_empty() {
        ui.label(
            RichText::new(tf("timeline.stats.units_none", lang, &[]))
                .small()
                .color(LABEL_DIM),
        );
        return;
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ui.horizontal_wrapped(|ui| {
        for (ty, count) in entries {
            let label = format!("{} {}", unit_abbrev(ty), count);
            let tooltip = tf(
                "timeline.tt.unit_chip",
                lang,
                &[("name", localize(ty, lang)), ("count", &count.to_string())],
            );
            chip(ui, &label, false, None).on_hover_text(tooltip);
        }
    });
}

/// Abreviação fixa de 3 letras (placeholder até sprites). Unidades não
/// mapeadas caem num fallback que pega os 3 primeiros chars do tipo em
/// caixa alta — não bonito pra morphs raros, mas raramente aparece.
fn unit_abbrev(entity_type: &str) -> String {
    match entity_type {
        // Terran
        "SCV" => "SCV",
        "MULE" => "MUL",
        "Marine" => "MAR",
        "Marauder" => "MAU",
        "Reaper" => "REA",
        "Ghost" | "GhostAlternate" => "GHO",
        "Hellion" => "HEL",
        "HellionTank" => "HBT",
        "SiegeTank" | "SiegeTankSieged" => "TNK",
        "Cyclone" => "CYC",
        "WidowMine" | "WidowMineBurrowed" => "WMN",
        "Thor" | "ThorAP" => "THR",
        "VikingFighter" | "VikingAssault" => "VIK",
        "Medivac" => "MDV",
        "Liberator" | "LiberatorAG" => "LIB",
        "Raven" => "RVN",
        "Banshee" => "BSH",
        "Battlecruiser" => "BC",
        // Protoss
        "Probe" => "PRB",
        "Zealot" => "ZEA",
        "Stalker" => "STK",
        "Sentry" => "SEN",
        "Adept" | "AdeptPhaseShift" => "ADP",
        "HighTemplar" => "HT",
        "DarkTemplar" => "DT",
        "Immortal" => "IMM",
        "Colossus" => "COL",
        "Disruptor" | "DisruptorPhased" => "DIS",
        "Archon" => "ARC",
        "Observer" | "ObserverSiegeMode" => "OBS",
        "WarpPrism" | "WarpPrismPhasing" => "PRI",
        "Phoenix" => "PHX",
        "VoidRay" => "VR",
        "Oracle" => "ORA",
        "Tempest" => "TMP",
        "Carrier" => "CAR",
        "Mothership" => "MS",
        // Zerg
        "Drone" => "DRO",
        "Queen" => "QUE",
        "Zergling" => "ZGL",
        "Baneling" | "BanelingCocoon" => "BLN",
        "Roach" | "RoachBurrowed" => "ROA",
        "Ravager" | "RavagerCocoon" => "RAV",
        "Hydralisk" | "HydraliskBurrowed" => "HYD",
        "LurkerMP" | "LurkerMPBurrowed" | "LurkerMPEgg" => "LUR",
        "Mutalisk" => "MUT",
        "Corruptor" => "COR",
        "BroodLord" | "BroodLordCocoon" => "BL",
        "Infestor" | "InfestorBurrowed" => "INF",
        "SwarmHostMP" | "SwarmHostBurrowedMP" => "SH",
        "Viper" => "VIP",
        "Ultralisk" | "UltraliskBurrowed" => "ULT",
        "Overlord" | "OverlordTransport" => "OVL",
        "Overseer" | "OverseerSiegeMode" => "OVS",
        // Neutrals / shared spawns (raramente aparecem no painel)
        "Larva" => "LAR",
        "Interceptor" => "INT",
        "AutoTurret" => "TUR",
        "Locust" | "LocustMP" | "LocustMPFlying" => "LOC",
        "Broodling" => "BRO",
        "Changeling" | "ChangelingMarine" | "ChangelingZealot" | "ChangelingZergling" => "CHG",
        other => {
            return other
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .take(3)
                .collect::<String>()
                .to_uppercase();
        }
    }
    .to_string()
}

// ── Structures block ───────────────────────────────────────────────────
//
// Mesma mecânica do `units_block`: chips `ABR N` com abreviação de 3
// letras por estrutura viva. Variantes `*Flying` / `SupplyDepotLowered`
// são agregadas ao tipo canônico (mesmo edifício físico, estado
// diferente) — soma os counts e mostra um único chip. `CreepTumor*` é
// filtrado: aparecem às dezenas em lategame Zerg e sujariam a linha
// inteira sem valor informativo.

fn structures_block(ui: &mut Ui, p: &PlayerTimeline, game_loop: u32, lang: Language) {
    let mut agg: HashMap<&'static str, i32> = HashMap::new();
    for ty in p.alive_count.keys() {
        if !is_structure_name(ty) || ty.starts_with("CreepTumor") {
            continue;
        }
        let count = p.alive_count_at(ty, game_loop);
        if count <= 0 {
            continue;
        }
        *agg.entry(structure_canonical(ty)).or_insert(0) += count;
    }
    if agg.is_empty() {
        ui.label(
            RichText::new(tf("timeline.stats.structures_none", lang, &[]))
                .small()
                .color(LABEL_DIM),
        );
        return;
    }
    let mut entries: Vec<(&'static str, i32)> = agg.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ui.horizontal_wrapped(|ui| {
        for (canonical, count) in entries {
            let label = format!("{} {}", structure_abbrev(canonical), count);
            let tooltip = tf(
                "timeline.tt.unit_chip",
                lang,
                &[
                    ("name", localize(canonical, lang)),
                    ("count", &count.to_string()),
                ],
            );
            chip(ui, &label, false, None).on_hover_text(tooltip);
        }
    });
}

/// Colapsa variantes de estado (voando, abaixada) no tipo base. Um
/// `CommandCenterFlying` é o mesmo edifício físico que um
/// `CommandCenter` pousado — mostrar os dois como chips separados só
/// polui o painel durante transições de relocação.
fn structure_canonical(name: &str) -> &'static str {
    match name {
        "CommandCenter" | "CommandCenterFlying" => "CommandCenter",
        "OrbitalCommand" | "OrbitalCommandFlying" => "OrbitalCommand",
        "Barracks" | "BarracksFlying" => "Barracks",
        "Factory" | "FactoryFlying" => "Factory",
        "Starport" | "StarportFlying" => "Starport",
        "SupplyDepot" | "SupplyDepotLowered" => "SupplyDepot",
        "PlanetaryFortress" => "PlanetaryFortress",
        "Refinery" => "Refinery",
        "EngineeringBay" => "EngineeringBay",
        "Armory" => "Armory",
        "FusionCore" => "FusionCore",
        "GhostAcademy" => "GhostAcademy",
        "Bunker" => "Bunker",
        "MissileTurret" => "MissileTurret",
        "SensorTower" => "SensorTower",
        "BarracksTechLab" => "BarracksTechLab",
        "FactoryTechLab" => "FactoryTechLab",
        "StarportTechLab" => "StarportTechLab",
        "BarracksReactor" => "BarracksReactor",
        "FactoryReactor" => "FactoryReactor",
        "StarportReactor" => "StarportReactor",
        "Hatchery" => "Hatchery",
        "Lair" => "Lair",
        "Hive" => "Hive",
        "Extractor" => "Extractor",
        "SpawningPool" => "SpawningPool",
        "RoachWarren" => "RoachWarren",
        "HydraliskDen" => "HydraliskDen",
        "BanelingNest" => "BanelingNest",
        "EvolutionChamber" => "EvolutionChamber",
        "Spire" => "Spire",
        "GreaterSpire" => "GreaterSpire",
        "InfestationPit" => "InfestationPit",
        "UltraliskCavern" => "UltraliskCavern",
        "NydusNetwork" => "NydusNetwork",
        "NydusCanal" => "NydusCanal",
        "LurkerDen" => "LurkerDen",
        "SpineCrawler" => "SpineCrawler",
        "SporeCrawler" => "SporeCrawler",
        "Nexus" => "Nexus",
        "Pylon" => "Pylon",
        "Assimilator" => "Assimilator",
        "Gateway" => "Gateway",
        "WarpGate" => "WarpGate",
        "Forge" => "Forge",
        "CyberneticsCore" => "CyberneticsCore",
        "TwilightCouncil" => "TwilightCouncil",
        "Stargate" => "Stargate",
        "RoboticsFacility" => "RoboticsFacility",
        "TemplarArchive" => "TemplarArchive",
        "DarkShrine" => "DarkShrine",
        "RoboticsBay" => "RoboticsBay",
        "FleetBeacon" => "FleetBeacon",
        "PhotonCannon" => "PhotonCannon",
        "ShieldBattery" => "ShieldBattery",
        _ => "",
    }
}

/// Abreviação fixa de 3 letras (placeholder até sprites). Espelha o
/// padrão de `unit_abbrev` — quando entrarem os ícones, a tabela vira
/// lookup de sprite atlas.
fn structure_abbrev(canonical: &str) -> &'static str {
    match canonical {
        // Terran
        "CommandCenter" => "CC",
        "OrbitalCommand" => "ORB",
        "PlanetaryFortress" => "PF",
        "SupplyDepot" => "SD",
        "Refinery" => "REF",
        "Barracks" => "BAR",
        "Factory" => "FAC",
        "Starport" => "STP",
        "EngineeringBay" => "EB",
        "Armory" => "ARM",
        "FusionCore" => "FC",
        "GhostAcademy" => "GA",
        "Bunker" => "BNK",
        "MissileTurret" => "MTR",
        "SensorTower" => "SNT",
        "BarracksTechLab" => "BTL",
        "FactoryTechLab" => "FTL",
        "StarportTechLab" => "STL",
        "BarracksReactor" => "BRC",
        "FactoryReactor" => "FRC",
        "StarportReactor" => "SRC",
        // Zerg
        "Hatchery" => "HAT",
        "Lair" => "LAI",
        "Hive" => "HIV",
        "Extractor" => "EXT",
        "SpawningPool" => "SPL",
        "RoachWarren" => "RW",
        "HydraliskDen" => "HD",
        "BanelingNest" => "BN",
        "EvolutionChamber" => "EVO",
        "Spire" => "SPI",
        "GreaterSpire" => "GSP",
        "InfestationPit" => "IP",
        "UltraliskCavern" => "UC",
        "NydusNetwork" => "NN",
        "NydusCanal" => "NYD",
        "LurkerDen" => "LD",
        "SpineCrawler" => "SPC",
        "SporeCrawler" => "SPO",
        // Protoss
        "Nexus" => "NEX",
        "Pylon" => "PYL",
        "Assimilator" => "ASM",
        "Gateway" => "GW",
        "WarpGate" => "WG",
        "Forge" => "FRG",
        "CyberneticsCore" => "CYB",
        "TwilightCouncil" => "TC",
        "Stargate" => "SG",
        "RoboticsFacility" => "RF",
        "TemplarArchive" => "TA",
        "DarkShrine" => "DS",
        "RoboticsBay" => "RB",
        "FleetBeacon" => "FB",
        "PhotonCannon" => "PC",
        "ShieldBattery" => "SB",
        _ => "STR",
    }
}

// ── Researches block ───────────────────────────────────────────────────
//
// Lista os upgrades pontuais concluídos até o instante corrente — Stim,
// WarpGate, Blink, etc. Os upgrades com níveis (`*Level1/2/3`) não
// aparecem aqui porque já estão representados como pips `⚔+N` / `🛡+N`
// no army block; duplicar seria ruído. A ordenação é cronológica —
// primeiro pesquisado à esquerda, último à direita, o que ajuda a ler
// a progressão de tech sem precisar decorar nomes.

fn researches_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    game_loop: u32,
    loops_per_second: f64,
    lang: Language,
) {
    let done: Vec<&UpgradeEntry> = p
        .upgrades
        .iter()
        .filter(|u| {
            // `Reward*` são achievements cosméticos (portrait, spray, voice
            // set) que entram na stream de upgrades mas não têm efeito de
            // jogo — poluiriam a lista sem valor tático.
            //
            // `game_loop == 0` pega upgrades "de bootstrap": buffs de raça
            // aplicados antes do jogo começar (spray pack, variações de
            // mapa, flags internas). Não representam decisões de tech, só
            // ruído no painel.
            u.game_loop > 0
                && u.game_loop <= game_loop
                && !is_level_upgrade_name(&u.name)
                && !u.name.starts_with("Reward")
        })
        .collect();
    if done.is_empty() {
        ui.label(
            RichText::new(tf("timeline.stats.researches_none", lang, &[]))
                .small()
                .color(LABEL_DIM),
        );
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for u in done {
            let full = localize(&u.name, lang);
            let label = short_research_label(full);
            let secs = (u.game_loop as f64 / loops_per_second).round() as u32;
            let tooltip = tf(
                "timeline.tt.research_chip",
                lang,
                &[
                    ("name", full),
                    ("mm", &format!("{:02}", secs / 60)),
                    ("ss", &format!("{:02}", secs % 60)),
                ],
            );
            chip(ui, label, false, None).on_hover_text(tooltip);
        }
    });
}

/// Espelha `build_order::classify::is_leveled_upgrade` — precisamos do
/// mesmo critério para filtrar os upgrades que já aparecem como pips
/// attack/armor no army block. Duplicar a heurística aqui evita expor
/// API cross-module só pra três linhas.
fn is_level_upgrade_name(name: &str) -> bool {
    name.ends_with("Level1") || name.ends_with("Level2") || name.ends_with("Level3")
}

/// Encurta nomes de pesquisa pra caber no chip sem quebrar linha a
/// cada entrada. Pega a primeira palavra (até 12 chars), preservando
/// o nome completo no tooltip. Cobre o caso típico onde a tradução é
/// "Extended Thermal Lance" ou "Concussive Shells" — a primeira palavra
/// dá contexto suficiente pro jogador reconhecer. Walk por `char_indices`
/// pra não partir caracteres multibyte em traduções pt-BR com acentos.
fn short_research_label(full: &str) -> &str {
    let mut end = full.len();
    let mut chars_taken = 0;
    for (i, c) in full.char_indices() {
        if c == ' ' || chars_taken >= 12 {
            end = i;
            break;
        }
        chars_taken += 1;
    }
    &full[..end]
}

// ── Efficiency block ───────────────────────────────────────────────────

fn efficiency_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    game_loop: u32,
    supply_blocks: &[SupplyBlockEntry],
    loops_per_second: f64,
    lang: Language,
) {
    // Building focus — retained metric with inline mini-bar.
    let (att, tot) = structure_attention_at(p, game_loop);
    let bldg_tt_key = if tot == 0 {
        "timeline.tt.bldg_focus_none"
    } else {
        "timeline.tt.bldg_focus"
    };
    ui.horizontal(|ui| {
        if tot == 0 {
            ui.label(
                RichText::new(tf("timeline.stats.bldg_focus_none", lang, &[]))
                    .small()
                    .color(LABEL_DIM),
            );
        } else {
            let pct = att as f32 * 100.0 / tot as f32;
            ui.label(
                RichText::new(format!("🏢 {pct:.0}%"))
                    .small()
                    .color(LABEL_SOFT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                draw_mini_bar(ui, 32.0, 3.0, pct / 100.0, LABEL_DIM);
            });
        }
    })
    .response
    .on_hover_text(tf(bldg_tt_key, lang, &[]));

    // Worker idle — T/P apenas; Zerg usa larva model (sem CC/Nexus cap).
    if !is_zerg_race(&p.race) {
        let (_, worker_idle, _) =
            compute_idle_periods(&p.worker_births, &p.worker_capacity, game_loop);
        if worker_idle > 0 {
            let secs = (worker_idle as f64 / loops_per_second).round() as u32;
            ui.label(
                RichText::new(tf(
                    "timeline.stats.idle_worker",
                    lang,
                    &[("secs", &secs.to_string())],
                ))
                .small()
                .color(LABEL_DIM),
            )
            .on_hover_text(tf("timeline.tt.idle_worker", lang, &[]));
        }
    }

    // Army idle — todas as raças (Zerg usa slots de Hatchery/Lair/Hive).
    let (_, army_idle, _) =
        compute_idle_periods_ranges(&p.army_productions, &p.army_capacity, game_loop);
    if army_idle > 0 {
        let secs = (army_idle as f64 / loops_per_second).round() as u32;
        ui.label(
            RichText::new(tf(
                "timeline.stats.idle_army",
                lang,
                &[("secs", &secs.to_string())],
            ))
            .small()
            .color(LABEL_DIM),
        )
        .on_hover_text(tf("timeline.tt.idle_army", lang, &[]));
    }

    // Supply block — acumulado até game_loop (contagem e tempo).
    let (count, total_loops) =
        supply_blocks
            .iter()
            .filter(|b| b.start_loop < game_loop)
            .fold((0u32, 0u32), |(c, t), b| {
                let end = b.end_loop.min(game_loop);
                (c + 1, t + end.saturating_sub(b.start_loop))
            });
    if count > 0 {
        let secs = (total_loops as f64 / loops_per_second).round() as u32;
        ui.label(
            RichText::new(tf(
                "timeline.stats.blocks",
                lang,
                &[("count", &count.to_string()), ("secs", &secs.to_string())],
            ))
            .small()
            .color(ACCENT_WARNING),
        )
        .on_hover_text(tf("timeline.tt.blocks", lang, &[]));
    }
}

// ── Drawing primitives ─────────────────────────────────────────────────

fn paint_mineral_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.4;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x, c.y - r),
            pos2(c.x + r * 0.75, c.y),
            pos2(c.x, c.y + r),
            pos2(c.x - r * 0.75, c.y),
        ],
        color,
        Stroke::NONE,
    ));
}

fn paint_gas_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.32;
    painter.circle_filled(c, r, color);
}

/// Barra fina usada como inline indicator (worker cap, building focus).
/// Track em cinza escuro, fill em `color`.
fn draw_mini_bar(ui: &mut Ui, width: f32, height: f32, frac: f32, color: Color32) {
    let (resp, painter) = ui.allocate_painter(vec2(width, height), Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 1.0, Color32::from_gray(50));
    let fill_w = rect.width() * frac.clamp(0.0, 1.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, vec2(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 1.0, color);
    }
}
