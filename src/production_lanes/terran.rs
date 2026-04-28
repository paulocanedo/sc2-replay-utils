//! Lógica específica da raça Terran. Atualmente cobre apenas a
//! resolução do parent de um addon (Reactor/TechLab) para emissão do
//! bloco `Impeded` na lane da estrutura-mãe.
//!
//! A produção paralela 2x via Reactor não é mais modelada — qualquer
//! produção de uma estrutura Terran vira uma única lane simples. Veja
//! o histórico do módulo se precisar reintroduzir slots paralelos.

use std::collections::HashMap;

use super::types::StructureLane;

/// Tipo canônico da estrutura-mãe esperada para cada addon.
pub(super) fn addon_parent_canonical(addon: &str) -> Option<&'static str> {
    match addon {
        "BarracksReactor" | "BarracksTechLab" => Some("Barracks"),
        "FactoryReactor" | "FactoryTechLab" => Some("Factory"),
        "StarportReactor" | "StarportTechLab" => Some("Starport"),
        _ => None,
    }
}

/// Resolve o parent de um addon Terran por proximidade espacial. O
/// `UnitInitEvent` do s2protocol não carrega `creator_unit_tag_index`,
/// então `creator_tag` chega como `None` para Reactor/TechLab e
/// precisamos achar a estrutura-mãe pelo posicionamento — addons são
/// sempre colados na estrutura, então a Barracks/Factory/Starport mais
/// próxima viva é virtualmente sempre a correta.
pub(super) fn resolve_addon_parent(
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
