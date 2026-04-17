// Biblioteca de replays: varre o `working_dir` do usuário, lista os
// arquivos .SC2Replay e parseia os metadados de cada um em threads
// worker. A UI consulta o estado (Pending/Parsed/Failed) e preenche
// progressivamente conforme os resultados chegam.
//
// Organização:
//   - `types`     — estruturas de dados (metadados, entrada, estado).
//   - `filter`    — enums e struct de filtro/ordenação.
//   - `scanner`   — `ReplayLibrary` + scanner de diretório e pool de parsers.
//   - `date`      — utilitários de data usados pelo filtro `DateRange`.
//   - `entry_row` — render de uma entrada (três zonas) + helpers de metadados.
//   - `stats`     — agregados derivados (winrate, MMR trend, matchups).
//   - `hero`      — KPI strip clicável no topo do painel central.
//   - `sidebar`   — painel lateral de filtros + seção de insights.
//   - `ui`        — compositor do painel central (hero + lista virtualizada).

pub mod date;
mod entry_row;
pub mod filter;
mod hero;
pub mod scanner;
mod sidebar;
pub mod stats;
pub mod types;
pub mod ui;

pub use filter::{DateRange, LibraryFilter};
pub use scanner::ReplayLibrary;
pub use sidebar::show as show_sidebar;
pub use types::{MetaState, ParsedMeta, PlayerMeta};
pub use ui::{LibraryAction, keep_alive, show};
