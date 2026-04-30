// Tempos de build/research extraídos da BalanceData oficial do SC2 que
// o crate `s2protocol` embute em seus assets compilados.
//
// Substitui a antiga tabela hardcoded `build_times.rs`. Em vez de
// manter manualmente a duração de cada unidade, estrutura e pesquisa,
// lemos os arquivos JSON `assets/BalanceData/<protocol_version>/<id>.json`
// que vêm bundlados no s2protocol — mas via um `build.rs` que extrai
// `(versão, nome, loops)` em tempo de compilação para um array
// `BALANCE_ENTRIES` gerado em `OUT_DIR/balance_data_generated.rs`.
//
// Por que não chamar `read_balance_data_from_included_assets()` do
// próprio s2protocol em runtime: ela tem um bug em Windows. O crate
// `include_assets` codifica nomes de arquivo com `\` no separador,
// mas o parser do s2protocol só faz `split('/')` — então toda chamada
// retorna vazio em Windows. O build script lê os mesmos arquivos
// direto do source dir do s2protocol no `.cargo/registry`, evitando
// esse caminho.
//
// Os tempos no JSON estão em segundos de Normal speed (a unidade
// canônica que a Blizzard usa internamente). O build script já
// converteu para game loops com `× 16`, independente da `game_speed`
// do replay: o número de game loops por segundo de jogo é constante;
// `game_speed` só altera quantos loops passam por segundo de tempo
// real.
//
// O lookup escolhe a versão de balance data mais próxima (≤) do
// `m_base_build` do replay; replays mais novos que a maior versão
// embutida caem na maior disponível, e replays mais antigos que a
// menor caem na menor — fallback "best effort". Quando o nome da ação
// não existe em nenhuma versão, retornamos 0 e o consumer (build
// order) preserva o instante de conclusão como fallback seguro.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/balance_data_generated.rs"));

/// Tabela `nome_ação → tempo em game loops` para uma única versão de
/// protocolo. As chaves são os `id` brutos que aparecem no replay
/// (ex.: `"Marine"`, `"BarracksTechLab"`, `"Stimpack"`,
/// `"TerranInfantryWeaponsLevel1"`).
type Table = HashMap<&'static str, u32>;

/// Tabela `producer → (ability_id, command_index) → action_id` para
/// uma única versão de protocolo. Usada pelo parser de game events
/// para resolver `m_abil_link`/`m_abil_cmd_index` em nomes de ação
/// canônicos (ex.: `Barracks[(161, 0)] → "Marine"`). É aninhada por
/// producer para permitir lookup com `&str` em vez de exigir
/// `&'static str` na chave.
type AbilityTable = HashMap<&'static str, HashMap<(u16, i64), &'static str>>;

/// Tabela `nome_unidade → supply_cost × 10` para uma única versão.
type SupplyTable = HashMap<&'static str, u32>;

/// Tabela `nome_unidade → (minerals, vespene)` para uma única versão.
type CostTable = HashMap<&'static str, (u32, u32)>;

/// Tabela `nome_unidade → sight_radius × 100` para uma única versão.
type SightTable = HashMap<&'static str, u32>;

/// Tabela `nome_estrutura → (width_tiles, height_tiles)` para uma única versão.
type FootprintTable = HashMap<&'static str, (u8, u8)>;

struct Balance {
    /// `BTreeMap` para permitir busca pelo maior `version ≤ alvo` em
    /// O(log n).
    versions: BTreeMap<u32, Table>,
    abilities: BTreeMap<u32, AbilityTable>,
    supply: BTreeMap<u32, SupplyTable>,
    cost: BTreeMap<u32, CostTable>,
    sight: BTreeMap<u32, SightTable>,
    footprint: BTreeMap<u32, FootprintTable>,
}

static GLOBAL: OnceLock<Balance> = OnceLock::new();

