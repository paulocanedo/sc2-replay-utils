// Extrator de build order — camada pura sobre `ReplayTimeline`.
//
// Não abre o MPQ nem decodifica eventos: consome `entity_events`,
// `upgrades` e `production_cmds` que o parser single-pass já produziu,
// mapeando cada um para `BuildOrderEntry` na semântica esperada pelos
// consumers (CSV, GUI, image renderer).
//
// Cada entrada armazena o `game_loop` no instante de **início** da
// ação. Há dois caminhos para descobrir esse instante:
//
// 1. **Cmd matching** (preferido): se o evento tem `creator_tag` (i.e.,
//    veio de um Train/Morph com produtor identificado) e o parser de
//    game events capturou um `ProductionCmd` correspondente nesse mesmo
//    produtor, usamos `start = max(cmd_loop, finish_anterior_no_mesmo
//    _produtor)`. Isso absorve Chrono Boost (Protoss), supply block e
//    idle gaps gratuitamente — só usamos tempos observados (clique do
//    jogador + UnitBorn real). Para upgrades o match é global por
//    nome (não há fila de pesquisas).
//
// 2. **Fallback** (legado): quando não há cmd correspondente — warp-ins
//    via UnitInit, spawns iniciais, replays sem game events, ou cmds
//    órfãos por seleção não resolvida — recuamos do `finish_loop`
//    bruto subtraindo `build_time_loops(action, base_build)`. Estruturas
//    vindas de `UnitInit` já são start-time e só projetam o
//    `finish_loop` somando o build time.

use std::collections::HashMap;

use s2protocol::tracker_events::unit_tag_index;

use crate::balance_data::build_time_loops;
use crate::replay::{
    EntityCategory, EntityEventKind, PlayerTimeline, ProductionCmd, ReplayTimeline,
    UNIT_INIT_MARKER,
};

// ── Structs de saída ──────────────────────────────────────────────────────────

/// Desfecho real de uma entrada do build order. A maioria das entradas
/// são `Completed` (produção terminou normalmente). As duas outras
/// variantes só se aplicam a estruturas cujo `UnitDied` chegou antes
/// do `UnitDone`, e são distinguidas pelo `killer_player_id` do
/// `ProductionCancelled` que o parser emite nesse caso:
///
/// - `Cancelled`: jogador clicou "cancel" no prédio em construção
///   (killer é o próprio dono ou None). SC2 reembolsa 75%.
/// - `DestroyedInProgress`: o inimigo derrubou o prédio antes de
///   completar (killer é um player diferente).
///
/// Unidades/workers/upgrades que nunca chegam a ser canceláveis ficam
/// sempre como `Completed` — pra elas o `UnitDied` posterior, se
/// existir, não afeta o build order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryOutcome {
    Completed,
    Cancelled,
    DestroyedInProgress,
}

impl EntryOutcome {
    /// Letra usada na coluna `outcome` do golden CSV. `C`/`X`/`D` são
    /// escolhidas pra serem visualmente distintas num diff.
    pub fn short_letter(self) -> &'static str {
        match self {
            EntryOutcome::Completed => "C",
            EntryOutcome::Cancelled => "X",
            EntryOutcome::DestroyedInProgress => "D",
        }
    }
}

#[derive(Clone)]
pub struct BuildOrderEntry {
    /// Supply usado no instante de início.
    pub supply: u8,
    /// Capacidade total de supply no instante de início (food_made).
    pub supply_made: u8,
    /// Instante de início da ação (start time).
    pub game_loop: u32,
    /// Instante de conclusão da ação. Significado depende do `outcome`:
    /// - `Completed`: instante projetado de conclusão (start + build_time).
    /// - `Cancelled` / `DestroyedInProgress`: instante real em que o
    ///   prédio foi cancelado/destruído durante a construção.
    pub finish_loop: u32,
    /// Sequência global vinda do parser, usada como tiebreaker entre
    /// `entity_events` e `upgrades` no mesmo `game_loop`. Não é
    /// exposto no CSV.
    pub seq: u32,
    pub action: String,
    pub count: u32,
    pub is_upgrade: bool,
    pub is_structure: bool,
    pub outcome: EntryOutcome,
    /// Número estimado de Chrono Boosts que aceleraram esta produção.
    /// 0 quando não houve chrono ou quando o start_loop veio do
    /// fallback (sem cmd matching, não dá pra saber). Calculado
    /// comparando o tempo real `finish - start` com o
    /// `build_time_loops` base da ação.
    pub chrono_boosts: u8,
}

