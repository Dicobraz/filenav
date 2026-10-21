//! Воркер (S5 §3.5): берёт работу из очереди, `checkpoint` перед каждой,
//! выполняет, учитывает итог, закрывает ребёнка каталога; работа с проблемой —
//! в парковку (S5-07), разобранные после вопроса — раньше очереди плана.
//! Паника в работе — job `Failed`, движок останавливается; сам поток выходит
//! штатно.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use urfin_core::{CancelToken, Runtime};

use super::job::{Event, ExecCx, LiveGuard, POLL, Shared, panic_text};
use super::{ItemResult, OpExec, Work, park};

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
    let retry = &sh.park.retry_rx;
    loop {
        // Разобранные после вопроса — первыми (S5 §5).
        let (w, retried) = if let Ok(w) = retry.try_recv() {
            (w, true)
        } else {
            match rx.recv_timeout(POLL) {
                Ok(w) => (w, false),
                // План кончился; запаркованные ещё ждут ответа — ждём и мы.
                Err(RecvTimeoutError::Disconnected) if sh.park.busy() => {
                    match retry.recv_timeout(POLL) {
                        Ok(w) => (w, true),
                        Err(_) if sh.ctl.stopped() => break,
                        Err(_) => continue,
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    if sh.ctl.stopped() {
                        break;
                    }
                    continue;
                }
            }
        };
        let go = step(sh, &cx, w);
        if retried {
            sh.park.retry_done();
        }
        if !go {
            break;
        }
    }
}

/// Одна работа. `false` — воркеру выйти (отмена, паника).
fn step<E: OpExec>(sh: &Shared<E>, cx: &ExecCx<'_>, w: Work<E>) -> bool {
    if cx.checkpoint().is_err() {
        return false;
    }
    let r = match catch_unwind(AssertUnwindSafe(|| sh.exec.exec(&w, cx))) {
        Ok(r) => r,
        Err(p) => {
            sh.ctl
                .fail(&format!("worker panicked: {}", panic_text(p.as_ref())));
            return false;
        }
    };
    if let ItemResult::Park(issue, bytes) = r {
        park::put(sh, w, *issue, bytes);
        return true;
    }
    if !sh.ctl.record(r, w.parent.as_ref()) {
        return false;
    }
    cx.progress().items_done.fetch_add(1, Ordering::Relaxed);
    if let Some(d) = &w.parent
        && d.complete_child()
    {
        let _ = sh.events.send(Event::DirReady(Arc::clone(d)));
    }
    true
}