fn load() -> &'static Balance {
    GLOBAL.get_or_init(|| {
        let mut versions: BTreeMap<u32, Table> = BTreeMap::new();
        for &(version, name, loops) in BALANCE_ENTRIES {
            versions.entry(version).or_default().insert(name, loops);
        }
        let mut abilities: BTreeMap<u32, AbilityTable> = BTreeMap::new();
        for &(version, producer, ability_id, cmd_index, action) in ABILITY_ENTRIES {
            abilities
                .entry(version)
                .or_default()
                .entry(producer)
                .or_default()
                .insert((ability_id, cmd_index), action);
        }
        let mut supply: BTreeMap<u32, SupplyTable> = BTreeMap::new();
        for &(version, name, cost_x10) in SUPPLY_ENTRIES {
            supply.entry(version).or_default().insert(name, cost_x10);
        }
        let mut cost: BTreeMap<u32, CostTable> = BTreeMap::new();
        for &(version, name, minerals, vespene) in COST_ENTRIES {
            cost.entry(version)
                .or_default()
                .insert(name, (minerals, vespene));
        }
        let mut sight: BTreeMap<u32, SightTable> = BTreeMap::new();
        for &(version, name, sight_x100) in SIGHT_RADIUS_ENTRIES {
            sight.entry(version).or_default().insert(name, sight_x100);
        }
        let mut footprint: BTreeMap<u32, FootprintTable> = BTreeMap::new();
        for &(version, name, w, h) in FOOTPRINT_ENTRIES {
            footprint.entry(version).or_default().insert(name, (w, h));
        }
        Balance {
            versions,
            abilities,
            supply,
            cost,
            sight,
            footprint,
        }
    })
}

/// Escolhe a tabela mais apropriada para o `base_build` do replay:
/// preferimos a maior versão `≤ base_build`; se nenhuma for `≤`
/// (replay anterior à menor versão embutida), usamos a menor; se o
/// mapa estiver vazio (impossível em prática), `None`.
fn pick_table(b: &Balance, base_build: u32) -> Option<&Table> {
    b.versions
        .range(..=base_build)
        .next_back()
        .or_else(|| b.versions.iter().next())
        .map(|(_, t)| t)
}

fn pick_ability_table(b: &Balance, base_build: u32) -> Option<&AbilityTable> {
    b.abilities
        .range(..=base_build)
        .next_back()
        .or_else(|| b.abilities.iter().next())
        .map(|(_, t)| t)
}

fn pick_supply_table(b: &Balance, base_build: u32) -> Option<&SupplyTable> {
    b.supply
        .range(..=base_build)
        .next_back()
        .or_else(|| b.supply.iter().next())
        .map(|(_, t)| t)
}

fn pick_cost_table(b: &Balance, base_build: u32) -> Option<&CostTable> {
    b.cost
        .range(..=base_build)
        .next_back()
        .or_else(|| b.cost.iter().next())
        .map(|(_, t)| t)
}

fn pick_sight_table(b: &Balance, base_build: u32) -> Option<&SightTable> {
    b.sight
        .range(..=base_build)
        .next_back()
        .or_else(|| b.sight.iter().next())
        .map(|(_, t)| t)
}

fn pick_footprint_table(b: &Balance, base_build: u32) -> Option<&FootprintTable> {
    b.footprint
        .range(..=base_build)
        .next_back()
        .or_else(|| b.footprint.iter().next())
        .map(|(_, t)| t)
}

/// Custo de supply de `name` em décimos (ex.: Marine = 10, Zergling = 5).
/// Retorna `0` quando desconhecido.
pub fn supply_cost_x10(name: &str, base_build: u32) -> u32 {
    let b = load();
    let Some(table) = pick_supply_table(b, base_build) else { return 0 };
    table.get(name).copied().unwrap_or(0)
}

/// Custo em recursos (minerals, vespene) de `name`. Retorna `(0, 0)`
/// quando desconhecido. Espelha o mesmo esquema de fallback de versão
/// usado pelos outros lookups de balance data.
pub fn resource_cost(name: &str, base_build: u32) -> (u32, u32) {
    let b = load();
    let Some(table) = pick_cost_table(b, base_build) else { return (0, 0) };
    table.get(name).copied().unwrap_or((0, 0))
}

/// Raio de visão (sight radius) de `name` em tiles do jogo. Retorna
/// `None` quando o nome não está presente na balance data — o consumer
/// (overlay de FOG no Timeline) decide o fallback.
pub fn sight_radius(name: &str, base_build: u32) -> Option<f32> {
    let b = load();
    let table = pick_sight_table(b, base_build)?;
    table.get(name).copied().map(|x100| x100 as f32 / 100.0)
}

/// Footprint de `name` em tiles `(width, height)`. Só estruturas têm
/// entrada — unidades mobile retornam `None`. O minimap usa isso para
/// desenhar prédios no tamanho real (5x5 para townhalls, 2x2 para
/// Pylon/SupplyDepot, 3x3 para Gateway/Barracks/Forge, 6x5 para
/// Hatchery, etc.) em vez do quadrado fixo de 9/18 px.
pub fn footprint(name: &str, base_build: u32) -> Option<(u8, u8)> {
    let b = load();
    let table = pick_footprint_table(b, base_build)?;
    table.get(name).copied()
}

