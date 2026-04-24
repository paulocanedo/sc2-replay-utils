// Walker do `replay.game.events` (stream que carrega Cmd events e
// SelectionDelta/ControlGroupUpdate). O propósito é colher os Cmd
// events de produção (Train/Build/Research/Morph) com o produtor
// real associado, alimentando `PlayerTimeline.production_cmds` que
// o `build_order` usa para corrigir tempos sob Chrono Boost.
//
// Como o Cmd não carrega o tag do produtor, reconstruímos a seleção
// ativa por user com a mesma lógica do s2protocol
// (`game_events::state::handle_selection_delta`/`update_control_group`):
// `SelectionDelta` para o `ACTIVE_UNITS_GROUP_IDX` substitui a seleção,
// `ControlGroupUpdate::ERecall` recupera um grupo salvo. Ignoramos
// outras variantes porque a heurística "primeiro tag selecionado é o
// produtor" basta para casar com a fila do tracker.
//
// O nome bruto da ação ("Marine", "Stimpack", …) é resolvido via
// `balance_data::resolve_ability_command`, que consulta a tabela
// `(producer, ability_id, cmd_index) → action_id` gerada em build
// time. Cmds que não casam com nenhuma ação de produção (movimentos,
// stop, attack, …) são silenciosamente descartados.

use std::collections::HashMap;

use s2protocol::game_events::{
    GameEControlGroupUpdate, GameSCmdData, GameSSelectionMask, ReplayGameEvent,
};
use s2protocol::tracker_events::unit_tag_index;

use crate::balance_data::resolve_ability_command;

use super::tracker::IndexOwnerMap;
use super::types::{
    CameraPosition, EntityEvent, EntityEventKind, InjectCmd, PlayerTimeline, ProductionCmd,
};

/// Fator de escala das coordenadas "full" de game events → tile coords.
/// `GameSMapCoord3D` / `GameSPoint3D` usam fixed-point com 12 bits
/// fracionários (20 bits totais).
const POS_RATIO: i64 = 4096;

/// Fator de escala das coordenadas "mini" de game events → tile coords.
/// `GameSPointMini` (usado em CameraUpdate) usa fixed-point com 8 bits
/// fracionários (16 bits totais).
const POS_RATIO_MINI: i64 = 256;

/// Mesma constante usada por `s2protocol::state::ACTIVE_UNITS_GROUP_IDX`
/// — o slot 10 do array de control groups guarda a seleção ativa do
/// jogador (separada dos 0..9 hotkey-able).
const ACTIVE_UNITS_GROUP_IDX: usize = 10;

/// Estado da seleção por usuário. Cada índice de 0 a 10 representa um
/// control group; o slot 10 é a seleção ativa. Cada entrada é uma lista
/// de **tags completos** de unidades (`u32`, embora o SC2 use o nome
/// `unit_tag` para o valor `u32` que vem nos game events — diferente
/// do `i64` do tracker; conversão via `unit_tag_index`).
#[derive(Default)]
struct UserSelection {
    control_groups: Vec<Vec<u32>>,
}

impl UserSelection {
    fn new() -> Self {
        Self {
            control_groups: vec![Vec::new(); 11],
        }
    }

    fn active(&self) -> &[u32] {
        &self.control_groups[ACTIVE_UNITS_GROUP_IDX]
    }
}

