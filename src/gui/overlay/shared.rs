//! Estado compartilhado entre a UI thread (produtora) e as threads do
//! servidor HTTP (consumidoras).
//!
//! Duas exigências moldam o desenho:
//!
//! 1. **A UI nunca pode bloquear.** `publish` constrói o `Arc<OverlayData>`
//!    *antes* de pegar o lock e faz um único store de ponteiro dentro dele.
//!    A seção crítica é literalmente uma escrita de ponteiro.
//! 2. **O long-poll precisa de um valor lock-free.** `/_overlay/revision`
//!    responde sem tocar no lock dos dados, e o predicado do condvar
//!    compara contra esse mesmo contador.
//!
//! `Mutex<Arc<T>>` e não `RwLock<Arc<T>>`: a seção crítica de leitura é um
//! `Arc::clone` (um incremento atômico) e a de escrita é um store — um
//! `RwLock` não teria o que otimizar aqui e traria mais superfície. É o
//! padrão do `arc_swap` sem a dependência.
//!
//! São **dois** produtores, cada um dono de uma parte do snapshot: a UI
//! thread publica o que vem da biblioteca (`publish`) e a thread do poller
//! publica quem está jogando agora (`publish_live`). Nenhum dos dois sabe
//! reconstruir a parte do outro, então os dois passam pelo mesmo `store`,
//! que preserva o que não lhe pertence.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::data::{LiveGame, OverlayData};

/// Quantos long-polls podem ficar parkados simultaneamente.
///
/// `tiny_http` não spawna thread nenhuma — nosso pool tem
/// [`super::server::WORKERS`] threads e cada long-poll parkado ocupa uma.
/// Se todas parkarem, o carregamento da própria página trava até algum
/// timeout expirar. Com o teto em `WORKERS - 1` sempre sobra uma thread
/// para servir HTML/assets; os waiters excedentes caem em resposta
/// imediata (o cliente vira short-poll sozinho, via o `setTimeout` de
/// retry do `live.js`).
const MAX_WAITERS: usize = super::server::WORKERS - 1;

pub struct OverlayState {
    data: Mutex<Arc<OverlayData>>,
    revision: AtomicU64,
    /// Lock exclusivo do condvar. Separado do lock dos dados de propósito:
    /// uma thread parkada em `wait_for_change` não pode disputar o mutex
    /// com quem só quer um `snapshot()`.
    wait_lock: Mutex<()>,
    wait_cv: Condvar,
    waiters: AtomicUsize,
    /// Contador de "acordem todos", incrementado por [`Self::wake_all`].
    ///
    /// O predicado do condvar precisa dele porque `notify_all` sozinho não
    /// solta ninguém: se a condição de espera continua verdadeira, o
    /// `wait_timeout_while` volta a dormir. É exatamente o caso do
    /// shutdown — o `Drop` do handle precisa liberar long-polls parkados
    /// *sem* que a revisão tenha mudado, senão o fechamento do app ficaria
    /// preso até o timeout de 25 s de cada um.
    wake_epoch: AtomicU64,
}

