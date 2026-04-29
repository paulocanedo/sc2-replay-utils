//! Loop principal de extração por jogador. Recebe um `PlayerTimeline`
//! e produz um `PlayerProductionLanes`. A função coordena:
//!
//! - Criação de lanes (`ProductionFinished` em estruturas-alvo).
//! - Detecção de morphs in-place (CC→Orbital/PF, Hatch→Lair→Hive,
//!   Gateway→WarpGate). O morph impeditivo Terran (CC→Orbital/PF, modo
//!   Workers) emite bloco `Morphing`; Zerg/Protoss apenas atualizam
//!   `canonical_type`/`warpgate_since_loop`.
//! - Janela `Impeded` de addon Terran (Reactor/TechLab em construção).
//! - Mapa Zerg `larva_tag → hatch_tag` para resolução posterior.
//! - Casamento `creator_tag` ↔ `production_cmds` para usar o instante
//!   real do click de Train como `start_loop` do bloco `Producing`.

use std::collections::HashMap;

use crate::balance_data;
use crate::replay::{
    is_incapacitating_addon, is_zerg_hatch, EntityEventKind, PlayerTimeline,
};

use super::classify::{intern_unit_name, is_target_unit, lane_canonical};
use super::morph::{is_morph_died, is_morph_finish, is_pure_morph_finish, morph_build_loops, morph_old_type};
use super::resolve::{consume_producer_cmd, merge_continuous, resolve_producer};
use super::terran::{
    resolve_addon_parent_by_exact_offset, resolve_addon_parent_by_proximity,
    resolve_addon_parent_via_cmd,
};
use super::types::{BlockKind, LaneMode, PlayerProductionLanes, ProductionBlock, StructureLane};

