// File watcher que observa a pasta do SC2 e notifica a UI quando
// surge um novo arquivo .SC2Replay. Implementado com o crate `notify`.
//
// O callback roda em uma thread separada (gerenciada pelo notify) e
// envia o PathBuf para um canal mpsc. A UI drena o canal no início de
// cada update() via poll_latest().

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub struct ReplayWatcher {
    // Mantido vivo pelo struct; Drop encerra a thread do notify.
    _watcher: RecommendedWatcher,
    rx: Receiver<PathBuf>,
    watched: PathBuf,
}

impl ReplayWatcher {
    pub fn start(dir: &Path) -> Result<Self, String> {
        let (tx, rx) = channel::<PathBuf>();
        let dir_owned = dir.to_path_buf();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(ev) = res else { return };
            // Só nos interessam eventos de criação/modificação.
            if !matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                return;
            }
            for p in ev.paths {
                let is_replay = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("SC2Replay"))
                    .unwrap_or(false);
                if is_replay {
                    let _ = tx.send(p);
                }
            }
        })
        .map_err(|e| e.to_string())?;

        watcher
            .watch(&dir_owned, RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            _watcher: watcher,
            rx,
            watched: dir_owned,
        })
    }

    /// Drena eventos acumulados e devolve o path mais recente (se houver).
    /// O SC2 costuma disparar múltiplos eventos Create/Modify por um
    /// único arquivo, então retornamos apenas o último para evitar
    /// recarregar o mesmo replay várias vezes em sequência.
    pub fn poll_latest(&self) -> Option<PathBuf> {
        let mut latest = None;
        while let Ok(p) = self.rx.try_recv() {
            latest = Some(p);
        }
        latest
    }

    pub fn watched_dir(&self) -> &Path {
        &self.watched
    }
}
