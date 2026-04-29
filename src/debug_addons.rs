//! Diagnóstico de Phase 0 para a investigação de resolução de parent
//! de addons Terran. Imprime cinco seções por replay × jogador:
//!
//! 1. Cmds candidatos (production_cmds com `Build*Reactor`/`Build*TechLab`).
//! 2. UnitInit de addons (`ProductionStarted` em Reactor/TechLab).
//! 3. Pareamento cmd ↔ addon dentro de uma janela curta de game_loops.
//! 4. Para cada addon: TODOS os parents-candidatos vivos no momento, com
//!    `Δx`, `Δy`, `d²` e marca de qual deles é o `producer_tag` do cmd.
//! 5. Resumo de offsets `(Δx, Δy)` do parent escolhido por addon×tipo
//!    de pai — base empírica para calibrar a opção B.
//!
//! O objetivo é responder duas perguntas:
//! - **(A)** Os cmds Build_*Reactor / Build_*TechLab carregam
//!   `producer_tags` confiáveis? Quão frequentemente o `producer_tag`
//!   bate com o parent geometricamente mais próximo?
//! - **(B)** Qual é o offset `(Δx, Δy)` real entre parent e addon na
//!   nossa escala u8? É determinístico por par parent×addon?

use std::collections::HashMap;
use std::path::Path;

use crate::build_order::extract_build_order;
use crate::replay::{
    is_incapacitating_addon, parse_replay, EntityEvent, EntityEventKind, PlayerTimeline,
    ProductionCmd, ReplayTimeline,
};

/// Janela em game_loops em torno do `UnitInit` do addon onde aceitamos
/// um cmd como pareado. Cmds emitidos depois do init não fazem sentido;
/// cmds emitidos muito antes provavelmente são de outro addon. ±5 já
/// cobre a latência típica entre cmd e Init (~1 frame), mas damos folga
/// pra redes lentas / orderings esquisitas.
const PAIR_WINDOW: i64 = 50;

/// Tipo canônico do parent esperado para cada addon. Precisa bater com
/// `production_lanes::terran::addon_parent_canonical` mas duplico aqui
/// porque aquele helper é `pub(super)`.
fn addon_parent_canonical(addon: &str) -> Option<&'static str> {
    match addon {
        "BarracksReactor" | "BarracksTechLab" => Some("Barracks"),
        "FactoryReactor" | "FactoryTechLab" => Some("Factory"),
        "StarportReactor" | "StarportTechLab" => Some("Starport"),
        _ => None,
    }
}

/// Filtro pra abilities relacionadas a addon. Não sabemos os nomes
/// exatos a priori (variam por base_build); aceitamos qualquer string
/// que mencione Reactor ou TechLab.
fn is_addon_ability(ability: &str) -> bool {
    let a = ability.to_ascii_lowercase();
    a.contains("reactor") || a.contains("techlab") || a.contains("tech_lab")
}

#[derive(Clone, Copy)]
struct StructurePos {
    tag: i64,
    canonical_type: &'static str,
    born_loop: u32,
    died_loop: Option<u32>,
    pos_x: u8,
    pos_y: u8,
}

/// Reconstrói posições conhecidas de Barracks/Factory/Starport ao longo
/// do tempo a partir dos `entity_events`. Não usa `production_lanes`
/// pra evitar amarrar o diagnóstico ao algoritmo cuja correção estamos
/// investigando. **Não** trata lift-off / land-down — usa a posição do
/// `ProductionFinished` original. (Captar relocates é o objetivo da
/// fase C, fora do escopo deste diagnóstico.)
fn collect_parent_structures(events: &[EntityEvent]) -> Vec<StructurePos> {
    let mut out = Vec::new();
    let mut alive: HashMap<i64, usize> = HashMap::new();
    for ev in events {
        let canonical = match ev.entity_type.as_str() {
            "Barracks" => "Barracks",
            "Factory" => "Factory",
            "Starport" => "Starport",
            _ => continue,
        };
        match ev.kind {
            EntityEventKind::ProductionFinished => {
                if alive.contains_key(&ev.tag) {
                    continue;
                }
                alive.insert(ev.tag, out.len());
                out.push(StructurePos {
                    tag: ev.tag,
                    canonical_type: canonical,
                    born_loop: ev.game_loop,
                    died_loop: None,
                    pos_x: ev.pos_x,
                    pos_y: ev.pos_y,
                });
            }
            EntityEventKind::Died => {
                if let Some(idx) = alive.remove(&ev.tag) {
                    out[idx].died_loop = Some(ev.game_loop);
                }
            }
            _ => {}
        }
    }
    out
}

