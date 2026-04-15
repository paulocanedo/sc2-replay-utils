// Pós-processamento do parser: ordena timelines fora de ordem e
// deriva todos os índices (`alive_count`, `creep_index`,
// `worker_capacity`, `army_capacity`, `worker_births`,
// `upgrade_cumulative`) a partir dos streams canônicos
// (`entity_events`, `upgrades`). Todas as derivações vivem aqui — o
// tracker não preenche índices em paralelo.

use std::collections::{HashMap, HashSet};

use super::classify::{
    is_armor_upgrade, is_army_producer, is_attack_upgrade, is_creep_tumor_name, is_worker_producer,
    upgrade_level,
};
use super::types::{CreepEntry, CreepKind, EntityEventKind, PlayerTimeline, StatsSnapshot};

// ── Constantes de morph ─────────────────────────────────────────────
//
// Tempos em game loops (speed Faster) usados para backfillar a saída
// do produtor de workers durante morphs in-place (CC → Orbital/PF).
// Vivem aqui porque a derivação de capacidades acontece em `finalize`;
// o tracker emite apenas os `EntityEvent`s crus.

/// Tempo de morph CC → Orbital Command em game loops (~25s em Faster).
const ORBITAL_MORPH_TIME: u32 = 560;

/// Tempo de morph CC → Planetary Fortress em game loops (~36s em Faster).
const PF_MORPH_TIME: u32 = 806;

fn morph_build_time(unit_type: &str) -> u32 {
    match unit_type {
        "OrbitalCommand" => ORBITAL_MORPH_TIME,
        "PlanetaryFortress" => PF_MORPH_TIME,
        _ => 0,
    }
}

/// Valores conhecidos do estado inicial de partida ladder para cada
/// raça: (supply_used, supply_made, workers, minerals).
///
/// Terran/Protoss começam com 1 townhall (15 supply cap) + 12 workers.
/// Zerg começa com 1 Hatchery (6) + 1 Overlord (8) = 14 cap, + 12
/// drones (o Overlord não consome supply). Todos começam com 50
/// minerals e 0 gas. Campaign / custom modes podem divergir, mas para
/// replays de ladder esses números são exatos.
fn initial_stats_for_race(race: &str) -> (i32, i32, i32, i32) {
    match race {
        "Zerg" => (12, 14, 12, 50),
        "Terran" | "Protoss" => (12, 15, 12, 50),
        // Raça desconhecida: cai nos defaults de Terran/Protoss que
        // cobrem 2/3 dos casos. Melhor mostrar algo plausível do que
        // deixar a timeline em branco.
        _ => (12, 15, 12, 50),
    }
}

/// Prepende um snapshot sintético em `game_loop = 0` se o primeiro
/// evento `PlayerStats` real vier depois (o que é sempre o caso —
/// PlayerStats tipicamente só começa a disparar ~loop 160). Isso
/// evita que a timeline/charts fiquem em branco no instante inicial.
fn prepend_initial_stats_snapshot(player: &mut PlayerTimeline) {
    let already_at_zero = player.stats.first().is_some_and(|s| s.game_loop == 0);
    if already_at_zero {
        return;
    }
    let (supply_used, supply_made, workers, minerals) = initial_stats_for_race(&player.race);
    player.stats.insert(
        0,
        StatsSnapshot {
            game_loop: 0,
            minerals,
            vespene: 0,
            minerals_rate: 0,
            vespene_rate: 0,
            workers,
            supply_used,
            supply_made,
            army_value_minerals: 0,
            army_value_vespene: 0,
            minerals_lost_army: 0,
            vespene_lost_army: 0,
            minerals_killed_army: 0,
            vespene_killed_army: 0,
        },
    );
}

