// Pós-processamento do parser: ordena timelines fora de ordem e
// constrói o índice cumulativo `alive_count` por tipo de entidade.

use std::collections::{HashMap, HashSet};

use super::classify::is_creep_tumor_name;
use super::types::{CreepEntry, CreepKind, EntityEventKind, PlayerTimeline, StatsSnapshot};

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
        player.worker_capacity.sort_by_key(|(l, _)| *l);
        player.worker_births.sort_unstable();
        player.army_capacity.sort_by_key(|(l, _)| *l);
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

    // entity_events já é ordenado por game_loop, então as inserções
    // acima saem ordenadas. Sort defensivo para garantir o invariante
    // que o consumer (`partition_point` em `draw_creep`) depende.
    player.creep_index.sort_by_key(|c| c.born_loop);
}