pub struct PlayerBuildOrder {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub entries: Vec<BuildOrderEntry>,
}

pub struct BuildOrderResult {
    pub players: Vec<PlayerBuildOrder>,
    pub datetime: String,
    pub map_name: String,
    pub loops_per_second: f64,
}

// ── Extração ──────────────────────────────────────────────────────────────────

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
    for ev in &player.entity_events {
        if ev.kind == EntityEventKind::ProductionCancelled {
            cancel_by_tag.insert(ev.tag, (ev.game_loop, ev.killer_player_id));
        }
    }

    // Estado mutável compartilhado pelo cmd matching:
    // - `cmds_by_producer[producer_index] -> Vec<cmd_idx>` em ordem
    //   de emissão (game.rs já empurra por game_loop crescente).
    // - `consumed[i]` marca que o cmd `i` já foi pareado a uma entrada,
    //   pra não ser reusado por outro evento.
    // - `prev_finish_by_producer` mantém o último `finish_loop`
    //   computado por produtor pra encadear `start = max(cmd, prev)`.
    let mut cmds_by_producer: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, cmd) in player.production_cmds.iter().enumerate() {
        if let Some(&p) = cmd.producer_tag_indexes.first() {
            cmds_by_producer.entry(p).or_default().push(i);
        }
    }
    let mut consumed = vec![false; player.production_cmds.len()];
    let mut prev_finish_by_producer: HashMap<u32, u32> = HashMap::new();

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
        let cmd_match: Option<(u32, u32)> = if from_unit_init {
            None
        } else {
            ev.creator_tag.and_then(|t| {
                let producer_idx = unit_tag_index(t);
                consume_producer_cmd(
                    &cmds_by_producer,
                    &mut consumed,
                    &player.production_cmds,
                    producer_idx,
                    &ev.entity_type,
                    max_cmd_loop,
                )
                .map(|loop_| (producer_idx, loop_))
            })
        };

        let start_loop = if from_unit_init {
            raw_loop
        } else if let Some((producer_idx, cmd_loop)) = cmd_match {
            let prev = prev_finish_by_producer
                .get(&producer_idx)
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
        if let Some((producer_idx, _)) = cmd_match {
            prev_finish_by_producer.insert(producer_idx, projected_finish);
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

        // Chrono boost: só estimamos quando temos start via cmd
        // matching (tempo real) e a entrada completou normalmente.
        let chrono_boosts = if cmd_match.is_some() && outcome == EntryOutcome::Completed {
            let expected_bt = build_time_loops(&ev.entity_type, base_build);
            let actual_bt = projected_finish.saturating_sub(start_loop);
            estimate_chrono_count(expected_bt, actual_bt)
        } else {
            0
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
        let cmd_loop =
            consume_global_cmd(&mut consumed, &player.production_cmds, &u.name, max_cmd);
        let start_loop =
            cmd_loop.unwrap_or_else(|| subtract_build_time(finish_loop, &u.name, base_build));
        let chrono_boosts = if cmd_loop.is_some() {
            let actual_bt = finish_loop.saturating_sub(start_loop);
            estimate_chrono_count(expected_bt, actual_bt)
        } else {
            0
        };
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
        });
    }

    // Sort por (game_loop, seq) — agora `game_loop` é o instante de
    // início, então a ordem cronológica é preservada na display.
    raw.sort_by_key(|e| (e.game_loop, e.seq));

    deduplicate(raw)
}

