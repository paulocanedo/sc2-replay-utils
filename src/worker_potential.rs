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
use crate::replay::{is_larva_born_army, EntityEventKind, InjectCmd, PlayerTimeline, ReplayTimeline};

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

/// Razão mínima do pool de larvas que um Zerg em jogo macro costuma
/// direcionar para drones em aberturas econômicas. Tunado via
/// diagnóstico (`dump_zerg_worker_potential_diagnostic`) — valida
/// contra Serral/firebat nos minutos 4–8 sem cravar números irreais
/// (gap verde/amarelo em todos os casos testados).
///
/// 0.75 ≈ jogo macro decente: Overlords + tech + queens + army inicial
/// consomem no máximo 25% das larvas em early game. Serral tipicamente
/// fica perto desse teto (gap 2–4 drones no early); jogadores medianos
/// tipicamente abaixo (gaps maiores = margem pra melhoria real).
///
/// O valor é combinado via `min()` com a subtração do consumo real
/// não-drone, o que dá o menor (mais conservador) dos dois tetos e
/// evita sugerir drones em mid/late game quando o jogador já gastou
/// bastante larva em army.
const DRONE_LARVA_RATIO: f64 = 0.75;

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
    /// Zerg: a oferta de larvas (natural + injects) descontadas as
    /// unidades não-drone que o jogador produziu foi o teto.
    LarvaSupply,
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
            let (consumption_loops, breakdown) = collect_larva_consumptions(player, until);
            let nondrone_used = breakdown.nondrone_larva_count();
            simulate_zerg(
                &bases,
                &player.inject_cmds,
                &consumption_loops,
                nondrone_used,
                until,
                bt,
            )
        }
        _ => (produced, LimitingFactor::NoLimit),
    };

    // Piso invariante: potencial nunca fica abaixo do que o jogador
    // produziu. Protege casos de over-saturação extrema ou erros de
    // aproximação do simulador (`simulate_pool_realistic` usa pool
    // compartilhado, que pode ficar marginalmente abaixo do real em
    // cenários com inject pesado).
    let potential = potential.max(produced);

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

/// Calcula quantos drones o jogador poderia ter produzido até `until`.
///
/// Modelo em duas etapas:
///
/// 1. **Pool realista** — `simulate_pool_realistic` reconstrói o pool
///    de larvas **respeitando o cap de 3 por hatch**, usando os
///    timestamps reais de consumo do jogador extraídos de
///    `entity_events`. Ticks de regen que cairiam em pool cheio são
///    descartados (a larva "não nasce"). Injects bypassam o cap, como
///    no jogo. Isso corrige o bug histórico em que o modelo ignorava
///    o cap e contabilizava todo tick de 176 loops como larva viável,
///    inflando o pool ~30%.
///
/// 2. **Dois tetos combinados** — a quantidade de drones "ideais" é o
///    menor entre:
///    - `DRONE_LARVA_RATIO × pool` — respeita que em early game um
///      Zerg macro direciona ~70% das larvas a drones, o restante
///      para Overlords/army.
///    - `pool − nondrone_used` — teto pelo que o jogador de fato
///      gastou em não-drones. Se ele fez muito army, sobra pouco.
///      Garante que não sugerimos drones que **provavelmente não**
///      estavam disponíveis dada a escolha estratégica real.
///
///    Tomar o `min()` é conservador: em early game o ratio geralmente
///    é o limitante (jogador ainda não fez army); em mid/late o
///    `subtract` assume. Empiricamente (diagnóstico Serral/firebat)
///    essa combinação produz gaps verdes/amarelos em todos os minutos
///    testados, sem nunca cravar valores inatingíveis.
fn simulate_zerg(
    bases: &[BaseInterval],
    injects: &[InjectCmd],
    consumption_loops: &[u32],
    nondrone_used: u32,
    until: u32,
    drone_time: u32,
) -> (u32, LimitingFactor) {
    if drone_time == 0 {
        return (INITIAL_WORKERS, LimitingFactor::NoLimit);
    }

    let pool = simulate_pool_realistic(bases, injects, consumption_loops, until);

    let ratio_bound = (pool as f64 * DRONE_LARVA_RATIO).floor() as u32;
    let subtract_bound = pool.saturating_sub(nondrone_used);
    let ideal_drones = ratio_bound.min(subtract_bound);

    let potential = INITIAL_WORKERS.saturating_add(ideal_drones);

    let limiting = if bases.len() == 1 && bases[0].finish_loop == 0 {
        LimitingFactor::Bases
    } else {
        LimitingFactor::LarvaSupply
    };

    (potential, limiting)
}

