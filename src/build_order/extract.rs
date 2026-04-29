//! Extração do build order a partir de um `ReplayTimeline` parseado.
//!
//! O trabalho pesado vive em `build_player_entries`, que funde três
//! streams canônicos do parser (`entity_events`, `upgrades`,
//! `inject_cmds`) em `Vec<BuildOrderEntry>` por jogador, aplicando
//! cmd matching (caminho preferido) ou fallback de build_time (legado)
//! para descobrir o instante real de início de cada ação.

use std::collections::HashMap;

use s2protocol::tracker_events::unit_tag_index;

use crate::balance_data::build_time_loops;
use crate::replay::{
    EntityCategory, EntityEventKind, PlayerTimeline, ProductionCmd, ReplayTimeline,
    UNIT_INIT_MARKER,
};

use super::types::{BuildOrderEntry, BuildOrderResult, EntryOutcome, PlayerBuildOrder};

/// Constrói o `BuildOrderResult` a partir de um `ReplayTimeline` já
/// parseado. Chama O(eventos), sem I/O.
pub fn extract_build_order(timeline: &ReplayTimeline) -> Result<BuildOrderResult, String> {
    let base_build = timeline.base_build;
    let players = timeline
        .players
        .iter()
        .map(|p| PlayerBuildOrder {
            name: p.name.clone(),
            race: p.race.clone(),
            mmr: p.mmr,
            entries: build_player_entries(p, base_build),
        })
        .collect();

    Ok(BuildOrderResult {
        players,
        datetime: timeline.datetime.clone(),
        map_name: timeline.map.clone(),
        loops_per_second: timeline.loops_per_second,
    })
}