pub(super) fn process_game_events(
    path_str: &str,
    mpq: &s2protocol::MPQ,
    file_contents: &[u8],
    user_to_player_idx: &HashMap<i64, usize>,
    index_owner: &IndexOwnerMap,
    base_build: u32,
    players: &mut [PlayerTimeline],
    max_loops: u32,
) -> Result<(), String> {
    let game_events = match s2protocol::read_game_events(path_str, mpq, file_contents) {
        Ok(events) => events,
        // Replays muito antigos / corrompidos podem falhar aqui. Não é
        // crítico — só impacta a precisão de timing pra Protoss; o
        // resto do parse continua válido.
        Err(_) => return Ok(()),
    };

    let mut selections: HashMap<i64, UserSelection> = HashMap::new();
    // Cursor por jogador: `game_loop` do último touchdown patchado. Land
    // cmds chegam e touchdowns acontecem em ordem cronológica crescente,
    // então o n-ésimo Land cmd casa com o n-ésimo touchdown. Guardar o
    // cursor evita que dois Land cmds consecutivos patchem o mesmo
    // evento (o que aconteceria se vários prédios estivessem voando ao
    // mesmo tempo).
    let mut land_cursor: HashMap<usize, u32> = HashMap::new();
    let mut game_loop: i64 = 0;

    for ev in game_events {
        game_loop += ev.delta;
        if max_loops != 0 && game_loop as u32 > max_loops {
            break;
        }
        let user_id = ev.user_id;
        let selection = selections.entry(user_id).or_insert_with(UserSelection::new);

        match ev.event {
            ReplayGameEvent::SelectionDelta(delta) => {
                if delta.m_control_group_id as usize == ACTIVE_UNITS_GROUP_IDX {
                    // Atalho do s2protocol: substitui a seleção ativa
                    // pelos `m_add_unit_tags`. Não aplica `m_remove_mask`
                    // — o jogo só costuma emitir um set completo aqui,
                    // então casamento do produtor seguinte funciona.
                    selection.control_groups[ACTIVE_UNITS_GROUP_IDX] =
                        delta.m_delta.m_add_unit_tags;
                }
            }

            ReplayGameEvent::ControlGroupUpdate(cg) => {
                let idx = cg.m_control_group_index as usize;
                if idx >= selection.control_groups.len() {
                    continue;
                }
                match cg.m_control_group_update {
                    GameEControlGroupUpdate::ESet
                    | GameEControlGroupUpdate::ESetAndSteal => {
                        selection.control_groups[idx] =
                            selection.control_groups[ACTIVE_UNITS_GROUP_IDX].clone();
                    }
                    GameEControlGroupUpdate::EAppend
                    | GameEControlGroupUpdate::EAppendAndSteal => {
                        let mut current =
                            selection.control_groups[ACTIVE_UNITS_GROUP_IDX].clone();
                        selection.control_groups[idx].append(&mut current);
                        selection.control_groups[idx].sort_unstable();
                        selection.control_groups[idx].dedup();
                    }
                    GameEControlGroupUpdate::EClear => {
                        selection.control_groups[idx].clear();
                    }
                    GameEControlGroupUpdate::ERecall => {
                        // O `m_mask` aqui é uma deselect mask, mas para
                        // produção raramente importa — uma recall típica
                        // recupera o grupo inteiro. Aceitamos a
                        // imprecisão.
                        let _ = GameSSelectionMask::None;
                        selection.control_groups[ACTIVE_UNITS_GROUP_IDX] =
                            selection.control_groups[idx].clone();
                    }
                }
            }

            ReplayGameEvent::Cmd(cmd) => {
                let Some(abil) = cmd.m_abil else { continue };
                let Some(&player_idx) = user_to_player_idx.get(&user_id) else {
                    continue;
                };

                // Detecção de Land ANTES da resolução normal do cmd.
                // Independe da seleção ativa porque nosso rastreamento
                // de seleção é incompleto (`SelectionDelta` ignora
                // `m_remove_mask` / `m_remove_unit_tags`), e na prática
                // a seleção do jogador pode virar vazia logo antes do
                // cmd de Land. Mas o Cmd carrega o `TargetPoint` direto,
                // e o tag do produtor pode ser recuperado varrendo
                // `entity_events` do jogador pelo próximo `Died(*Flying)`
                // em ordem cronológica — Land cmds e touchdowns vêm
                // ambos em ordem de `game_loop`, então o pareamento 1:1
                // é determinístico.
                if try_patch_landing(
                    &mut players[player_idx].entity_events,
                    abil.m_abil_link,
                    abil.m_abil_cmd_index,
                    &cmd.m_data,
                    game_loop as u32,
                    base_build,
                    land_cursor.entry(player_idx).or_insert(0),
                ) {
                    continue;
                }

                if selection.active().is_empty() {
                    continue;
                }
                // Pega o primeiro produtor selecionado — heurística do
                // SC2: o cmd vai pro primeiro prédio idle ou similar. A
                // imprecisão é absorvida pelo encadeamento por
                // finish_loop no build_order.
                let producer_tag = selection.active()[0] as i64;
                let Some(producer) = index_owner.get(&producer_tag) else {
                    continue;
                };
                if producer.player_idx != player_idx {
                    // Sanity: a unidade selecionada pertence ao mesmo
                    // jogador que emitiu o cmd. Caso contrário ignora —
                    // pode ser um spectate / shared control corner case.
                    continue;
                }

                let action = match resolve_ability_command(
                    &producer.unit_type,
                    abil.m_abil_link,
                    abil.m_abil_cmd_index,
                    base_build,
                ) {
                    Some(name) => name,
                    None => continue,
                };

                // SpawnLarva (Inject Larva) — captura separada com info
                // do alvo (Hatchery/Lair/Hive) extraída de m_data.
                if action == "SpawnLarva" {
                    if let GameSCmdData::TargetUnit(ref tu) = cmd.m_data {
                        let target_tag = tu.m_tag as i64;
                        let target_idx = unit_tag_index(target_tag);
                        let target_type = index_owner
                            .get(&target_tag)
                            .map(|e| e.unit_type.clone())
                            .unwrap_or_default();
                        let target_x = (tu.m_snapshot_point.x / POS_RATIO) as u8;
                        let target_y = (tu.m_snapshot_point.y / POS_RATIO) as u8;
                        players[player_idx].inject_cmds.push(InjectCmd {
                            game_loop: game_loop as u32,
                            target_tag_index: target_idx,
                            target_type,
                            target_x,
                            target_y,
                        });
                    }
                    continue;
                }

                players[player_idx].production_cmds.push(ProductionCmd {
                    game_loop: game_loop as u32,
                    ability: action.to_string(),
                    producer_tags: vec![producer_tag],
                    consumed: false,
                });
            }

            ReplayGameEvent::CameraUpdate(cam) => {
                let Some(&player_idx) = user_to_player_idx.get(&user_id) else {
                    continue;
                };
                let Some(ref target) = cam.m_target else {
                    continue;
                };
                let x = (target.x / POS_RATIO_MINI) as u8;
                let y = (target.y / POS_RATIO_MINI) as u8;
                // Dedup: skip se mesma posição do último sample.
                if let Some(last) = players[player_idx].camera_positions.last() {
                    if last.x == x && last.y == y {
                        continue;
                    }
                }
                players[player_idx].camera_positions.push(CameraPosition {
                    game_loop: game_loop as u32,
                    x,
                    y,
                });
            }

            _ => {}
        }
    }

    Ok(())
}

