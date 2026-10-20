//! Воркер (S5 §3.5): берёт работу из очереди, `checkpoint` перед каждой,
//! выполняет, учитывает итог, закрывает ребёнка каталога. Паника в работе —
//! job `Failed`, движок останавливается; сам поток выходит штатно.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use urfin_core::{CancelToken, Runtime};

use super::job::{Event, ExecCx, LiveGuard, POLL, Shared, panic_text};
use super::{OpExec, Work};

/// Запускает `n` воркеров на выделенных потоках (UI-поток: нужен `Runtime`).
pub(super) fn spawn<E: OpExec>(
    rt: &Runtime,
    live: &Arc<AtomicUsize>,
    sh: &Arc<Shared<E>>,
    rx: &Receiver<Work<E>>,
    n: u16,
) -> Vec<JoinHandle<()>> {
    let id = sh.ctl.job.id().0;
    (0..n)
        .map(|i| {
            let (sh, rx, live) = (Arc::clone(sh), rx.clone(), LiveGuard::enter(live));
            // Свой токен: поток стартует всегда и отправит `Exit` координатору.
            rt.spawn_dedicated(format!("ops-w{id}-{i}"), CancelToken::new(), move |_| {
                let _live = live;
                run(&sh, &rx);
            })
        })
        .collect()
}

/// `Exit` координатору при любом выходе, в том числе при раскрутке паники.
struct ExitGuard<'a, D>(&'a Sender<Event<D>>);

impl<D> Drop for ExitGuard<'_, D> {
    fn drop(&mut self) {
        let _ = self.0.send(Event::Exit);
    }
}

fn run<E: OpExec>(sh: &Shared<E>, rx: &Receiver<Work<E>>) {
    let _span = sh.ctl.span.enter();
    let _exit = ExitGuard(&sh.events);
    let cx = ExecCx { ctl: &sh.ctl };
    loop {
        let w = match rx.recv_timeout(POLL) {
            Ok(w) => w,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if sh.ctl.stop.is_cancelled() || sh.ctl.job.cancel_token().is_cancelled() {
                    break;
                }
                continue;
            }
        };
        if cx.checkpoint().is_err() {
            break;
        }
        let r = match catch_unwind(AssertUnwindSafe(|| sh.exec.exec(&w, &cx))) {
            Ok(r) => r,
            Err(p) => {
                sh.ctl
                    .fail(&format!("worker panicked: {}", panic_text(p.as_ref())));
                break;
            }
        };
        if !sh.ctl.record(r, w.parent.as_ref()) {
            break;
        }
        cx.progress().items_done.fetch_add(1, Ordering::Relaxed);
        if let Some(d) = &w.parent
            && d.complete_child()
        {
            let _ = sh.events.send(Event::DirReady(Arc::clone(d)));
        }
    }
}
