//! Insight: quantos workers o jogador **produziu** vs quantos
//! **poderia ter produzido** até um instante X do jogo. A ideia é
//! expor lapsos de macro ("esqueci de clicar o Nexus"), não perdas
//! em combate — por isso ambos os lados da comparação ignoram mortes.
//!
//! `produced` = 12 iniciais + todo worker finalizado pelo jogador até
//! `until`, independente de ter morrido depois. `potential` = mesmo
//! cálculo para uma produção ideal (cada base produz non-stop a
//! partir do seu `finish_loop`), respeitando chronos em probes
//! (Protoss) e a regra de 80% das larvas para drones (Zerg).

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{EntityEventKind, InjectCmd, ReplayTimeline};

/// Workers iniciais de qualquer raça.
const INITIAL_WORKERS: u32 = 12;

/// Larva natural — 1 larva gerada a cada 11s (176 loops) até o cap de 3.
const LARVA_PERIOD_LOOPS: u32 = 176;

/// Cap de larvas naturais por hatch (injects ignoram esse cap).
const LARVA_NATURAL_CAP: u32 = 3;

/// Delay entre inject e spawn das 3 larvas: 29s = 464 loops.
const INJECT_DELAY_LOOPS: u32 = 464;

/// Quantidade de larvas extras por inject.
const INJECT_LARVAE: u32 = 3;

/// Aceleração do chrono boost (produção fica 1.5× mais rápida durante
/// ~20s). Expressamos como divisor de tempo de build.
const CHRONO_SPEEDUP: f64 = 1.5;

/// Fator limitante do potencial. Ajuda o usuário a entender onde o macro
/// "parou" antes de chegar ao teto teórico.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitingFactor {
    /// Só a main estava disponível no intervalo — expansões tardias
    /// ou ausentes.
    Bases,
    /// Protoss: o orçamento de chronos gastos em probes foi consumido
    /// antes do fim do intervalo — mais chronos nos probes teriam
    /// gerado mais workers.
    ChronoBudget,
    /// Zerg: a regra de 80% das larvas pra drones foi o teto.
    LarvaSupply80,
    /// Produção rodou sem travas: todas as bases produziram non-stop.
    NoLimit,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerPotential {
    /// Total de workers que o jogador produziu até `until_loop`
    /// (inclui os 12 iniciais; ignora mortes — o foco é macro, não
    /// preservação).
    pub produced: u32,
    /// Total de workers que o jogador poderia ter produzido pelo
    /// algoritmo no mesmo intervalo.
    pub potential: u32,
    pub limited_by: LimitingFactor,
    /// `true` quando `until_loop` foi clamped para o fim real do replay
    /// (replay terminou antes de X minutos).
    pub clamped: bool,
}

/// Calcula o potencial de workers em `until_loop` para o jogador
/// `player_idx` do replay.
///
/// `chrono_probe_count` é o número de chronos que o jogador de fato
/// aplicou em produção de probes — a GUI calcula isso a partir do
/// `BuildOrderResult` (soma `chrono_boosts` em entradas cujo
/// `action == "Probe"`). Para Terran/Zerg ignoramos esse valor.
pub fn compute_worker_potential(
    timeline: &ReplayTimeline,
    player_idx: usize,
    until_loop: u32,
    chrono_probe_count: u32,
) -> WorkerPotential {
    let player = match timeline.players.get(player_idx) {
        Some(p) => p,
        None => {
            return WorkerPotential {
                produced: 0,
                potential: 0,
                limited_by: LimitingFactor::NoLimit,
                clamped: false,
            };
        }
    };

    let clamped = until_loop > timeline.game_loops;
    let until = until_loop.min(timeline.game_loops);

    let produced = count_produced_workers(timeline, player_idx, until);
    let bases = collect_bases(timeline, player_idx, until);
    let base_build = timeline.base_build;

    // Nenhuma base pronta até `until` → só a main (loop 0). Se nem a
    // main apareceu nos eventos, garantimos uma base virtual em 0.
    let bases = if bases.is_empty() {
        vec![BaseInterval {
            finish_loop: 0,
            death_loop: u32::MAX,
            fly_periods: Vec::new(),
        }]
    } else {
        bases
    };

    let (potential, limited_by) = match player.race.as_str() {
        "Terran" => {
            let scv_bt = balance_data::build_time_loops("SCV", base_build);
            let oc_bt = balance_data::build_time_loops("OrbitalCommand", base_build);
            simulate_terran(&bases, until, scv_bt, oc_bt)
        }
        "Protoss" => {
            let bt = balance_data::build_time_loops("Probe", base_build);
            simulate_greedy(&bases, until, bt, chrono_probe_count)
        }
        "Zerg" => {
            let bt = balance_data::build_time_loops("Drone", base_build);
            simulate_zerg(&bases, until, bt, &player.inject_cmds)
        }
        _ => (produced, LimitingFactor::NoLimit),
    };

    WorkerPotential {
        produced,
        potential,
        limited_by,
        clamped,
    }
}

