// Gera `$OUT_DIR/balance_data_generated.rs` com duas tabelas extraídas
// das JSONs de BalanceData embutidas pelo crate s2protocol:
//
// 1. `BALANCE_ENTRIES: &[(protocol_version, action_name, build_loops)]`
//    — usada por `build_time_loops` para o cálculo de build order.
// 2. `ABILITY_ENTRIES: &[(protocol_version, producer, ability_id,
//    command_index, action_name)]` — usada por `resolve_ability_command`
//    no parser de game events para descobrir, dado um Cmd
//    `(ability_id, cmd_index)` emitido por um produtor (ex.: Barracks),
//    qual ação foi disparada (ex.: "Marine"). Necessário porque o
//    `read_game_events` do s2protocol devolve `m_abil.ability` vazio em
//    versões byte-aligned — só o `SC2EventIterator` enriquece, e a gente
//    não usa o iterator do s2protocol.
//
// Por que aqui e não em runtime: o s2protocol expõe
// `read_balance_data_from_included_assets()`, mas ela tem um bug em
// Windows — o crate `include_assets` que ele usa internamente codifica
// nomes com `\`, mas o parser do s2protocol só faz `split('/')`. O
// resultado é uma lista vazia em qualquer build Windows. Em vez de
// monkey-patch, lemos os mesmos arquivos JSON diretamente do source
// dir do s2protocol no `.cargo/registry`, no momento do build. Sai
// `cargo update` com versão nova → tabela atualizada automaticamente
// no próximo build.
//
// Os tempos no JSON estão em segundos de Normal speed (a unidade
// canônica que a Blizzard usa). Convertemos para game loops com
// `× 16` (constante da engine — independente da game_speed).

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;
use serde_json::Value;

/// Uma entrada da tabela de abilities. A chave é
/// `(version, producer, ability_id, command_index)`; o valor é o
/// `action_id` (ex.: "Marine", "Stimpack", "TerranInfantryWeaponsLevel1").
type AbilityKey = (u32, String, u16, i64);

const LOOPS_PER_GAME_SECOND: f32 = 16.0;

fn main() {
    let metadata = MetadataCommand::new()
        .exec()
        .expect("cargo metadata deveria funcionar — necessário para localizar s2protocol");

    let s2 = metadata
        .packages
        .iter()
        .find(|p| p.name.as_str() == "s2protocol")
        .expect("dependência s2protocol não encontrada no metadata");

    let s2_root: PathBuf = s2
        .manifest_path
        .parent()
        .expect("manifest_path do s2protocol deveria ter parent")
        .to_owned()
        .into();

    let balance_dir = s2_root.join("assets").join("BalanceData");
    assert!(
        balance_dir.is_dir(),
        "esperava encontrar BalanceData em {}",
        balance_dir.display()
    );

    // (versão, nome) → loops. BTreeMap dá saída determinística (e
    // diff-friendly) no arquivo gerado.
    let mut entries: BTreeMap<(u32, String), u32> = BTreeMap::new();
    // (versão, producer, ability_id, command_index) → action_id.
    let mut abilities: BTreeMap<AbilityKey, String> = BTreeMap::new();

    for version_entry in fs::read_dir(&balance_dir).expect("read_dir BalanceData") {
        let version_entry = version_entry.expect("entry");
        let version_path = version_entry.path();
        if !version_path.is_dir() {
            continue;
        }
        let Some(version_name) = version_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(version) = version_name.parse::<u32>() else {
            continue;
        };

        for unit_entry in fs::read_dir(&version_path).expect("read_dir version") {
            let unit_entry = unit_entry.expect("entry");
            let unit_path = unit_entry.path();
            if unit_path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read_to_string(&unit_path).expect("ler JSON");
            let json: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => panic!("falha ao parsear {}: {e}", unit_path.display()),
            };
            collect_unit(&json, version, &mut entries);
            collect_abilities(&json, version, &mut abilities);
        }
    }

    assert!(
        !entries.is_empty(),
        "nenhuma entrada de BalanceData extraída — algo está errado"
    );
    assert!(
        !abilities.is_empty(),
        "nenhuma entrada de abilities extraída — algo está errado"
    );

    write_generated(&entries, &abilities);

    // Re-roda o build script se a árvore de balance data mudar (cargo
    // update do s2protocol troca o source dir, e o cargo já invalida
    // por isso, mas marcar explicitamente é barato).
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", balance_dir.display());
}

