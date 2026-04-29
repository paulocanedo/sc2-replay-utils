//! Lógica específica da raça Terran. Cobre três modos de resolução de
//! parent (estrutura-mãe) de um addon, usados em cascata:
//!
//! 1. **Offset exato (`resolve_addon_parent_by_exact_offset`)** —
//!    primary. Procura uma lane do tipo certo cuja posição bate
//!    EXATAMENTE `(addon.x - 3, addon.y)`. O offset `(+3, 0)` é o
//!    encaixe físico canônico do jogo (verificado empiricamente em
//!    todos os pares parent×addon Terran). Quando todas as posições
//!    estão atualizadas (i.e. lift/land foi rastreado), esta função
//!    discrimina perfeitamente — cada addon tem exatamente uma lane no
//!    offset canônico e só uma. Retorna `None` se nenhum candidato
//!    bate o offset exato (típico em janelas com lift/land ainda não
//!    capturado, ou em geometrias atípicas raras).
//!
//! 2. **Cmd matching (`resolve_addon_parent_via_cmd`)** — fallback
//!    secundário. Usa `producer_tags` do `production_cmds` quando o
//!    player emitiu Build_*Reactor/Build_*TechLab. Útil para os casos
//!    em que (1) falhou. **Importante**: cmd não é primary porque
//!    quando o jogador tem control group com várias Barracks/etc., o
//!    SC2 despacha o build para múltiplas estruturas mas registra
//!    apenas UM `producer_tag` (a primeira da seleção). Confiar no
//!    cmd nesse cenário ataca o addon errado — ver
//!    `army_terran_two_addons_built_simultaneously_via_control_group`
//!    nos testes.
//!
//! 3. **Proximidade pura (`resolve_addon_parent_by_proximity`)** —
//!    last resort. Lane mais próxima por `d²`. Usada apenas quando os
//!    dois caminhos acima falharam. Reproduz o comportamento legacy
//!    como rede de segurança.
//!
//! A produção paralela 2x via Reactor não é mais modelada — qualquer
//! produção de uma estrutura Terran vira uma única lane simples. Veja
//! o histórico do módulo se precisar reintroduzir slots paralelos.

use std::collections::HashMap;

use crate::replay::ProductionCmd;

use super::types::StructureLane;

/// Janela em game_loops em torno do `UnitInit` do addon onde aceitamos
/// um cmd como pareado. Cmds emitidos depois do init não fazem sentido
/// (player não pode emitir Build_Reactor *após* o reactor já ter
/// começado a ser construído), mas mantemos folga simétrica de ±50
/// pra absorver reordenamentos esquisitos do tracker.
const PAIR_WINDOW: i64 = 50;

/// Offset canônico do addon relativo à estrutura-mãe, em células u8.
/// Confirmado empiricamente como (+3, 0) para todos os pares
/// parent×addon Terran (Barracks, Factory, Starport × Reactor,
/// TechLab). Variações observadas em replays reais são consequência
/// de relocates não-rastreados (cobertos pela lógica de lift/land em
/// `player.rs`), não do offset físico do jogo.
const EXPECTED_DX: i32 = 3;
const EXPECTED_DY: i32 = 0;

/// Tipo canônico da estrutura-mãe esperada para cada addon.
pub(super) fn addon_parent_canonical(addon: &str) -> Option<&'static str> {
    match addon {
        "BarracksReactor" | "BarracksTechLab" => Some("Barracks"),
        "FactoryReactor" | "FactoryTechLab" => Some("Factory"),
        "StarportReactor" | "StarportTechLab" => Some("Starport"),
        _ => None,
    }
}

