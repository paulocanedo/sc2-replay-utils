//! Classificação de uma entrada do build order em `EntryKind`.

use super::types::BuildOrderEntry;

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
    Inject,
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
            EntryKind::Inject => "I",
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
            EntryKind::Inject => "Inject",
        }
    }
}

/// Classifica uma entrada do build order em uma `EntryKind`. A decisão
/// usa os flags já armazenados (`is_upgrade`/`is_structure`) e o nome
/// bruto da ação para distinguir worker/unit e research/upgrade.
#[allow(dead_code)] // consumido apenas pelo binário GUI
pub fn classify_entry(entry: &BuildOrderEntry) -> EntryKind {
    if entry.action.starts_with("InjectLarva") {
        return EntryKind::Inject;
    }
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
fn is_leveled_upgrade(name: &str) -> bool {
    name.ends_with("Level1") || name.ends_with("Level2") || name.ends_with("Level3")
}