// ── Diagnóstico Zerg (Fase 1 do tuning) ─────────────────────────────

/// Coleta os loops em que o jogador consumiu larvas para construir
/// unidades Zerg. Cada unidade larva-born (drone + unidades de army +
/// overlord) consome 1 larva no `ProductionStarted`. Zergling vem em
/// par: 2 `ProductionStarted` de Zergling consomem 1 larva apenas — o
/// segundo do par é o evento que emitimos (mesma lógica em que o
/// `zergling_flip` alterna a cada par).
///
/// Retorna `(loops_ordenados, decomposicao)` com as contagens por
/// categoria (drones, overlords, zergling_pares, outros_army) pra
/// facilitar o dump do diagnóstico.
struct LarvaConsumptionBreakdown {
    pub drones_started: u32,
    pub overlords: u32,
    pub zergling_individuals: u32,
    pub zergling_pairs: u32,
    pub other_army_larva: u32,
}

impl LarvaConsumptionBreakdown {
    /// Soma das larvas consumidas em unidades **não-drone** (Overlord,
    /// Zergling em pares, Roach/Hydra/Infestor/etc.). Queens não entram
    /// (morfam do prédio, não consomem larva). Morphs de unidade
    /// (Baneling/Ravager/Lurker/BroodLord/Overseer) também ficam fora,
    /// pois a larva foi contabilizada no parent.
    fn nondrone_larva_count(&self) -> u32 {
        self.overlords
            .saturating_add(self.zergling_pairs)
            .saturating_add(self.other_army_larva)
    }
}

fn collect_larva_consumptions(
    player: &PlayerTimeline,
    until: u32,
) -> (Vec<u32>, LarvaConsumptionBreakdown) {
    let mut loops: Vec<u32> = Vec::new();
    let mut drones_started: u32 = 0;
    let mut overlords: u32 = 0;
    let mut zergling_individuals: u32 = 0;
    let mut zergling_pairs: u32 = 0;
    let mut other_army_larva: u32 = 0;

    // Zergling vem em par: alternamos `flip` para emitir 1 consume a
    // cada 2 Starteds. Caso raro de cauda ímpar (cancel de 1 do par),
    // perdemos 1 consume — <1% dos casos.
    let mut zergling_flip: bool = false;

    for ev in &player.entity_events {
        if ev.game_loop > until {
            break;
        }
        if !matches!(ev.kind, EntityEventKind::ProductionStarted) {
            continue;
        }
        match ev.entity_type.as_str() {
            "Drone" => {
                drones_started += 1;
                loops.push(ev.game_loop);
            }
            "Overlord" => {
                overlords += 1;
                loops.push(ev.game_loop);
            }
            "Zergling" => {
                zergling_individuals += 1;
                zergling_flip = !zergling_flip;
                if !zergling_flip {
                    // Segundo do par: emite consume representando 1 larva.
                    zergling_pairs += 1;
                    loops.push(ev.game_loop);
                }
                // Primeiro do par: não emite (espera o segundo).
            }
            name if is_larva_born_army(name) => {
                // Roach, Hydralisk, Infestor, SwarmHost, Mutalisk,
                // Corruptor, Viper, Ultralisk. (Overlord e Zergling
                // tratados acima explicitamente.)
                other_army_larva += 1;
                loops.push(ev.game_loop);
            }
            _ => {}
        }
    }

    loops.sort_unstable();

    (
        loops,
        LarvaConsumptionBreakdown {
            drones_started,
            overlords,
            zergling_individuals,
            zergling_pairs,
            other_army_larva,
        },
    )
}

