//! Classificadores de abertura e follow-up para Terran.

use super::facts::WindowFacts;
use super::types::{Confidence, OpeningLabel};

pub(super) fn classify(f: &WindowFacts) -> Option<OpeningLabel> {
    let rax = f.first_barracks_loop?; // precisa ter pelo menos 1 rax
    let cc = f.first_expansion_loop;

    let opening = if let Some(cc_loop) = cc {
        if cc_loop < rax {
            // CC antes do Rax — "CC First" (super greedy, raro).
            "CC First".to_string()
        } else if f.third_barracks_loop.is_some()
            && f.third_barracks_loop.unwrap() < cc_loop
        {
            "3 Rax".to_string()
        } else if f.second_barracks_loop.is_some()
            && f.second_barracks_loop.unwrap() < cc_loop
        {
            "2 Rax".to_string()
        } else if f.factory_loop.is_some()
            && f.starport_loop.is_some()
            && f.factory_loop.unwrap() <= cc_loop.saturating_add(1)
        {
            // Ordem clássica: Rax → Factory → Starport antes ou perto
            // da expansão. Se os três existem até 5 min, rotulamos.
            "1-1-1".to_string()
        } else if f.reactor_loop.is_some()
            && f.reactor_loop.unwrap() < cc_loop
            && f.reapers >= 1
        {
            "Reaper Expand".to_string()
        } else {
            "1 Rax FE".to_string()
        }
    } else {
        // Sem expansão até 5 min — pressure/all-in.
        if f.third_barracks_loop.is_some() {
            "3 Rax".to_string()
        } else if f.second_barracks_loop.is_some() {
            "2 Rax".to_string()
        } else if f.factory_loop.is_some() && f.starport_loop.is_some() {
            "1-1-1".to_string()
        } else {
            "1 Rax".to_string()
        }
    };

    Some(OpeningLabel {
        opening,
        follow_up: follow_up(f),
        confidence: Confidence::Named,
    })
}

fn follow_up(f: &WindowFacts) -> Option<String> {
    if f.banshees >= 1 {
        return Some("Banshee".to_string());
    }
    if f.hellions >= 2 && f.reactor_loop.is_some() {
        return Some("Reactor Hellion".to_string());
    }
    if f.factory_loop.is_some() && f.marines < 4 {
        return Some("Mech".to_string());
    }
    if f.stimpack_loop.is_some() {
        return Some("Stim Timing".to_string());
    }
    if f.marauders >= 2 && f.marines >= 4 {
        return Some("Bio + Marauder".to_string());
    }
    if f.marines >= 6 {
        return Some("Bio".to_string());
    }
    if f.reapers >= 1 && f.reactor_loop.is_some() {
        return Some("Reaper Pressure".to_string());
    }
    None
}