/// Ações de pouso das 5 estruturas voadoras terran. Cobertura completa —
/// `LocustMPFlying` é a única outra estrutura com sufixo `Flying` no
/// balance data e não tem ability de Land.
fn is_land_action(action: &str) -> bool {
    matches!(
        action,
        "CommandCenterLand"
            | "OrbitalCommandLand"
            | "BarracksLand"
            | "FactoryLand"
            | "StarportLand"
    )
}

/// Nomes dos 5 produtores voadores terran. Usados por `try_patch_landing`
/// para testar candidatos quando `index_owner.unit_type` está stale.
const FLYING_PRODUCERS: &[&str] = &[
    "CommandCenterFlying",
    "OrbitalCommandFlying",
    "BarracksFlying",
    "FactoryFlying",
    "StarportFlying",
];

/// Se o Cmd é um `*Land`, descobre o tag da estrutura voadora a partir
/// do próximo `Died(*Flying)` em `entity_events` a partir de `game_loop`,
/// patcha a tripla de pouso com a posição do `TargetPoint` e devolve
/// `true`.
///
/// Duas razões para não usar `index_owner.unit_type` / `producer_tag` da
/// seleção aqui:
///
/// 1. `index_owner.unit_type` reflete o ÚLTIMO type change do tag (estado
///    final após processar todo o tracker). Um CC que pousou, morphou em
///    Orbital e liftou de novo aparece em `index_owner` como
///    `OrbitalCommandFlying`, mas o Land do primeiro pouso foi emitido
///    quando o produtor era `CommandCenterFlying`. Lookup pelo tipo final
///    falha (abil 148 só casa com `CommandCenterFlying`, não com
///    `OrbitalCommandFlying`).
///
/// 2. Nosso rastreamento de seleção é incompleto — `SelectionDelta`
///    ignora `m_remove_mask`/`m_remove_unit_tags`, então `selection.active()`
///    pode ficar vazio antes do Land cmd, mesmo o jogador tendo a
///    estrutura voadora selecionada de fato.
///
/// O pareamento "próximo `Died(*Flying)` do jogador em ordem cronológica"
/// é determinístico: Land cmds e touchdowns chegam ambos em ordem crescente
/// de `game_loop`, então o n-ésimo Land cmd casa com o n-ésimo touchdown.
/// Testamos os 5 candidatos de produtor voador para identificar Land
/// (ability ids distintos por tipo: 148/233/158/155/156).
fn try_patch_landing(
    events: &mut [EntityEvent],
    ability_id: u16,
    cmd_index: i64,
    data: &GameSCmdData,
    game_loop: u32,
    base_build: u32,
    cursor: &mut u32,
) -> bool {
    let is_land = FLYING_PRODUCERS.iter().any(|c| {
        resolve_ability_command(c, ability_id, cmd_index, base_build)
            .map(is_land_action)
            .unwrap_or(false)
    });
    if !is_land {
        return false;
    }
    // Só `TargetPoint` carrega o ponto de pouso. Land com outras variantes
    // de `m_data` não acontece na prática — devolvemos `true` pra sinalizar
    // que o cmd foi reconhecido como Land (não cair no fluxo de produção).
    let GameSCmdData::TargetPoint(p) = data else {
        return true;
    };
    let lx = (p.x / POS_RATIO).clamp(0, 255) as u8;
    let ly = (p.y / POS_RATIO).clamp(0, 255) as u8;
    // Busca o próximo touchdown não patchado. O floor `game_loop` cobre
    // o instante do cmd; `*cursor` avança além do último touchdown já
    // patchado (evita colisão com múltiplas estruturas voadoras
    // simultâneas). A distinção entre pouso e kill em voo (ambos emitem
    // `Died(*Flying)`) é feita checando o próximo evento: pouso emite
    // `Died + Started + Finished` via `apply_type_change`, enquanto kill
    // é um `Died` isolado. O `Started` imediato, no mesmo tag e mesmo
    // `game_loop`, é o sinal.
    let floor = game_loop.max(*cursor);
    let Some(start) = events.iter().enumerate().position(|(i, ev)| {
        if ev.game_loop < floor
            || ev.kind != EntityEventKind::Died
            || !ev.entity_type.ends_with("Flying")
        {
            return false;
        }
        match events.get(i + 1) {
            Some(next) => {
                next.tag == ev.tag
                    && next.game_loop == ev.game_loop
                    && next.kind == EntityEventKind::ProductionStarted
            }
            None => false,
        }
    }) else {
        return true;
    };
    let tag = events[start].tag;
    let touchdown_loop = events[start].game_loop;
    patch_landing_position(events, tag, game_loop, lx, ly);
    // Avança o cursor para o loop seguinte ao touchdown — próximo Land
    // cmd não pode casar com este mesmo touchdown.
    *cursor = touchdown_loop + 1;
    true
}

