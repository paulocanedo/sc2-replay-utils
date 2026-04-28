//! Extrator de "lanes" de produção por estrutura, generalizado para
//! dois modos:
//!
//! - `LaneMode::Workers` — uma lane por townhall (Nexus / CommandCenter /
//!   OrbitalCommand / PlanetaryFortress / Hatchery / Lair / Hive). Cada
//!   bloco representa uma janela de produção de SCV/Probe/Drone ou um
//!   morph in-place impeditivo (CC→Orbital, CC→PF). Hatch→Lair / Lair→
//!   Hive não emite bloco — a estrutura continua produzindo drones
//!   durante o morph.
//!
//! - `LaneMode::Army` — uma lane por estrutura produtora de army:
//!   - Zerg: Hatchery / Lair / Hive (cada larva-born-army).
//!   - Terran: Barracks / Factory / Starport. Janelas de produção de
//!     unidade são blocos cheios. Produção paralela via Reactor não é
//!     mais modelada — a partir do momento em que pelo menos uma
//!     unidade está sendo produzida, a lane vira um bloco simples
//!     `Producing`; estruturas ociosas continuam mostrando o traço fino
//!     padrão. Durante a construção de um addon (Reactor/TechLab) a
//!     estrutura-mãe continua emitindo bloco `Impeded` cobrindo a
//!     janela impeditiva.
//!   - Protoss: Gateway / WarpGate (mesma tag — morph in-place),
//!     RoboticsFacility, Stargate. Quando uma Gateway morpha em WarpGate,
//!     setamos `warpgate_since_loop` na lane; o render distingue blocos
//!     pré-WarpGate (cheios, single-track) dos blocos pós-WarpGate
//!     (thin sub-tracks, estilo Hatchery).
//!
//! Resolução unit → producer mantém o pipeline em cascata do worker mode:
//! 1. `creator_tag` no `ProductionStarted` companheiro (índice `i-1`).
//! 2. Larva-born (Zerg): map `larva_tag → hatch_tag` populado quando a
//!    larva nasceu.
//! 3. Fallback de proximidade espacial (Probe warp-in).

mod classify;
mod morph;
mod player;
mod resolve;
mod terran;
mod types;

#[cfg(test)]
mod tests;

pub use types::{BlockKind, LaneMode, PlayerProductionLanes, ProductionBlock, StructureLane};

use crate::replay::ReplayTimeline;

/// Constrói as lanes para todos os jogadores do replay, na mesma ordem
/// de `timeline.players`.
pub fn extract(timeline: &ReplayTimeline, mode: LaneMode) -> Vec<PlayerProductionLanes> {
    timeline
        .players
        .iter()
        .map(|p| player::extract_player(p, timeline.base_build, mode))
        .collect()
}
