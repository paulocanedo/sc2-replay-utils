//! Testes integration-style do extrator de build order. Todos partem
//! de um replay real (`examples/*.SC2Replay`), rodam `extract_build_order`
//! e validam shape + timings da saída. O `build_order_matches_golden_csv`
//! é o teste mais abrangente — captura qualquer regressão pipeline-wide.

use super::types::format_time;
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

/// Invariante: nenhuma entrada de unidade ou pesquisa pode ter
/// `game_loop == finish_loop` (duração zero, "instantânea") — não
/// existe unidade ou upgrade que treine em zero loops no jogo.
///
/// O sintoma mais comum dessa quebra era o "Marine instantâneo às
/// 8:48" reportado no replay Winter Madness LE: numa Barracks com
/// Reactor o player clica Train_Marine uma vez e o tracker emite 2
/// `UnitBornEvent`s no mesmo `game_loop`. Sem detecção de par
/// paralelo, a segunda Marine caía em
/// `cmd_loop.max(prev_finish=projected_finish)` e o `start_loop`
/// virava o próprio `finish_loop`, gerando uma entrada com duração
/// zero.
///
/// `InjectLarva` é instantâneo por natureza (cmd, não unidade
/// produzida) e fica fora do invariante.
#[test]
fn no_entries_have_zero_duration() {
    let t = parse_replay(&example(), 0).expect("parse");
    let bo = extract_build_order(&t).expect("bo");
    for player in &bo.players {
        for entry in &player.entries {
            if entry.action.starts_with("InjectLarva") {
                continue;
            }
            assert!(
                entry.game_loop < entry.finish_loop,
                "entry de duração zero em {}: action={}, start={} finish={} \
                 (provável regressão da detecção de par paralelo do Reactor)",
                player.name,
                entry.action,
                entry.game_loop,
                entry.finish_loop,
            );
        }
    }
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
        "# columns: start,finish,supply_used,supply_made,kind,action,count,outcome,producer,producer_id\n",
    );
    for entry in &player.entries {
        let kind = classify_entry(entry).short_letter();
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            format_time(entry.game_loop, lps),
            format_time(entry.finish_loop, lps),
            entry.supply,
            entry.supply_made,
            kind,
            entry.action,
            entry.count,
            entry.outcome.short_letter(),
            entry.producer_type.as_deref().unwrap_or(""),
            entry
                .producer_id
                .map(|n| n.to_string())
                .unwrap_or_default(),
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
                 dica: rode `cargo test --bin sc2-replay-utils bless_build_order_goldens -- --ignored` \
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
                 dica: rode `cargo test --bin sc2-replay-utils bless_build_order_goldens -- --ignored` \
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
fn inject_action_uses_producer_id_format() {
    // Replay com Zerg: as entradas de InjectLarva devem codificar a
    // Hatchery alvo via `#N` (ID sequencial do produtor) no `action`,
    // permitindo distinguir bases sem depender de coordenadas.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/serral.SC2Replay");
    let t = parse_replay(&path, 0).expect("parse");
    let bo = extract_build_order(&t).expect("bo");

    let zerg = bo
        .players
        .iter()
        .find(|p| p.race == "Zerg")
        .expect("zerg player");
    let injects: Vec<&BuildOrderEntry> = zerg
        .entries
        .iter()
        .filter(|e| e.action.starts_with("InjectLarva"))
        .collect();
    assert!(
        !injects.is_empty(),
        "esperava ao menos um InjectLarva no replay zerg",
    );
    let with_id = injects
        .iter()
        .filter(|e| {
            e.action
                .strip_prefix("InjectLarva@")
                .map(|rest| rest.contains('#'))
                .unwrap_or(false)
        })
        .count();
    assert!(
        with_id > 0,
        "esperava ao menos um inject com formato `#N` (resolvido via target_tag_index); \
         viram só {} de {} no formato antigo com coordenadas",
        injects.len() - with_id,
        injects.len(),
    );
}

#[test]
fn zerg_units_show_hatch_as_producer_not_larva() {
    // Para unidades Zerg morfadas a partir de Larva (Drone, Zergling,
    // Overlord, etc.), o `producer_type` deve ser a Hatchery/Lair/Hive
    // de origem, não "Larva" — o engine populamos `creator_unit_tag_*`
    // de cada Larva com a base que a gerou, então conseguimos saltar
    // a Larva no display.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/serral.SC2Replay");
    let t = parse_replay(&path, 0).expect("parse");
    let bo = extract_build_order(&t).expect("bo");

    let zerg = bo
        .players
        .iter()
        .find(|p| p.race == "Zerg")
        .expect("zerg player");

    let larva_morphs: Vec<&BuildOrderEntry> = zerg
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.action.as_str(),
                "Drone" | "Zergling" | "Overlord" | "Roach" | "Hydralisk" | "Mutalisk"
            )
        })
        .collect();

    assert!(
        !larva_morphs.is_empty(),
        "esperava ao menos uma unidade Zerg morfada de Larva no replay",
    );

    // Nenhuma entry deve mostrar "Larva" como producer_type — todas
    // devem apontar pra Hatchery/Lair/Hive.
    let leaked: Vec<&BuildOrderEntry> = larva_morphs
        .iter()
        .copied()
        .filter(|e| e.producer_type.as_deref() == Some("Larva"))
        .collect();
    assert!(
        leaked.is_empty(),
        "{} entrada(s) de unidade Zerg ainda mostram 'Larva' como producer (hop falhou): {:?}",
        leaked.len(),
        leaked
            .iter()
            .take(3)
            .map(|e| (e.action.clone(), e.producer_id))
            .collect::<Vec<_>>(),
    );

    // E pelo menos uma deve apontar pra Hatchery/Lair/Hive.
    let hatched: usize = larva_morphs
        .iter()
        .filter(|e| {
            matches!(
                e.producer_type.as_deref(),
                Some("Hatchery") | Some("Lair") | Some("Hive")
            )
        })
        .count();
    assert!(
        hatched > 0,
        "esperava ao menos uma unidade Zerg com producer Hatchery/Lair/Hive; viram 0 de {}",
        larva_morphs.len(),
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