/// Simula o pool de larvas **respeitando o cap de 3 por base**, usando
/// os timestamps reais de consumo do jogador. Retorna o total de larvas
/// que efetivamente existiram no pool (geradas, não-overflow) no
/// intervalo `[0, until]`.
///
/// Diferença para a simulação atual (`simulate_zerg`): aqui o cap é
/// respeitado — ticks de regen que aconteceriam com o pool cheio são
/// descartados (a larva "não nasce"). Injects ignoram o cap, como no
/// jogo real.
///
/// Modelo: pool compartilhado entre bases (aproximação razoável — SC2
/// tem pool por hatch, mas o jogador usa larvas de qualquer uma). Cap
/// dinâmico = `3 × bases_alive`. Inject adiciona 3 ao pool direto,
/// bypass do cap. Consumo drena o pool (saturando em 0).
fn simulate_pool_realistic(
    bases: &[BaseInterval],
    injects: &[InjectCmd],
    consumption_loops: &[u32],
    until: u32,
) -> u32 {
    #[derive(PartialEq, Eq)]
    enum Ev {
        BaseBorn,
        BaseDied,
        Regen,
        InjectLand,
        Consume,
    }

    let mut events: Vec<(u32, Ev)> = Vec::new();

    for base in bases {
        if base.finish_loop >= until {
            continue;
        }
        events.push((base.finish_loop, Ev::BaseBorn));

        // Ticks de regen a cada 176 loops após finish, enquanto base viva
        // e antes de `until`.
        let base_end = base.death_loop.min(until);
        let mut tick = base.finish_loop.saturating_add(LARVA_PERIOD_LOOPS);
        while tick < base_end {
            events.push((tick, Ev::Regen));
            tick = tick.saturating_add(LARVA_PERIOD_LOOPS);
        }

        // Morte de base dentro de `until` (atualiza cap).
        if base.death_loop != u32::MAX && base.death_loop < until {
            events.push((base.death_loop, Ev::BaseDied));
        }
    }

    for inj in injects {
        let land = inj.game_loop.saturating_add(INJECT_DELAY_LOOPS);
        if land < until {
            events.push((land, Ev::InjectLand));
        }
    }

    for &c in consumption_loops {
        if c <= until {
            events.push((c, Ev::Consume));
        }
    }

    // Ordenação por loop; em empate, BaseBorn/InjectLand primeiro (abrem
    // espaço), depois Regen, Consume, BaseDied (última pra não
    // interferir com eventos no mesmo loop).
    events.sort_by_key(|(loop_, ev)| {
        let order = match ev {
            Ev::BaseBorn => 0,
            Ev::InjectLand => 1,
            Ev::Regen => 2,
            Ev::Consume => 3,
            Ev::BaseDied => 4,
        };
        (*loop_, order)
    });

    let mut pool: u32 = 0;
    let mut total_generated: u32 = 0;
    let mut bases_alive: u32 = 0;

    for (_, ev) in events {
        let cap = bases_alive.saturating_mul(LARVA_NATURAL_CAP);
        match ev {
            Ev::BaseBorn => {
                bases_alive += 1;
                // 3 larvas iniciais (contam como geradas).
                pool += LARVA_NATURAL_CAP;
                total_generated += LARVA_NATURAL_CAP;
            }
            Ev::BaseDied => {
                bases_alive = bases_alive.saturating_sub(1);
                // Larvas já geradas permanecem contadas; apenas o cap
                // futuro diminui.
            }
            Ev::Regen => {
                if pool < cap {
                    pool += 1;
                    total_generated += 1;
                }
                // else: overflow descartado (correção do Bug 1).
            }
            Ev::InjectLand => {
                pool = pool.saturating_add(INJECT_LARVAE);
                total_generated = total_generated.saturating_add(INJECT_LARVAE);
            }
            Ev::Consume => {
                pool = pool.saturating_sub(1);
            }
        }
    }

    total_generated
}