fn build_player_entries(player: &PlayerTimeline, base_build: u32) -> Vec<BuildOrderEntry> {
    let mut raw: Vec<BuildOrderEntry> = Vec::new();

    // Index tag → (loop do cancel/destroy, killer). Só `ProductionCancelled`
    // — o parser emite essa variante quando `UnitDied` chega enquanto a
    // tag ainda está `Lifecycle::InProgress` (tracker.rs:367). Entries
    // cujo tag aparece aqui não chegaram a completar.
    //
    // `Died` (com lifecycle=Finished) significa que o prédio completou e
    // morreu depois — não afeta o outcome do build order, então é
    // ignorado deliberadamente.
    let mut cancel_by_tag: HashMap<i64, (u32, Option<u8>)> = HashMap::new();
    // Index tag → entity_type. Construído a partir de qualquer evento
    // do tracker (ProductionStarted/Finished) que reveste a tag com seu
    // tipo. Usado para resolver `creator_tag` em "Barracks", "Larva",
    // "Forge" etc. ao montar o `producer_type` da entry.
    let mut tag_to_type: HashMap<i64, String> = HashMap::new();
    // ID sequencial 1-based por tipo, atribuído na primeira vez que
    // cada `tag` aparece. Permite distinguir "Barracks #1" de
    // "Barracks #2" no display do produtor. A iteração é feita na
    // mesma ordem cronológica de `entity_events` (tracker.rs já
    // garante essa ordem), então a numeração reflete a ordem em que
    // o jogador construiu/recebeu os produtores.
    let mut tag_to_producer_id: HashMap<i64, u32> = HashMap::new();
    let mut next_id_per_type: HashMap<String, u32> = HashMap::new();
    // Index → producer_id, usado por injects (que carregam só o
    // `target_tag_index`, sem recycle). Last-write-wins na ordem
    // cronológica — se o `unit_tag_index` for reciclado, a entrada
    // mais recente prevalece, que é o estado plausível no momento do
    // inject (que sempre vem depois da estrutura nascer).
    let mut index_to_producer_id: HashMap<u32, u32> = HashMap::new();
    for ev in &player.entity_events {
        if ev.kind == EntityEventKind::ProductionCancelled {
            cancel_by_tag.insert(ev.tag, (ev.game_loop, ev.killer_player_id));
        }
        if !tag_to_type.contains_key(&ev.tag) {
            tag_to_type.insert(ev.tag, ev.entity_type.clone());
            let counter = next_id_per_type
                .entry(ev.entity_type.clone())
                .or_insert(0);
            *counter += 1;
            tag_to_producer_id.insert(ev.tag, *counter);
            index_to_producer_id.insert(unit_tag_index(ev.tag), *counter);
        }
    }

    // Estado mutável compartilhado pelo cmd matching:
    // - `cmds_by_producer[producer_tag] -> Vec<cmd_idx>` em ordem
    //   de emissão (game.rs já empurra por game_loop crescente).
    //   Chave é o tag completo (`i64`) porque indexes são reciclados —
    //   ver comentário em `ProductionCmd::producer_tags`.
    // - `consumed[i]` marca que o cmd `i` já foi pareado a uma entrada,
    //   pra não ser reusado por outro evento.
    // - `prev_finish_by_producer` mantém o último `finish_loop`
    //   computado por produtor pra encadear `start = max(cmd, prev)`.
    let mut cmds_by_producer: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, cmd) in player.production_cmds.iter().enumerate() {
        if let Some(&p) = cmd.producer_tags.first() {
            cmds_by_producer.entry(p).or_default().push(i);
        }
    }
    let mut consumed = vec![false; player.production_cmds.len()];
    let mut prev_finish_by_producer: HashMap<i64, u32> = HashMap::new();
    // Último `start_loop` computado por produtor — usado para detectar
    // pares paralelos (Reactor): quando duas unidades têm
    // `finish_loop` PRÓXIMO no mesmo producer (gap ≤
    // PARALLEL_PAIR_TOLERANCE), são "siblings" do mesmo cmd (1 click,
    // Reactor emite 2 unidades simultâneas que o engine reporta com
    // 0-15 loops de offset). A segunda do par herda o `start` da
    // primeira em vez de tentar consumir um cmd separado e cair no
    // override `cmd.max(prev_finish=projected_finish)` — esse
    // override transformava a segunda Marine em entrada instantânea
    // (start = finish, ex.: "Marine às 8:48 → 8:48").
    let mut prev_start_by_producer: HashMap<i64, u32> = HashMap::new();
    // Tolerância em game loops para detectar par paralelo do Reactor
    // (mecânica exclusiva Terran — Reactor é o único caso onde uma
    // estrutura emite 2 unidades por 1 cmd no SC2). Pares paralelos
    // observados no replay Winter Madness LE têm gap de 0-37 loops
    // entre os dois Born events.
    //
    // Sequencial mínimo para qualquer unidade SC2:
    //   - Marine (Terran, build fixo): 380 loops.
    //   - Probe (Protoss) com chronoboost máximo: ~179 loops.
    //   - Drone (Zerg, larva-born): ~269 loops.
    //
    // 50 cobre o pior par paralelo observado com folga ~3.5× para o
    // sequencial mais curto teórico (Probe+chrono). Sem falso
    // positivo conhecido.
    const PARALLEL_PAIR_TOLERANCE: u32 = 50;

    // Entidades — só ProductionStarted, filtrado por origem da habilidade.
    for ev in &player.entity_events {
        if ev.kind != EntityEventKind::ProductionStarted {
            continue;
        }
        if ev.game_loop == 0 {
            continue;
        }
        let Some(ability) = ev.creator_ability.as_deref() else {
            // Sem ability associada → spawn inicial / coisa fora de
            // build order (CC inicial, larvas, etc.).
            continue;
        };

        let from_unit_init = ability == UNIT_INIT_MARKER;
        let from_train = ability.contains("Train");
        let from_morph = ability.starts_with("MorphTo");
        if !from_unit_init && !from_train && !from_morph {
            continue;
        }

        // is_structure: UnitInit sempre cria estrutura; morphs criam
        // estrutura quando o tipo destino é uma estrutura. Trains nunca
        // criam estrutura.
        let is_structure = from_unit_init
            || (from_morph && matches!(ev.category, EntityCategory::Structure));

        // Estruturas via UnitInit já vêm com o `game_loop` de início
        // (UnitInit é emitido quando o SCV/Probe começa a construir).
        // Os demais (trains, morphs) vêm com o loop de conclusão.
        let raw_loop = ev.game_loop;
        let projected_finish = if from_unit_init {
            add_build_time(raw_loop, &ev.entity_type, base_build)
        } else {
            raw_loop
        };

        // Tenta o caminho 1 (cmd matching) só pra trains/morphs com
        // creator_tag. UnitInit nunca tem produtor — vai direto pro
        // fallback de start = raw_loop.
        //
        // Restrição de causalidade: o cmd só vale se ocorreu cedo o
        // suficiente pra ter plausivelmente produzido a unidade. O
        // SC2 emite Born events para Probes/Drones iniciais com
        // `creator_unit_tag_*` apontando pro Nexus/Hatchery; sem
        // filtro, esses spawns "instantâneos" (finish ~loop 11)
        // canibalizariam os cmds reais que o jogador emitiu para
        // produzir as próximas Probes. Exigimos
        // `cmd_loop + build_time/2 <= finish_loop` — chrono boost
        // (1.5×) acelera no máximo pra ~0.67×build_time, então a
        // metade é uma margem segura.
        let max_cmd_loop = projected_finish
            .saturating_sub(build_time_loops(&ev.entity_type, base_build) / 2);

        // Detecção de par paralelo (Reactor) ANTES de tentar cmd
        // matching: se já houve uma unidade emitida pelo mesmo
        // `creator_tag` com `projected_finish` PRÓXIMO (gap ≤
        // PARALLEL_PAIR_TOLERANCE), esta é a "irmã" do mesmo cmd —
        // não consumimos cmd novo e herdamos o `start_loop`. Sem
        // isso, a segunda Marine cairia no
        // `cmd_loop.max(prev_finish=projected_finish)` e renderiza
        // como entrada instantânea (start = finish).
        let is_parallel_pair = !from_unit_init
            && ev.creator_tag
                .map(|t| {
                    prev_finish_by_producer
                        .get(&t)
                        .copied()
                        .map(|prev| {
                            prev > 0
                                && projected_finish.saturating_sub(prev)
                                    <= PARALLEL_PAIR_TOLERANCE
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(false);

        let cmd_match: Option<(i64, u32)> = if from_unit_init || is_parallel_pair {
            None
        } else {
            ev.creator_tag.and_then(|t| {
                consume_producer_cmd(
                    &cmds_by_producer,
                    &mut consumed,
                    &player.production_cmds,
                    t,
                    &ev.entity_type,
                    max_cmd_loop,
                )
                .map(|loop_| (t, loop_))
            })
        };

        let start_loop = if from_unit_init {
            raw_loop
        } else if is_parallel_pair {
            // Par paralelo: herda o `start_loop` da primeira do par.
            // `creator_tag` está garantidamente Some pelo `is_parallel_pair`.
            ev.creator_tag
                .and_then(|t| prev_start_by_producer.get(&t).copied())
                .unwrap_or_else(|| {
                    subtract_build_time(raw_loop, &ev.entity_type, base_build)
                })
        } else if let Some((producer_tag, cmd_loop)) = cmd_match {
            let prev = prev_finish_by_producer
                .get(&producer_tag)
                .copied()
                .unwrap_or(0);
            cmd_loop.max(prev)
        } else {
            // Fallback: mantém o cálculo legado para entradas sem
            // produtor identificável (warp-ins e ramos onde game.rs
            // não conseguiu resolver a seleção ou a versão de balance
            // data não conhece o `(producer, ability_id, cmd_index)`).
            subtract_build_time(raw_loop, &ev.entity_type, base_build)
        };

        // Encadeia o próximo cmd do mesmo produtor no `projected_finish`
        // observado — assim a próxima unidade da fila não pode começar
        // antes do término da atual, mesmo que o jogador tenha clicado
        // o train cedo (queue de cmds enquanto a anterior produz).
        if let Some((producer_tag, _)) = cmd_match {
            prev_finish_by_producer.insert(producer_tag, projected_finish);
            prev_start_by_producer.insert(producer_tag, start_loop);
        } else if is_parallel_pair {
            // Em par paralelo, a segunda da dupla tem `projected_finish`
            // alguns loops após a primeira (engine emite com pequeno
            // offset). Avança o chain para o finish da SEGUNDA — caso
            // contrário a próxima unidade sequencial encadearia do
            // finish da primeira, ficando 2-15 loops antes do real.
            if let Some(t) = ev.creator_tag {
                prev_finish_by_producer.insert(t, projected_finish);
            }
        }

        // Se essa tag aparece no cancel_by_tag, a produção não chegou
        // ao fim — o `finish_loop` real é o instante do cancel, e o
        // outcome vem do killer_player_id: mesmo player = cancel
        // intencional, outro player = destruído pelo inimigo.
        let (finish_loop, outcome) = match cancel_by_tag.get(&ev.tag).copied() {
            Some((cancel_loop, killer)) => {
                let outcome = match killer {
                    Some(kid) if kid != player.player_id => {
                        EntryOutcome::DestroyedInProgress
                    }
                    _ => EntryOutcome::Cancelled,
                };
                (cancel_loop, outcome)
            }
            None => (projected_finish, EntryOutcome::Completed),
        };

        // Supply (used + made) é amostrado no instante de início — é o
        // que o jogador tinha quando emitiu o comando.
        let (supply, supply_made) = supply_at(player, start_loop);

        // Chrono boost: só Protoss pode acelerar a própria produção. Para
        // Terran/Zerg a estimativa baseada em (expected − actual) gera
        // falsos positivos sempre que o cmd matching pareia um cmd com o
        // "slot" errado da fila do produtor (cmd emitido depois do
        // anterior nascer mas antes do atual — ele é o cmd do próximo
        // SCV/Marine, não desse). Como a heurística não tem como
        // distinguir, restringimos por raça.
        let chrono_boosts = if player.race == "Protoss"
            && cmd_match.is_some()
            && outcome == EntryOutcome::Completed
        {
            let expected_bt = build_time_loops(&ev.entity_type, base_build);
            let actual_bt = projected_finish.saturating_sub(start_loop);
            estimate_chrono_count(expected_bt, actual_bt)
        } else {
            0
        };

        // Producer: para Trains/Morphs (e par paralelo) o `creator_tag`
        // aponta diretamente pra estrutura/Larva produtora; UnitInit
        // (warp-in / construção via worker) e fallbacks ficam sem
        // producer rastreado — ver "trabalho futuro" no plano de design.
        let (producer_type, producer_id) = if from_unit_init {
            (None, None)
        } else {
            let t_ = ev.creator_tag.and_then(|t| tag_to_type.get(&t).cloned());
            let id = ev
                .creator_tag
                .and_then(|t| tag_to_producer_id.get(&t).copied());
            (t_, id)
        };

        raw.push(BuildOrderEntry {
            supply,
            supply_made,
            game_loop: start_loop,
            finish_loop,
            seq: ev.seq,
            action: ev.entity_type.clone(),
            count: 1,
            is_upgrade: false,
            is_structure,
            outcome,
            chrono_boosts,
            producer_type,
            producer_id,
        });
    }

    // Upgrades — Sprays já filtrados pelo parser. O `game_loop` cru é
    // de conclusão; recuamos para o início. Tentativa 1: matching
    // global por nome contra os production_cmds (pesquisas não
    // enfileiram, então não há chaining por produtor — basta achar o
    // primeiro cmd com `ability == upgrade.name`). Fallback: subtrai
    // build_time_loops como antes.
    for u in &player.upgrades {
        if u.game_loop == 0 {
            continue;
        }
        let finish_loop = u.game_loop;
        let expected_bt = build_time_loops(&u.name, base_build);
        let max_cmd = finish_loop.saturating_sub(expected_bt / 2);
        let cmd_match =
            consume_global_cmd(&mut consumed, &player.production_cmds, &u.name, max_cmd);
        let start_loop = cmd_match
            .map(|(loop_, _)| loop_)
            .unwrap_or_else(|| subtract_build_time(finish_loop, &u.name, base_build));
        let chrono_boosts = if player.race == "Protoss" && cmd_match.is_some() {
            let actual_bt = finish_loop.saturating_sub(start_loop);
            estimate_chrono_count(expected_bt, actual_bt)
        } else {
            0
        };
        // Producer da pesquisa: tag candidato no `producer_tags[0]` do
        // cmd consumido (estrutura selecionada no momento do click).
        // Resolvemos via `tag_to_type` para algo como "Forge", "EngBay".
        let producer_tag = cmd_match.and_then(|(_, tag)| tag);
        let producer_type = producer_tag.and_then(|t| tag_to_type.get(&t).cloned());
        let producer_id = producer_tag.and_then(|t| tag_to_producer_id.get(&t).copied());
        let (supply, supply_made) = supply_at(player, start_loop);
        raw.push(BuildOrderEntry {
            supply,
            supply_made,
            game_loop: start_loop,
            finish_loop,
            seq: u.seq,
            action: u.name.clone(),
            count: 1,
            is_upgrade: true,
            is_structure: false,
            // Upgrades não têm lifecycle cancelável via tag (o tracker
            // só emite o evento quando o research conclui).
            outcome: EntryOutcome::Completed,
            chrono_boosts,
            producer_type,
            producer_id,
        });
    }

    // Inject Larva — cada inject vira uma entrada própria indicando
    // a Hatchery/Lair/Hive alvo. A posição é codificada na action
    // string pra permitir distinguir bases diferentes na UI.
    for inject in &player.inject_cmds {
        let (supply, supply_made) = supply_at(player, inject.game_loop);
        // Resolve a Hatchery/Lair/Hive alvo pelo `target_tag_index` →
        // `producer_id`. Quando achamos o ID, codificamos no action
        // como `InjectLarva@Hatchery#N` (preferido, mais conciso e
        // estável que coordenadas). Caso o índice não esteja no mapa
        // (eventos faltando ou recycle ambíguo), caímos no formato
        // antigo com coordenadas para preservar a desambiguação.
        let target_id = index_to_producer_id.get(&inject.target_tag_index).copied();
        let action = match target_id {
            Some(id) => format!("InjectLarva@{}#{}", inject.target_type, id),
            None => format!(
                "InjectLarva@{}@{}_{}",
                inject.target_type, inject.target_x, inject.target_y
            ),
        };
        raw.push(BuildOrderEntry {
            supply,
            supply_made,
            game_loop: inject.game_loop,
            finish_loop: inject.game_loop, // ação instantânea
            seq: u32::MAX,
            action,
            count: 1,
            is_upgrade: false,
            is_structure: false,
            outcome: EntryOutcome::Completed,
            chrono_boosts: 0,
            // Queen produtora ainda não é capturada em inject_cmds —
            // trabalho futuro.
            producer_type: None,
            producer_id: None,
        });
    }

    // Sort por (game_loop, seq) — agora `game_loop` é o instante de
    // início, então a ordem cronológica é preservada na display.
    raw.sort_by_key(|e| (e.game_loop, e.seq));

    deduplicate(raw)
}

/// Procura o cmd não-consumido emitido pelo `producer_tag` cuja
/// `ability` bate com `action` E cujo `game_loop` satisfaz
/// `cmd_loop <= max_cmd_loop` (constraint de causalidade). Quando há
/// múltiplos candidatos, escolhe o **mais recente** (maior
/// `game_loop`) — é o que tem maior probabilidade de ter realmente
/// produzido a unidade concluída em `finish_loop`. Cmds "fantasma"
/// mais antigos (cliques cancelados, double-clicks, queue cheia)
/// ficam não-consumidos e não roubam o cmd da próxima unidade real,
/// evitando entries com `start_loop` muito anterior ao treino e o
/// sintoma de "Marine instantâneo" quando uma unidade real depois
/// fica sem cmd e cai no fallback `subtract_build_time`.
///
/// Iteramos a fila inteira (não só o front) porque um produtor pode
/// receber cmds de tipos diferentes intercalados (ex.: Stargate
/// alternando Phoenix/Voidray) e queremos a ocorrência mais recente
/// dentro da janela válida.
fn consume_producer_cmd(
    by_producer: &HashMap<i64, Vec<usize>>,
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    producer_tag: i64,
    action: &str,
    max_cmd_loop: u32,
) -> Option<u32> {
    let queue = by_producer.get(&producer_tag)?;
    let mut last_valid: Option<usize> = None;
    for &i in queue {
        if consumed[i] {
            continue;
        }
        if cmds[i].ability != action {
            continue;
        }
        if cmds[i].game_loop > max_cmd_loop {
            // A fila está ordenada por game_loop crescente — todos os
            // próximos seriam ainda mais tarde, então paramos aqui.
            break;
        }
        last_valid = Some(i);
    }
    if let Some(i) = last_valid {
        consumed[i] = true;
        return Some(cmds[i].game_loop);
    }
    None
}

/// Match global por nome de ação (sem filtrar por produtor). Usado
/// para upgrades, que são one-shot e não enfileiram — basta o primeiro
/// cmd disponível com a ability certa que respeite a mesma constraint
/// de causalidade `cmd_loop <= max_cmd_loop`. Retorna o `game_loop` do
/// cmd e o primeiro `producer_tag` candidato (a estrutura selecionada
/// no momento do click), quando disponível.
fn consume_global_cmd(
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    action: &str,
    max_cmd_loop: u32,
) -> Option<(u32, Option<i64>)> {
    for (i, cmd) in cmds.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        if cmd.ability != action {
            continue;
        }
        if cmd.game_loop > max_cmd_loop {
            continue;
        }
        consumed[i] = true;
        return Some((cmd.game_loop, cmd.producer_tags.first().copied()));
    }
    None
}

/// Estima quantos Chrono Boosts aceleraram uma produção comparando
/// o build time observado com o esperado. Só faz sentido quando
/// `start_loop` veio de cmd matching (tempo real).
///
/// Modelo simplificado do Chrono Boost LotV (4.0+):
/// - Duração: 20s game time = 320 game loops (Normal speed, ×16).
/// - Efeito: +50% produção durante a janela (1.5× speed).
/// - Economia por chrono em builds > 320 loops: ~160 loops.
/// - Economia em builds ≤ 320 loops: ~expected/3 loops (build
///   inteiro cabe numa janela).
fn estimate_chrono_count(expected_bt: u32, actual_bt: u32) -> u8 {
    if actual_bt >= expected_bt || expected_bt == 0 {
        return 0;
    }
    let time_saved = expected_bt - actual_bt;

    // Builds curtos (≤ 320 loops = 20s): cabe dentro de uma janela
    // de chrono. Máximo 1 chrono, economia ≈ expected/3.
    if expected_bt <= 320 {
        let threshold = expected_bt / 6; // metade da economia máxima
        return if time_saved > threshold { 1 } else { 0 };
    }

    // Builds longos: cada chrono economiza ~160 loops.
    const SAVE_PER_CHRONO: u32 = 160;
    let threshold = SAVE_PER_CHRONO / 2; // 80 loops mínimo
    if time_saved < threshold {
        return 0;
    }
    ((time_saved + SAVE_PER_CHRONO / 2) / SAVE_PER_CHRONO).min(10) as u8
}

/// Subtrai o `build_time_loops(action, base_build)` do `raw_loop`.
/// Quando o nome não consta no balance data (`delta == 0`) o loop
/// original é mantido — fallback seguro pra ações desconhecidas.
fn subtract_build_time(raw_loop: u32, action: &str, base_build: u32) -> u32 {
    let delta = build_time_loops(action, base_build);
    raw_loop.saturating_sub(delta)
}

/// Soma o `build_time_loops(action, base_build)` ao `raw_loop`. Usado
/// para estruturas vindas de `UnitInit`, cujo loop bruto é o início:
/// projetamos o tempo de conclusão somando o build time do balance
/// data.
fn add_build_time(raw_loop: u32, action: &str, base_build: u32) -> u32 {
    let delta = build_time_loops(action, base_build);
    raw_loop.saturating_add(delta)
}

/// Lê `(supply_used, supply_made)` no instante mais recente <= `loop_`.
/// Retorna `(0, 0)` se não houver nenhum snapshot prévio.
///
/// Os valores vêm como `i32` do tracker (já em unidades de supply, divididos
/// por 4096 pela própria s2protocol). Late-game Zerg/Protoss frequentemente
/// supera 200 em `food_made` (overlords/pylons além do cap), e morphs/transições
/// podem fazer `food_used` flutuar acima do cap; portanto preservamos o
/// número bruto em `u16` em vez de truncar para `u8` (que faria wrap mod 256
/// para qualquer valor ≥ 256). O `.max(0)` blinda contra valores negativos
/// inesperados — não devem ocorrer mas o tipo de origem é signed.
fn supply_at(player: &PlayerTimeline, loop_: u32) -> (u16, u16) {
    player
        .stats_at(loop_)
        .map(|s| (s.supply_used.max(0) as u16, s.supply_made.max(0) as u16))
        .unwrap_or((0, 0))
}

/// Funde entradas consecutivas com a mesma ação em uma única com `count`
/// incrementado. Só funde se o **outcome** também for igual — um
/// SupplyDepot cancelado seguido de um SupplyDepot que completou precisam
/// aparecer como linhas separadas para que o usuário veja a diferença.
///
/// Também só funde quando o **produtor** é o mesmo (mesmo `producer_id`):
/// duas Marines treinadas em Barracks diferentes no mesmo instante
/// devem aparecer como linhas separadas, caso contrário a coluna de
/// produtor mostra só uma das fontes e a outra se perde.
fn deduplicate(entries: Vec<BuildOrderEntry>) -> Vec<BuildOrderEntry> {
    let mut out: Vec<BuildOrderEntry> = Vec::new();
    for entry in entries {
        match out.last_mut() {
            Some(last)
                if last.action == entry.action
                    && last.outcome == entry.outcome
                    && last.producer_id == entry.producer_id
                    && !last.action.starts_with("InjectLarva") =>
            {
                last.count += 1;
                // Acumula chronos do grupo — o display mostra o total.
                last.chrono_boosts = last.chrono_boosts.saturating_add(entry.chrono_boosts);
            }
            _ => out.push(entry),
        }
    }
    out
}
