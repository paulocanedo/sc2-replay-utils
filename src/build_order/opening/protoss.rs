//! Classificadores de abertura e follow-up para Protoss.

use super::facts::WindowFacts;
use super::types::{Confidence, OpeningLabel};

pub(super) fn classify(f: &WindowFacts) -> Option<OpeningLabel> {
    let gateway = f.first_gateway_loop;
    let nexus = f.first_expansion_loop;

    // "Cannon Rush" precisa preceder a pressão normal: Forge antes
    // do Gateway + pelo menos 1 cannon construído cedo.
    if let Some(forge) = f.forge_loop {
        let cannon_rush = match gateway {
            Some(gate) => forge < gate && f.photon_cannon_loop.is_some(),
            None => f.photon_cannon_loop.is_some(),
        };
        if cannon_rush {
            return Some(OpeningLabel {
                opening: "Cannon Rush".to_string(),
                follow_up: None,
                confidence: Confidence::Named,
            });
        }
    }

    let opening = match (nexus, gateway) {
        // Nexus antes do Gateway → FFE (quando há Forge cedo) ou
        // Nexus First (quando não há).
        (Some(n), Some(g)) if n < g => {
            if f.forge_loop.map_or(false, |fg| fg < n) {
                "Nexus First (FFE)".to_string()
            } else {
                "Nexus First".to_string()
            }
        }
        (Some(n), Some(_)) => {
            // Gateway antes do Nexus.
            if f.fourth_gateway_loop.is_some() && f.fourth_gateway_loop.unwrap() < n {
                "4 Gate".to_string()
            } else if f.third_gateway_loop.is_some() && f.third_gateway_loop.unwrap() < n {
                "3 Gate Expand".to_string()
            } else {
                "Gate Expand".to_string()
            }
        }
        (None, Some(_)) => {
            // Sem expansão até 5 min.
            if f.fourth_gateway_loop.is_some() {
                "4 Gate".to_string()
            } else if f.third_gateway_loop.is_some() {
                "3 Gate".to_string()
            } else {
                "1 Gate Tech".to_string()
            }
        }
        // Sem Gateway até a janela — não classificamos.
        _ => return None,
    };

    Some(OpeningLabel {
        opening,
        follow_up: follow_up(f),
        confidence: Confidence::Named,
    })
}

fn follow_up(f: &WindowFacts) -> Option<String> {
    if f.dark_shrine_loop.is_some() || f.dark_templars >= 1 {
        return Some("DT".to_string());
    }
    if f.blink_loop.is_some() {
        return Some("Blink".to_string());
    }
    if f.immortals >= 1 || f.robotics_loop.is_some() {
        return Some("Immortal".to_string());
    }
    if f.phoenixes >= 1 {
        return Some("Phoenix".to_string());
    }
    if f.void_rays >= 1 || f.stargate_loop.is_some() {
        return Some("Void Ray".to_string());
    }
    if f.stalkers >= 3 && f.sentries >= 1 {
        return Some("Stalker/Sentry".to_string());
    }
    if f.stalkers >= 3 {
        return Some("Stalker".to_string());
    }
    None
}
