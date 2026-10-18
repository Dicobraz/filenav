//! Общий обходчик дерева каталогов (S0 v5 №14): пул потоков поверх
//! `Vfs::list`, общая очередь каталогов. Используют размер каталогов (S4-17),
//! позже S5 (не-локальные источники) и S8.
//!
//! Ссылки на каталоги (`LinkDir`: symlink, junction) не обходятся — циклов нет.
//! Ошибки отдельных каталогов (нет доступа) не прерывают обход: считаются в
//! [`WalkStats::errors`].

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use urfin_core::{CancelToken, Entry, EntryKind, Location};

use crate::api::{ListOpts, Vfs};

/// Как часто проверять отмену, пока поток ждёт работы.
const IDLE_CHECK: Duration = Duration::from_millis(20);

/// Итоги обхода.
#[derive(Debug, Default)]
pub struct WalkStats {
    pub dirs: AtomicU64,
    pub errors: AtomicU64,
}

struct Queue {
    /// (корень, каталог).
    dirs: Vec<(usize, Location)>,
    /// Каталоги в работе у потоков.
    busy: usize,
}

/// Обойти деревья `roots` в `threads` потоках. `visit(root, dir, entry)` —
/// для каждой записи каждого каталога (из любого потока); `root_done(root)` —
/// когда дерево корня обойдено целиком. `tick` зовётся в вызывающем потоке
/// примерно раз в `period`, пока идёт обход. Отмена — `cancel` (и между
/// каталогами — `stop`); проверяется на каждой порции листинга.
#[allow(clippy::too_many_arguments, reason = "обходчик с колбэками прогресса")]
pub fn walk(
    vfs: &dyn Vfs,
    roots: &[Location],
    threads: usize,
    cancel: &CancelToken,
    stop: &(dyn Fn() -> bool + Sync),
    visit: &(dyn Fn(usize, &Location, &Entry) + Sync),
    root_done: &(dyn Fn(usize) + Sync),
    mut tick: impl FnMut(),
    period: Duration,
) -> WalkStats {
    let stats = WalkStats::default();
    // Сколько каталогов корня ещё не дочитано (в очереди или в работе).
    let pending: Vec<AtomicUsize> = roots.iter().map(|_| AtomicUsize::new(1)).collect();
    let queue = Mutex::new(Queue {
        dirs: roots.iter().cloned().enumerate().collect(),
        busy: 0,
    });
    let cv = Condvar::new();
    let halted = || cancel.is_cancelled() || stop();
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                worker(
                    vfs, &queue, &cv, &pending, &stats, cancel, &halted, visit, root_done,
                );
            });
        }
        let mut next = Instant::now() + period;
        loop {
            let q = queue.lock().unwrap_or_else(PoisonError::into_inner);
            if (q.dirs.is_empty() && q.busy == 0) || halted() {
                cv.notify_all();
                break;
            }
            let wait = next
                .saturating_duration_since(Instant::now())
                .min(IDLE_CHECK);
            drop(
                cv.wait_timeout(q, wait)
                    .unwrap_or_else(PoisonError::into_inner),
            );
            if Instant::now() >= next {
                tick();
                next = Instant::now() + period;
            }
        }
    });
    stats
}

#[allow(clippy::too_many_arguments, reason = "контекст потока обхода")]
fn worker(
    vfs: &dyn Vfs,
    queue: &Mutex<Queue>,
    cv: &Condvar,
    pending: &[AtomicUsize],
    stats: &WalkStats,
    cancel: &CancelToken,
    halted: &(dyn Fn() -> bool + Sync),
    visit: &(dyn Fn(usize, &Location, &Entry) + Sync),
    root_done: &(dyn Fn(usize) + Sync),
) {
    let mut out = Vec::new();
    loop {
        let (root, dir) = {
            let mut q = queue.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if halted() {
                    return;
                }
                if let Some(job) = q.dirs.pop() {
                    q.busy += 1;
                    break job;
                }
                if q.busy == 0 {
                    cv.notify_all();
                    return;
                }
                q = cv
                    .wait_timeout(q, IDLE_CHECK)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        };
        let mut children = Vec::new();
        match vfs.list(&dir, &ListOpts::default()) {
            Ok(mut st) => loop {
                out.clear();
                let more = st.next_batch(&mut out, cancel).unwrap_or_else(|_| {
                    stats.errors.fetch_add(1, Ordering::Relaxed);
                    false
                });
                for e in &out {
                    visit(root, &dir, e);
                    if e.kind == EntryKind::Dir {
                        children.push((root, child(&dir, e)));
                    }
                }
                if !more || halted() {
                    break;
                }
            },
            Err(_) => {
                stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        stats.dirs.fetch_add(1, Ordering::Relaxed);
        pending[root].fetch_add(children.len(), Ordering::AcqRel);
        let mut q = queue.lock().unwrap_or_else(PoisonError::into_inner);
        q.dirs.extend(children);
        q.busy -= 1;
        drop(q);
        cv.notify_all();
        if pending[root].fetch_sub(1, Ordering::AcqRel) == 1 && !halted() {
            root_done(root);
        }
    }
}

/// Подкаталог `e` каталога `dir` (тот же backend и контейнер).
pub fn child(dir: &Location, e: &Entry) -> Location {
    Location {
        backend: dir.backend.clone(),
        path: crate::name::join_os(dir, e),
        outer: dir.outer.clone(),
    }
}
