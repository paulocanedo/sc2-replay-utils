// Extractor de "lanes" de produção por estrutura, generalizado para
// dois modos:
//
// - `LaneMode::Workers` — uma lane por townhall (Nexus / CommandCenter /
//   OrbitalCommand / PlanetaryFortress / Hatchery / Lair / Hive). Cada
//   bloco representa uma janela de produção de SCV/Probe/Drone ou um
//   morph in-place impeditivo (CC→Orbital, CC→PF). Hatch→Lair / Lair→
//   Hive não emite bloco — a estrutura continua produzindo drones
//   durante o morph.
//
// - `LaneMode::Army` — uma lane por estrutura produtora de army:
//   - Zerg: Hatchery / Lair / Hive (cada larva-born-army).
//   - Terran: Barracks / Factory / Starport. Janelas de produção de
//     unidade são blocos cheios. Adicionalmente, durante a construção
//     de um addon (Reactor/TechLab) a estrutura-mãe não pode produzir
//     — emitimos um bloco `Impeded` cobrindo essa janela.
//   - Protoss: Gateway / WarpGate (mesma tag — morph in-place),
//     RoboticsFacility, Stargate. Quando uma Gateway morpha em WarpGate,
//     setamos `warpgate_since_loop` na lane; o render distingue blocos
//     pré-WarpGate (cheios, single-track) dos blocos pós-WarpGate
//     (thin sub-tracks, estilo Hatchery).
//
// Resolução unit → producer mantém o pipeline em cascata do worker mode:
// 1. `creator_tag` no `ProductionStarted` companheiro (índice `i-1`).
// 2. Larva-born (Zerg): map `larva_tag → hatch_tag` populado quando a
//    larva nasceu.
// 3. Fallback de proximidade espacial (Probe warp-in).

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{
    is_incapacitating_addon, is_larva_born_army, is_worker_name, is_zerg_hatch, EntityEvent,
    EntityEventKind, PlayerTimeline, ProductionCmd, ReplayTimeline,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneMode {
    Workers,
    Army,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    Producing,
    Morphing,
    /// Estrutura existe mas não pode produzir — Terran com addon em
    /// construção. Renderizada com cor distinta de `Producing`/`Morphing`.
    Impeded,
}

#[derive(Clone, Copy, Debug)]
pub struct ProductionBlock {
    pub start_loop: u32,
    pub end_loop: u32,
    pub kind: BlockKind,
    /// Tipo da unidade (ou addon) produzida nesta janela. `None` para
    /// blocos onde o tipo não é interessante (worker mode — o ícone à
    /// esquerda já comunica) ou desconhecido.
    pub produced_type: Option<&'static str>,
    /// Trilha vertical dentro da lane. 0 = trilha única (full-height) ou
    /// trilha superior. 1 = trilha inferior — usada apenas em lanes
    /// Terran com Reactor anexado, para blocos `Producing` sobrepostos
    /// pós-reactor. Hatch/WarpGate continuam 0 (renderização thin
    /// centralizada permanece inalterada).
    pub sub_track: u8,
}

#[derive(Clone, Debug)]
pub struct StructureLane {
    pub tag: i64,
    /// Tipo final da estrutura (após morphs).
    pub canonical_type: &'static str,
    pub born_loop: u32,
    pub died_loop: Option<u32>,
    pub pos_x: u8,
    pub pos_y: u8,
    pub blocks: Vec<ProductionBlock>,
    /// Para lanes Protoss: loop em que a Gateway virou WarpGate. Blocos
    /// com `start_loop >= warpgate_since_loop` são renderizados em
    /// estilo "thin sub-tracks" (warp-in discreto). `None` para
    /// estruturas que nunca foram WarpGate.
    pub warpgate_since_loop: Option<u32>,
    /// Para lanes Terran: loop em que um Reactor terminou de ser
    /// construído nesta estrutura. Blocos `Producing` com
    /// `start_loop >= reactor_since_loop` ganham `sub_track` 0 ou 1
    /// (renderizados em duas faixas top/bottom representando a
    /// capacidade paralela 2x). `None` se nunca teve reactor.
    pub reactor_since_loop: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerProductionLanes {
    pub lanes: Vec<StructureLane>,
}

const CONTINUITY_TOLERANCE_LOOPS: u32 = 5;

/// Reactor (addon Terran que habilita produção paralela 2x). Subset de
/// `is_incapacitating_addon` que exclui TechLabs — TechLab também bloqueia
/// a estrutura durante a construção, mas não habilita paralelismo depois.
fn is_reactor_addon(name: &str) -> bool {
    matches!(name, "BarracksReactor" | "FactoryReactor" | "StarportReactor")
}

/// Tipo canônico da estrutura-mãe esperada para cada addon.
fn addon_parent_canonical(addon: &str) -> Option<&'static str> {
    match addon {
        "BarracksReactor" | "BarracksTechLab" => Some("Barracks"),
        "FactoryReactor" | "FactoryTechLab" => Some("Factory"),
        "StarportReactor" | "StarportTechLab" => Some("Starport"),
        _ => None,
    }
}

/// Resolve o parent de um addon Terran por proximidade espacial. O
/// `UnitInitEvent` do s2protocol não carrega `creator_unit_tag_index`,
/// então `creator_tag` chega como `None` para Reactor/TechLab e
/// precisamos achar a estrutura-mãe pelo posicionamento — addons são
/// sempre colados na estrutura, então a Barracks/Factory/Starport mais
/// próxima viva é virtualmente sempre a correta.
fn resolve_addon_parent(
    addon: &str,
    addon_x: u8,
    addon_y: u8,
    at_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
) -> Option<i64> {
    let parent_type = addon_parent_canonical(addon)?;
    let mut best: Option<(i64, i32)> = None;
    for lane in lanes.values() {
        if lane.canonical_type != parent_type {
            continue;
        }
        if lane.born_loop > at_loop {
            continue;
        }
        if lane.died_loop.map(|d| d <= at_loop).unwrap_or(false) {
            continue;
        }
        let dx = lane.pos_x as i32 - addon_x as i32;
        let dy = lane.pos_y as i32 - addon_y as i32;
        let d2 = dx * dx + dy * dy;
        if best.map(|(_, b)| d2 < b).unwrap_or(true) {
            best = Some((lane.tag, d2));
        }
    }
    best.map(|(tag, _)| tag)
}

/// Tipos de townhall (modo Workers).
fn townhall_canonical(name: &str) -> Option<&'static str> {
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
fn army_producer_canonical(name: &str) -> Option<&'static str> {
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

fn lane_canonical(name: &str, mode: LaneMode) -> Option<&'static str> {
    match mode {
        LaneMode::Workers => townhall_canonical(name),
        LaneMode::Army => army_producer_canonical(name),
    }
}

