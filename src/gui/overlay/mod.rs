//! Overlay de transmissão (OBS Studio).
//!
//! Um servidor HTTP embarcado em `127.0.0.1` serve uma página HTML editável
//! pelo usuário, renderizada com as estatísticas atuais. No OBS o usuário
//! adiciona uma fonte *Navegador* apontando para a URL.
//!
//! ```text
//! UI thread (egui)                  threads overlay-http-N (4)
//! ────────────────                  ──────────────────────────
//! publish(OverlayData) ──► OverlayState ◄── snapshot / revision / wait_for_change
//!                          Mutex<Arc<OverlayData>>
//!                          AtomicU64 revision
//!                          Condvar (long-poll)
//!                                              │
//!                                              ├─ GET /            → render minijinja
//!                                              ├─ GET /_overlay/revision?since&wait
//!                                              ├─ GET /_overlay/live.js
//!                                              ├─ GET /_overlay/data.json
//!                                              └─ GET /<arquivo>   → asset estático
//! ```
//!
//! Organização:
//!   - `data`   — `OverlayData` e structs-espelho `Serialize` (o contexto
//!                do template; nomes de campo são API pública).
//!   - `shared` — `OverlayState`, a ponte UI ↔ servidor.
//!   - `assets` — pasta de templates, guard de path traversal, content-type.
//!   - `render` — `Environment` do minijinja por request + página de erro.
//!   - `server` — bind, pool de workers, tabela de rotas, `live.js`.
//!
//! A projeção que constrói o `OverlayData` **não** mora aqui: ela está em
//! `crate::library::overlay_snapshot`, porque precisa de helpers
//! `pub(super)` de dentro daquele módulo. A direção de dependência é
//! `library → overlay::data`; este módulo nunca importa `library`.

pub mod assets;
pub mod data;
pub mod render;
pub mod server;
pub mod shared;

pub use server::{start, OverlayHandle};
pub use shared::OverlayState;