#[derive(Clone, Copy)]
struct AddonInit {
    tag: i64,
    addon_type: &'static str,
    game_loop: u32,
    pos_x: u8,
    pos_y: u8,
}

fn intern_addon(name: &str) -> Option<&'static str> {
    Some(match name {
        "BarracksReactor" => "BarracksReactor",
        "BarracksTechLab" => "BarracksTechLab",
        "FactoryReactor" => "FactoryReactor",
        "FactoryTechLab" => "FactoryTechLab",
        "StarportReactor" => "StarportReactor",
        "StarportTechLab" => "StarportTechLab",
        _ => return None,
    })
}

fn collect_addon_inits(events: &[EntityEvent]) -> Vec<AddonInit> {
    events
        .iter()
        .filter(|ev| {
            matches!(ev.kind, EntityEventKind::ProductionStarted)
                && is_incapacitating_addon(ev.entity_type.as_str())
        })
        .filter_map(|ev| {
            Some(AddonInit {
                tag: ev.tag,
                addon_type: intern_addon(ev.entity_type.as_str())?,
                game_loop: ev.game_loop,
                pos_x: ev.pos_x,
                pos_y: ev.pos_y,
            })
        })
        .collect()
}

/// Procura o cmd mais próximo (em game_loop) cuja ability menciona
/// `Reactor`/`TechLab` E pertence ao tipo do addon (best-effort: o
/// nome bruto da ability geralmente contém o tipo do addon, ex.
/// `BuildBarracksReactor`). Retorna `None` se nenhum cmd está dentro
/// de `PAIR_WINDOW`.
fn pair_cmd_for_addon(
    cmds: &[ProductionCmd],
    addon: AddonInit,
) -> Option<usize> {
    let mut best: Option<(usize, i64)> = None;
    for (i, cmd) in cmds.iter().enumerate() {
        if !is_addon_ability(&cmd.ability) {
            continue;
        }
        // Match fraco por tipo do addon: a ability do cmd deveria
        // mencionar pelo menos o tipo do parent ou o nome do addon.
        // Se a ability não bater, ainda aceita (best-effort) — o
        // diagnóstico mostra a string crua pra inspeção manual.
        let gap = cmd.game_loop as i64 - addon.game_loop as i64;
        if gap.abs() > PAIR_WINDOW {
            continue;
        }
        match best {
            Some((_, prev_gap)) if prev_gap.abs() <= gap.abs() => {}
            _ => best = Some((i, gap)),
        }
    }
    best.map(|(i, _)| i)
}

#[derive(Clone, Copy)]
struct Candidate {
    parent: StructurePos,
    dx: i32,
    dy: i32,
    d2: i32,
}

fn candidates_for_addon(
    parents: &[StructurePos],
    addon: AddonInit,
) -> Vec<Candidate> {
    let parent_type = addon_parent_canonical(addon.addon_type);
    let mut out: Vec<Candidate> = parents
        .iter()
        .filter(|p| Some(p.canonical_type) == parent_type)
        .filter(|p| p.born_loop <= addon.game_loop)
        .filter(|p| p.died_loop.map(|d| d > addon.game_loop).unwrap_or(true))
        .map(|p| {
            let dx = addon.pos_x as i32 - p.pos_x as i32;
            let dy = addon.pos_y as i32 - p.pos_y as i32;
            Candidate {
                parent: *p,
                dx,
                dy,
                d2: dx * dx + dy * dy,
            }
        })
        .collect();
    out.sort_by_key(|c| c.d2);
    out
}