/// Resolve o parent de um addon via `production_cmds`. Procura um cmd
/// com `ability == addon` (o `resolve_ability_command` do parser usa
/// o nome literal do addon como ability, ex. `"BarracksReactor"`)
/// dentro de `PAIR_WINDOW` loops do init, com `producer_tags` apontando
/// pra uma lane existente no momento. Marca o cmd como consumido em
/// `consumed` para que o próximo addon não case com o mesmo cmd.
///
/// Retorna `None` quando: nenhum cmd casa, todos já foram consumidos,
/// ou o `producer_tag` não bate com nenhuma lane viva (cmd órfão).
pub(super) fn resolve_addon_parent_via_cmd(
    cmds: &[ProductionCmd],
    consumed: &mut [bool],
    addon: &str,
    addon_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
) -> Option<i64> {
    let parent_type = addon_parent_canonical(addon)?;
    let mut best: Option<(usize, i64, i64)> = None; // (idx, abs_gap, parent_tag)

    for (i, cmd) in cmds.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        if cmd.ability != addon {
            continue;
        }
        let gap = (cmd.game_loop as i64) - (addon_loop as i64);
        if gap.abs() > PAIR_WINDOW {
            continue;
        }
        let Some(&tag) = cmd.producer_tags.first() else {
            continue;
        };
        // Validação de coerência: o producer_tag tem que existir como
        // lane do tipo correto. Sem essa checagem, um cmd órfão (com
        // tag de uma estrutura que já morreu, ou de outra raça) seria
        // aceito e o `pending_addon` ficaria inválido.
        let Some(lane) = lanes.get(&tag) else { continue };
        if lane.canonical_type != parent_type {
            continue;
        }
        let abs_gap = gap.abs();
        match best {
            Some((_, prev_gap, _)) if prev_gap <= abs_gap => {}
            _ => best = Some((i, abs_gap, tag)),
        }
    }

    if let Some((i, _, tag)) = best {
        consumed[i] = true;
        return Some(tag);
    }
    None
}

/// Procura a lane viva do tipo certo cuja posição bate **exatamente**
/// `(addon.x - 3, addon.y)`. Retorna `None` se nenhum candidato
/// satisfaz. Não cai em fallback por proximidade — quando esta função
/// retorna `None`, a cascata em `player.rs` tenta o cmd matching antes
/// de cair na lane mais próxima.
pub(super) fn resolve_addon_parent_by_exact_offset(
    addon: &str,
    addon_x: u8,
    addon_y: u8,
    at_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
) -> Option<i64> {
    let parent_type = addon_parent_canonical(addon)?;
    let expected_x = addon_x as i32 - EXPECTED_DX;
    let expected_y = addon_y as i32 - EXPECTED_DY;

    for lane in lanes.values() {
        if lane.canonical_type != parent_type {
            continue;
        }
        if lane.born_loop > at_loop {
            continue;
        }
        if lane.died_loop.map(|d| d <= at_loop).unwrap_or(false) {
            continue;
        }
        if lane.pos_x as i32 == expected_x && lane.pos_y as i32 == expected_y {
            return Some(lane.tag);
        }
    }
    None
}

/// Lane mais próxima por `d²` entre os candidatos vivos do tipo
/// correto. Last-resort da cascata — reproduz o heurístico legacy só
/// quando offset exato e cmd matching falharam. Sem garantia de
/// correção em cenários de control-group / addons construídos
/// simultaneamente.
pub(super) fn resolve_addon_parent_by_proximity(
    addon: &str,
    addon_x: u8,
    addon_y: u8,
    at_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
) -> Option<i64> {
    let parent_type = addon_parent_canonical(addon)?;
    let mut best: Option<(i64, i32)> = None;
    for lane in lanes.values() {
        if lane.canonical_type != parent_type {
            continue;
        }
        if lane.born_loop > at_loop {
            continue;
        }
        if lane.died_loop.map(|d| d <= at_loop).unwrap_or(false) {
            continue;
        }
        let dx = lane.pos_x as i32 - addon_x as i32;
        let dy = lane.pos_y as i32 - addon_y as i32;
        let d2 = dx * dx + dy * dy;
        if best.map(|(_, b)| d2 < b).unwrap_or(true) {
            best = Some((lane.tag, d2));
        }
    }
    best.map(|(tag, _)| tag)
}