pub(super) fn finalize_indices(players: &mut [PlayerTimeline]) {
    for player in players.iter_mut() {
        prepend_initial_stats_snapshot(player);
        // Eventos podem ter sido emitidos fora de ordem por morphs
        // (apply_type_change empilha múltiplos no mesmo loop). A
        // ordenação é estável, então a ordem relativa entre eventos
        // do mesmo loop é preservada.
        player.entity_events.sort_by_key(|e| e.game_loop);
        // `unit_positions` chega na ordem natural do tracker, então
        // já está ordenado — mas um sort estável defensivo garante
        // o invariante esperado pelos consumers (`last_known_positions`).
        player.unit_positions.sort_by_key(|s| s.game_loop);
        player.camera_positions.sort_by_key(|c| c.game_loop);

        // alive_count: ProductionFinished ++; Died --; ignora
        // Started/Cancelled.
        let mut counts: HashMap<String, i32> = HashMap::new();
        for ev in &player.entity_events {
            match ev.kind {
                EntityEventKind::ProductionFinished => {
                    let c = counts.entry(ev.entity_type.clone()).or_insert(0);
                    *c += 1;
                    let v = player
                        .alive_count
                        .entry(ev.entity_type.clone())
                        .or_default();
                    v.push((ev.game_loop, *c));
                }
                EntityEventKind::Died => {
                    let c = counts.entry(ev.entity_type.clone()).or_insert(0);
                    *c -= 1;
                    let v = player
                        .alive_count
                        .entry(ev.entity_type.clone())
                        .or_default();
                    v.push((ev.game_loop, *c));
                }
                _ => {}
            }
        }

        derive_capacity_indices(player);
        derive_upgrade_cumulative(player);
        build_creep_index(player);
    }
}

/// Detecta se `entity_type` é uma fonte de creep (townhall ou tumor) e
/// retorna seu `CreepKind`. Usado por `build_creep_index` para filtrar
/// `entity_events` em uma única passada.
fn classify_creep(name: &str) -> Option<CreepKind> {
    match name {
        "Hatchery" | "Lair" | "Hive" => Some(CreepKind::Townhall),
        n if is_creep_tumor_name(n) => Some(CreepKind::Tumor),
        _ => None,
    }
}

/// Materializa `creep_index` a partir de `entity_events` para render
/// O(log n) na aba Timeline. Identidade é por `tag` único — morphs
/// in-place (Hatchery→Lair→Hive) reusam o mesmo tag, então geram uma
/// única entry no índice. Isso impede que a mancha de creep "pisque"
/// no instante do morph (quando `apply_type_change` emite um `Died`
/// sintético do tipo antigo no mesmo loop do `Started` do novo).
fn build_creep_index(player: &mut PlayerTimeline) {
    // Set de (loop, tag) onde houve algum `ProductionStarted`. Usado
    // pra distinguir o `Died` sintético de morph (tem Started companheiro
    // no mesmo loop+tag) de uma morte real (não tem).
    let mut starts_at_loop: HashSet<(u32, i64)> = HashSet::new();
    for ev in &player.entity_events {
        if matches!(ev.kind, EntityEventKind::ProductionStarted)
            && classify_creep(&ev.entity_type).is_some()
        {
            starts_at_loop.insert((ev.game_loop, ev.tag));
        }
    }

    let mut by_tag: HashMap<i64, usize> = HashMap::new();
    for ev in &player.entity_events {
        let kind = match classify_creep(&ev.entity_type) {
            Some(k) => k,
            None => continue,
        };
        match ev.kind {
            EntityEventKind::ProductionFinished => {
                if by_tag.contains_key(&ev.tag) {
                    // Morph in-place — entry já existe. Não duplicar.
                    continue;
                }
                let idx = player.creep_index.len();
                player.creep_index.push(CreepEntry {
                    tag: ev.tag,
                    x: ev.pos_x,
                    y: ev.pos_y,
                    born_loop: ev.game_loop,
                    died_loop: u32::MAX,
                    kind,
                });
                by_tag.insert(ev.tag, idx);
            }
            EntityEventKind::Died | EntityEventKind::ProductionCancelled => {
                // Filtra Died sintético de morph (Hatchery→Lair etc.):
                // se há Started no mesmo loop+tag, é morph. Ignora.
                if starts_at_loop.contains(&(ev.game_loop, ev.tag)) {
                    continue;
                }
                if let Some(&idx) = by_tag.get(&ev.tag) {
                    player.creep_index[idx].died_loop = ev.game_loop;
                }
            }
            EntityEventKind::ProductionStarted => {}
        }
    }

    // Inserções acima saem em ordem de `game_loop` (entity_events já
    // está ordenado pelo sort em `finalize_indices`), então nenhum
    // sort extra é necessário aqui.
}