pub fn run(replay_path: &Path) -> Result<(), String> {
    let timeline = parse_replay(replay_path, 0)?;
    let bo = extract_build_order(&timeline)?;
    println!(
        "Replay        : {}",
        replay_path.file_name().and_then(|s| s.to_str()).unwrap_or("?")
    );
    println!("Base build    : {}", timeline.base_build);
    println!("Players       : {}", timeline.players.len());
    println!("Loops/sec     : {:.2}", timeline.loops_per_second);
    println!();

    // Distribuição de offsets agregada cross-player no final.
    let mut offset_samples: HashMap<(&'static str, &'static str), Vec<(i32, i32)>> =
        HashMap::new();

    for (idx, player) in timeline.players.iter().enumerate() {
        if !matches!(player.race.as_str(), "Terran") {
            continue;
        }
        report_player(player, idx, &mut offset_samples);
        report_marines(player, idx, &timeline, &bo);
    }

    println!("=== OFFSET SUMMARY (Δx, Δy) por (parent_type → addon_type) ===");
    println!();
    if offset_samples.is_empty() {
        println!("  (nenhum addon resolvido)");
    } else {
        let mut keys: Vec<_> = offset_samples.keys().collect();
        keys.sort();
        for k in keys {
            let samples = &offset_samples[k];
            let n = samples.len();
            let dx_min = samples.iter().map(|(x, _)| *x).min().unwrap();
            let dx_max = samples.iter().map(|(x, _)| *x).max().unwrap();
            let dy_min = samples.iter().map(|(_, y)| *y).min().unwrap();
            let dy_max = samples.iter().map(|(_, y)| *y).max().unwrap();
            // Moda simples (Δx, Δy) mais frequente.
            let mut counts: HashMap<(i32, i32), usize> = HashMap::new();
            for s in samples {
                *counts.entry(*s).or_insert(0) += 1;
            }
            let mode = counts
                .iter()
                .max_by_key(|(_, c)| *c)
                .map(|((x, y), c)| (*x, *y, *c))
                .unwrap();
            println!(
                "  {:8} → {:18}  n={}  Δx∈[{:+},{:+}]  Δy∈[{:+},{:+}]  mode=({:+},{:+}) ({}/{})",
                k.0,
                k.1,
                n,
                dx_min,
                dx_max,
                dy_min,
                dy_max,
                mode.0,
                mode.1,
                mode.2,
                n,
            );
        }
    }
    println!();
    Ok(())
}