impl OverlayState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(Arc::new(OverlayData::default())),
            revision: AtomicU64::new(0),
            wait_lock: Mutex::new(()),
            wait_cv: Condvar::new(),
            waiters: AtomicUsize::new(0),
            wake_epoch: AtomicU64::new(0),
        })
    }

    /// Publica o snapshot vindo da biblioteca. **Só a UI thread chama isto.**
    ///
    /// Preserva o `live` que já estava publicado: aquele campo pertence à
    /// thread do poller, e `overlay_snapshot::build` — que só enxerga
    /// replays — não teria como reconstruí-lo. Sem isto, cada replay novo
    /// apagaria da tela quem está jogando agora.
    pub fn publish(&self, data: OverlayData) -> u64 {
        self.store(|prev| OverlayData {
            live: prev.live.clone(),
            ..data
        })
    }

    /// Publica só a parte ao vivo. **Só a thread do poller chama isto.**
    ///
    /// O caminho espelha o `publish`: o resto do snapshot é preservado, a
    /// revisão sobe e os long-polls acordam — é isso que faz a página do OBS
    /// recarregar quando uma partida começa.
    pub fn publish_live(&self, live: LiveGame) -> u64 {
        self.store(|prev| OverlayData {
            live,
            ..(**prev).clone()
        })
    }

    /// Tronco comum dos dois publishers.
    ///
    /// **A construção do valor acontece com o lock na mão**, ao contrário da
    /// versão que só a UI thread usava. É o preço de ter dois produtores: um
    /// deles precisa ler o snapshot anterior para preservar o campo que não
    /// é dele, e ler-modificar-escrever fora do lock perderia a publicação
    /// do outro numa corrida. A seção crítica passa a ser um clone de algumas
    /// centenas de bytes, e cada publish é um evento raro (replay novo, ou
    /// mudança de estado no cliente do SC2) — nunca algo por frame.
    ///
    /// O `revision` é escrito *depois* dos dados, com `Release`; os leitores
    /// carregam com `Acquire`. É essa ordem que torna verdadeira a
    /// afirmação "a revisão mudou ⇒ os dados que você vai ler são pelo
    /// menos tão novos quanto ela". Com o lock segurando os dois publishers,
    /// duas revisões nunca colidem.
    fn store(&self, build: impl FnOnce(&Arc<OverlayData>) -> OverlayData) -> u64 {
        // Um handler que deu panic com o lock na mão envenenaria o mutex.
        // Preferimos seguir publicando (o dado continua íntegro — a única
        // escrita é uma troca de ponteiro) a deixar o overlay congelado.
        let mut slot = match self.data.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let rev = self.revision.load(Ordering::Relaxed) + 1;
        let mut next = build(&slot);
        next.revision = rev;
        *slot = Arc::new(next);
        self.revision.store(rev, Ordering::Release);
        drop(slot);
        self.wake_all();
        rev
    }

    /// Snapshot atual + sua revisão. Chamado pelas threads do servidor.
    pub fn snapshot(&self) -> (Arc<OverlayData>, u64) {
        let rev = self.revision.load(Ordering::Acquire);
        let data = match self.data.lock() {
            Ok(g) => Arc::clone(&g),
            // Um handler que deu panic com o lock na mão envenenaria o
            // mutex. Preferimos servir o dado (que continua íntegro — a
            // única escrita é uma troca de ponteiro) a derrubar o overlay.
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        };
        (data, rev)
    }

    /// Revisão atual, sem tocar no lock dos dados.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Bloqueia até a revisão sair de `since`, ou até `timeout`. Devolve a
    /// revisão observada na saída (igual a `since` se deu timeout).
    ///
    /// Retorna imediatamente quando já há [`MAX_WAITERS`] threads parkadas —
    /// ver a doc daquela constante.
    pub fn wait_for_change(&self, since: u64, timeout: Duration) -> u64 {
        let current = self.revision();
        if current != since {
            return current;
        }
        if self.waiters.fetch_add(1, Ordering::AcqRel) >= MAX_WAITERS {
            self.waiters.fetch_sub(1, Ordering::AcqRel);
            return current;
        }
        let Ok(guard) = self.wait_lock.lock() else {
            self.waiters.fetch_sub(1, Ordering::AcqRel);
            return self.revision();
        };
        // A epoch é lida com o lock na mão: qualquer `wake_all` que venha
        // depois deste ponto ou já incrementou (e o predicado abaixo vê a
        // diferença na primeira avaliação), ou ainda não pegou o lock e vai
        // notificar depois que estivermos dormindo. É o que fecha a janela
        // de lost wakeup.
        let epoch = self.wake_epoch.load(Ordering::Acquire);
        // `wait_timeout_while` já lida com wakeups espúrios e com o tempo
        // restante entre eles.
        let _ = self.wait_cv.wait_timeout_while(guard, timeout, |_| {
            self.revision() == since && self.wake_epoch.load(Ordering::Acquire) == epoch
        });
        self.waiters.fetch_sub(1, Ordering::AcqRel);
        self.revision()
    }

    /// Acorda todo mundo parkado em `wait_for_change`, tenha a revisão
    /// mudado ou não. Chamado pelo `publish` e pelo `Drop` do handle (que
    /// precisa soltar os long-polls antes do `unblock()` do servidor).
    ///
    /// O lock é tomado de propósito antes de notificar: sem ele, um waiter
    /// que já avaliou o predicado mas ainda não parkou perderia o
    /// `notify_all` e dormiria o timeout inteiro.
    pub fn wake_all(&self) {
        let guard = self.wait_lock.lock();
        self.wake_epoch.fetch_add(1, Ordering::AcqRel);
        drop(guard);
        self.wait_cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn publish_bumps_revision_monotonically() {
        let s = OverlayState::new();
        assert_eq!(s.revision(), 0);
        assert_eq!(s.publish(OverlayData::default()), 1);
        assert_eq!(s.publish(OverlayData::default()), 2);
        assert_eq!(s.revision(), 2);
    }

    #[test]
    fn snapshot_carries_the_published_revision() {
        let s = OverlayState::new();
        s.publish(OverlayData {
            generated_at: "x".into(),
            ..Default::default()
        });
        let (data, rev) = s.snapshot();
        assert_eq!(rev, 1);
        // `publish` carimba a revisão dentro do próprio payload — é o que
        // o template emite como `window.__overlayRev`.
        assert_eq!(data.revision, 1);
        assert_eq!(data.generated_at, "x");
    }

    fn in_game() -> LiveGame {
        LiveGame {
            connected: true,
            in_game: true,
            players: Vec::new(),
        }
    }

    #[test]
    fn the_two_publishers_do_not_overwrite_each_other() {
        let s = OverlayState::new();
        s.publish_live(in_game());
        s.publish(OverlayData {
            generated_at: "x".into(),
            ..Default::default()
        });
        let (data, rev) = s.snapshot();
        assert_eq!(rev, 2);
        assert_eq!(data.generated_at, "x");
        assert!(
            data.live.in_game,
            "um replay novo não pode apagar quem está jogando agora"
        );

        // E na direção contrária.
        s.publish_live(LiveGame::default());
        let (data, _) = s.snapshot();
        assert_eq!(data.generated_at, "x", "o poller não mexe na biblioteca");
        assert!(!data.live.in_game);
    }

    #[test]
    fn concurrent_publishers_never_collide_on_a_revision() {
        // As duas threads existem de verdade (UI e poller). Com o `rev` sendo
        // lido fora do lock, duas publicações simultâneas poderiam carimbar o
        // mesmo número e o long-poll perderia uma atualização.
        const N: u64 = 200;
        let s = OverlayState::new();
        let s2 = Arc::clone(&s);
        let t = thread::spawn(move || {
            for _ in 0..N {
                s2.publish_live(in_game());
                s2.publish_live(LiveGame::default());
            }
        });
        for i in 0..N {
            s.publish(OverlayData {
                generated_at: i.to_string(),
                ..Default::default()
            });
        }
        t.join().unwrap();
        assert_eq!(s.revision(), N * 3, "toda publicação avançou a revisão");
        let (data, rev) = s.snapshot();
        assert_eq!(data.revision, rev, "o payload carrega a própria revisão");
    }

    #[test]
    fn wait_returns_immediately_when_already_stale() {
        let s = OverlayState::new();
        s.publish(OverlayData::default());
        let t0 = Instant::now();
        assert_eq!(s.wait_for_change(0, Duration::from_secs(5)), 1);
        assert!(t0.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn wait_wakes_on_publish_from_another_thread() {
        let s = OverlayState::new();
        let s2 = Arc::clone(&s);
        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            s2.publish(OverlayData::default());
        });
        let t0 = Instant::now();
        let rev = s.wait_for_change(0, Duration::from_secs(5));
        assert_eq!(rev, 1);
        assert!(t0.elapsed() < Duration::from_secs(4));
        t.join().unwrap();
    }

    #[test]
    fn wait_returns_unchanged_revision_on_timeout() {
        let s = OverlayState::new();
        let t0 = Instant::now();
        assert_eq!(s.wait_for_change(0, Duration::from_millis(50)), 0);
        assert!(t0.elapsed() >= Duration::from_millis(50));
    }

    #[test]
    fn wake_all_releases_waiters_without_a_revision_change() {
        // É disto que o `Drop` do `OverlayHandle` depende: no shutdown a
        // revisão não muda, e um `notify_all` puro faria o waiter reavaliar
        // o predicado, vê-lo ainda verdadeiro e voltar a dormir os 25 s.
        let s = OverlayState::new();
        let s2 = Arc::clone(&s);
        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            s2.wake_all();
        });
        let t0 = Instant::now();
        let rev = s.wait_for_change(0, Duration::from_secs(10));
        assert_eq!(rev, 0, "a revisão não mudou");
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "wake_all deveria soltar o waiter, levou {:?}",
            t0.elapsed()
        );
        t.join().unwrap();
    }

    #[test]
    fn admission_counter_caps_parked_waiters() {
        // Com MAX_WAITERS threads já parkadas, a próxima não parka: volta
        // na hora com a revisão corrente em vez de segurar a última thread
        // livre do pool.
        let s = OverlayState::new();
        let mut handles = Vec::new();
        for _ in 0..MAX_WAITERS {
            let s2 = Arc::clone(&s);
            handles.push(thread::spawn(move || {
                s2.wait_for_change(0, Duration::from_millis(600));
            }));
        }
        // Dá tempo das threads acima entrarem no wait.
        thread::sleep(Duration::from_millis(120));
        let t0 = Instant::now();
        assert_eq!(s.wait_for_change(0, Duration::from_secs(5)), 0);
        assert!(
            t0.elapsed() < Duration::from_millis(400),
            "waiter excedente deveria voltar na hora, levou {:?}",
            t0.elapsed()
        );
        for h in handles {
            h.join().unwrap();
        }
    }
}
