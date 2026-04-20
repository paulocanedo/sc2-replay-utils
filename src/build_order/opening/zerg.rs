//! Classificadores de abertura e follow-up para Zerg.

use super::facts::WindowFacts;
use super::types::{Confidence, OpeningLabel};

pub(super) fn classify(f: &WindowFacts) -> Option<OpeningLabel> {
    let pool = f.spawning_pool_loop?;
    let hatch = f.first_expansion_loop;
    let gas = f.first_gas_loop;

    let opening = match hatch {
        // Sem expansão até 5 min: extremamente aggressive; rotula
        // pela posição do pool.
        None => format!("{} Pool", f.supply_at_pool.unwrap_or(0)),
        Some(h) if pool < h => {
            // Pool antes da 2ª Hatch.
            match gas {
                Some(g) if g < pool => "Gas First".to_string(),
                _ => format!("{} Pool", f.supply_at_pool.unwrap_or(0)),
            }
        }
        Some(_) => {
            // Hatch antes do Pool.
            match gas {
                Some(g) if g < pool => "Hatch Gas Pool".to_string(),
                _ => "Hatch First".to_string(),
            }
        }
    };

    Some(OpeningLabel {
        opening,
        follow_up: follow_up(f),
        confidence: Confidence::Named,
    })
}

fn follow_up(f: &WindowFacts) -> Option<String> {
    // Ordem de prioridade: tech mais "assinante" primeiro.
    if f.baneling_nest_loop.is_some() && f.banelings >= 4 {
        return Some("Baneling Bust".to_string());
    }
    if f.ravagers >= 1 {
        return Some("Roach/Ravager".to_string());
    }
    if f.roach_warren_loop.is_some() && f.roaches >= 3 {
        return Some("Roach".to_string());
    }
    if f.lair_loop.is_some() {
        return Some("Fast Lair".to_string());
    }
    if f.metabolic_boost_loop.is_some() && f.zerglings >= 8 {
        return Some("Speedling".to_string());
    }
    if f.zerglings >= 2 {
        return Some("Ling/Queen".to_string());
    }
    if f.first_expansion_loop.is_some() {
        return Some("Macro".to_string());
    }
    None
}