fn report_player(
    player: &PlayerTimeline,
    idx: usize,
    offset_samples: &mut HashMap<(&'static str, &'static str), Vec<(i32, i32)>>,
) {
    println!("--- Player {}: {} ({}) ---", idx + 1, player.name, player.race);
    println!();

    // [1] Cmds Build_*Reactor / Build_*TechLab.
    let addon_cmds: Vec<(usize, &ProductionCmd)> = player
        .production_cmds
        .iter()
        .enumerate()
        .filter(|(_, c)| is_addon_ability(&c.ability))
        .collect();

    println!("[1] BUILD-ADDON CMDS ({}):", addon_cmds.len());
    if addon_cmds.is_empty() {
        println!("    (nenhum)");
    } else {
        println!(
            "    {:>6}  {:32}  {:>15}  ",
            "loop", "ability", "producer_tag"
        );
        for (_, cmd) in &addon_cmds {
            let tag_str = cmd
                .producer_tags
                .first()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "(none)".to_string());
            println!(
                "    {:>6}  {:32}  {:>15}",
                cmd.game_loop, cmd.ability, tag_str
            );
        }
    }
    println!();

    // [2] UnitInit dos addons.
    let inits = collect_addon_inits(&player.entity_events);
    println!("[2] ADDON UnitInit EVENTS ({}):", inits.len());
    if inits.is_empty() {
        println!("    (nenhum)");
    } else {
        println!(
            "    {:>6}  {:18}  {:>10}  pos",
            "loop", "addon_type", "tag"
        );
        for a in &inits {
            println!(
                "    {:>6}  {:18}  {:>10}  ({:>3},{:>3})",
                a.game_loop, a.addon_type, a.tag, a.pos_x, a.pos_y
            );
        }
    }
    println!();

    // [3] Pareamento cmd ↔ addon.
    println!("[3] CMD ↔ ADDON PAIRING:");
    if inits.is_empty() {
        println!("    (sem addons pra parear)");
    } else {
        println!(
            "    {:>6}  {:18}  {:>10}  {:>6}  {:32}  {:>15}  gap",
            "init", "addon_type", "addon_tag", "cmd", "ability", "prod_tag"
        );
        for a in &inits {
            match pair_cmd_for_addon(&player.production_cmds, *a) {
                Some(ci) => {
                    let cmd = &player.production_cmds[ci];
                    let tag_str = cmd
                        .producer_tags
                        .first()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "(none)".to_string());
                    let gap = cmd.game_loop as i64 - a.game_loop as i64;
                    println!(
                        "    {:>6}  {:18}  {:>10}  {:>6}  {:32}  {:>15}  {:+}",
                        a.game_loop,
                        a.addon_type,
                        a.tag,
                        cmd.game_loop,
                        cmd.ability,
                        tag_str,
                        gap,
                    );
                }
                None => {
                    println!(
                        "    {:>6}  {:18}  {:>10}  (no cmd within ±{} loops)",
                        a.game_loop, a.addon_type, a.tag, PAIR_WINDOW,
                    );
                }
            }
        }
    }
    println!();

    // [4] Candidatos por addon.
    let parents = collect_parent_structures(&player.entity_events);
    println!("[4] ADDON ↔ CANDIDATE PARENTS (todos os candidatos vivos):");
    println!();
    let mut cmd_matches_geo_closest = 0usize;
    let mut cmd_matches_geo_other = 0usize;
    let mut no_cmd = 0usize;
    for a in &inits {
        let cands = candidates_for_addon(&parents, *a);
        let cmd_idx = pair_cmd_for_addon(&player.production_cmds, *a);
        let cmd_producer_tag = cmd_idx
            .and_then(|ci| player.production_cmds[ci].producer_tags.first().copied());

        println!(
            "    addon: {} (tag {}, loop {}, pos ({},{}))",
            a.addon_type, a.tag, a.game_loop, a.pos_x, a.pos_y
        );
        if cands.is_empty() {
            println!("        (nenhum parent vivo)");
        } else {
            println!(
                "        {:>10}  {:10}  {:>10}  {:>4}  {:>4}  {:>6}  {}",
                "parent_tag", "type", "pos", "Δx", "Δy", "d²", "note"
            );
            for (rank, c) in cands.iter().enumerate() {
                let mut note = String::new();
                if rank == 0 {
                    note.push_str("CLOSEST ");
                }
                if Some(c.parent.tag) == cmd_producer_tag {
                    note.push_str("← cmd's producer_tag ");
                }
                println!(
                    "        {:>10}  {:10}  ({:>3},{:>3})  {:>+4}  {:>+4}  {:>6}  {}",
                    c.parent.tag,
                    c.parent.canonical_type,
                    c.parent.pos_x,
                    c.parent.pos_y,
                    c.dx,
                    c.dy,
                    c.d2,
                    note,
                );
            }

            // Coleta amostra de offset usando o cmd se disponível,
            // caso contrário o parent geometricamente mais próximo.
            let chosen = match cmd_producer_tag {
                Some(tag) => cands.iter().find(|c| c.parent.tag == tag).copied(),
                None => cands.first().copied(),
            };
            if let Some(c) = chosen {
                offset_samples
                    .entry((c.parent.canonical_type, a.addon_type))
                    .or_default()
                    .push((c.dx, c.dy));
            }

            // Estatística de "cmd está de acordo com proximidade?"
            match (cmd_producer_tag, cands.first().map(|c| c.parent.tag)) {
                (Some(cmd_tag), Some(closest_tag)) if cmd_tag == closest_tag => {
                    cmd_matches_geo_closest += 1;
                }
                (Some(_), Some(_)) => cmd_matches_geo_other += 1,
                (None, _) => no_cmd += 1,
                _ => {}
            }
        }
        println!();
    }

    println!("[5] PLAYER SUMMARY:");
    println!("    addons total                : {}", inits.len());
    println!("    cmd matches geo-closest     : {}", cmd_matches_geo_closest);
    println!("    cmd matches a NON-closest   : {}  (← casos interessantes)", cmd_matches_geo_other);
    println!("    no cmd matched              : {}", no_cmd);
    println!();
}