// ── Extração de eventos do replay ───────────────────────────────────

fn is_worker_name(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone")
}

fn is_base_type(name: &str) -> bool {
    matches!(
        name,
        "CommandCenter"
            | "OrbitalCommand"
            | "PlanetaryFortress"
            | "Nexus"
            | "Hatchery"
            | "Lair"
            | "Hive"
    )
}

/// Conta quantos workers o jogador **produziu** até `until`, ignorando
/// mortes — o card mede macro de produção, não sobrevivência. Soma 12
/// iniciais aos `ProductionFinished` de Probe/SCV/Drone no intervalo.
fn count_produced_workers(timeline: &ReplayTimeline, player_idx: usize, until: u32) -> u32 {
    let player = &timeline.players[player_idx];
    let mut finished: u32 = 0;
    for ev in &player.entity_events {
        if ev.game_loop > until {
            break;
        }
        if !is_worker_name(&ev.entity_type) {
            continue;
        }
        if matches!(ev.kind, EntityEventKind::ProductionFinished) {
            finished += 1;
        }
    }
    // Os 12 workers iniciais aparecem como UnitBorn em loop 0 → já
    // vêm contados em `finished` (o tracker emite Started+Finished
    // no mesmo loop). Não somamos INITIAL_WORKERS aqui.
    finished
}

#[derive(Clone, Debug)]
struct BaseInterval {
    finish_loop: u32,
    /// `u32::MAX` quando a base não morreu dentro de `until`.
    death_loop: u32,
    /// Intervalos `[lift, land]` em que a base estava voando. Terran
    /// usa isso para descontar tempo improdutivo da janela de produção.
    fly_periods: Vec<(u32, u32)>,
}