fn is_target_unit(name: &str, mode: LaneMode, is_zerg: bool) -> bool {
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

/// Tempo de morph in-place em game loops. Usado para morph impeditivo
/// CC→Orbital/PF.
fn morph_build_loops(new_type: &str, base_build: u32) -> u32 {
    let from_balance = balance_data::build_time_loops(new_type, base_build);
    if from_balance > 0 {
        return from_balance;
    }
    match new_type {
        "OrbitalCommand" => 560,
        "PlanetaryFortress" => 806,
        "Lair" => 1424,
        "Hive" => 2160,
        _ => 0,
    }
}

fn morph_old_type<'a>(events: &'a [EntityEvent], i: usize) -> Option<&'a str> {
    if i == 0 {
        return None;
    }
    let prev = &events[i - 1];
    let cur = &events[i];
    if matches!(prev.kind, EntityEventKind::Died)
        && prev.tag == cur.tag
        && prev.game_loop == cur.game_loop
    {
        Some(prev.entity_type.as_str())
    } else {
        None
    }
}

fn is_morph_finish(events: &[EntityEvent], i: usize) -> bool {
    if i < 2 {
        return false;
    }
    let cur = &events[i];
    let s = &events[i - 1];
    let d = &events[i - 2];
    matches!(s.kind, EntityEventKind::ProductionStarted)
        && s.tag == cur.tag
        && s.game_loop == cur.game_loop
        && matches!(d.kind, EntityEventKind::Died)
        && d.tag == cur.tag
        && d.game_loop == cur.game_loop
}

fn is_morph_died(events: &[EntityEvent], i: usize) -> bool {
    let cur = &events[i];
    let Some(next) = events.get(i + 1) else {
        return false;
    };
    matches!(next.kind, EntityEventKind::ProductionStarted)
        && next.tag == cur.tag
        && next.game_loop == cur.game_loop
}

/// Tipos "consumíveis" cujo `Died` representa uma produção real em
/// progresso (não um simples toggle/transform). Larva é a fonte óbvia
/// (Drone/Zergling/Overlord/etc.); cocoons e eggs cobrem morphs Zerg de
/// segundo nível (Baneling vem de BanelingCocoon, Lurker de
/// LurkerMPEgg, Ravager de RavagerCocoon, BroodLord de
/// BroodLordCocoon, Overlord de OverlordCocoon).
///
/// Tudo fora dessa lista que dispara o pattern Died→Started→Finished no
/// mesmo tag/loop é um morph mecânico (siege mode, hellbat transform,
/// viking assault, widowmine burrow, liberator AG) — a unidade original
/// já foi contada quando nasceu, então o transform não vira bloco novo.
fn is_consumable_progenitor(name: &str) -> bool {
    name == "Larva" || name.ends_with("Cocoon") || name.ends_with("Egg")
}

/// `is_morph_finish` mas restrito a transforms PUROS (não-larva,
/// não-cocoon, não-egg). Usado para descartar `ProductionFinished` de
/// toggles Terran (Hellion↔Hellbat, SiegeTank siege mode, etc.) que de
/// outro modo gerariam blocos `Producing` fantasmas atribuídos por
/// proximidade. Mirror semântico do filtro `creator_ability` em
/// `build_order/extract.rs:79-97`, sem depender das strings cruas do
/// SC2 (que variam por base_build).
fn is_pure_morph_finish(events: &[EntityEvent], i: usize) -> bool {
    if !is_morph_finish(events, i) {
        return false;
    }
    !is_consumable_progenitor(events[i - 2].entity_type.as_str())
}

/// Captura o nome estaticamente embutido pra unidades-alvo. Como
/// `EntityEvent.entity_type` é `String`, precisamos de uma tabela de
/// nomes-com-ciclo-de-vida-`'static` para colocar em `produced_type`.
/// Cobre todas as unidades army (T/P/Z), workers e os addons Terran.
fn intern_unit_name(name: &str) -> Option<&'static str> {
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