/// Extrai os 3 grupos de tempos relevantes de um JSON de unit/struct:
/// 1. `cost.@time` — tempo da própria unidade/estrutura.
/// 2. `researches.upgrade[].cost.@time` — pesquisas one-shot iniciadas
///    a partir desta estrutura (Stimpack/ShieldWall/PunisherGrenades
///    em BarracksTechLab, etc).
/// 3. `upgrades.upgrade[].level[].cost.@time` — upgrades com níveis
///    (Weapons/Armor 1/2/3, Shields, etc). O `@id` do level já vem
///    com o sufixo `Level1/2/3` que casa com o evento do tracker.
fn collect_unit(json: &Value, version: u32, out: &mut BTreeMap<(u32, String), u32>) {
    let Some(id) = json.get("@id").and_then(Value::as_str) else {
        return;
    };

    if let Some(time) = json
        .get("cost")
        .and_then(|c| c.get("@time"))
        .and_then(Value::as_f64)
    {
        if time > 0.0 {
            insert(out, version, id.to_string(), seconds_to_loops(time as f32));
        }
    }

    if let Some(upgrades) = json
        .get("researches")
        .and_then(|r| r.get("upgrade"))
        .and_then(Value::as_array)
    {
        for u in upgrades {
            let Some(uid) = u.get("@id").and_then(Value::as_str) else {
                continue;
            };
            if uid.is_empty() {
                continue;
            }
            let Some(time) = u
                .get("cost")
                .and_then(|c| c.get("@time"))
                .and_then(Value::as_f64)
            else {
                continue;
            };
            if time <= 0.0 {
                continue;
            }
            insert(out, version, uid.to_string(), seconds_to_loops(time as f32));
        }
    }

    if let Some(upgrades) = json
        .get("upgrades")
        .and_then(|u| u.get("upgrade"))
        .and_then(Value::as_array)
    {
        for upg in upgrades {
            let Some(levels) = upg.get("level").and_then(Value::as_array) else {
                continue;
            };
            for level in levels {
                let Some(lid) = level.get("@id").and_then(Value::as_str) else {
                    continue;
                };
                if lid.is_empty() {
                    continue;
                }
                let Some(time) = level
                    .get("cost")
                    .and_then(|c| c.get("@time"))
                    .and_then(Value::as_f64)
                else {
                    continue;
                };
                if time <= 0.0 {
                    continue;
                }
                insert(out, version, lid.to_string(), seconds_to_loops(time as f32));
            }
        }
    }
}

/// `or_insert`: se duas estruturas listarem o mesmo upgrade na mesma
/// versão (raro mas possível com pesquisas compartilhadas), preserva
/// a primeira leitura. Os valores devem coincidir no balance data.
fn insert(map: &mut BTreeMap<(u32, String), u32>, version: u32, name: String, loops: u32) {
    map.entry((version, name)).or_insert(loops);
}

/// Walks `trains.unit[]`, `builds.unit[]` e `researches.upgrade[]` de
/// um JSON de produtor (Barracks, Gateway, Forge, etc.) e popula a
/// tabela `(version, producer, ability_id, cmd_index) → action_id`.
///
/// É essa tabela que permite ao parser de game events traduzir um Cmd
/// `(m_abil_link, m_abil_cmd_index)` emitido por um produtor na seleção
/// ativa para o nome canônico da ação ("Marine", "Stimpack", etc.) que
/// o build_order pode casar contra o `entity_type` do EntityEvent.
fn collect_abilities(json: &Value, version: u32, out: &mut BTreeMap<AbilityKey, String>) {
    let Some(producer) = json.get("@id").and_then(Value::as_str) else {
        return;
    };

    for section in ["trains", "builds"] {
        if let Some(units) = json
            .get(section)
            .and_then(|s| s.get("unit"))
            .and_then(Value::as_array)
        {
            for u in units {
                let Some(action_id) = u.get("@id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(ability_id) = u.get("@ability").and_then(Value::as_u64) else {
                    continue;
                };
                let cmd_index = u.get("@index").and_then(Value::as_i64).unwrap_or(0);
                if action_id.is_empty() {
                    continue;
                }
                out.entry((version, producer.to_string(), ability_id as u16, cmd_index))
                    .or_insert_with(|| action_id.to_string());
            }
        }
    }

    if let Some(researches) = json
        .get("researches")
        .and_then(|r| r.get("upgrade"))
        .and_then(Value::as_array)
    {
        for r in researches {
            let Some(action_id) = r.get("@id").and_then(Value::as_str) else {
                continue;
            };
            let Some(ability_id) = r.get("@ability").and_then(Value::as_u64) else {
                continue;
            };
            let cmd_index = r.get("@index").and_then(Value::as_i64).unwrap_or(0);
            if action_id.is_empty() {
                continue;
            }
            out.entry((version, producer.to_string(), ability_id as u16, cmd_index))
                .or_insert_with(|| action_id.to_string());
        }
    }
}

fn seconds_to_loops(seconds_normal_speed: f32) -> u32 {
    (seconds_normal_speed * LOOPS_PER_GAME_SECOND).round() as u32
}

fn write_generated(
    entries: &BTreeMap<(u32, String), u32>,
    abilities: &BTreeMap<AbilityKey, String>,
) {
    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR não definido");
    let out_path = Path::new(&out_dir).join("balance_data_generated.rs");

    let mut s = String::new();
    s.push_str("// @generated por build.rs — não editar à mão.\n");
    s.push_str("// Cada tupla é (protocol_version, action_name, build_time_loops).\n");
    s.push_str("pub static BALANCE_ENTRIES: &[(u32, &str, u32)] = &[\n");
    for ((version, name), loops) in entries {
        // `name` vem do JSON de balance data — IDs do SC2 não contêm
        // aspas duplas nem barras invertidas, então o `format!` simples
        // é seguro. Defendendo só por garantia futura:
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        s.push_str(&format!("    ({version}, \"{escaped}\", {loops}),\n"));
    }
    s.push_str("];\n\n");

    s.push_str(
        "// Cada tupla é (protocol_version, producer, ability_id, command_index, action_name).\n",
    );
    s.push_str("pub static ABILITY_ENTRIES: &[(u32, &str, u16, i64, &str)] = &[\n");
    for ((version, producer, ability_id, cmd_index), action) in abilities {
        let producer_esc = producer.replace('\\', "\\\\").replace('"', "\\\"");
        let action_esc = action.replace('\\', "\\\\").replace('"', "\\\"");
        s.push_str(&format!(
            "    ({version}, \"{producer_esc}\", {ability_id}, {cmd_index}, \"{action_esc}\"),\n",
        ));
    }
    s.push_str("];\n");

    fs::write(&out_path, s).expect("escrever balance_data_generated.rs");
}