/// Após um comando `*Land` chegar no cmd stream, localiza no
/// `entity_events` do jogador o próximo `Died(X*Flying)` do mesmo tag
/// (com `game_loop >= cmd_loop`) e sobrescreve a posição da tripla que
/// `tracker::apply_type_change` emitiu para o pouso, além de propagar a
/// mesma posição para todos os eventos subsequentes daquele tag até o
/// próximo lift-off (inclusive — o lift emite a partir da posição
/// pousada). A propagação é necessária porque o tracker atualiza
/// `tag_map[tag].pos_x/pos_y` apenas via `UnitBorn`/`UnitPosition`, e
/// `UnitPosition` pra estruturas voadoras é muito esparso (ciclos lift/
/// land rápidos podem não receber nenhuma amostra). Sem a propagação,
/// um morph in-place após o pouso (CC→Orbital, CC→PlanetaryFortress)
/// herdaria a posição de nascimento.
///
/// Se o jogador emitir múltiplos Lands para o mesmo tag em sequência
/// (reposicionamento antes do touchdown), cada cmd subsequente re-patcha
/// a mesma tripla — o último Land antes do touchdown vence.
fn patch_landing_position(
    events: &mut [EntityEvent],
    tag: i64,
    cmd_loop: u32,
    target_x: u8,
    target_y: u8,
) {
    let Some(start) = events.iter().position(|ev| {
        ev.game_loop >= cmd_loop
            && ev.tag == tag
            && ev.kind == EntityEventKind::Died
            && ev.entity_type.ends_with("Flying")
    }) else {
        return;
    };
    // Patcha a tripla de pouso e propaga para eventos subsequentes do
    // mesmo tag até (inclusive) o próximo `Finished(*Flying)` — esse é o
    // último evento emitido pelo próximo lift-off, e ele deve refletir a
    // posição pousada (edifício ainda não saiu do lugar no instante do
    // lift). Eventos posteriores ao `Finished(*Flying)` ocorrem em voo e
    // são sobrescritos pela interpolação de `unit_positions` na GUI (ou
    // por outro `Land` cmd subsequente). Se a estrutura nunca levanta
    // de novo (morph terminal para PF, ou destruição), a propagação vai
    // até o fim dos eventos do tag.
    for j in start..events.len() {
        if events[j].tag != tag {
            continue;
        }
        events[j].pos_x = target_x;
        events[j].pos_y = target_y;
        let is_flying_finished = events[j].kind == EntityEventKind::ProductionFinished
            && events[j].entity_type.ends_with("Flying");
        // `start` em si é um `Died(*Flying)` — não casa com o filtro
        // acima, então o break só dispara na próxima tripla de lift-off.
        if is_flying_finished {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::EntityCategory;

    fn ev(tag: i64, loop_: u32, kind: EntityEventKind, ty: &str, x: u8, y: u8) -> EntityEvent {
        EntityEvent {
            game_loop: loop_,
            seq: 0,
            kind,
            entity_type: ty.to_string(),
            category: EntityCategory::Structure,
            tag,
            pos_x: x,
            pos_y: y,
            creator_ability: None,
            creator_tag: None,
            killer_player_id: None,
        }
    }

    #[test]
    fn patch_overrides_triple_for_matching_tag() {
        let tag = 42;
        // Cenário: Barracks construído em (10,10), lift e depois Land em (80,60).
        // Tracker emitiu os eventos do pouso na posição antiga (10,10).
        let mut events = vec![
            ev(tag, 100, EntityEventKind::ProductionFinished, "Barracks", 10, 10),
            // Lift-off em (10,10) — correto, ficam intocados.
            ev(tag, 500, EntityEventKind::Died, "Barracks", 10, 10),
            ev(tag, 500, EntityEventKind::ProductionStarted, "BarracksFlying", 10, 10),
            ev(tag, 500, EntityEventKind::ProductionFinished, "BarracksFlying", 10, 10),
            // Landing em (80,60) — emitido com posição errada (10,10).
            ev(tag, 900, EntityEventKind::Died, "BarracksFlying", 10, 10),
            ev(tag, 900, EntityEventKind::ProductionStarted, "Barracks", 10, 10),
            ev(tag, 900, EntityEventKind::ProductionFinished, "Barracks", 10, 10),
        ];
        // Cmd de Land emitido em loop 850 com destino (80,60).
        patch_landing_position(&mut events, tag, 850, 80, 60);

        // Lift permanece em (10,10).
        assert_eq!((events[1].pos_x, events[1].pos_y), (10, 10));
        assert_eq!((events[2].pos_x, events[2].pos_y), (10, 10));
        assert_eq!((events[3].pos_x, events[3].pos_y), (10, 10));
        // Landing patchado para (80,60).
        assert_eq!((events[4].pos_x, events[4].pos_y), (80, 60));
        assert_eq!((events[5].pos_x, events[5].pos_y), (80, 60));
        assert_eq!((events[6].pos_x, events[6].pos_y), (80, 60));
    }

    #[test]
    fn patch_no_match_leaves_events_untouched() {
        let tag = 42;
        let other = 99;
        let mut events = vec![
            ev(other, 900, EntityEventKind::Died, "BarracksFlying", 10, 10),
            ev(other, 900, EntityEventKind::ProductionStarted, "Barracks", 10, 10),
            ev(other, 900, EntityEventKind::ProductionFinished, "Barracks", 10, 10),
        ];
        let snapshot: Vec<(u8, u8)> =
            events.iter().map(|e| (e.pos_x, e.pos_y)).collect();
        patch_landing_position(&mut events, tag, 800, 80, 60);
        let after: Vec<(u8, u8)> = events.iter().map(|e| (e.pos_x, e.pos_y)).collect();
        assert_eq!(snapshot, after);
    }

    #[test]
    fn patch_last_wins_when_multiple_lands_target_same_touchdown() {
        let tag = 42;
        let mut events = vec![
            ev(tag, 900, EntityEventKind::Died, "BarracksFlying", 10, 10),
            ev(tag, 900, EntityEventKind::ProductionStarted, "Barracks", 10, 10),
            ev(tag, 900, EntityEventKind::ProductionFinished, "Barracks", 10, 10),
        ];
        // Jogador emite Land em (80,60), muda de ideia antes do touchdown
        // e re-emite em (120,100). A tripla deve refletir a última coord.
        patch_landing_position(&mut events, tag, 800, 80, 60);
        patch_landing_position(&mut events, tag, 850, 120, 100);
        assert_eq!((events[0].pos_x, events[0].pos_y), (120, 100));
        assert_eq!((events[1].pos_x, events[1].pos_y), (120, 100));
        assert_eq!((events[2].pos_x, events[2].pos_y), (120, 100));
    }

    #[test]
    fn patch_propagates_to_subsequent_morph_and_next_liftoff() {
        let tag = 42;
        // CC nasce em (10,10), lift, land em (80,60), morph para Orbital,
        // lift de novo. A propagação deve carregar (80,60) até o
        // Finished(OrbitalCommandFlying) do lift subsequente (inclusive).
        let mut events = vec![
            ev(tag, 100, EntityEventKind::ProductionFinished, "CommandCenter", 10, 10),
            // Land em (80,60) — posição inicial errada (10,10).
            ev(tag, 500, EntityEventKind::Died, "CommandCenterFlying", 10, 10),
            ev(tag, 500, EntityEventKind::ProductionStarted, "CommandCenter", 10, 10),
            ev(tag, 500, EntityEventKind::ProductionFinished, "CommandCenter", 10, 10),
            // Morph para Orbital.
            ev(tag, 700, EntityEventKind::Died, "CommandCenter", 10, 10),
            ev(tag, 700, EntityEventKind::ProductionStarted, "OrbitalCommand", 10, 10),
            ev(tag, 700, EntityEventKind::ProductionFinished, "OrbitalCommand", 10, 10),
            // Lift de novo.
            ev(tag, 900, EntityEventKind::Died, "OrbitalCommand", 10, 10),
            ev(tag, 900, EntityEventKind::ProductionStarted, "OrbitalCommandFlying", 10, 10),
            ev(tag, 900, EntityEventKind::ProductionFinished, "OrbitalCommandFlying", 10, 10),
            // Evento em voo (sample de tracker posterior, ex.: morte em voo).
            ev(tag, 1500, EntityEventKind::Died, "OrbitalCommandFlying", 99, 99),
        ];
        patch_landing_position(&mut events, tag, 450, 80, 60);

        // Finished(CommandCenter) de nascimento: intocado.
        assert_eq!((events[0].pos_x, events[0].pos_y), (10, 10));
        // Land triple: patchado.
        for i in 1..=3 {
            assert_eq!((events[i].pos_x, events[i].pos_y), (80, 60), "idx {i}");
        }
        // Morph triple: propagado.
        for i in 4..=6 {
            assert_eq!((events[i].pos_x, events[i].pos_y), (80, 60), "idx {i}");
        }
        // Próximo lift (Died(Orbital), Started/Finished(OrbitalFlying)): propagado até Finished(Flying).
        for i in 7..=9 {
            assert_eq!((events[i].pos_x, events[i].pos_y), (80, 60), "idx {i}");
        }
        // Died em voo posterior: intocado (passamos além do Finished(Flying)).
        assert_eq!((events[10].pos_x, events[10].pos_y), (99, 99));
    }

    #[test]
    fn patch_propagates_to_end_when_no_subsequent_liftoff() {
        let tag = 42;
        // CC lift, land, morph para PF (terminal — PF não voa).
        // Propagação vai até o fim dos eventos do tag.
        let mut events = vec![
            ev(tag, 500, EntityEventKind::Died, "CommandCenterFlying", 10, 10),
            ev(tag, 500, EntityEventKind::ProductionStarted, "CommandCenter", 10, 10),
            ev(tag, 500, EntityEventKind::ProductionFinished, "CommandCenter", 10, 10),
            ev(tag, 700, EntityEventKind::Died, "CommandCenter", 10, 10),
            ev(tag, 700, EntityEventKind::ProductionStarted, "PlanetaryFortress", 10, 10),
            ev(tag, 700, EntityEventKind::ProductionFinished, "PlanetaryFortress", 10, 10),
        ];
        patch_landing_position(&mut events, tag, 450, 80, 60);
        for (i, ev) in events.iter().enumerate() {
            assert_eq!((ev.pos_x, ev.pos_y), (80, 60), "idx {i}");
        }
    }

    /// Helper: assumimos que sempre que `balance_data` foi gerado (pelo
    /// build.rs), o `(CommandCenterFlying, 148, 0)` resolve para
    /// `CommandCenterLand`. Se isto falhar, a tabela não tem o Land —
    /// invariante coberta por outro teste em balance_data.
    fn target_point(x: i64, y: i64) -> GameSCmdData {
        GameSCmdData::TargetPoint(s2protocol::game_events::GameSMapCoord3D {
            x: x * POS_RATIO,
            y: y * POS_RATIO,
            z: 0,
        })
    }

    #[test]
    fn try_patch_landing_picks_next_touchdown_distinguishes_kill() {
        // Cenário: player tem uma Barracks voando que morre em voo
        // (Died(*Flying) solitário, não é um touchdown) antes de pousar
        // uma OUTRA Barracks. O detector de Land deve pular o kill e
        // casar com o touchdown real.
        let mut events = vec![
            // tag 1: killed in flight at loop 600.
            ev(1, 600, EntityEventKind::Died, "BarracksFlying", 50, 50),
            // tag 2: actual landing at loop 700.
            ev(2, 700, EntityEventKind::Died, "BarracksFlying", 20, 20),
            ev(2, 700, EntityEventKind::ProductionStarted, "Barracks", 20, 20),
            ev(2, 700, EntityEventKind::ProductionFinished, "Barracks", 20, 20),
        ];
        let mut cursor = 0u32;
        // Cmd de Land em loop 650 com destino (100,100). Abil 158 = BarracksLand.
        let handled = try_patch_landing(&mut events, 158, 0, &target_point(100, 100), 650, 96592, &mut cursor);
        assert!(handled);
        // Kill em voo permanece intocado.
        assert_eq!((events[0].pos_x, events[0].pos_y), (50, 50));
        // Tripla do touchdown patchada.
        assert_eq!((events[1].pos_x, events[1].pos_y), (100, 100));
        assert_eq!((events[2].pos_x, events[2].pos_y), (100, 100));
        assert_eq!((events[3].pos_x, events[3].pos_y), (100, 100));
        assert_eq!(cursor, 701);
    }

    #[test]
    fn try_patch_landing_cursor_separates_concurrent_flyings() {
        // Duas Barracks voando simultaneamente, pousam em locais diferentes.
        // Os Land cmds chegam em ordem cronológica; o cursor garante que
        // cada cmd casa com o touchdown correspondente.
        let mut events = vec![
            ev(1, 700, EntityEventKind::Died, "BarracksFlying", 10, 10),
            ev(1, 700, EntityEventKind::ProductionStarted, "Barracks", 10, 10),
            ev(1, 700, EntityEventKind::ProductionFinished, "Barracks", 10, 10),
            ev(2, 750, EntityEventKind::Died, "BarracksFlying", 20, 20),
            ev(2, 750, EntityEventKind::ProductionStarted, "Barracks", 20, 20),
            ev(2, 750, EntityEventKind::ProductionFinished, "Barracks", 20, 20),
        ];
        let mut cursor = 0u32;
        // 1º Land cmd em loop 650, alvo (100,100). Casa com touchdown loop 700 (tag 1).
        assert!(try_patch_landing(&mut events, 158, 0, &target_point(100, 100), 650, 96592, &mut cursor));
        assert_eq!((events[0].pos_x, events[0].pos_y), (100, 100));
        assert_eq!((events[3].pos_x, events[3].pos_y), (20, 20), "2º touchdown não deve ter sido tocado ainda");
        // 2º Land cmd em loop 680, alvo (200,200). Casa com touchdown loop 750 (tag 2).
        assert!(try_patch_landing(&mut events, 158, 0, &target_point(200, 200), 680, 96592, &mut cursor));
        assert_eq!((events[3].pos_x, events[3].pos_y), (200, 200));
        assert_eq!((events[4].pos_x, events[4].pos_y), (200, 200));
        assert_eq!((events[5].pos_x, events[5].pos_y), (200, 200));
        // Tag 1 não deve ter sido re-patchado.
        assert_eq!((events[0].pos_x, events[0].pos_y), (100, 100));
    }

    #[test]
    fn try_patch_landing_ignores_non_land_cmds() {
        let mut events = vec![
            ev(1, 700, EntityEventKind::Died, "BarracksFlying", 10, 10),
            ev(1, 700, EntityEventKind::ProductionStarted, "Barracks", 10, 10),
            ev(1, 700, EntityEventKind::ProductionFinished, "Barracks", 10, 10),
        ];
        let mut cursor = 0u32;
        // Abil 1 = não é Land. Função retorna false e não toca em nada.
        let handled = try_patch_landing(&mut events, 1, 0, &target_point(100, 100), 650, 96592, &mut cursor);
        assert!(!handled);
        assert_eq!((events[0].pos_x, events[0].pos_y), (10, 10));
    }

    #[test]
    fn is_land_action_matches_all_five_and_rejects_others() {
        assert!(is_land_action("CommandCenterLand"));
        assert!(is_land_action("OrbitalCommandLand"));
        assert!(is_land_action("BarracksLand"));
        assert!(is_land_action("FactoryLand"));
        assert!(is_land_action("StarportLand"));
        // Falsos positivos que terminam em "Land" mas não são pouso.
        assert!(!is_land_action("HoldPos"));
        assert!(!is_land_action("Move"));
        // Guarda contra futuras abilities que terminem em "Land" por acaso.
        assert!(!is_land_action("HighLand"));
    }
}