/// Coleta bases (producers de worker) do jogador, deduplicadas por tag.
/// Morphs in-place (CC→OC/PF, Hatch→Lair→Hive) mantêm o mesmo tag — a
/// base continua viva, então contamos apenas a primeira conclusão e
/// ignoramos o `Died` sintético que acompanha o morph.
///
/// Também detecta ciclos lift/land (X → XFlying → X) e registra cada
/// intervalo de voo em `fly_periods` — Terran usa esses intervalos pra
/// descontar tempo improdutivo da janela de produção.
fn collect_bases(timeline: &ReplayTimeline, player_idx: usize, until: u32) -> Vec<BaseInterval> {
    let player = &timeline.players[player_idx];

    // Agrupa eventos por tag — state machine por base fica mais clara.
    let mut by_tag: HashMap<i64, Vec<&crate::replay::EntityEvent>> = HashMap::new();
    for ev in &player.entity_events {
        if ev.game_loop > until {
            break;
        }
        by_tag.entry(ev.tag).or_default().push(ev);
    }

    let mut out = Vec::new();
    for (_, evs) in by_tag {
        // Primeira ProductionFinished como base (normal ou pouso — o
        // pouso emite Finished do tipo terrestre, mas só aceita o
        // primeiro, então pousos posteriores não contam como "new base").
        let Some(first) = evs.iter().position(|e| {
            matches!(e.kind, EntityEventKind::ProductionFinished) && is_base_type(&e.entity_type)
        }) else {
            continue;
        };
        let finish_loop = evs[first].game_loop;

        let mut fly_periods: Vec<(u32, u32)> = Vec::new();
        let mut fly_start: Option<u32> = None;
        let mut death_loop: u32 = u32::MAX;

        for ev in &evs[first + 1..] {
            if !matches!(ev.kind, EntityEventKind::Died) {
                continue;
            }
            if is_base_type(&ev.entity_type) {
                // Lift: Died(X) acompanhado de Started(XFlying) no mesmo loop.
                let flying = format!("{}Flying", ev.entity_type);
                let is_lift = evs.iter().any(|e| {
                    e.game_loop == ev.game_loop
                        && matches!(e.kind, EntityEventKind::ProductionStarted)
                        && e.entity_type == flying
                });
                if is_lift {
                    fly_start = Some(ev.game_loop);
                    continue;
                }
                // Morph CC→OC/PF: Died(X) acompanhado de Started(Y) com Y base-type.
                let is_morph = evs.iter().any(|e| {
                    e.game_loop == ev.game_loop
                        && matches!(e.kind, EntityEventKind::ProductionStarted)
                        && is_base_type(&e.entity_type)
                });
                if is_morph {
                    continue;
                }
                // Morte real.
                if let Some(start) = fly_start.take() {
                    fly_periods.push((start, ev.game_loop));
                }
                death_loop = ev.game_loop;
                break;
            }
            if ev.entity_type.ends_with("Flying") {
                // Pouso ou morte voando. Em ambos, o intervalo de voo
                // termina aqui. Pouso: há Started(base) pareado.
                let landed = ev
                    .entity_type
                    .strip_suffix("Flying")
                    .map(|base| {
                        evs.iter().any(|e| {
                            e.game_loop == ev.game_loop
                                && matches!(e.kind, EntityEventKind::ProductionStarted)
                                && e.entity_type == base
                        })
                    })
                    .unwrap_or(false);
                if let Some(start) = fly_start.take() {
                    fly_periods.push((start, ev.game_loop));
                }
                if !landed {
                    death_loop = ev.game_loop;
                    break;
                }
            }
        }

        // Voo ainda em curso no fim do intervalo: considera o voo
        // como contínuo até `until` (improdutivo).
        if let Some(start) = fly_start {
            fly_periods.push((start, u32::MAX));
        }

        out.push(BaseInterval {
            finish_loop,
            death_loop,
            fly_periods,
        });
    }

    out.sort_by_key(|b| b.finish_loop);
    out
}

// ── Simulação Terran ────────────────────────────────────────────────

/// Fórmula Terran: cada CC só produz workers depois de virar Orbital
/// Command (único pré-requisito quantificável para produção "ótima" —
/// o morph bloqueia a produção por `orbital_build` loops). Produção
/// começa em `cc_finish + orbital_build` e segue até `min(until, death)`.
///
/// `scvs_por_cc = (janela_produtiva) / scv_build_time`.
///
/// Se `cc_finish + orbital_build >= until`, o CC não contribui.
fn simulate_terran(
    bases: &[BaseInterval],
    until: u32,
    scv_build_time: u32,
    orbital_build_time: u32,
) -> (u32, LimitingFactor) {
    if scv_build_time == 0 {
        return (INITIAL_WORKERS, LimitingFactor::NoLimit);
    }

    let mut total = INITIAL_WORKERS;
    let mut contributing = 0u32;
    for base in bases {
        let start = base.finish_loop.saturating_add(orbital_build_time);
        let end = until.min(base.death_loop);
        if start >= end {
            continue;
        }
        // Janela produtiva = [start, end] menos tempo de voo que
        // intersecta essa janela.
        let mut productive: u32 = end - start;
        for &(fs, fe) in &base.fly_periods {
            let ovl_start = fs.max(start);
            let ovl_end = fe.min(end);
            if ovl_end > ovl_start {
                productive = productive.saturating_sub(ovl_end - ovl_start);
            }
        }
        let scvs = productive / scv_build_time;
        total += scvs;
        if scvs > 0 {
            contributing += 1;
        }
    }

    let limiting = if contributing <= 1 {
        LimitingFactor::Bases
    } else {
        LimitingFactor::NoLimit
    };
    (total, limiting)
}