/// Decomposição completa do cálculo de potencial de drones pro Zerg.
/// Usado pelo teste `#[ignore]` de dump pra tunarmos o ratio com base
/// em números reais dos replays de referência.
#[derive(Debug)]
struct ZergDiagnostic {
    pub bases_alive: u32,
    pub natural_per_base: Vec<u32>,
    pub pool_natural_total: u32,
    pub injects_landed: u32,
    pub pool_inject_total: u32,
    /// Pool calculado pelo método atual (não respeita cap-3 — toda tick
    /// conta). Útil pra comparar contra o `pool_total_realistic`.
    pub pool_total_optimistic: u32,
    /// Pool calculado respeitando cap-3 + consumo real.
    pub pool_total_realistic: u32,
    pub overlords: u32,
    pub zergling_individuals: u32,
    pub zergling_pairs: u32,
    pub other_army_larva: u32,
    pub nondrone_total: u32,
    pub drones_produced: u32,
    pub drones_started: u32,
    pub produced_workers: u32,
}

impl ZergDiagnostic {
    /// Potencial aplicando um ratio fixo sobre o pool realista, somado
    /// dos 12 iniciais. Igual ao modelo da fase 2 do plano.
    pub fn potential_at(&self, ratio: f64) -> u32 {
        let drones = (self.pool_total_realistic as f64 * ratio).floor() as u32;
        INITIAL_WORKERS.saturating_add(drones)
    }

    /// Alternativa: não usa ratio, subtrai consumo não-drone do pool
    /// realista diretamente. Retorna o teto caso o jogador tivesse
    /// convertido todo o restante do pool em drones.
    pub fn potential_subtract_actual(&self) -> u32 {
        let available = self.pool_total_realistic.saturating_sub(self.nondrone_total);
        INITIAL_WORKERS.saturating_add(available)
    }
}