/// Deriva `worker_capacity`, `army_capacity` e `worker_births` a partir
/// de `entity_events` (stream canônico). Esta função é a única fonte
/// dessas listas — o tracker não as popula em paralelo.
///
/// Regras:
/// - `ProductionFinished` de producer de worker (CC/Orbital/PF/Nexus) →
///   `worker_capacity += 1`.
/// - `Died` de producer de worker → `worker_capacity -= 1`.
/// - Idem para army producers (Barracks/Factory/Starport/Gateway/
///   WarpGate/RoboticsFacility/Stargate) em `army_capacity`.
/// - `ProductionFinished` de SCV/Probe cujo `ProductionStarted`
///   companheiro (mesmo loop, mesmo tag) tenha `creator_ability` com
///   "Train" → `worker_births.push(loop)`.
///
/// Morphs in-place (CC → Orbital/PF, Hatchery → Lair, etc.) aparecem em
/// `entity_events` como uma tripla consecutiva
/// `Died(A) → Started(B) → Finished(B)` no mesmo `game_loop` e mesmo
/// `tag`. Quando ambos os tipos são producers, o `Died` do antigo e o
/// `Finished` do novo se cancelam no net (−1 + +1 = 0). Porém, para
/// CC→Orbital e CC→PF queremos backfillar a saída do CC pelo tempo do
/// morph (o produtor fica offline durante a transformação): emitimos
/// `(morph_start, -1)` onde `morph_start = finish_loop - morph_build_time`
/// e `(finish_loop, +1)`.
fn derive_capacity_indices(player: &mut PlayerTimeline) {
    let events = &player.entity_events;
    // Pares (game_loop, tag) onde há `ProductionStarted` — usado para
    // identificar morphs in-place e descartar o `Died` companheiro
    // (já contabilizado via Started + Finished).
    let mut morph_started: HashSet<(u32, i64)> = HashSet::new();
    for ev in events {
        if matches!(ev.kind, EntityEventKind::ProductionStarted) {
            morph_started.insert((ev.game_loop, ev.tag));
        }
    }

    // `creator_ability` do Started companheiro (mesmo loop+tag) — usado
    // para decidir se um `ProductionFinished` de worker conta como
    // "nascido via Train" (entra em `worker_births`).
    let mut started_abilities: HashMap<(u32, i64), Option<String>> = HashMap::new();
    for ev in events {
        if matches!(ev.kind, EntityEventKind::ProductionStarted) {
            started_abilities.insert((ev.game_loop, ev.tag), ev.creator_ability.clone());
        }
    }

    for (i, ev) in events.iter().enumerate() {
        match ev.kind {
            EntityEventKind::Died => {
                // Morph: `Died` seguido de `Started` no mesmo loop+tag.
                // A capacidade do tipo antigo sai via backfill abaixo
                // (no ramo `ProductionStarted`), nunca aqui.
                if morph_started.contains(&(ev.game_loop, ev.tag)) {
                    continue;
                }
                if is_worker_producer(&ev.entity_type) {
                    player.worker_capacity.push((ev.game_loop, -1));
                }
                if is_army_producer(&ev.entity_type) {
                    player.army_capacity.push((ev.game_loop, -1));
                }
            }
            EntityEventKind::ProductionStarted => {
                // Detecta morph in-place: Died do mesmo tag/loop está
                // imediatamente antes (sort estável preserva a ordem
                // Died → Started → Finished do `apply_type_change`).
                let old_type = if i > 0 {
                    let prev = &events[i - 1];
                    if matches!(prev.kind, EntityEventKind::Died)
                        && prev.tag == ev.tag
                        && prev.game_loop == ev.game_loop
                    {
                        Some(prev.entity_type.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                };
                let Some(old_type) = old_type else { continue };

                let new_type = ev.entity_type.as_str();
                let old_w = is_worker_producer(old_type);
                let new_w = is_worker_producer(new_type);
                match (old_w, new_w) {
                    (true, true) => {
                        let mt = morph_build_time(new_type);
                        if mt > 0 {
                            let morph_start = ev.game_loop.saturating_sub(mt);
                            player.worker_capacity.push((morph_start, -1));
                            player.worker_capacity.push((ev.game_loop, 1));
                        }
                    }
                    (true, false) => {
                        player.worker_capacity.push((ev.game_loop, -1));
                    }
                    (false, true) => {
                        player.worker_capacity.push((ev.game_loop, 1));
                    }
                    (false, false) => {}
                }

                let old_a = is_army_producer(old_type);
                let new_a = is_army_producer(new_type);
                match (old_a, new_a) {
                    (true, true) => {
                        let mt = morph_build_time(new_type);
                        if mt > 0 {
                            let morph_start = ev.game_loop.saturating_sub(mt);
                            player.army_capacity.push((morph_start, -1));
                            player.army_capacity.push((ev.game_loop, 1));
                        }
                    }
                    (true, false) => {
                        player.army_capacity.push((ev.game_loop, -1));
                    }
                    (false, true) => {
                        player.army_capacity.push((ev.game_loop, 1));
                    }
                    (false, false) => {}
                }
            }
            EntityEventKind::ProductionFinished => {
                // Morph completion: o par `(Died antigo, Started novo)`
                // no mesmo loop+tag já emitiu a capacidade via ramo
                // `Started` acima. Evita contagem dupla.
                let is_morph_finish = i > 0
                    && matches!(events[i - 1].kind, EntityEventKind::ProductionStarted)
                    && events[i - 1].tag == ev.tag
                    && events[i - 1].game_loop == ev.game_loop
                    && i >= 2
                    && matches!(events[i - 2].kind, EntityEventKind::Died)
                    && events[i - 2].tag == ev.tag
                    && events[i - 2].game_loop == ev.game_loop;
                if !is_morph_finish {
                    if is_worker_producer(&ev.entity_type) {
                        player.worker_capacity.push((ev.game_loop, 1));
                    }
                    if is_army_producer(&ev.entity_type) {
                        player.army_capacity.push((ev.game_loop, 1));
                    }
                }

                // worker_births: SCV/Probe nascidos via Train*. O
                // `creator_ability` fica no Started companheiro (mesmo
                // loop+tag), não no Finished.
                if matches!(ev.entity_type.as_str(), "SCV" | "Probe") {
                    if let Some(Some(ability)) = started_abilities.get(&(ev.game_loop, ev.tag)) {
                        if ability.contains("Train") {
                            player.worker_births.push(ev.game_loop);
                        }
                    }
                }
            }
            EntityEventKind::ProductionCancelled => {}
        }
    }

    // As listas são construídas em ordem crescente de `game_loop`
    // porque `entity_events` já está ordenado. Exceção: o backfill de
    // morph empurra `(morph_start, -1)` antes de `(finish_loop, +1)`,
    // mas `morph_start < finish_loop` também respeita a ordem — exceto
    // quando outro evento entre `morph_start` e `finish_loop` já foi
    // emitido. Um sort estável defensivo garante o invariante.
    player.worker_capacity.sort_by_key(|(l, _)| *l);
    player.army_capacity.sort_by_key(|(l, _)| *l);
    // `worker_births` sai estritamente crescente (só vem de Finished
    // em ordem de loop) — nenhum sort necessário.
}

/// Deriva `upgrade_cumulative` a partir do stream canônico `upgrades`.
/// Cada entrada é `(game_loop, attack_level_apos, armor_level_apos)`
/// com os níveis cumulativos monotônicos (nunca diminuem).
fn derive_upgrade_cumulative(player: &mut PlayerTimeline) {
    let mut cur_attack: u8 = 0;
    let mut cur_armor: u8 = 0;
    for up in &player.upgrades {
        let level = upgrade_level(&up.name);
        if is_attack_upgrade(&up.name) && level > 0 {
            cur_attack = cur_attack.max(level);
        }
        if is_armor_upgrade(&up.name) && level > 0 {
            cur_armor = cur_armor.max(level);
        }
        player
            .upgrade_cumulative
            .push((up.game_loop, cur_attack, cur_armor));
    }
}
