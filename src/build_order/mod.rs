// Extrator de build order — camada pura sobre `ReplayTimeline`.
//
// Não abre o MPQ nem decodifica eventos: consome `entity_events`,
// `upgrades` e `production_cmds` que o parser single-pass já produziu,
// mapeando cada um para `BuildOrderEntry` na semântica esperada pelos
// consumers (CSV, GUI, image renderer).
//
// Cada entrada armazena o `game_loop` no instante de **início** da
// ação. Há dois caminhos para descobrir esse instante:
//
// 1. **Cmd matching** (preferido): se o evento tem `creator_tag` (i.e.,
//    veio de um Train/Morph com produtor identificado) e o parser de
//    game events capturou um `ProductionCmd` correspondente nesse mesmo
//    produtor, usamos `start = max(cmd_loop, finish_anterior_no_mesmo
//    _produtor)`. Isso absorve Chrono Boost (Protoss), supply block e
//    idle gaps gratuitamente — só usamos tempos observados (clique do
//    jogador + UnitBorn real). Para upgrades o match é global por
//    nome (não há fila de pesquisas).
//
// 2. **Fallback** (legado): quando não há cmd correspondente — warp-ins
//    via UnitInit, spawns iniciais, replays sem game events, ou cmds
//    órfãos por seleção não resolvida — recuamos do `finish_loop`
//    bruto subtraindo `build_time_loops(action, base_build)`. Estruturas
//    vindas de `UnitInit` já são start-time e só projetam o
//    `finish_loop` somando o build time.
//
// Organização:
//   - `types`    — structs/enum de saída + `format_time`.
//   - `extract`  — `extract_build_order` + lógica de `build_player_entries`.
//   - `classify` — `EntryKind` + `classify_entry`.

mod classify;
mod extract;
mod types;

#[cfg(test)]
mod tests;

pub use classify::{classify_entry, EntryKind};
pub use extract::extract_build_order;
// `BuildOrderEntry` é consumido apenas por `#[cfg(test)]` em `gui/salt.rs`,
// então em builds non-test o import parece não-usado. Silenciamos pra
// manter a API pública estável.
#[allow(unused_imports)]
pub use types::BuildOrderEntry;
pub use types::{BuildOrderResult, EntryOutcome, PlayerBuildOrder};