fn diagnostic_zerg(
    timeline: &ReplayTimeline,
    player_idx: usize,
    until_loop: u32,
) -> ZergDiagnostic {
    let player = &timeline.players[player_idx];
    let until = until_loop.min(timeline.game_loops);

    let bases = collect_bases(timeline, player_idx, until);
    let produced_workers = count_produced_workers(timeline, player_idx, until);

    let mut natural_per_base: Vec<u32> = Vec::new();
    let mut pool_natural_total: u32 = 0;
    let mut bases_alive: u32 = 0;
    for base in &bases {
        if base.finish_loop >= until {
            natural_per_base.push(0);
            continue;
        }
        if base.death_loop == u32::MAX || base.death_loop > until {
            bases_alive += 1;
        }
        let end = base.death_loop.min(until);
        let window = end.saturating_sub(base.finish_loop);
        let ticks = window / LARVA_PERIOD_LOOPS;
        let per_base = LARVA_NATURAL_CAP + ticks;
        natural_per_base.push(per_base);
        pool_natural_total = pool_natural_total.saturating_add(per_base);
    }

    let mut injects_landed: u32 = 0;
    let mut pool_inject_total: u32 = 0;
    for inj in &player.inject_cmds {
        let land = inj.game_loop.saturating_add(INJECT_DELAY_LOOPS);
        if land < until {
            injects_landed += 1;
            pool_inject_total = pool_inject_total.saturating_add(INJECT_LARVAE);
        }
    }

    let pool_total_optimistic = pool_natural_total.saturating_add(pool_inject_total);

    let (consumption_loops, breakdown) = collect_larva_consumptions(player, until);
    let pool_total_realistic =
        simulate_pool_realistic(&bases, &player.inject_cmds, &consumption_loops, until);

    let nondrone_total = breakdown.nondrone_larva_count();

    // produced_workers já conta Drone ProductionFinished. Também queremos
    // saber quantos Drone Starteds houveram (consumo de larva real) pra
    // comparar com o count de finisheds (diferença = morfados em
    // estruturas ou drones em flight no `until`).
    let drones_produced = produced_workers.saturating_sub(INITIAL_WORKERS);
    let drones_started = breakdown.drones_started;

    ZergDiagnostic {
        bases_alive,
        natural_per_base,
        pool_natural_total,
        injects_landed,
        pool_inject_total,
        pool_total_optimistic,
        pool_total_realistic,
        overlords: breakdown.overlords,
        zergling_individuals: breakdown.zergling_individuals,
        zergling_pairs: breakdown.zergling_pairs,
        other_army_larva: breakdown.other_army_larva,
        nondrone_total,
        drones_produced,
        drones_started,
        produced_workers,
    }
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

    // ── Testes unitários do pool realista ────────────────────────

    fn mk_base(finish: u32) -> BaseInterval {
        BaseInterval {
            finish_loop: finish,
            death_loop: u32::MAX,
            fly_periods: Vec::new(),
        }
    }

    fn mk_inject(loop_: u32) -> InjectCmd {
        InjectCmd {
            game_loop: loop_,
            target_tag_index: 0,
            target_type: "Hatchery".to_string(),
            target_x: 0,
            target_y: 0,
        }
    }

    #[test]
    fn pool_respects_cap3_with_no_consumption() {
        // 1 hatch viva sem consumo algum: pool fica no cap de 3, não
        // acumula por tick. Regressão do Bug 1 (pool nunca decrescia).
        let bases = vec![mk_base(0)];
        let pool = simulate_pool_realistic(&bases, &[], &[], 10_000);
        assert_eq!(pool, 3, "pool sem consumo deve ficar em cap=3, got {}", pool);
    }

    #[test]
    fn pool_full_regen_when_consumption_matches_rate() {
        // Consumo a cada 176 loops (taxa = regen rate). Pool deve gerar
        // quase todas as ticks (3 iniciais + N ticks).
        let bases = vec![mk_base(0)];
        let until = 1760_u32; // 10 regen intervals
        // Consome logo após cada regen (offset de 1 loop).
        let consumption: Vec<u32> = (177..until).step_by(176).collect();
        let pool = simulate_pool_realistic(&bases, &[], &consumption, until);
        // 3 initial + 9 regen ticks (176..=1584) = 12
        assert!(
            pool >= 9 && pool <= 12,
            "pool com consumo perfeito deve ≈ initial+regens (9-12), got {}",
            pool
        );
    }

    #[test]
    fn pool_inject_bypasses_cap() {
        // 1 hatch sem consumo, 1 inject. Pool = 3 natural + 3 inject = 6.
        // Ticks de regen depois do inject land (464) caem no cap (pool=6,
        // cap=3, bloqueado), então só as larvas do inject somam.
        let bases = vec![mk_base(0)];
        let injects = vec![mk_inject(0)];
        let pool = simulate_pool_realistic(&bases, &injects, &[], 1_000);
        assert_eq!(pool, 6, "3 initial + 3 inject = 6, got {}", pool);
    }

    #[test]
    fn pool_multi_base_shares_cap() {
        // 2 hatches, sem consumo. Pool compartilhado cap = 3 × 2 = 6.
        // Cada hatch tem 3 iniciais = 6 total. Regens bloqueados.
        let bases = vec![mk_base(0), mk_base(1000)];
        let pool = simulate_pool_realistic(&bases, &[], &[], 5_000);
        assert_eq!(pool, 6, "2 bases * 3 iniciais com cap compartilhado = 6, got {}", pool);
    }

    #[test]
    fn serral_potential_at_6min_achievable() {
        // Regressão: no replay do Serral (top Zerg mundial), o gap no
        // minuto 6 deve ser pequeno (≤ 10 drones). Com o bug antigo o gap
        // era ~34 (potencial de 120 vs real 86).
        let tl = load_replay("serral.SC2Replay");
        let until = (6.0 * 60.0 * tl.loops_per_second).round() as u32;
        let mut checked = false;
        for i in 0..tl.players.len() {
            if tl.players[i].race != "Zerg" {
                continue;
            }
            let wp = compute_worker_potential(&tl, i, until, 0);
            let gap = wp.potential.saturating_sub(wp.produced);
            assert!(
                gap <= 10,
                "Serral gap at 6min é {} (potential {} vs produced {}), esperado ≤ 10",
                gap,
                wp.potential,
                wp.produced
            );
            assert!(wp.potential >= wp.produced, "invariante: potential >= produced");
            checked = true;
        }
        assert!(checked, "Serral replay deve ter um jogador Zerg");
    }

    #[test]
    fn zerg_invariant_potential_ge_produced_via_floor() {
        // Invariante é garantido via `potential.max(produced)` floor.
        for name in [
            "replay_sefi1.SC2Replay",
            "replay_sefi2.SC2Replay",
            "replay_sefi3.SC2Replay",
            "serral.SC2Replay",
            "firebat_vs_ai.SC2Replay",
        ] {
            let tl = load_replay(name);
            for minute in [3, 4, 5, 6, 8, 10] {
                let until = ((minute as f64) * 60.0 * tl.loops_per_second).round() as u32;
                for i in 0..tl.players.len() {
                    let wp = compute_worker_potential(&tl, i, until, 0);
                    assert!(
                        wp.potential >= wp.produced,
                        "{} p{} min{}: potential {} < produced {}",
                        name,
                        i,
                        minute,
                        wp.potential,
                        wp.produced
                    );
                }
            }
        }
    }

    /// Dump de diagnóstico pros replays de referência (Serral + firebat).
    /// Roda com:
    ///
    /// ```sh
    /// cargo test --release --lib \
    ///   worker_potential::tests::dump_zerg_worker_potential_diagnostic \
    ///   -- --ignored --nocapture
    /// ```
    ///
    /// Output orienta a escolha do `DRONE_LARVA_RATIO` e valida se o
    /// bug do pool (cap-3 ignorado) infla os números em cenários reais.
    #[test]
    #[ignore = "diagnostic — run with --ignored when tuning zerg ratio"]
    fn dump_zerg_worker_potential_diagnostic() {
        for name in ["serral.SC2Replay", "firebat_vs_ai.SC2Replay"] {
            let tl = load_replay(name);
            let lps = tl.loops_per_second;
            println!("\n============================================================");
            println!("REPLAY: {} (loops_per_second = {})", name, lps);
            println!("============================================================");
            for minute in [4, 5, 6, 8] {
                let until = ((minute as f64) * 60.0 * lps).round() as u32;
                for (i, player) in tl.players.iter().enumerate() {
                    if player.race != "Zerg" {
                        continue;
                    }
                    let d = diagnostic_zerg(&tl, i, until);
                    let current = compute_worker_potential(&tl, i, until, 0);
                    println!(
                        "\n--- p{} {} ({}) | min {} ({} loops) ---",
                        i, player.name, player.race, minute, until
                    );
                    println!("  Bases alive at until       : {}", d.bases_alive);
                    println!("  Natural larvae per-base    : {:?}", d.natural_per_base);
                    println!("  Natural pool total         : {}", d.pool_natural_total);
                    println!(
                        "  Injects landing in window  : {} ({} larvae)",
                        d.injects_landed, d.pool_inject_total
                    );
                    println!(
                        "  Pool TOTAL (optimista,atual): {}",
                        d.pool_total_optimistic
                    );
                    println!(
                        "  Pool TOTAL (realista,cap3)  : {}  <-- bug-fix",
                        d.pool_total_realistic
                    );
                    println!("  Non-drone larva breakdown:");
                    println!("    Overlords                : {}", d.overlords);
                    println!(
                        "    Zergling pairs           : {} (from {} individuals)",
                        d.zergling_pairs, d.zergling_individuals
                    );
                    println!("    Other larva-born army    : {}", d.other_army_larva);
                    println!("    Non-drone total          : {}", d.nondrone_total);
                    println!(
                        "  Drones started (consumed)   : {}",
                        d.drones_started
                    );
                    println!(
                        "  Drones produced (finished)  : {} (excludes 12 initial)",
                        d.drones_produced
                    );
                    println!(
                        "  Produced workers (+ initial): {}",
                        d.produced_workers
                    );
                    println!(
                        "  Potential ATUAL (80/20 bug) : {} (gap vs produced = {})",
                        current.potential,
                        current.potential as i32 - current.produced as i32
                    );
                    for &r in &[0.80_f64, 0.75, 0.70, 0.65, 0.60] {
                        let p = d.potential_at(r);
                        println!(
                            "  Potential @ {:>4.0}% ratio     : {} (gap vs produced = {})",
                            r * 100.0,
                            p,
                            p as i32 - d.produced_workers as i32
                        );
                    }
                    let psub = d.potential_subtract_actual();
                    println!(
                        "  Potential (subtract actual) : {} (gap vs produced = {})",
                        psub,
                        psub as i32 - d.produced_workers as i32
                    );
                }
            }
        }
    }
}
