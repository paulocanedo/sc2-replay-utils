//! Detecção e cálculo de morphs in-place. Usado por todas as raças:
//!
//! - Terran (Workers): CC → Orbital/PF, morph impeditivo.
//! - Zerg (qualquer modo): Hatch → Lair → Hive (sem bloco — drones
//!   continuam saindo). Larva → Drone/Zergling/etc. e morphs de
//!   segundo nível via cocoon/egg.
//! - Protoss (Army): Gateway → WarpGate (apenas atualiza
//!   `canonical_type` + `warpgate_since_loop`).
//!
//! Filtros como `is_pure_morph_finish` cortam transforms cosméticos
//! Terran (Hellion↔Hellbat, SiegeTank siege, Viking, WidowMine).

use crate::balance_data;
use crate::replay::{EntityEvent, EntityEventKind};

/// Tempo de morph in-place em game loops. Usado para morph impeditivo
/// CC→Orbital/PF.
pub(super) fn morph_build_loops(new_type: &str, base_build: u32) -> u32 {
    let from_balance = balance_data::build_time_loops(new_type, base_build);
    if from_balance > 0 {
        return from_balance;
    }
    match new_type {
        "OrbitalCommand" => 560,
        "PlanetaryFortress" => 806,
        "Lair" => 1424,
        "Hive" => 2160,
        _ => 0,
    }
}

pub(super) fn morph_old_type<'a>(events: &'a [EntityEvent], i: usize) -> Option<&'a str> {
    if i == 0 {
        return None;
    }
    let prev = &events[i - 1];
    let cur = &events[i];
    if matches!(prev.kind, EntityEventKind::Died)
        && prev.tag == cur.tag
        && prev.game_loop == cur.game_loop
    {
        Some(prev.entity_type.as_str())
    } else {
        None
    }
}

pub(super) fn is_morph_finish(events: &[EntityEvent], i: usize) -> bool {
    if i < 2 {
        return false;
    }
    let cur = &events[i];
    let s = &events[i - 1];
    let d = &events[i - 2];
    matches!(s.kind, EntityEventKind::ProductionStarted)
        && s.tag == cur.tag
        && s.game_loop == cur.game_loop
        && matches!(d.kind, EntityEventKind::Died)
        && d.tag == cur.tag
        && d.game_loop == cur.game_loop
}

pub(super) fn is_morph_died(events: &[EntityEvent], i: usize) -> bool {
    let cur = &events[i];
    let Some(next) = events.get(i + 1) else {
        return false;
    };
    matches!(next.kind, EntityEventKind::ProductionStarted)
        && next.tag == cur.tag
        && next.game_loop == cur.game_loop
}

/// Tipos "consumíveis" cujo `Died` representa uma produção real em
/// progresso (não um simples toggle/transform). Larva é a fonte óbvia
/// (Drone/Zergling/Overlord/etc.); cocoons e eggs cobrem morphs Zerg de
/// segundo nível (Baneling vem de BanelingCocoon, Lurker de
/// LurkerMPEgg, Ravager de RavagerCocoon, BroodLord de
/// BroodLordCocoon, Overlord de OverlordCocoon).
///
/// Tudo fora dessa lista que dispara o pattern Died→Started→Finished no
/// mesmo tag/loop é um morph mecânico (siege mode, hellbat transform,
/// viking assault, widowmine burrow, liberator AG) — a unidade original
/// já foi contada quando nasceu, então o transform não vira bloco novo.
pub(super) fn is_consumable_progenitor(name: &str) -> bool {
    name == "Larva" || name.ends_with("Cocoon") || name.ends_with("Egg")
}

/// `is_morph_finish` mas restrito a transforms PUROS (não-larva,
/// não-cocoon, não-egg). Usado para descartar `ProductionFinished` de
/// toggles Terran (Hellion↔Hellbat, SiegeTank siege mode, etc.) que de
/// outro modo gerariam blocos `Producing` fantasmas atribuídos por
/// proximidade. Mirror semântico do filtro `creator_ability` em
/// `build_order/extract.rs:79-97`, sem depender das strings cruas do
/// SC2 (que variam por base_build).
pub(super) fn is_pure_morph_finish(events: &[EntityEvent], i: usize) -> bool {
    if !is_morph_finish(events, i) {
        return false;
    }
    !is_consumable_progenitor(events[i - 2].entity_type.as_str())
}