/// Procura o primeiro cmd não-consumido emitido pelo `producer_idx`
/// cuja `ability` bate com `action` E cujo `game_loop` satisfaz
/// `cmd_loop <= max_cmd_loop` (constraint de causalidade). Marca o
/// cmd como consumido e retorna seu `game_loop`. Quando não há match,
/// retorna `None` e o caller cai no fallback `subtract_build_time`.
///
/// Iteramos a fila inteira (não só o front) porque um produtor pode
/// receber cmds de tipos diferentes intercalados (ex.: Stargate
/// alternando Phoenix/Voidray) e queremos sempre achar a próxima
/// ocorrência da ação certa, não a primeira da fila.
fn consume_producer_cmd(
    by_producer: &HashMap<u32, Vec<usize>>,
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    producer_idx: u32,
    action: &str,
    max_cmd_loop: u32,
) -> Option<u32> {
    let queue = by_producer.get(&producer_idx)?;
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
        consumed[i] = true;
        return Some(cmds[i].game_loop);
    }
    None
}

/// Match global por nome de ação (sem filtrar por produtor). Usado
/// para upgrades, que são one-shot e não enfileiram — basta o primeiro
/// cmd disponível com a ability certa que respeite a mesma constraint
/// de causalidade `cmd_loop <= max_cmd_loop`.
fn consume_global_cmd(
    consumed: &mut [bool],
    cmds: &[ProductionCmd],
    action: &str,
    max_cmd_loop: u32,
) -> Option<u32> {
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
        return Some(cmd.game_loop);
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
fn supply_at(player: &PlayerTimeline, loop_: u32) -> (u8, u8) {
    player
        .stats_at(loop_)
        .map(|s| (s.supply_used as u8, s.supply_made as u8))
        .unwrap_or((0, 0))
}

/// Funde entradas consecutivas com a mesma ação em uma única com `count`
/// incrementado. Só funde se o **outcome** também for igual — um
/// SupplyDepot cancelado seguido de um SupplyDepot que completou precisam
/// aparecer como linhas separadas para que o usuário veja a diferença.
fn deduplicate(entries: Vec<BuildOrderEntry>) -> Vec<BuildOrderEntry> {
    let mut out: Vec<BuildOrderEntry> = Vec::new();
    for entry in entries {
        match out.last_mut() {
            Some(last) if last.action == entry.action && last.outcome == entry.outcome => {
                last.count += 1;
                // Acumula chronos do grupo — o display mostra o total.
                last.chrono_boosts = last.chrono_boosts.saturating_add(entry.chrono_boosts);
            }
            _ => out.push(entry),
        }
    }
    out
}

// ── Classificação de entradas ─────────────────────────────────────────────────

/// Categoria de uma entrada do build order. `Worker` é um subtipo
/// especial de `Unit` para SCV/Probe/Drone/MULE — útil pra filtros de
/// UI que querem esconder spam de workers sem sumir com o resto das
/// unidades. `Research` vs `Upgrade` distingue pesquisas pontuais
/// (Stimpack, Blink, WarpGate…) de upgrades com níveis
/// (InfantryWeaponsLevel1/2/3, Armor…).
#[allow(dead_code)] // consumido apenas pelo binário GUI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Worker,
    Unit,
    Structure,
    Research,
    Upgrade,
}

#[allow(dead_code)] // consumido apenas pelo binário GUI
impl EntryKind {
    /// Letra curta usada em UIs compactas (coluna "tipo" na GUI).
    /// `U` colide entre Unit e Upgrade — escolhemos `U` para Unit e
    /// `P` (de u**p**grade) para o segundo, já que Unit é mais comum.
    pub fn short_letter(self) -> &'static str {
        match self {
            EntryKind::Worker => "W",
            EntryKind::Unit => "U",
            EntryKind::Structure => "S",
            EntryKind::Research => "R",
            EntryKind::Upgrade => "P",
        }
    }

    /// Nome completo em inglês — útil como tooltip.
    pub fn full_name(self) -> &'static str {
        match self {
            EntryKind::Worker => "Worker",
            EntryKind::Unit => "Unit",
            EntryKind::Structure => "Structure",
            EntryKind::Research => "Research",
            EntryKind::Upgrade => "Upgrade",
        }
    }
}

/// Classifica uma entrada do build order em uma `EntryKind`. A decisão
/// usa os flags já armazenados (`is_upgrade`/`is_structure`) e o nome
/// bruto da ação para distinguir worker/unit e research/upgrade.
#[allow(dead_code)] // consumido apenas pelo binário GUI
pub fn classify_entry(entry: &BuildOrderEntry) -> EntryKind {
    if entry.is_upgrade {
        if is_leveled_upgrade(&entry.action) {
            EntryKind::Upgrade
        } else {
            EntryKind::Research
        }
    } else if entry.is_structure {
        EntryKind::Structure
    } else if is_worker_name(&entry.action) {
        EntryKind::Worker
    } else {
        EntryKind::Unit
    }
}