fn extract_player(
    player: &PlayerTimeline,
    base_build: u32,
    mode: LaneMode,
) -> PlayerProductionLanes {
    let events = &player.entity_events;
    let mut lanes_by_tag: HashMap<i64, StructureLane> = HashMap::new();
    let mut larva_to_hatch: HashMap<i64, i64> = HashMap::new();
    // Modo Army Terran: addon_tag → (parent_tag, start_loop, name).
    // Ao ver Finished/Cancelled/Died do addon, fechamos a janela.
    let mut pending_addon: HashMap<i64, (i64, u32, &'static str)> = HashMap::new();

    let is_zerg = matches!(player.race.as_str(), "Zerg");

    // Cmd matching: índice cmds_by_producer (creator_tag → cmds). Mesma
    // estratégia do `build_order::extract` para que o gráfico use o
    // instante real em que o jogador clicou Train, não uma estimativa
    // de balance_data subtraída do finish_loop. Mantém duas pipelines
    // alinhadas no que mostram pra unidades produzidas.
    let mut cmds_by_producer: HashMap<i64, Vec<usize>> = HashMap::new();
    if mode == LaneMode::Army {
        for (i, cmd) in player.production_cmds.iter().enumerate() {
            if let Some(&p) = cmd.producer_tags.first() {
                cmds_by_producer.entry(p).or_default().push(i);
            }
        }
    }
    let mut consumed = vec![false; player.production_cmds.len()];

    // Slot scheduling: cada `creator_tag` (estrutura Terran ou larva
    // Zerg) tem N slots de produção (1 por padrão; 2 quando um Reactor
    // termina e expande a capacidade da Barracks/Factory/Starport).
    // O slot guarda o `finish_loop` da última produção que ocupou — a
    // próxima unidade pareada usa `start = max(cmd_loop, slot_finish)`,
    // herdando a semântica de fila do `build_order` mas permitindo
    // paralelismo via múltiplos slots.
    let mut slots_by_creator: HashMap<i64, Vec<u32>> = HashMap::new();

    for i in 0..events.len() {
        let ev = &events[i];
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                let new_type = ev.entity_type.as_str();

                // Morph in-place de estrutura — atualiza canonical_type
                // ou emite bloco Morphing impeditivo (CC→Orbital/PF).
                if let Some(new_canonical) = lane_canonical(new_type, mode) {
                    if let Some(old_type) = morph_old_type(events, i) {
                        if lane_canonical(old_type, mode).is_some() {
                            if let Some(lane) = lanes_by_tag.get_mut(&ev.tag) {
                                let is_impeditive_morph = matches!(
                                    new_canonical,
                                    "OrbitalCommand" | "PlanetaryFortress"
                                );
                                if mode == LaneMode::Workers && is_impeditive_morph {
                                    let mt = morph_build_loops(new_canonical, base_build);
                                    if mt > 0 {
                                        let start = ev.game_loop.saturating_sub(mt);
                                        lane.blocks.push(ProductionBlock {
                                            start_loop: start,
                                            end_loop: ev.game_loop,
                                            kind: BlockKind::Morphing,
                                            produced_type: None,
                                            sub_track: 0,
                                        });
                                    }
                                }
                                // Detecta Gateway → WarpGate. A pesquisa
                                // de Warpgate dispara esse morph na
                                // mesma tag, simultaneamente em todas
                                // as Gateways do jogador.
                                if new_canonical == "WarpGate" && old_type == "Gateway" {
                                    lane.warpgate_since_loop = Some(ev.game_loop);
                                }
                                lane.canonical_type = new_canonical;
                            }
                        }
                    }
                }

                // Larva nasce: registra para resolução posterior de
                // unidades larva-born (Drone em workers, ou army units
                // em Zerg).
                if new_type == "Larva" {
                    if let Some(creator) = ev.creator_tag {
                        larva_to_hatch.insert(ev.tag, creator);
                    }
                }

                // Modo Army Terran: addon começou. Abre janela.
                // O `UnitInitEvent` não carrega creator no protocolo —
                // `ev.creator_tag` é sempre `None` para Reactor/TechLab.
                // Caímos em proximidade espacial: addons ficam colados
                // na estrutura-mãe, então a Barracks/Factory/Starport
                // viva mais próxima é virtualmente sempre a certa.
                if mode == LaneMode::Army && is_incapacitating_addon(new_type) {
                    let parent = ev.creator_tag.or_else(|| {
                        resolve_addon_parent(
                            new_type,
                            ev.pos_x,
                            ev.pos_y,
                            ev.game_loop,
                            &lanes_by_tag,
                        )
                    });
                    if let Some(parent) = parent {
                        if let Some(name) = intern_unit_name(new_type) {
                            pending_addon.insert(ev.tag, (parent, ev.game_loop, name));
                        }
                    }
                }
            }
            EntityEventKind::ProductionFinished => {
                // Transforms mecânicos Terran (Hellion↔Hellbat, SiegeTank
                // siege mode, Viking assault, WidowMine burrow, Liberator
                // AG) emitem Died(old)→Started(new)→Finished(new) no mesmo
                // tag/loop via apply_type_change com creator_ability=None.
                // A unidade original já foi contada quando nasceu — sem
                // este skip, cada toggle viraria um bloco fantasma
                // atribuído por proximidade à Factory/Barracks/Starport
                // mais próxima. Larva-borns e cocoons Zerg passam (são
                // produções reais consumindo o progenitor).
                if is_pure_morph_finish(events, i) {
                    continue;
                }

                let new_type = ev.entity_type.as_str();

                // Born real de uma estrutura-lane: cria a lane.
                if let Some(canonical) = lane_canonical(new_type, mode) {
                    if !is_morph_finish(events, i) && !lanes_by_tag.contains_key(&ev.tag) {
                        lanes_by_tag.insert(
                            ev.tag,
                            StructureLane {
                                tag: ev.tag,
                                canonical_type: canonical,
                                born_loop: ev.game_loop,
                                died_loop: None,
                                pos_x: ev.pos_x,
                                pos_y: ev.pos_y,
                                blocks: Vec::new(),
                                warpgate_since_loop: None,
                                reactor_since_loop: None,
                            },
                        );
                    }
                }

                // Unidade-alvo concluída.
                if is_target_unit(new_type, mode, is_zerg) {
                    // creator_tag vem do `ProductionStarted` companheiro
                    // (mesmo tag, mesmo game_loop). Para Terran é o tag
                    // da estrutura produtora; para Zerg morphs é o tag
                    // da larva. É o mesmo valor que o `producer_tag` em
                    // `production_cmds`, então cmd matching usa esse.
                    let creator_tag = events
                        .get(i.wrapping_sub(1))
                        .filter(|prev| {
                            i > 0
                                && matches!(prev.kind, EntityEventKind::ProductionStarted)
                                && prev.tag == ev.tag
                                && prev.game_loop == ev.game_loop
                        })
                        .and_then(|prev| prev.creator_tag);

                    let lane_tag = resolve_producer(
                        events,
                        i,
                        new_type,
                        ev.tag,
                        ev.pos_x,
                        ev.pos_y,
                        ev.game_loop,
                        &lanes_by_tag,
                        &larva_to_hatch,
                        mode,
                    );

                    if let Some(lane_tag) = lane_tag {
                        let finish_loop = ev.game_loop;
                        let expected_bt = balance_data::build_time_loops(new_type, base_build);
                        let bt_fallback = if expected_bt > 0 { expected_bt } else { 272 };
                        // Mesma constraint causal do build_order: o cmd
                        // só é aceito se foi emitido cedo o suficiente
                        // pra plausivelmente ter produzido essa unidade.
                        // Filtra Born events de spawn inicial canibalizando
                        // cmds reais.
                        let max_cmd_loop = finish_loop.saturating_sub(bt_fallback / 2);

                        let cmd_loop = creator_tag.and_then(|ct| {
                            consume_producer_cmd(
                                &cmds_by_producer,
                                &mut consumed,
                                &player.production_cmds,
                                ct,
                                new_type,
                                max_cmd_loop,
                            )
                        });

                        let (sub_track, start_loop) = if let Some(ct) = creator_tag {
                            let slots = slots_by_creator.entry(ct).or_insert_with(|| vec![0]);
                            let (slot_idx, slot_prev) =
                                pick_slot(slots, cmd_loop.unwrap_or(0));
                            let start = match cmd_loop {
                                Some(c) => c.max(slot_prev),
                                None => finish_loop.saturating_sub(bt_fallback),
                            };
                            slots[slot_idx] = finish_loop;
                            (slot_idx as u8, start)
                        } else {
                            (0u8, finish_loop.saturating_sub(bt_fallback))
                        };

                        if let Some(lane) = lanes_by_tag.get_mut(&lane_tag) {
                            lane.blocks.push(ProductionBlock {
                                start_loop,
                                end_loop: finish_loop,
                                kind: BlockKind::Producing,
                                produced_type: intern_unit_name(new_type),
                                sub_track,
                            });
                        }
                    }
                }

                // Modo Army Terran: addon terminou.
                if mode == LaneMode::Army && is_incapacitating_addon(new_type) {
                    if let Some((parent, start, name)) = pending_addon.remove(&ev.tag) {
                        if let Some(lane) = lanes_by_tag.get_mut(&parent) {
                            lane.blocks.push(ProductionBlock {
                                start_loop: start,
                                end_loop: ev.game_loop,
                                kind: BlockKind::Impeded,
                                produced_type: Some(name),
                                sub_track: 0,
                            });
                            // Reactor concluído habilita produção paralela
                            // 2x na estrutura-mãe a partir deste loop. O
                            // render usa esse marcador para alocar duas
                            // sub-trilhas top/bottom; aqui também
                            // expandimos a capacidade de slots da
                            // estrutura para que o cmd matching aloque
                            // os Marines paralelos em sub_track 0 e 1.
                            if is_reactor_addon(new_type) {
                                lane.reactor_since_loop = Some(ev.game_loop);
                                let slots = slots_by_creator
                                    .entry(parent)
                                    .or_insert_with(|| vec![0]);
                                if slots.len() < 2 {
                                    slots.resize(2, 0);
                                }
                            }
                        }
                    }
                }
            }
            EntityEventKind::ProductionCancelled => {
                if mode == LaneMode::Army {
                    if let Some((parent, start, name)) = pending_addon.remove(&ev.tag) {
                        if let Some(lane) = lanes_by_tag.get_mut(&parent) {
                            lane.blocks.push(ProductionBlock {
                                start_loop: start,
                                end_loop: ev.game_loop,
                                kind: BlockKind::Impeded,
                                produced_type: Some(name),
                                sub_track: 0,
                            });
                        }
                    }
                }
            }
            EntityEventKind::Died => {
                if !is_morph_died(events, i) {
                    if let Some(lane) = lanes_by_tag.get_mut(&ev.tag) {
                        lane.died_loop = Some(ev.game_loop);
                    }
                    // Addon morto antes de terminar: trata como cancel.
                    if mode == LaneMode::Army {
                        if let Some((parent, start, name)) = pending_addon.remove(&ev.tag) {
                            if let Some(lane) = lanes_by_tag.get_mut(&parent) {
                                lane.blocks.push(ProductionBlock {
                                    start_loop: start,
                                    end_loop: ev.game_loop,
                                    kind: BlockKind::Impeded,
                                    produced_type: Some(name),
                                    sub_track: 0,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let mut lanes: Vec<StructureLane> = lanes_by_tag.into_values().collect();
    lanes.sort_by_key(|l| (l.born_loop, l.tag));

    for lane in &mut lanes {
        lane.blocks.sort_by_key(|b| (b.start_loop, b.sub_track));
        // Em estruturas com paralelismo real (Hatch/Lair/Hive em qualquer
        // modo, ou WarpGate pós-research), preservamos overlaps. Aqui
        // a lane é per-estrutura, então mesmo Hatch só tem paralelismo
        // via larvas distintas (cada larva é um creator_tag separado).
        let parallel_lane = is_zerg_hatch(lane.canonical_type);
        lane.blocks = merge_continuous(std::mem::take(&mut lane.blocks), parallel_lane);
    }

    PlayerProductionLanes { lanes }
}

/// Escolhe o slot de produção em que a próxima unidade deve cair.
/// Preferimos o menor índice já livre no `cmd_loop` (slot.finish ≤
/// cmd_loop) — assim a sub_track de cada unidade fica estável ao longo
/// do tempo. Se nenhum slot está livre, escolhe o que termina antes
/// (vai bloquear menos). Retorna `(slot_idx, slot.finish)`.
fn pick_slot(slots: &[u32], cmd_loop: u32) -> (usize, u32) {
    debug_assert!(!slots.is_empty());
    for (i, &f) in slots.iter().enumerate() {
        if f <= cmd_loop {
            return (i, f);
        }
    }
    let mut best = 0usize;
    let mut best_f = slots[0];
    for (i, &f) in slots.iter().enumerate().skip(1) {
        if f < best_f {
            best_f = f;
            best = i;
        }
    }
    (best, best_f)
}

/// Procura o primeiro cmd não-consumido emitido pelo `producer_tag`
/// cuja `ability` bate com `action` E cujo `game_loop` satisfaz a
/// constraint de causalidade `cmd_loop <= max_cmd_loop`. Idêntico ao
/// helper homônimo em `build_order::extract` — manter as duas
/// pipelines com a mesma lógica de pareamento garante que o gráfico de
/// produção e a aba de build order mostrem o mesmo conjunto de eventos
/// pareados aos mesmos cmds.
fn consume_producer_cmd(
    by_producer: &HashMap<i64, Vec<usize>>,
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    producer_tag: i64,
    action: &str,
    max_cmd_loop: u32,
) -> Option<u32> {
    let queue = by_producer.get(&producer_tag)?;
    for &i in queue {
        if consumed[i] {
            continue;
        }
        if cmds[i].ability != action {
            continue;
        }
        if cmds[i].game_loop > max_cmd_loop {
            break;
        }
        consumed[i] = true;
        return Some(cmds[i].game_loop);
    }
    None
}

fn merge_continuous(
    blocks: Vec<ProductionBlock>,
    parallel_lane: bool,
) -> Vec<ProductionBlock> {
    let mut out: Vec<ProductionBlock> = Vec::with_capacity(blocks.len());
    for b in blocks {
        let mut merged = false;
        for prev in out.iter_mut().rev() {
            if prev.sub_track != b.sub_track {
                continue;
            }
            if prev.kind != b.kind {
                continue;
            }
            // Não mesclar blocos com produced_type diferente: preserva
            // distinção visual entre unidades sequenciais (ícone muda).
            if prev.produced_type != b.produced_type {
                break;
            }
            let overlap = b.start_loop < prev.end_loop;
            if overlap {
                if parallel_lane {
                    continue;
                }
                prev.end_loop = prev.end_loop.max(b.end_loop);
                merged = true;
                break;
            }
            if b.start_loop.saturating_sub(prev.end_loop) <= CONTINUITY_TOLERANCE_LOOPS {
                prev.end_loop = prev.end_loop.max(b.end_loop);
                merged = true;
                break;
            }
            break;
        }
        if !merged {
            out.push(b);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn resolve_producer(
    events: &[EntityEvent],
    finished_index: usize,
    unit_type: &str,
    self_tag: i64,
    pos_x: u8,
    pos_y: u8,
    finish_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
    larva_to_hatch: &HashMap<i64, i64>,
    mode: LaneMode,
) -> Option<i64> {
    // (1) Started companheiro com creator_tag.
    if finished_index > 0 {
        let started = &events[finished_index - 1];
        if matches!(started.kind, EntityEventKind::ProductionStarted)
            && started.tag == self_tag
            && started.game_loop == events[finished_index].game_loop
        {
            if let Some(t) = started.creator_tag {
                if t != self_tag && lanes.contains_key(&t) {
                    return Some(t);
                }
            }
        }
    }

    // (2) Larva-born (Zerg).
    if let Some(&hatch) = larva_to_hatch.get(&self_tag) {
        if lanes.contains_key(&hatch) {
            return Some(hatch);
        }
    }

    // (3) Fallback de proximidade.
    resolve_by_proximity(lanes, unit_type, finish_loop, pos_x, pos_y, mode)
}

fn resolve_by_proximity(
    lanes: &HashMap<i64, StructureLane>,
    unit_type: &str,
    at_loop: u32,
    x: u8,
    y: u8,
    mode: LaneMode,
) -> Option<i64> {
    let allowed: &[&str] = match (mode, unit_type) {
        (LaneMode::Workers, "SCV") => &["CommandCenter", "OrbitalCommand", "PlanetaryFortress"],
        (LaneMode::Workers, "Probe") => &["Nexus"],
        (LaneMode::Workers, "Drone") => &["Hatchery", "Lair", "Hive"],
        (LaneMode::Workers, _) => return None,
        (LaneMode::Army, _) => &[
            "Barracks",
            "Factory",
            "Starport",
            "Gateway",
            "WarpGate",
            "RoboticsFacility",
            "Stargate",
            "Hatchery",
            "Lair",
            "Hive",
        ],
    };

    let mut best: Option<(i64, i32)> = None;
    for lane in lanes.values() {
        if lane.born_loop > at_loop {
            continue;
        }
        if lane.died_loop.map(|d| d <= at_loop).unwrap_or(false) {
            continue;
        }
        if !allowed.contains(&lane.canonical_type) {
            continue;
        }
        let dx = lane.pos_x as i32 - x as i32;
        let dy = lane.pos_y as i32 - y as i32;
        let d2 = dx * dx + dy * dy;
        if best.map(|(_, b)| d2 < b).unwrap_or(true) {
            best = Some((lane.tag, d2));
        }
    }
    best.map(|(tag, _)| tag)
}

/// Constrói as lanes para todos os jogadores do replay, na mesma ordem
/// de `timeline.players`.
pub fn extract(timeline: &ReplayTimeline, mode: LaneMode) -> Vec<PlayerProductionLanes> {
    timeline
        .players
        .iter()
        .map(|p| extract_player(p, timeline.base_build, mode))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{EntityCategory, EntityEvent, EntityEventKind};

    fn ev(
        gl: u32,
        seq: u32,
        kind: EntityEventKind,
        ty: &str,
        tag: i64,
        creator: Option<i64>,
    ) -> EntityEvent {
        EntityEvent {
            game_loop: gl,
            seq,
            kind,
            entity_type: ty.to_string(),
            category: if matches!(ty, "SCV" | "Probe" | "Drone") {
                EntityCategory::Worker
            } else if matches!(ty, "Larva")
                || is_larva_born_army(ty)
                || matches!(
                    ty,
                    "Marine" | "Marauder" | "Reaper" | "Ghost" | "Zealot" | "Stalker"
                )
            {
                EntityCategory::Unit
            } else {
                EntityCategory::Structure
            },
            tag,
            pos_x: 0,
            pos_y: 0,
            creator_ability: None,
            creator_tag: creator,
            killer_player_id: None,
        }
    }

    fn player_with_events(events: Vec<EntityEvent>, race: &str) -> PlayerTimeline {
        PlayerTimeline {
            name: "p".into(),
            clan: String::new(),
            race: race.into(),
            mmr: None,
            player_id: 1,
            result: None,
            toon: None,
            stats: vec![],
            upgrades: vec![],
            entity_events: events,
            production_cmds: vec![],
            inject_cmds: vec![],
            unit_positions: vec![],
            camera_positions: vec![],
            alive_count: Default::default(),
            worker_capacity: vec![],
            worker_births: vec![],
            army_capacity: vec![],
            army_productions: vec![],
            worker_capacity_cumulative: vec![],
            army_capacity_cumulative: vec![],
            upgrade_cumulative: vec![],
            creep_index: vec![],
        }
    }

    #[test]
    fn workers_terran_cc_morphs_to_orbital_emits_morphing_block() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "CommandCenter",
                1,
                None,
            ),
            ev(1000, 1, EntityEventKind::Died, "CommandCenter", 1, None),
            ev(
                1000,
                2,
                EntityEventKind::ProductionStarted,
                "OrbitalCommand",
                1,
                None,
            ),
            ev(
                1000,
                3,
                EntityEventKind::ProductionFinished,
                "OrbitalCommand",
                1,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Workers);
        assert_eq!(out.lanes.len(), 1);
        assert_eq!(out.lanes[0].canonical_type, "OrbitalCommand");
        assert_eq!(out.lanes[0].blocks.len(), 1);
        assert_eq!(out.lanes[0].blocks[0].kind, BlockKind::Morphing);
    }

    #[test]
    fn workers_scv_resolves_via_started_companion() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "CommandCenter",
                1,
                None,
            ),
            ev(
                472,
                1,
                EntityEventKind::ProductionStarted,
                "SCV",
                10,
                Some(1),
            ),
            ev(472, 1, EntityEventKind::ProductionFinished, "SCV", 10, None),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Workers);
        assert_eq!(out.lanes[0].blocks.len(), 1);
        assert_eq!(out.lanes[0].blocks[0].kind, BlockKind::Producing);
    }

    #[test]
    fn workers_zerg_drone_resolves_via_larva_to_hatch_map() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Hatchery",
                1,
                None,
            ),
            ev(150, 1, EntityEventKind::ProductionStarted, "Larva", 5, Some(1)),
            ev(150, 1, EntityEventKind::ProductionFinished, "Larva", 5, None),
            ev(472, 2, EntityEventKind::Died, "Larva", 5, None),
            ev(
                472,
                2,
                EntityEventKind::ProductionStarted,
                "Drone",
                5,
                Some(5),
            ),
            ev(472, 2, EntityEventKind::ProductionFinished, "Drone", 5, None),
        ];
        let p = player_with_events(events, "Zerg");
        let out = extract_player(&p, 0, LaneMode::Workers);
        assert_eq!(out.lanes[0].blocks.len(), 1);
    }

    #[test]
    fn army_terran_addon_construction_emits_impeded_block() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                600,
                2,
                EntityEventKind::ProductionFinished,
                "BarracksReactor",
                2,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes.len(), 1);
        let imp: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Impeded)
            .collect();
        assert_eq!(imp.len(), 1);
        assert_eq!(imp[0].start_loop, 200);
        assert_eq!(imp[0].end_loop, 600);
        assert_eq!(imp[0].produced_type, Some("BarracksReactor"));
    }

    #[test]
    fn army_terran_marine_attributed_to_barracks() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                500,
                1,
                EntityEventKind::ProductionStarted,
                "Marine",
                10,
                Some(1),
            ),
            ev(
                500,
                2,
                EntityEventKind::ProductionFinished,
                "Marine",
                10,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1);
        assert_eq!(prod[0].produced_type, Some("Marine"));
    }

    #[test]
    fn army_protoss_gateway_morphs_to_warpgate_sets_warpgate_since_loop() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Gateway",
                1,
                None,
            ),
            ev(2000, 1, EntityEventKind::Died, "Gateway", 1, None),
            ev(
                2000,
                2,
                EntityEventKind::ProductionStarted,
                "WarpGate",
                1,
                None,
            ),
            ev(
                2000,
                3,
                EntityEventKind::ProductionFinished,
                "WarpGate",
                1,
                None,
            ),
        ];
        let p = player_with_events(events, "Protoss");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes.len(), 1);
        assert_eq!(out.lanes[0].canonical_type, "WarpGate");
        assert_eq!(out.lanes[0].warpgate_since_loop, Some(2000));
    }

    #[test]
    fn army_zerg_zergling_attributed_to_hatchery_via_larva() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Hatchery",
                1,
                None,
            ),
            ev(150, 1, EntityEventKind::ProductionStarted, "Larva", 5, Some(1)),
            ev(150, 1, EntityEventKind::ProductionFinished, "Larva", 5, None),
            ev(472, 2, EntityEventKind::Died, "Larva", 5, None),
            ev(
                472,
                2,
                EntityEventKind::ProductionStarted,
                "Zergling",
                5,
                Some(5),
            ),
            ev(
                472,
                2,
                EntityEventKind::ProductionFinished,
                "Zergling",
                5,
                None,
            ),
        ];
        let p = player_with_events(events, "Zerg");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1);
        assert_eq!(prod[0].produced_type, Some("Zergling"));
    }

    #[test]
    fn army_terran_addon_cancelled_emits_partial_impeded() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                400,
                2,
                EntityEventKind::ProductionCancelled,
                "BarracksReactor",
                2,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let imp: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Impeded)
            .collect();
        assert_eq!(imp.len(), 1);
        assert_eq!(imp[0].end_loop, 400);
    }

    fn ev_at(
        gl: u32,
        seq: u32,
        kind: EntityEventKind,
        ty: &str,
        tag: i64,
        creator: Option<i64>,
        x: u8,
        y: u8,
    ) -> EntityEvent {
        let mut e = ev(gl, seq, kind, ty, tag, creator);
        e.pos_x = x;
        e.pos_y = y;
        e
    }

    #[test]
    fn army_terran_addon_resolves_parent_via_proximity_when_creator_tag_missing() {
        // O UnitInit do s2protocol não traz creator_tag para addons
        // Terran. O parent precisa ser resolvido por proximidade
        // espacial à Barracks/Factory/Starport mais próxima.
        let events = vec![
            // Duas Barracks: tag 1 perto (50, 50), tag 2 longe (200, 200).
            ev_at(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
                50,
                50,
            ),
            ev_at(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                2,
                None,
                200,
                200,
            ),
            // Reactor inicia em (53, 50) — adjacente à Barracks 1.
            // creator_tag = None (como vem do parser real).
            ev_at(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                10,
                None,
                53,
                50,
            ),
            ev_at(
                600,
                2,
                EntityEventKind::ProductionFinished,
                "BarracksReactor",
                10,
                None,
                53,
                50,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        // Lane 1 (próxima) recebeu o Impeded e o reactor_since_loop.
        let lane1 = out.lanes.iter().find(|l| l.tag == 1).unwrap();
        let lane2 = out.lanes.iter().find(|l| l.tag == 2).unwrap();
        assert_eq!(lane1.reactor_since_loop, Some(600));
        assert_eq!(lane2.reactor_since_loop, None);
        let imp1: Vec<_> = lane1
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Impeded)
            .collect();
        assert_eq!(imp1.len(), 1);
        let imp2: Vec<_> = lane2
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Impeded)
            .collect();
        assert_eq!(imp2.len(), 0);
    }

    #[test]
    fn army_terran_reactor_finished_sets_reactor_since_loop() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                600,
                2,
                EntityEventKind::ProductionFinished,
                "BarracksReactor",
                2,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes.len(), 1);
        assert_eq!(out.lanes[0].reactor_since_loop, Some(600));
    }

    #[test]
    fn army_terran_techlab_does_not_set_reactor_since_loop() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksTechLab",
                2,
                Some(1),
            ),
            ev(
                600,
                2,
                EntityEventKind::ProductionFinished,
                "BarracksTechLab",
                2,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes.len(), 1);
        assert_eq!(out.lanes[0].reactor_since_loop, None);
        // Impeded block do TechLab continua sendo emitido normalmente.
        let imp: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Impeded)
            .collect();
        assert_eq!(imp.len(), 1);
    }

    #[test]
    fn army_terran_reactor_cancelled_does_not_set_reactor_since_loop() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                400,
                2,
                EntityEventKind::ProductionCancelled,
                "BarracksReactor",
                2,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes[0].reactor_since_loop, None);
    }

    #[test]
    fn army_terran_reactor_enables_parallel_blocks_with_subtracks() {
        // Build time real do Marine no balance_data ~400 loops. Usamos
        // tempos de finish bem acima do reactor (1500) para garantir
        // que ambos os blocos `Producing` caiam pós-reactor, e tempos
        // de finish próximos (delta=100) para forçar overlap das
        // janelas de produção.
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            // Reactor: 200..1500
            ev(
                200,
                1,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                1500,
                2,
                EntityEventKind::ProductionFinished,
                "BarracksReactor",
                2,
                None,
            ),
            // Marine 1
            ev(
                2000,
                3,
                EntityEventKind::ProductionStarted,
                "Marine",
                10,
                Some(1),
            ),
            ev(
                2000,
                3,
                EntityEventKind::ProductionFinished,
                "Marine",
                10,
                None,
            ),
            // Marine 2 — finish 100 loops depois força overlap.
            ev(
                2100,
                4,
                EntityEventKind::ProductionStarted,
                "Marine",
                11,
                Some(1),
            ),
            ev(
                2100,
                4,
                EntityEventKind::ProductionFinished,
                "Marine",
                11,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        assert_eq!(out.lanes.len(), 1);
        let lane = &out.lanes[0];
        assert_eq!(lane.reactor_since_loop, Some(1500));
        let prod: Vec<_> = lane
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        // Os dois Marines pós-reactor não devem mesclar.
        assert_eq!(prod.len(), 2);
        let mut tracks: Vec<u8> = prod.iter().map(|b| b.sub_track).collect();
        tracks.sort();
        assert_eq!(tracks, vec![0, 1]);
    }

    #[test]
    fn army_terran_pre_and_post_reactor_marines_handled_separately() {
        // Marine 1 finaliza bem antes do reactor (single-track).
        // Marine 2 e 3 finalizam bem depois com overlap (top/bottom).
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Barracks",
                1,
                None,
            ),
            // Marine 1 pré-reactor — finish em 1000, bloco totalmente
            // antes do reactor.
            ev(
                1000,
                1,
                EntityEventKind::ProductionStarted,
                "Marine",
                10,
                Some(1),
            ),
            ev(
                1000,
                1,
                EntityEventKind::ProductionFinished,
                "Marine",
                10,
                None,
            ),
            // Reactor: 1100..2500 (concluído em 2500).
            ev(
                1100,
                2,
                EntityEventKind::ProductionStarted,
                "BarracksReactor",
                2,
                Some(1),
            ),
            ev(
                2500,
                3,
                EntityEventKind::ProductionFinished,
                "BarracksReactor",
                2,
                None,
            ),
            // Marine 2 pós-reactor.
            ev(
                3000,
                4,
                EntityEventKind::ProductionStarted,
                "Marine",
                11,
                Some(1),
            ),
            ev(
                3000,
                4,
                EntityEventKind::ProductionFinished,
                "Marine",
                11,
                None,
            ),
            // Marine 3 — finish 100 loops depois força overlap pós-reactor.
            ev(
                3100,
                5,
                EntityEventKind::ProductionStarted,
                "Marine",
                12,
                Some(1),
            ),
            ev(
                3100,
                5,
                EntityEventKind::ProductionFinished,
                "Marine",
                12,
                None,
            ),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let lane = &out.lanes[0];
        assert_eq!(lane.reactor_since_loop, Some(2500));
        let prod: Vec<_> = lane
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        // Pré-reactor: 1 bloco. Pós-reactor: 2 blocos não-mesclados.
        assert_eq!(prod.len(), 3);

        let pre: Vec<_> = prod.iter().filter(|b| b.start_loop < 2500).collect();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].sub_track, 0);

        let post: Vec<_> = prod.iter().filter(|b| b.start_loop >= 2500).collect();
        assert_eq!(post.len(), 2);
        let mut tracks: Vec<u8> = post.iter().map(|b| b.sub_track).collect();
        tracks.sort();
        assert_eq!(tracks, vec![0, 1]);
    }

    /// Integration: a contagem de unidades de army por jogador no
    /// gráfico tem que bater com o que o build_order extrai do mesmo
    /// replay. As duas pipelines consomem os mesmos `production_cmds`
    /// + `entity_events` e devem chegar nos mesmos eventos pareados.
    #[test]
    fn army_lanes_match_build_order_counts_on_real_terran_replay() {
        use crate::build_order::extract_build_order;
        use crate::replay::parse_replay;
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/replay1.SC2Replay");
        let timeline = parse_replay(&path, 0).expect("parse replay");
        let bo = extract_build_order(&timeline).expect("build_order");
        let lanes_per_player = extract(&timeline, LaneMode::Army);

        // Para cada jogador Terran, conta unidades de army produzidas
        // (Marine/Marauder/etc.) em ambas as pipelines e compara.
        let mut compared_any = false;
        for (p_idx, player) in timeline.players.iter().enumerate() {
            if player.race != "Terran" {
                continue;
            }
            let bo_player = &bo.players[p_idx];
            let lanes_player = &lanes_per_player[p_idx];

            // Conta por tipo no build_order: entries de army (não
            // estrutura, não upgrade, completed) com count agregado.
            let mut bo_counts: HashMap<String, usize> = HashMap::new();
            for entry in &bo_player.entries {
                if entry.is_structure || entry.is_upgrade {
                    continue;
                }
                if entry.outcome != crate::build_order::EntryOutcome::Completed {
                    continue;
                }
                if is_worker_name(&entry.action) {
                    continue;
                }
                if !matches!(
                    entry.action.as_str(),
                    "Marine"
                        | "Marauder"
                        | "Reaper"
                        | "Ghost"
                        | "Hellion"
                        | "Hellbat"
                        | "WidowMine"
                        | "SiegeTank"
                        | "Cyclone"
                        | "Thor"
                        | "VikingFighter"
                        | "Medivac"
                        | "Liberator"
                        | "Banshee"
                        | "Raven"
                        | "Battlecruiser"
                ) {
                    continue;
                }
                *bo_counts.entry(entry.action.clone()).or_default() += entry.count as usize;
            }

            // Conta por tipo nas lanes: cada bloco Producing é uma
            // unidade. Como `merge_continuous` mescla blocos contíguos
            // do mesmo tipo na mesma sub_track, a contagem aqui pode
            // ser ≤ build_order. Mas o conjunto de tipos produzidos
            // tem que ser o mesmo, e a cardinalidade não pode ser zero
            // quando build_order tem entradas.
            let mut lanes_types: HashMap<&'static str, usize> = HashMap::new();
            for lane in &lanes_player.lanes {
                for block in &lane.blocks {
                    if block.kind != BlockKind::Producing {
                        continue;
                    }
                    if let Some(t) = block.produced_type {
                        if matches!(
                            t,
                            "Marine"
                                | "Marauder"
                                | "Reaper"
                                | "Ghost"
                                | "Hellion"
                                | "Hellbat"
                                | "WidowMine"
                                | "SiegeTank"
                                | "Cyclone"
                                | "Thor"
                                | "VikingFighter"
                                | "Medivac"
                                | "Liberator"
                                | "Banshee"
                                | "Raven"
                                | "Battlecruiser"
                        ) {
                            *lanes_types.entry(t).or_default() += 1;
                        }
                    }
                }
            }

            // Validação: todo tipo presente no build_order tem que
            // aparecer nas lanes, e vice-versa.
            for (action, _) in &bo_counts {
                assert!(
                    lanes_types.contains_key(action.as_str()),
                    "Player {}: build_order tem '{}' mas lanes não",
                    player.name,
                    action,
                );
            }
            for action in lanes_types.keys() {
                assert!(
                    bo_counts.contains_key(*action),
                    "Player {}: lanes tem '{}' mas build_order não",
                    player.name,
                    action,
                );
            }

            // Sem blocos `Producing` com `produced_type=None`: como
            // `is_target_unit` agora gateia em `intern_unit_name.is_some()`,
            // todo bloco aceito tem nome canônico. Se este invariante
            // quebra, é sinal de que `is_target_unit` está deixando algo
            // passar (ou `intern_unit_name` divergiu).
            for lane in &lanes_player.lanes {
                for block in &lane.blocks {
                    if block.kind == BlockKind::Producing {
                        assert!(
                            block.produced_type.is_some(),
                            "Player {}: bloco Producing em lane '{}' com produced_type=None — divergência entre is_target_unit e intern_unit_name",
                            player.name,
                            lane.canonical_type,
                        );
                    }
                }
            }
            compared_any = true;
        }
        assert!(compared_any, "replay1.SC2Replay não tem jogador Terran");
    }

    /// Terran transform: Hellion ↔ Hellbat. Sem o filtro
    /// `is_pure_morph_finish`, o `Finished(Hellbat)` viraria um bloco
    /// fantasma — a Hellion já foi contada quando nasceu da Factory.
    #[test]
    fn army_terran_hellion_hellbat_morph_does_not_create_phantom_block() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Factory",
                1,
                None,
            ),
            // Hellion produzido na Factory (UnitBorn fresh tag).
            ev(500, 1, EntityEventKind::ProductionStarted, "Hellion", 10, Some(1)),
            ev(500, 1, EntityEventKind::ProductionFinished, "Hellion", 10, None),
            // Morph Hellion → Hellbat via UnitTypeChange (apply_type_change
            // emite Died+Started+Finished com creator_ability=None).
            ev(2000, 2, EntityEventKind::Died, "Hellion", 10, None),
            ev(2000, 3, EntityEventKind::ProductionStarted, "Hellbat", 10, Some(10)),
            ev(2000, 3, EntityEventKind::ProductionFinished, "Hellbat", 10, None),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1, "esperava 1 bloco (Hellion), Hellbat morph não conta");
        assert_eq!(prod[0].produced_type, Some("Hellion"));
    }

    /// Terran toggle: SiegeTank ↔ SiegeTankSieged. Em uma partida típica
    /// o jogador siege/unsiege dezenas de vezes. Cada toggle emite
    /// Died→Started→Finished e geraria 2 blocos fantasmas por ciclo
    /// sem o fix.
    #[test]
    fn army_terran_siegetank_siege_cycle_zero_extra_blocks() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Factory",
                1,
                None,
            ),
            ev(800, 1, EntityEventKind::ProductionStarted, "SiegeTank", 20, Some(1)),
            ev(800, 1, EntityEventKind::ProductionFinished, "SiegeTank", 20, None),
            // Siege.
            ev(2000, 2, EntityEventKind::Died, "SiegeTank", 20, None),
            ev(2000, 3, EntityEventKind::ProductionStarted, "SiegeTankSieged", 20, Some(20)),
            ev(2000, 3, EntityEventKind::ProductionFinished, "SiegeTankSieged", 20, None),
            // Unsiege.
            ev(3000, 4, EntityEventKind::Died, "SiegeTankSieged", 20, None),
            ev(3000, 5, EntityEventKind::ProductionStarted, "SiegeTank", 20, Some(20)),
            ev(3000, 5, EntityEventKind::ProductionFinished, "SiegeTank", 20, None),
            // Re-siege.
            ev(4000, 6, EntityEventKind::Died, "SiegeTank", 20, None),
            ev(4000, 7, EntityEventKind::ProductionStarted, "SiegeTankSieged", 20, Some(20)),
            ev(4000, 7, EntityEventKind::ProductionFinished, "SiegeTankSieged", 20, None),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1, "esperava 1 bloco (Tank), 3 toggles não contam");
        assert_eq!(prod[0].produced_type, Some("SiegeTank"));
    }

    /// Terran toggle: VikingFighter ↔ VikingAssault.
    #[test]
    fn army_terran_viking_transform_zero_extra_blocks() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Starport",
                1,
                None,
            ),
            ev(800, 1, EntityEventKind::ProductionStarted, "VikingFighter", 30, Some(1)),
            ev(800, 1, EntityEventKind::ProductionFinished, "VikingFighter", 30, None),
            // Transform para assault mode.
            ev(2000, 2, EntityEventKind::Died, "VikingFighter", 30, None),
            ev(2000, 3, EntityEventKind::ProductionStarted, "VikingAssault", 30, Some(30)),
            ev(2000, 3, EntityEventKind::ProductionFinished, "VikingAssault", 30, None),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1);
        assert_eq!(prod[0].produced_type, Some("VikingFighter"));
    }

    /// Terran toggle: WidowMine ↔ WidowMineBurrowed.
    #[test]
    fn army_terran_widowmine_burrow_zero_extra_blocks() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Factory",
                1,
                None,
            ),
            ev(800, 1, EntityEventKind::ProductionStarted, "WidowMine", 40, Some(1)),
            ev(800, 1, EntityEventKind::ProductionFinished, "WidowMine", 40, None),
            // Burrow.
            ev(1500, 2, EntityEventKind::Died, "WidowMine", 40, None),
            ev(1500, 3, EntityEventKind::ProductionStarted, "WidowMineBurrowed", 40, Some(40)),
            ev(1500, 3, EntityEventKind::ProductionFinished, "WidowMineBurrowed", 40, None),
            // Unburrow.
            ev(2500, 4, EntityEventKind::Died, "WidowMineBurrowed", 40, None),
            ev(2500, 5, EntityEventKind::ProductionStarted, "WidowMine", 40, Some(40)),
            ev(2500, 5, EntityEventKind::ProductionFinished, "WidowMine", 40, None),
        ];
        let p = player_with_events(events, "Terran");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1);
        assert_eq!(prod[0].produced_type, Some("WidowMine"));
    }

    /// Helper logic test: `is_consumable_progenitor` deve aceitar Larva,
    /// cocoons (BanelingCocoon, RavagerCocoon, BroodLordCocoon,
    /// OverlordCocoon) e eggs (LurkerMPEgg) — confirmados em
    /// `s2protocol-3.5.2/assets/BalanceData/`. E rejeitar tudo o mais.
    #[test]
    fn is_consumable_progenitor_accepts_larva_cocoons_and_eggs() {
        // Aceitar.
        assert!(is_consumable_progenitor("Larva"));
        assert!(is_consumable_progenitor("BanelingCocoon"));
        assert!(is_consumable_progenitor("BroodLordCocoon"));
        assert!(is_consumable_progenitor("OverlordCocoon"));
        assert!(is_consumable_progenitor("RavagerCocoon"));
        assert!(is_consumable_progenitor("LurkerMPEgg"));
        // Rejeitar transforms Terran.
        assert!(!is_consumable_progenitor("Hellion"));
        assert!(!is_consumable_progenitor("Hellbat"));
        assert!(!is_consumable_progenitor("SiegeTank"));
        assert!(!is_consumable_progenitor("SiegeTankSieged"));
        assert!(!is_consumable_progenitor("VikingFighter"));
        assert!(!is_consumable_progenitor("VikingAssault"));
        assert!(!is_consumable_progenitor("WidowMine"));
        assert!(!is_consumable_progenitor("WidowMineBurrowed"));
        assert!(!is_consumable_progenitor("Liberator"));
        assert!(!is_consumable_progenitor("LiberatorAG"));
        // Rejeitar unidades Zerg base (não progenitoras).
        assert!(!is_consumable_progenitor("Zergling"));
        assert!(!is_consumable_progenitor("Roach"));
        assert!(!is_consumable_progenitor("Overlord"));
    }

    /// Zerg via Cocoon: Larva → OverlordCocoon → Overlord. Quando o
    /// tracker emite a transição final (Cocoon → Overlord), o
    /// `is_consumable_progenitor("OverlordCocoon")` é `true` e o
    /// `Finished(Overlord)` continua gerando bloco. Confirma que o fix
    /// não regrediu Zerg que passa por intermediário cocoon.
    #[test]
    fn army_zerg_overlord_via_cocoon_creates_block() {
        let events = vec![
            ev(
                100,
                0,
                EntityEventKind::ProductionFinished,
                "Hatchery",
                1,
                None,
            ),
            ev(150, 1, EntityEventKind::ProductionStarted, "Larva", 5, Some(1)),
            ev(150, 1, EntityEventKind::ProductionFinished, "Larva", 5, None),
            // Larva → OverlordCocoon (intermediário, descartado por
            // is_target_unit pra Zerg porque OverlordCocoon não está em
            // is_larva_born_army).
            ev(472, 2, EntityEventKind::Died, "Larva", 5, None),
            ev(472, 3, EntityEventKind::ProductionStarted, "OverlordCocoon", 5, Some(5)),
            ev(472, 3, EntityEventKind::ProductionFinished, "OverlordCocoon", 5, None),
            // OverlordCocoon → Overlord (final morph, IS em is_larva_born_army).
            ev(900, 4, EntityEventKind::Died, "OverlordCocoon", 5, None),
            ev(900, 5, EntityEventKind::ProductionStarted, "Overlord", 5, Some(5)),
            ev(900, 5, EntityEventKind::ProductionFinished, "Overlord", 5, None),
        ];
        let p = player_with_events(events, "Zerg");
        let out = extract_player(&p, 0, LaneMode::Army);
        let prod: Vec<_> = out.lanes[0]
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Producing)
            .collect();
        assert_eq!(prod.len(), 1, "Overlord via cocoon deve gerar 1 bloco");
        assert_eq!(prod[0].produced_type, Some("Overlord"));
    }
}
