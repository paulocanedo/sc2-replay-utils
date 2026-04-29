//! Lógica específica da raça Terran. Cobre:
//!
//! - Resolução do parent (estrutura-mãe) de um addon via cmd matching
//!   (`resolve_addon_parent_via_cmd`) — primary, usa `producer_tags`
//!   do `production_cmds` quando o player emitiu Build_*Reactor/
//!   Build_*TechLab. É determinístico quando funciona (cobertura típica
//!   ~80% das construções reais).
//!
//! - Resolução do parent por geometria com offset esperado
//!   (`resolve_addon_parent_by_offset`) — fallback, usado quando o cmd
//!   não está disponível. Prefere o parent no offset canônico (+3, 0)
//!   relativo ao addon (verificado empiricamente em todos os pares
//!   parent×addon Terran). Cai em proximidade pura por `d²` apenas
//!   quando nenhum candidato bate o offset exato.
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

/// Resolve o parent de um addon por geometria. Estratégia em duas
/// camadas:
///
/// 1. **Offset canônico**: prefere a lane viva do tipo certo cuja
///    posição bate exatamente `(addon.x - 3, addon.y)`. Quando duas
///    Barracks estão adjacentes (caso comum em wall-offs), só uma
///    delas tem o addon no offset correto — essa camada discrimina
///    perfeitamente.
///
/// 2. **Proximidade**: se nenhuma lane bate o offset exato (typically:
///    desfase causado por relocate ainda não capturado, ou por addon
///    construído em geometria atípica), cai na lane mais próxima por
///    `d²`. Comportamento equivalente ao heurístico legacy, preservado
///    como rede de segurança.
pub(super) fn resolve_addon_parent_by_offset(
    addon: &str,
    addon_x: u8,
    addon_y: u8,
    at_loop: u32,
    lanes: &HashMap<i64, StructureLane>,
) -> Option<i64> {
    let parent_type = addon_parent_canonical(addon)?;
    let expected_x = addon_x as i32 - EXPECTED_DX;
    let expected_y = addon_y as i32 - EXPECTED_DY;

    // Primary: parent no offset exato (+3, 0).
    let mut exact: Option<i64> = None;
    let mut nearest: Option<(i64, i32)> = None;
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
        let lx = lane.pos_x as i32;
        let ly = lane.pos_y as i32;
        if lx == expected_x && ly == expected_y && exact.is_none() {
            exact = Some(lane.tag);
        }
        let dx = lx - addon_x as i32;
        let dy = ly - addon_y as i32;
        let d2 = dx * dx + dy * dy;
        if nearest.map(|(_, b)| d2 < b).unwrap_or(true) {
            nearest = Some((lane.tag, d2));
        }
    }
    exact.or_else(|| nearest.map(|(tag, _)| tag))
}
