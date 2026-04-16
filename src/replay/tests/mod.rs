//! Testes integration-style do parser single-pass. Todos carregam
//! `examples/replay1.SC2Replay` (ou `replay_observed` para o caso de
//! observers no lobby) e validam shape/ordem/monotonicidade do
//! `ReplayTimeline` produzido.

mod derived_indexes;
mod events;
mod observer;
mod parse;
mod positions;
mod query;
mod resources;

use std::collections::HashMap;
use std::path::PathBuf;

use super::*;

/// Caminho para o replay de exemplo (Terran vs Protoss, com morphs
/// CC→Orbital e uma estrutura cancelada). Usado como golden em
/// vários testes.
pub(super) fn example_replay() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/replay1.SC2Replay")
}

pub(super) fn load() -> ReplayTimeline {
    parse_replay(&example_replay(), 0).expect("parse_replay")
}