/// Retorna `true` se o nome da unidade é um worker (coletor de
/// recursos). Inclui MULE por gerar recurso como os demais, ainda
/// que seja invocado pela Orbital Command em vez de treinado.
#[allow(dead_code)] // consumido apenas pelo binário GUI
pub fn is_worker_name(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone" | "MULE")
}

/// Heurística para separar upgrades com níveis (Weapons/Armor 1-3)
/// de pesquisas pontuais. SC2 sufixa os níveis com "Level1/2/3".
#[allow(dead_code)] // consumido apenas pelo binário GUI
fn is_leveled_upgrade(name: &str) -> bool {
    name.ends_with("Level1") || name.ends_with("Level2") || name.ends_with("Level3")
}

// ── Formatação de tempo ──────────────────────────────────────────────────────

pub fn format_time(game_loop: u32, lps: f64) -> String {
    let total_secs = (game_loop as f64 / lps).round() as u32;
    format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::parse_replay;
    use std::path::PathBuf;

    fn example() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/replay1.SC2Replay")
    }

    /// Replay de referência usado pelo golden CSV. É um arquivo
    /// específico que o usuário escolheu pra ter um build order
    /// "canônico" auditado à mão; mantemos separado do `example()`
    /// pra não acoplar os outros testes a um replay que pode mudar.
    fn golden_example() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/old_republic_50.SC2Replay")
    }

    #[test]
    fn entries_are_chronologically_sorted_by_start_loop() {
        let t = parse_replay(&example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        for player in &bo.players {
            for w in player.entries.windows(2) {
                assert!(
                    w[0].game_loop <= w[1].game_loop,
                    "build_order fora de ordem em {}: {} > {}",
                    player.name, w[0].game_loop, w[1].game_loop,
                );
            }
        }
    }

    #[test]
    fn orbital_command_morphs_appear_in_build_order() {
        // O exemplo tem CC→OrbitalCommand. Antes do fix do
        // synthetic_morph_ability esses morphs eram filtrados por falta
        // de creator_ability — nunca apareciam no build order.
        let t = parse_replay(&example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        let terran = bo
            .players
            .iter()
            .find(|p| p.race == "Terran")
            .expect("terran player");
        let count = terran
            .entries
            .iter()
            .filter(|e| e.action == "OrbitalCommand")
            .count();
        assert!(
            count > 0,
            "esperava ao menos um OrbitalCommand no build order, achei {count}",
        );
    }

    #[test]
    fn upgrade_start_time_subtracts_build_time() {
        // Stimpack tem 140s Normal speed (= 2240 game loops) no LotV
        // atual. O `game_loop` cru no UpgradeEntry é o instante de
        // conclusão; a entrada do build order deve estar em
        // `finish - build_time_loops("Stimpack")`, e `finish_loop` deve
        // casar com o loop bruto do upgrade. O delta vem do balance
        // data versionado por `base_build`, não de uma constante.
        use crate::balance_data::build_time_loops;

        let t = parse_replay(&example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");

        let terran = t.players.iter().find(|p| p.race == "Terran").unwrap();
        let stimpack_finish = terran
            .upgrades
            .iter()
            .find(|u| u.name == "Stimpack")
            .map(|u| u.game_loop)
            .expect("stimpack research");
        let expected_start =
            stimpack_finish.saturating_sub(build_time_loops("Stimpack", t.base_build));

        let bo_terran = bo.players.iter().find(|p| p.race == "Terran").unwrap();
        let stimpack_entry = bo_terran
            .entries
            .iter()
            .find(|e| e.action == "Stimpack")
            .expect("stimpack entry no build order");
        assert_eq!(stimpack_entry.game_loop, expected_start);
        assert_eq!(stimpack_entry.finish_loop, stimpack_finish);
    }

    #[test]
    fn supply_made_is_populated_and_geq_supply_used() {
        // O `supply_made` (capacidade) tem que ser >= `supply` (usado)
        // em todos os snapshots — caso contrário o jogador estaria
        // supply blocked impossível. E pelo menos algumas entradas
        // precisam ter `supply_made > 0` (sanity check de que o campo
        // está sendo populado a partir de `food_made`).
        let t = parse_replay(&example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        let mut nonzero = 0usize;
        for player in &bo.players {
            for entry in &player.entries {
                assert!(
                    entry.supply_made >= entry.supply,
                    "supply_made ({}) < supply ({}) em {} para {}",
                    entry.supply_made, entry.supply, player.name, entry.action,
                );
                if entry.supply_made > 0 {
                    nonzero += 1;
                }
            }
        }
        assert!(
            nonzero > 0,
            "esperava ao menos uma entrada com supply_made > 0",
        );
    }

    /// Renderiza o build order de um player no formato golden CSV.
    /// Cabeçalho fixo + uma linha por entrada. Tempo em mm:ss para
    /// facilitar correção manual. A coluna `outcome` (C/X/D) existe
    /// pra que mudanças na detecção de cancelamento/destruição em
    /// progresso sejam capturadas pelo teste golden.
    fn render_golden_csv(player: &PlayerBuildOrder, lps: f64) -> String {
        let mut out = String::new();
        out.push_str("# old_republic_50.SC2Replay — build order golden\n");
        out.push_str(&format!(
            "# player: {} ({}) mmr={}\n",
            player.name,
            player.race,
            player.mmr.map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
        ));
        out.push_str(
            "# columns: start,finish,supply_used,supply_made,kind,action,count,outcome\n",
        );
        for entry in &player.entries {
            let kind = classify_entry(entry).short_letter();
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                format_time(entry.game_loop, lps),
                format_time(entry.finish_loop, lps),
                entry.supply,
                entry.supply_made,
                kind,
                entry.action,
                entry.count,
                entry.outcome.short_letter(),
            ));
        }
        out
    }

    fn golden_path(player_name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/golden")
            .join(format!("old_republic_50_build_order_{player_name}.csv"))
    }

    /// Helper de "bless" — escreve o golden atual no disco. Não é
    /// chamado por nenhum teste; existe pra ser invocado manualmente
    /// via `cargo test bless_build_order_goldens -- --ignored --nocapture`
    /// quando se quer regenerar os arquivos do zero. Em uso normal o
    /// usuário corrige os CSVs à mão.
    #[test]
    #[ignore]
    fn bless_build_order_goldens() {
        let t = parse_replay(&golden_example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        let lps = bo.loops_per_second;
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/golden");
        std::fs::create_dir_all(&dir).expect("mkdir");
        for player in &bo.players {
            let path = golden_path(&player.name);
            let csv = render_golden_csv(player, lps);
            std::fs::write(&path, &csv).expect("write golden");
            println!("wrote {}", path.display());
        }
    }

    /// Compara o build order do replay golden com o conteúdo de
    /// `examples/golden/replay1_build_order_<player>.csv`. Em caso de
    /// divergência, imprime as primeiras linhas que diferem para
    /// facilitar localizar o problema. Para regenerar do zero use o
    /// helper `bless_build_order_goldens` (ignored).
    #[test]
    fn build_order_matches_golden_csv() {
        let t = parse_replay(&golden_example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        let lps = bo.loops_per_second;
        for player in &bo.players {
            let path = golden_path(&player.name);
            let actual = render_golden_csv(player, lps);
            let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "não consegui ler o golden {}: {e}\n\
                     dica: rode `cargo test --bin sc2-replay-gui bless_build_order_goldens -- --ignored` \
                     para regenerar.",
                    path.display(),
                )
            });
            // Normaliza CRLF → LF para tolerar checkout no Windows.
            let expected_norm = expected.replace("\r\n", "\n");
            let actual_norm = actual.replace("\r\n", "\n");
            if expected_norm != actual_norm {
                let first_diff = expected_norm
                    .lines()
                    .zip(actual_norm.lines())
                    .enumerate()
                    .find(|(_, (e, a))| e != a)
                    .map(|(i, (e, a))| format!("linha {}: esperado={:?} atual={:?}", i + 1, e, a));
                panic!(
                    "build order divergente para {} ({}):\n  golden: {}\n  {}\n\
                     dica: rode `cargo test --bin sc2-replay-gui bless_build_order_goldens -- --ignored` \
                     se a divergência for esperada.",
                    player.name,
                    player.race,
                    path.display(),
                    first_diff.unwrap_or_else(|| {
                        format!(
                            "número de linhas difere (esperado {}, atual {})",
                            expected_norm.lines().count(),
                            actual_norm.lines().count(),
                        )
                    }),
                );
            }
        }
    }

    #[test]
    fn golden_bunker_at_0244_is_destroyed_in_progress() {
        // No replay golden, firebat (Terran, p2) começa um Bunker às
        // 02:44 que é derrubado por Terror (Protoss, p1) antes de
        // completar. O outcome tem que ser DestroyedInProgress e o
        // finish_loop tem que estar no instante real da morte
        // (~03:10, NÃO o 03:13 projetado pelo balance data).
        let t = parse_replay(&golden_example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        let lps = bo.loops_per_second;
        let firebat = bo
            .players
            .iter()
            .find(|p| p.name == "firebat")
            .expect("firebat player");

        let bunker = firebat
            .entries
            .iter()
            .find(|e| {
                e.action == "Bunker" && format_time(e.game_loop, lps) == "02:44"
            })
            .expect("bunker em 02:44 no build order");
        assert_eq!(
            bunker.outcome,
            EntryOutcome::DestroyedInProgress,
            "bunker às 02:44 deveria ter outcome DestroyedInProgress, veio {:?}",
            bunker.outcome,
        );
        // Morte real às 03:10 (loop 4261 conforme lifecycle do replay).
        assert_eq!(
            format_time(bunker.finish_loop, lps),
            "03:10",
            "finish_loop deveria estar no instante real da morte",
        );
    }

    #[test]
    fn golden_supply_depot_at_0343_is_cancelled_by_player() {
        // firebat inicia um SupplyDepot às 03:43 e cancela 1-2s depois
        // (03:45 em mm:ss). killer_player_id = 2 (firebat mesmo),
        // então é Cancelled (intencional), não DestroyedInProgress.
        let t = parse_replay(&golden_example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        let lps = bo.loops_per_second;
        let firebat = bo
            .players
            .iter()
            .find(|p| p.name == "firebat")
            .expect("firebat player");

        let depot = firebat
            .entries
            .iter()
            .find(|e| {
                e.action == "SupplyDepot"
                    && format_time(e.game_loop, lps) == "03:43"
            })
            .expect("supply depot em 03:43 no build order");
        assert_eq!(
            depot.outcome,
            EntryOutcome::Cancelled,
            "depot às 03:43 deveria ter outcome Cancelled, veio {:?}",
            depot.outcome,
        );
        // Cancelado ~1.4s depois do start (03:45 em mm:ss arredondado).
        let finish_mmss = format_time(depot.finish_loop, lps);
        assert!(
            finish_mmss == "03:44" || finish_mmss == "03:45",
            "finish_loop do depot cancelado deveria estar em 03:44/03:45, veio {finish_mmss}",
        );
    }

    #[test]
    fn structure_unit_init_populates_finish_loop() {
        // Estruturas vindas de UnitInit têm `game_loop` no instante de
        // início (quando o SCV/Probe começa a construir). O extractor
        // precisa projetar `finish_loop = start + build_time`. O delta
        // exato vem do balance data versionado por `base_build`.
        use crate::balance_data::build_time_loops;

        let t = parse_replay(&example(), 0).expect("parse");
        let bo = extract_build_order(&t).expect("bo");
        let expected_delta = build_time_loops("SupplyDepot", t.base_build);
        assert!(
            expected_delta > 0,
            "balance data deveria conhecer SupplyDepot",
        );

        let bo_terran = bo.players.iter().find(|p| p.race == "Terran").unwrap();
        let depot = bo_terran
            .entries
            .iter()
            .find(|e| e.action == "SupplyDepot")
            .expect("supply depot no build order");
        assert_eq!(
            depot.finish_loop - depot.game_loop,
            expected_delta,
            "esperava finish - start = build_time(SupplyDepot) loops",
        );
    }
}