/// Tempo de build/research/upgrade para `name` em **game loops**,
/// resolvido contra o `base_build` (protocol version) do replay.
///
/// Retorna `0` quando o nome não está presente em nenhuma versão de
/// balance data — sinaliza ao consumer que ele deve preservar o
/// instante de conclusão original.
pub fn build_time_loops(name: &str, base_build: u32) -> u32 {
    let b = load();
    let Some(table) = pick_table(b, base_build) else { return 0 };
    table.get(name).copied().unwrap_or(0)
}

/// Resolve um Cmd `(producer, ability_id, command_index)` no nome
/// canônico da ação que ele dispara — exatamente o `@id` que aparece
/// nos eventos do tracker (ex.: `"Marine"`, `"Stimpack"`,
/// `"TerranInfantryWeaponsLevel1"`).
///
/// Retorna `None` quando a combinação não corresponde a uma produção
/// conhecida (cmds que não são train/build/research, ou unidades que
/// não estão no balance data). O parser usa `None` como sinal de
/// "ignorar este cmd" — o build_order vai cair no fallback antigo.
pub fn resolve_ability_command(
    producer: &str,
    ability_id: u16,
    command_index: i64,
    base_build: u32,
) -> Option<&'static str> {
    let b = load();
    let table = pick_ability_table(b, base_build)?;
    table
        .get(producer)
        .and_then(|inner| inner.get(&(ability_id, command_index)).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanidade: a maior versão embutida tem que conhecer um conjunto
    /// mínimo de ações canônicas. Se o build.rs ou o crate s2protocol
    /// mudarem o formato, esse teste explode antes do build_order
    /// silenciosamente passar a usar `0` em tudo.
    #[test]
    fn known_actions_resolve_to_nonzero_loops() {
        let b = load();
        let max_build = *b.versions.keys().next_back().expect("at least one version");
        for action in [
            "SCV",
            "Marine",
            "BarracksTechLab",
            "Stimpack",
            "TerranInfantryWeaponsLevel1",
        ] {
            let loops = build_time_loops(action, max_build);
            assert!(
                loops > 0,
                "esperava build_time_loops({action}) > 0 na versão {max_build}",
            );
        }
    }

    /// Para um `base_build` muito alto, devemos cair na maior versão
    /// embutida (em vez de retornar 0 / panicar).
    #[test]
    fn future_base_build_falls_back_to_highest() {
        let scv = build_time_loops("SCV", u32::MAX);
        assert!(scv > 0, "SCV deveria resolver mesmo em base_build futurista");
    }

    /// Sanity check: footprints conhecidos devem casar com o que aparece
    /// in-game. 5x5 para townhalls (Nexus/CC/Hatchery), 3x3 para produção
    /// padrão (Gateway/Barracks), 2x2 para Pylon/SupplyDepot. Se o
    /// `parse_footprint_dims` do build.rs quebrar, esse teste alerta
    /// antes do minimap silenciosamente cair no tamanho fallback.
    #[test]
    fn known_footprints_are_present() {
        let b = load();
        let max_build = *b.footprint.keys().next_back().expect("at least one footprint version");
        assert_eq!(footprint("Nexus", max_build), Some((5, 5)));
        assert_eq!(footprint("CommandCenter", max_build), Some((5, 5)));
        assert_eq!(footprint("Pylon", max_build), Some((2, 2)));
        assert_eq!(footprint("SupplyDepot", max_build), Some((2, 2)));
        assert_eq!(footprint("Gateway", max_build), Some((3, 3)));
        assert_eq!(footprint("Barracks", max_build), Some((3, 3)));
        // Unidades mobile não têm footprint.
        assert_eq!(footprint("Marine", max_build), None);
        assert_eq!(footprint("Probe", max_build), None);
    }

    /// Sanity check: sightRadius do Marine deve estar em torno de 9 tiles
    /// e do Probe em torno de 8 tiles. Se a extração do build.rs quebrar,
    /// esse teste alerta antes do overlay de FOG silenciosamente cair no
    /// fallback de 8.0 para tudo.
    #[test]
    fn known_sight_radii_are_present() {
        let b = load();
        let max_build = *b.sight.keys().next_back().expect("at least one sight version");
        let marine = sight_radius("Marine", max_build).expect("Marine sight");
        assert!(
            (8.0..=10.0).contains(&marine),
            "Marine sight radius esperado ~9, obtido {marine}",
        );
        let probe = sight_radius("Probe", max_build).expect("Probe sight");
        assert!(
            (7.0..=9.0).contains(&probe),
            "Probe sight radius esperado ~8, obtido {probe}",
        );
    }
}