// ── Simulação Protoss ───────────────────────────────────────────────

/// Simulação gulosa por base: cada base produz workers non-stop a
/// partir de `finish_loop` até `min(until, death_loop)`. Chrono budget
/// acelera 1.5× um worker por cada chrono gasto pelo jogador em probes.
fn simulate_greedy(
    bases: &[BaseInterval],
    until: u32,
    build_time: u32,
    mut chrono_budget: u32,
) -> (u32, LimitingFactor) {
    if build_time == 0 {
        return (INITIAL_WORKERS, LimitingFactor::NoLimit);
    }

    let chrono_bt = ((build_time as f64) / CHRONO_SPEEDUP).round() as u32;
    let chrono_available_initial = chrono_budget;

    // Produz non-stop por base até `until`. Estratégia gulosa para
    // alocar chronos: pegamos a base com o próximo slot mais cedo e
    // aplicamos chrono ali (maximiza workers finalizados em `until`).
    let mut next_free: Vec<u32> = bases.iter().map(|b| b.finish_loop).collect();
    let mut workers: u32 = INITIAL_WORKERS;

    loop {
        let mut best: Option<(usize, u32)> = None;
        for (i, &next) in next_free.iter().enumerate() {
            let b = &bases[i];
            if next >= b.death_loop {
                continue;
            }
            if next > until {
                continue;
            }
            match best {
                None => best = Some((i, next)),
                Some((_, n)) if next < n => best = Some((i, next)),
                _ => {}
            }
        }
        let Some((i, start)) = best else {
            break;
        };

        let bt = if chrono_budget > 0 {
            chrono_budget -= 1;
            chrono_bt
        } else {
            build_time
        };
        let finish = start + bt;
        next_free[i] = finish;
        if finish <= until {
            workers += 1;
        } else {
            next_free[i] = until + 1;
        }
    }

    // Limiting factor: só main a intervalo inteiro → Bases; acabou
    // chrono (e tinha algum) → ChronoBudget; senão NoLimit.
    let limiting = if bases.len() == 1 && bases[0].finish_loop == 0 && until > 0 {
        LimitingFactor::Bases
    } else if chrono_available_initial > 0 && chrono_budget == 0 {
        LimitingFactor::ChronoBudget
    } else {
        LimitingFactor::NoLimit
    };

    (workers, limiting)
}

// ── Simulação Zerg ──────────────────────────────────────────────────

