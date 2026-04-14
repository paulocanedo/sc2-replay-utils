// Parser single-pass do replay.
//
// Abre o MPQ uma única vez, lê tracker events e message events em
// um único loop cada e produz um `ReplayTimeline` indexado por tempo
// que serve como fonte única de verdade para todos os extractors
// (build_order, army_value, supply_block, production_gap, chat) e
// para a GUI.
//
// O parser **traduz** os eventos crus do replay (UnitInit/UnitBorn/
// UnitDone/UnitDied/UnitTypeChange) para um vocabulário semântico do
// app — `EntityEvent { kind: ProductionStarted | ProductionFinished
// | ProductionCancelled | Died, … }`. Os consumers nunca tocam no
// formato bruto.
//
// Layout do módulo:
// - `types`     — structs/enums expostos publicamente
// - `query`     — API de scrubbing (`stats_at`, `alive_count_at`, …)
// - `classify`  — heurísticas worker/structure/upgrade (privado)
// - `tracker`   — tradução dos tracker events (privado)
// - `game`      — tradução dos game events (ProductionCmd/Camera, privado)
// - `message`   — chat (privado)
// - `finalize`  — pós-processamento dos índices (privado)
// - `parse`     — orquestrador `parse_replay` (privado, re-exportado)

mod classify;
mod finalize;
mod game;
mod message;
mod parse;
mod query;
mod tracker;
mod types;

#[cfg(test)]
mod tests;

pub use parse::parse_replay;
pub use types::{
    ChatEntry, EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline, UNIT_INIT_MARKER,
};
// Re-exportados para que `EntityEvent`/`StatsSnapshot`/`UpgradeEntry`/
// `UnitPositionSample`, que aparecem como campos públicos das structs
// acima, sejam alcançáveis via `crate::replay::*` quando consumers
// precisarem nomeá-los explicitamente.
#[allow(unused_imports)]
pub use types::{
    CameraPosition, CreepEntry, CreepKind, EntityEvent, InjectCmd, ProductionCmd, ResourceKind,
    ResourceNode, StatsSnapshot, UnitPositionSample, UpgradeEntry,
};
