//! Потолок потоков (S0 v2 №7): воркеров всех job вместе не больше
//! `ops.max_threads`. Носитель, AIMD и `DeviceGate` — S5-19.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use urfin_core::{CancelToken, Cancelled};

/// Как часто ожидание свободных потоков перепроверяет отмену.
const POLL: Duration = Duration::from_millis(20);

/// Общий счётчик занятых воркерами потоков.
pub(crate) struct ThreadBudget {
    max: u16,
    used: Mutex<u16>,
    cv: Condvar,
}

/// Взятые потоки; возвращаются при drop.
pub(crate) struct Lease {
    budget: Arc<ThreadBudget>,
    pub n: u16,
}

impl ThreadBudget {
    pub(crate) fn new(max: u16) -> Arc<ThreadBudget> {
        Arc::new(ThreadBudget {
            max: max.max(1),
            used: Mutex::new(0),
            cv: Condvar::new(),
        })
    }

    /// Сколько свободно, но не больше `want`; может быть 0.
    pub(crate) fn try_take(self: &Arc<Self>, want: u16) -> Lease {
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        let n = want.min(self.max - *used);
        *used += n;
        Lease {
            budget: Arc::clone(self),
            n,
        }
    }

    /// Ждёт хотя бы одного свободного потока (`n ≥ 1`). Отмена — `Err`.
    pub(crate) fn take_wait(
        self: &Arc<Self>,
        want: u16,
        cancel: &CancelToken,
        alt: &CancelToken,
    ) -> Result<Lease, Cancelled> {
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            cancel.check()?;
            alt.check()?;
            if *used < self.max {
                let n = want.max(1).min(self.max - *used);
                *used += n;
                return Ok(Lease {
                    budget: Arc::clone(self),
                    n,
                });
            }
            used = self
                .cv
                .wait_timeout(used, POLL)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    #[cfg(test)]
    pub(crate) fn used(&self) -> u16 {
        *self.used.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.n == 0 {
            return;
        }
        let mut used = self
            .budget
            .used
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *used -= self.n;
        self.budget.cv.notify_all();
    }
}

impl fmt::Debug for ThreadBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThreadBudget")
            .field("max", &self.max)
            .field(
                "used",
                &*self.used.lock().unwrap_or_else(PoisonError::into_inner),
            )
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Lease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Lease").field(&self.n).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_caps_and_returns() {
        let budget = ThreadBudget::new(5);
        let first = budget.try_take(3);
        assert_eq!(first.n, 3);
        let second = budget.try_take(4);
        assert_eq!(second.n, 2);
        assert_eq!(budget.try_take(1).n, 0);
        assert_eq!(budget.used(), 5);
        drop(first);
        assert_eq!(budget.used(), 2);
        let token = CancelToken::new();
        let third = budget.take_wait(8, &token, &token).unwrap();
        assert_eq!(third.n, 3);
        drop((second, third));
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn take_wait_wakes_on_release_and_cancel() {
        let b = ThreadBudget::new(1);
        let held = b.try_take(1);
        let t = CancelToken::new();
        let h = {
            let (b, t) = (Arc::clone(&b), t.clone());
            std::thread::spawn(move || b.take_wait(2, &t, &CancelToken::new()).map(|l| l.n))
        };
        std::thread::sleep(Duration::from_millis(30));
        drop(held);
        assert_eq!(h.join().unwrap().unwrap(), 1);

        let held = b.try_take(1);
        let h = {
            let (b, t) = (Arc::clone(&b), t.clone());
            std::thread::spawn(move || b.take_wait(1, &t, &CancelToken::new()).is_err())
        };
        t.cancel();
        assert!(h.join().unwrap());
        drop(held);
    }
}
