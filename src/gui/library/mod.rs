// Biblioteca de replays: varre o `working_dir` do usuário, lista os
// arquivos .SC2Replay e parseia os metadados de cada um em threads
// worker. A UI consulta o estado (Pending/Parsed/Failed) e preenche
// progressivamente conforme os resultados chegam.
//
// Organização:
//   - `types`   — estruturas de dados (metadados, entrada, estado).
//   - `filter`  — enums e struct de filtro/ordenação.
//   - `scanner` — `ReplayLibrary` + scanner de diretório e pool de parsers.
//   - `date`    — utilitários de data usados pelo filtro `DateRange`.
//   - `ui`      — render egui + `LibraryAction` + `keep_alive`.

pub mod date;
pub mod filter;
pub mod scanner;
pub mod types;
pub mod ui;

pub use filter::{DateRange, LibraryFilter};
pub use scanner::ReplayLibrary;
pub use types::{MetaState, ParsedMeta, PlayerMeta};
pub use ui::{LibraryAction, keep_alive, show};
