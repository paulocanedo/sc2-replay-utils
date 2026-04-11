// Extrator de build order — agora é uma camada pura sobre `ReplayTimeline`.
//
// Não abre o MPQ nem decodifica eventos: consome `entity_events` e
// `upgrades` que o parser single-pass já produziu, mapeando cada um
// para `BuildOrderEntry` na semântica esperada pelos consumers
// (CSV, GUI, image renderer).
//
// Cada entrada armazena o `game_loop` no instante de **início** da
// ação, não de conclusão. Para upgrades, unidades e morphs (que vêm
// do parser com o loop de conclusão) subtraímos o `build_time_loops`
// da ação. Estruturas vindas de `UnitInit` já são start-time e ficam
// como estão.

use crate::balance_data::build_time_loops;
use crate::replay::{
    EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline, UNIT_INIT_MARKER,
};

// ── Structs de saída ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct BuildOrderEntry {
    /// Supply usado no instante de início.
    pub supply: u8,
    /// Capacidade total de supply no instante de início (food_made).
    pub supply_made: u8,
    /// Instante de início da ação (start time).
    pub game_loop: u32,
    /// Instante de conclusão da ação (finish time). Igual ao
    /// `game_loop` quando o tempo de build não é conhecido.
    pub finish_loop: u32,
    /// Sequência global vinda do parser, usada como tiebreaker entre
    /// `entity_events` e `upgrades` no mesmo `game_loop`. Não é
    /// exposto no CSV.
    pub seq: u32,
    pub action: String,
    pub count: u32,
    pub is_upgrade: bool,
    pub is_structure: bool,
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
        // Os demais (trains, morphs, upgrades) vêm com o loop de
        // conclusão e precisam do recuo para o instante de início.
        let raw_loop = ev.game_loop;
        let (start_loop, finish_loop) = if from_unit_init {
            (raw_loop, add_build_time(raw_loop, &ev.entity_type, base_build))
        } else {
            (
                subtract_build_time(raw_loop, &ev.entity_type, base_build),
                raw_loop,
            )
        };

        // Supply (used + made) é amostrado no instante de início — é o
        // que o jogador tinha quando emitiu o comando.
        let (supply, supply_made) = supply_at(player, start_loop);

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
        });
    }

    // Upgrades — Sprays já filtrados pelo parser. O `game_loop` cru é
    // de conclusão; recuamos para o início para casar com a semântica
    // do build order.
    for u in &player.upgrades {
        if u.game_loop == 0 {
            continue;
        }
        let finish_loop = u.game_loop;
        let start_loop = subtract_build_time(finish_loop, &u.name, base_build);
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
        });
    }

    // Sort por (game_loop, seq) — agora `game_loop` é o instante de
    // início, então a ordem cronológica é preservada na display.
    raw.sort_by_key(|e| (e.game_loop, e.seq));

    deduplicate(raw)
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

/// Funde entradas consecutivas com a mesma ação em uma única com `count` incrementado.
fn deduplicate(entries: Vec<BuildOrderEntry>) -> Vec<BuildOrderEntry> {
    let mut out: Vec<BuildOrderEntry> = Vec::new();
    for entry in entries {
        match out.last_mut() {
            Some(last) if last.action == entry.action => last.count += 1,
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
    /// facilitar correção manual.
    fn render_golden_csv(player: &PlayerBuildOrder, lps: f64) -> String {
        let mut out = String::new();
        out.push_str("# old_republic_50.SC2Replay — build order golden\n");
        out.push_str(&format!(
            "# player: {} ({}) mmr={}\n",
            player.name,
            player.race,
            player.mmr.map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
        ));
        out.push_str("# columns: start,finish,supply_used,supply_made,kind,action,count\n");
        for entry in &player.entries {
            let kind = classify_entry(entry).short_letter();
            out.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                format_time(entry.game_loop, lps),
                format_time(entry.finish_loop, lps),
                entry.supply,
                entry.supply_made,
                kind,
                entry.action,
                entry.count,
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