/// Simulação do pool de larvas por hatch + regra de 80% para drones.
/// Geramos todos os eventos de larva disponíveis até `until` e
/// consumimos 4 a cada 5 (80%) em ordem cronológica, respeitando o
/// cap de saturação no instante de conclusão de cada drone.
fn simulate_zerg(
    bases: &[BaseInterval],
    until: u32,
    drone_time: u32,
    injects: &[InjectCmd],
) -> (u32, LimitingFactor) {
    if drone_time == 0 {
        return (INITIAL_WORKERS, LimitingFactor::NoLimit);
    }

    // Coleta eventos de "larva disponível em L" em ordem cronológica,
    // simulando o pool natural (cap 3) por base + injects (sem cap).
    let mut larva_times: Vec<u32> = Vec::new();

    for (i, base) in bases.iter().enumerate() {
        // Pool natural por base.
        let mut pool: u32 = 3; // cada base nasce com 3 larvas
        let mut clock = base.finish_loop;
        // Registra as 3 larvas iniciais como disponíveis em finish_loop.
        for _ in 0..pool {
            larva_times.push(clock);
        }
        while clock < until && clock < base.death_loop {
            clock += LARVA_PERIOD_LOOPS;
            if clock >= until || clock >= base.death_loop {
                break;
            }
            if pool < LARVA_NATURAL_CAP {
                pool += 1;
            } else {
                // Estouro: como 80% serão consumidas, assumimos que a
                // larva foi usada imediatamente e a contabilizamos.
                larva_times.push(clock);
            }
        }

        // Injects endereçados a esta base (por posição aproximada). O
        // replay guarda `target_tag_index` nos injects, mas nosso
        // `BaseInterval` não preservou o tag_index — distribuímos
        // injects pela ordem cronológica total (próxima seção). Aqui
        // só contabilizamos as larvas iniciais e naturais.
        let _ = i;
    }

    // Injects: cada um gera 3 larvas em `game_loop + INJECT_DELAY`.
    // Não distinguimos qual hatch — assumimos que o jogador injectou
    // hatches vivas (já que o inject foi emitido, uma queen resolveu).
    for inj in injects {
        let spawn_loop = inj.game_loop + INJECT_DELAY_LOOPS;
        if spawn_loop >= until {
            continue;
        }
        for _ in 0..INJECT_LARVAE {
            larva_times.push(spawn_loop);
        }
    }

    larva_times.sort_unstable();

    // Consome 80% em ordem cronológica. De cada 5 larvas, 4 viram
    // drone; a quinta (conceitualmente) alimenta outras unidades.
    let mut workers: u32 = INITIAL_WORKERS;
    for (idx, &larva_loop) in larva_times.iter().enumerate() {
        if idx % 5 == 4 {
            continue;
        }
        let drone_ready = larva_loop + drone_time;
        if drone_ready > until {
            continue;
        }
        workers += 1;
    }

    let limiting = if bases.len() == 1 && bases[0].finish_loop == 0 {
        LimitingFactor::Bases
    } else {
        LimitingFactor::LarvaSupply80
    };

    (workers, limiting)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn load_replay(name: &str) -> ReplayTimeline {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name);
        crate::replay::parse_replay(&path, 0).expect("parse")
    }

    #[test]
    fn smoke_examples_dont_panic() {
        for name in ["replay1.SC2Replay", "replay2.SC2Replay", "replay3.SC2Replay"] {
            let tl = load_replay(name);
            for i in 0..tl.players.len() {
                let wp = compute_worker_potential(&tl, i, 6 * 960, 0);
                assert!(wp.potential >= wp.produced.min(INITIAL_WORKERS));
            }
        }
    }

    #[test]
    fn at_loop_0_potential_is_initial() {
        let tl = load_replay("replay1.SC2Replay");
        for i in 0..tl.players.len() {
            let wp = compute_worker_potential(&tl, i, 0, 0);
            assert_eq!(wp.potential, INITIAL_WORKERS);
        }
    }

    #[test]
    fn potential_at_6min_is_at_least_initial() {
        let tl = load_replay("replay1.SC2Replay");
        for i in 0..tl.players.len() {
            let wp = compute_worker_potential(&tl, i, 6 * 960, 0);
            assert!(
                wp.potential >= INITIAL_WORKERS,
                "player {} potential {} < initial",
                i,
                wp.potential
            );
        }
    }

    #[test]
    fn potential_exceeds_produced_on_examples() {
        // Invariante: o potencial é um *teto* da produção, então não
        // pode ficar abaixo do que o jogador de fato produziu.
        for name in ["replay1.SC2Replay", "replay2.SC2Replay", "replay3.SC2Replay"] {
            let tl = load_replay(name);
            let until = (6.0 * 60.0 * tl.loops_per_second).round() as u32;
            for i in 0..tl.players.len() {
                let wp = compute_worker_potential(&tl, i, until, 0);
                assert!(
                    wp.potential >= wp.produced,
                    "{}: player {} potential {} < produced {}",
                    name, i, wp.potential, wp.produced
                );
            }
        }
    }

    #[test]
    fn clamp_flag_when_past_game_end() {
        let tl = load_replay("replay1.SC2Replay");
        let huge = tl.game_loops + 10_000;
        let wp = compute_worker_potential(&tl, 0, huge, 0);
        assert!(wp.clamped);
    }
}
