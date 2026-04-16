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
// Streams canônicos e índices derivados:
// - `entity_events` é o stream canônico para unidades e estruturas
//   (traduzido das 5 variantes brutas acima).
// - `upgrades` é um stream canônico **paralelo** a `entity_events` —
//   tecnologias têm fluxo próprio (`ReplayTrackerEvent::Upgrade`) e
//   não são traduzidas para `EntityEvent`. Isso é deliberado: upgrades
//   não têm tag, position nem lifecycle de Born/Died.
// - `stats`, `unit_positions` (tracker), `camera_positions`,
//   `production_cmds`, `inject_cmds` (game events), `chat` (message
//   events), `resource_nodes` (tracker loop=0) são streams canônicos
//   próprios — cada um tem uma fonte bruta distinta no MPQ.
// - `alive_count`, `worker_capacity`, `army_capacity`, `worker_births`,
//   `creep_index`, `upgrade_cumulative` são **índices derivados**,
//   construídos em `finalize.rs` a partir dos streams canônicos. O
//   tracker nunca os popula em paralelo — reconstruir `finalize` sobre
//   `entity_events`/`upgrades` produz os mesmos valores.
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

pub use classify::{is_structure_name, is_worker_name};
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
    ResourceNode, StatsSnapshot, Toon, UnitPositionSample, UpgradeEntry,
};