/// Converte um `game_loop` em string `mm:ss`, usando os loops_per_second
/// do replay. Útil pra cruzar com a UI da GUI.
fn fmt_loop(game_loop: u32, lps: f64) -> String {
    let secs = (game_loop as f64) / lps;
    let m = (secs / 60.0).floor() as u32;
    let s = (secs - (m as f64) * 60.0).floor() as u32;
    format!("{:02}:{:02}", m, s)
}

/// Dump focado em Marines: lista todos os UnitBornEvents de Marine,
/// todos os Train_Marine cmds, e os entries do build_order com action=
/// Marine. Marca entries com `start == finish` (sintoma "instantâneo").
/// Também imprime trace de pares paralelos detectáveis pelo predicado
/// `prev_finish_by_producer == projected_finish` para ajudar a entender
/// quais Marines deveriam ter sido detectados como par paralelo do
/// Reactor.
fn report_marines(
    player: &PlayerTimeline,
    idx: usize,
    timeline: &ReplayTimeline,
    bo: &crate::build_order::BuildOrderResult,
) {
    let lps = timeline.loops_per_second;
    println!(
        "--- Player {}: {} ({}) — Marines ---",
        idx + 1,
        player.name,
        player.race
    );
    println!();

    // [M1] Eventos de Marine (ProductionStarted + Finished, Born events
    // emitem ambos no mesmo loop).
    let marine_events: Vec<&EntityEvent> = player
        .entity_events
        .iter()
        .filter(|e| {
            e.entity_type == "Marine"
                && matches!(e.kind, EntityEventKind::ProductionStarted)
        })
        .collect();
    println!("[M1] MARINE BORN EVENTS (ProductionStarted, {}):", marine_events.len());
    if marine_events.is_empty() {
        println!("    (nenhum)");
    } else {
        println!(
            "    {:>6} {:8}  {:>10}  {:>15}  ability",
            "loop", "(mm:ss)", "tag", "creator_tag"
        );
        for e in &marine_events {
            let creator = e
                .creator_tag
                .map(|t| t.to_string())
                .unwrap_or_else(|| "(none)".to_string());
            let ability = e.creator_ability.as_deref().unwrap_or("(none)");
            println!(
                "    {:>6} ({:6})  {:>10}  {:>15}  {}",
                e.game_loop,
                fmt_loop(e.game_loop, lps),
                e.tag,
                creator,
                ability,
            );
        }
    }
    println!();

    // [M2] Cmds Train_Marine.
    let marine_cmds: Vec<(usize, &ProductionCmd)> = player
        .production_cmds
        .iter()
        .enumerate()
        .filter(|(_, c)| c.ability == "Marine" || c.ability.contains("Marine"))
        .collect();
    println!("[M2] MARINE CMDS ({}):", marine_cmds.len());
    if marine_cmds.is_empty() {
        println!("    (nenhum)");
    } else {
        println!(
            "    {:>6} {:8}  {:24}  {:>15}",
            "loop", "(mm:ss)", "ability", "producer_tag"
        );
        for (_, c) in &marine_cmds {
            let tag = c
                .producer_tags
                .first()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "(none)".to_string());
            println!(
                "    {:>6} ({:6})  {:24}  {:>15}",
                c.game_loop,
                fmt_loop(c.game_loop, lps),
                c.ability,
                tag,
            );
        }
    }
    println!();

    // [M3] Trace de detecção de par paralelo: para cada Marine, qual o
    // estado de prev_finish_by_producer no momento do processamento?
    // Reproduz o predicado do build_order/extract.rs para mostrar quais
    // Marines o detector identificaria como par paralelo.
    println!("[M3] PARALLEL-PAIR DETECTION TRACE:");
    println!(
        "    {:>6} {:8}  {:>10}  {:>15}  {:>14}  {}",
        "loop", "(mm:ss)", "tag", "creator_tag", "prev_finish", "is_parallel?"
    );
    const TOLERANCE: u32 = 50;
    let mut prev_finish_by_producer: HashMap<i64, u32> = HashMap::new();
    let mut instant_count = 0usize;
    for e in &marine_events {
        let projected_finish = e.game_loop;
        let creator = e.creator_tag;
        let prev_finish = creator
            .and_then(|t| prev_finish_by_producer.get(&t).copied())
            .unwrap_or(0);
        let is_parallel = prev_finish > 0
            && projected_finish.saturating_sub(prev_finish) <= TOLERANCE;

        let creator_str = creator
            .map(|t| t.to_string())
            .unwrap_or_else(|| "(none)".to_string());
        let prev_str = if prev_finish > 0 {
            prev_finish.to_string()
        } else {
            "—".to_string()
        };
        println!(
            "    {:>6} ({:6})  {:>10}  {:>15}  {:>14}  {}",
            e.game_loop,
            fmt_loop(e.game_loop, lps),
            e.tag,
            creator_str,
            prev_str,
            if is_parallel { "YES (parallel — no cmd consumed)" } else { "no" },
        );

        // Avança chain SEMPRE — para par paralelo, atualiza para o
        // finish do segundo (que é o real fim da janela do par).
        if let Some(t) = creator {
            prev_finish_by_producer.insert(t, projected_finish);
        }
    }
    println!();

    // [M4] Entries do build_order action="Marine" — saída final que o
    // usuário vê na GUI (após dedup). Marca entries com start == finish.
    let bo_player = bo.players.iter().find(|p| p.name == player.name);
    if let Some(bo_player) = bo_player {
        let marine_entries: Vec<&crate::build_order::BuildOrderEntry> = bo_player
            .entries
            .iter()
            .filter(|e| e.action == "Marine")
            .collect();
        println!("[M4] BUILD_ORDER MARINE ENTRIES (post-dedup, {}):", marine_entries.len());
        println!(
            "    {:>6} {:8}  {:>6} {:8}  count  outcome  duration  comment",
            "start", "(mm:ss)", "finish", "(mm:ss)"
        );
        for e in &marine_entries {
            let dur = e.finish_loop.saturating_sub(e.game_loop);
            let comment = if e.game_loop == e.finish_loop {
                instant_count += 1;
                "← INSTANT (start == finish, BUG)"
            } else if dur < 100 {
                "← suspeito (duração muito curta)"
            } else {
                ""
            };
            println!(
                "    {:>6} ({:6})  {:>6} ({:6})  {:>5}  {:?}  {:>8}  {}",
                e.game_loop,
                fmt_loop(e.game_loop, lps),
                e.finish_loop,
                fmt_loop(e.finish_loop, lps),
                e.count,
                e.outcome,
                dur,
                comment,
            );
        }
    }
    println!();

    println!("[M5] PLAYER MARINE SUMMARY:");
    println!("    born events           : {}", marine_events.len());
    println!("    cmds                  : {}", marine_cmds.len());
    println!("    instant entries (BUG) : {}", instant_count);
    println!();
}