pub(super) fn extract_player(
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
    // `consumed` separado para resolução de parent de addon via cmd —
    // são cmds com ability = nome literal do addon (ex.
    // "BarracksReactor"), distintos dos cmds de Train consumidos por
    // `consumed`. Manter dois Vecs evita interferência entre as duas
    // pipelines de pareamento.
    let mut addon_cmd_consumed = vec![false; player.production_cmds.len()];

    // Last finish loop por creator_tag. Cada unidade pareada começa em
    // `max(cmd_loop, last_finish)` para herdar a semântica de fila do
    // `build_order` (sem paralelismo: produções concorrentes da mesma
    // estrutura ficam encostadas em vez de sobrepostas, e o
    // `merge_continuous` posteriormente as funde no mesmo bloco).
    let mut last_finish_by_creator: HashMap<i64, u32> = HashMap::new();
    // Last start loop por creator_tag — usado para detectar pares
    // paralelos do Reactor: quando duas unidades têm finish_loops
    // **próximos** no mesmo producer (gap ≤ PARALLEL_PAIR_TOLERANCE),
    // são "siblings" do mesmo cmd (ex.: 2 Marines treinando em
    // paralelo numa Barracks com Reactor — o engine emite os Born
    // events com 0-15 loops de diferença, não exatamente o mesmo
    // loop). A segunda do par herda o start da primeira em vez de
    // consumir um cmd separado.
    let mut last_start_by_creator: HashMap<i64, u32> = HashMap::new();
    // Tolerância em game loops para detectar par paralelo do Reactor
    // (mecânica exclusiva Terran — Reactor é o único caso onde uma
    // estrutura emite 2 unidades por 1 cmd no SC2). Pares paralelos
    // observados no replay Winter Madness LE têm gap de 0-37 loops
    // entre os dois Born events (engine emite com pequeno offset por
    // razões internas de tick/serialização).
    //
    // Sequencial mínimo para qualquer unidade SC2:
    //   - Marine (Terran, build fixo, sem chrono): 380 loops.
    //   - Probe (Protoss) com chronoboost máximo (1.5×): ~179 loops.
    //   - Drone (Zerg, larva-born): ~269 loops.
    //
    // 50 cobre o pior par paralelo observado com folga de ~3.5× para
    // o sequencial mais curto teórico (Probe+chrono). Sem risco de
    // falso positivo.
    const PARALLEL_PAIR_TOLERANCE: u32 = 50;

    for i in 0..events.len() {
        let ev = &events[i];
        match ev.kind {
            EntityEventKind::ProductionStarted => {
                let new_type = ev.entity_type.as_str();
                let morphed_from = morph_old_type(events, i);

                // Land detection: estrutura voadora (`*Flying`) virou de
                // volta uma estrutura grounded. Atualiza `pos_x/pos_y` da
                // lane para refletir onde ela está agora — sem isso, a
                // posição congelada no born_loop fica obsoleta após
                // qualquer relocate, contaminando a resolução de parent
                // de addon (offset esperado +3,0) e o fallback de
                // proximidade do `resolve_by_proximity`. O lift inverso
                // (`canonical → *Flying`) não importa para nós: enquanto
                // a estrutura voa ela não produz nem ganha addons.
                if let Some(old_type) = morphed_from {
                    if old_type.ends_with("Flying") {
                        if let Some(lane) = lanes_by_tag.get_mut(&ev.tag) {
                            lane.pos_x = ev.pos_x;
                            lane.pos_y = ev.pos_y;
                        }
                    }
                }

                // Morph in-place de estrutura — atualiza canonical_type
                // ou emite bloco Morphing impeditivo (CC→Orbital/PF).
                if let Some(new_canonical) = lane_canonical(new_type, mode) {
                    if let Some(old_type) = morphed_from {
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
                                            // Tipo destino do morph (Orbital/PF) — o
                                            // render desenha o ícone dentro da faixa
                                            // pra mostrar o motivo do impedimento.
                                            produced_type: Some(new_canonical),
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
                //
                // Distingue construção real (UnitInit, sem morph
                // antecedente) de SWAP de owner (UnitTypeChange via
                // lift+land de outra estrutura no mesmo addon, ex.
                // BarracksReactor → FactoryReactor quando o player
                // decola a Barracks e pousa Factory no mesmo Reactor).
                // Apenas a construção emite `Impeded` — swap não tem
                // janela impeditiva (o addon já existe e está pronto).
                //
                // Resolução de parent em quatro etapas, em ordem de
                // confiabilidade decrescente:
                //   1. `creator_tag` do evento (sempre `None` para
                //      addons no s2protocol — UnitInitEvent não tem o
                //      campo — mas mantido na cascata por simetria).
                //   2. **Offset exato (+3, 0)**: lane do tipo certo
                //      em `(addon.x - 3, addon.y)` exato. Determinístico
                //      quando todas as posições estão atualizadas (C),
                //      e crucial para distinguir entre múltiplas
                //      Barracks adjacentes — cada addon tem exatamente
                //      um parent no offset físico canônico.
                //   3. Cmd matching: como FALLBACK, não primary. Cmd
                //      é não-confiável quando o player tem control
                //      group de várias estruturas e emite Build_Addon
                //      (SC2 despacha pra múltiplas mas o cmd só
                //      registra `selection.active()[0]`); usar cmd
                //      como primary atribui múltiplos addons à mesma
                //      Barracks. Geometria pelo offset físico é mais
                //      robusta nesse caso.
                //   4. Proximidade pura por `d²`: last resort, caso
                //      offset exato e cmd ambos falhem (geometria
                //      atípica, ou posição da lane ainda stale por
                //      lift/land não rastreado).
                let is_swap = morphed_from.is_some();
                if mode == LaneMode::Army
                    && is_incapacitating_addon(new_type)
                    && !is_swap
                {
                    let parent = ev
                        .creator_tag
                        .or_else(|| {
                            resolve_addon_parent_by_exact_offset(
                                new_type,
                                ev.pos_x,
                                ev.pos_y,
                                ev.game_loop,
                                &lanes_by_tag,
                            )
                        })
                        .or_else(|| {
                            resolve_addon_parent_via_cmd(
                                &player.production_cmds,
                                &mut addon_cmd_consumed,
                                new_type,
                                ev.game_loop,
                                &lanes_by_tag,
                            )
                        })
                        .or_else(|| {
                            resolve_addon_parent_by_proximity(
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

                        // Detecção de par paralelo (Reactor): se já há
                        // uma unidade emitida pelo mesmo `creator_tag`
                        // com `finish_loop` PRÓXIMO (dentro de
                        // PARALLEL_PAIR_TOLERANCE), esta é a "irmã" —
                        // o player clicou Train uma vez e o Reactor
                        // produziu 2 unidades simultâneas (que o
                        // engine emite com 0-15 loops de offset). Não
                        // consumimos um cmd novo (compartilham o cmd
                        // da primeira) e herdamos o `start_loop`. Sem
                        // esta detecção, a segunda Marine cairia no
                        // `cmd_loop.max(prev=finish)` e renderizaria
                        // como bloco instantâneo (start = finish),
                        // sobreposto à primeira.
                        let raw_start = if let Some(ct) = creator_tag {
                            let prev_finish =
                                last_finish_by_creator.get(&ct).copied().unwrap_or(0);
                            let is_parallel_pair = prev_finish > 0
                                && finish_loop.saturating_sub(prev_finish)
                                    <= PARALLEL_PAIR_TOLERANCE;

                            if is_parallel_pair {
                                // Avança chain para o finish da SEGUNDA
                                // do par (que é tipicamente alguns loops
                                // depois da primeira). A próxima unidade
                                // sequencial chega só depois do par
                                // inteiro liberar a estrutura.
                                last_finish_by_creator.insert(ct, finish_loop);
                                last_start_by_creator
                                    .get(&ct)
                                    .copied()
                                    .unwrap_or_else(|| {
                                        finish_loop.saturating_sub(bt_fallback)
                                    })
                            } else {
                                let cmd_loop = consume_producer_cmd(
                                    &cmds_by_producer,
                                    &mut consumed,
                                    &player.production_cmds,
                                    ct,
                                    new_type,
                                    max_cmd_loop,
                                );
                                let start = match cmd_loop {
                                    Some(c) => c.max(prev_finish),
                                    None => finish_loop.saturating_sub(bt_fallback),
                                };
                                last_finish_by_creator.insert(ct, finish_loop);
                                last_start_by_creator.insert(ct, start);
                                start
                            }
                        } else {
                            finish_loop.saturating_sub(bt_fallback)
                        };

                        // Empurra `start_loop` para depois de qualquer
                        // bloco `Morphing`/`Impeded` da mesma lane que
                        // sobreponha a janela [raw_start, finish_loop].
                        // Em SC2 o jogador pode ENFILEIRAR um Train cmd
                        // enquanto a estrutura está construindo addon
                        // (Reactor/TechLab) ou morphando (CC→Orbital);
                        // o cmd entra em `production_cmds` no instante
                        // do clique, mas o treino real só começa quando
                        // a janela impeditiva termina. Sem este ajuste
                        // o bloco `Producing` aparece sobreposto com o
                        // `Impeded`/`Morphing`, dando a impressão de
                        // que a estrutura produzia duas coisas ao mesmo
                        // tempo. Build_order continua usando o cmd_loop
                        // raw (visualização diferente, intencional).
                        let start_loop = if let Some(lane) = lanes_by_tag.get(&lane_tag) {
                            let mut s = raw_start;
                            for b in &lane.blocks {
                                if matches!(
                                    b.kind,
                                    BlockKind::Morphing | BlockKind::Impeded
                                ) && b.end_loop > s
                                    && b.start_loop < finish_loop
                                {
                                    s = s.max(b.end_loop);
                                }
                            }
                            s.min(finish_loop)
                        } else {
                            raw_start
                        };

                        if start_loop < finish_loop {
                            if let Some(lane) = lanes_by_tag.get_mut(&lane_tag) {
                                lane.blocks.push(ProductionBlock {
                                    start_loop,
                                    end_loop: finish_loop,
                                    kind: BlockKind::Producing,
                                    produced_type: intern_unit_name(new_type),
                                });
                            }
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
                            });
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
        lane.blocks.sort_by_key(|b| b.start_loop);
        // Em estruturas com paralelismo real (Hatch/Lair/Hive em qualquer
        // modo, ou WarpGate pós-research), preservamos overlaps. Aqui
        // a lane é per-estrutura, então mesmo Hatch só tem paralelismo
        // via larvas distintas (cada larva é um creator_tag separado).
        let parallel_lane = is_zerg_hatch(lane.canonical_type);
        lane.blocks = merge_continuous(std::mem::take(&mut lane.blocks), parallel_lane);
    }

    PlayerProductionLanes { lanes }
}
