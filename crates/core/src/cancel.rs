//! Иерархическая отмена задач.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Токен отмены. Клоны разделяют состояние; отмена родителя отменяет всех потомков,
/// отмена потомка родителя не трогает.
#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<CancelInner>);

#[derive(Default, Debug)]
struct CancelInner {
    flag: AtomicBool,
    parent: Option<CancelToken>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Дочерний токен: отменён, если отменён он сам или любой предок.
    #[must_use]
    pub fn child(&self) -> Self {
        Self(Arc::new(CancelInner {
            flag: AtomicBool::new(false),
            parent: Some(self.clone()),
        }))
    }

    pub fn cancel(&self) {
        self.0.flag.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        if self.0.flag.load(Ordering::Acquire) {
            return true;
        }
        let mut cur = self.0.parent.as_ref();
        while let Some(t) = cur {
            if t.0.flag.load(Ordering::Acquire) {
                // Запоминаем, чтобы следующие проверки не обходили цепочку.
                self.0.flag.store(true, Ordering::Release);
                return true;
            }
            cur = t.0.parent.as_ref();
        }
        false
    }

    /// `Err(Cancelled)`, если токен отменён. Для `?` в циклах воркеров.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Задача отменена.
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

#[cfg(test)]
mod tests {
    use super::CancelToken;

    #[test]
    fn parent_cancels_descendants() {
        let root = CancelToken::new();
        let child = root.child();
        let grandchild = child.child();
        let sibling = root.child();
        assert!(!grandchild.is_cancelled());
        root.cancel();
        assert!(child.is_cancelled());
        assert!(grandchild.is_cancelled());
        assert!(sibling.is_cancelled());
        assert!(grandchild.check().is_err());
    }

    #[test]
    fn child_does_not_cancel_parent_or_siblings() {
        let root = CancelToken::new();
        let a = root.child();
        let b = root.child();
        let a1 = a.child();
        a.cancel();
        assert!(a.is_cancelled());
        assert!(a1.is_cancelled());
        assert!(!root.is_cancelled());
        assert!(!b.is_cancelled());
        assert!(root.check().is_ok());
    }

    #[test]
    fn clones_share_state_across_threads() {
        let t = CancelToken::new();
        let child = t.child();
        let h = std::thread::spawn({
            let t = t.clone();
            move || t.cancel()
        });
        h.join().unwrap();
        assert!(child.is_cancelled());
    }
}
