//! Узлы каталогов для Finalizer (S5 §3.4, §3.6): счётчик незавершённых детей,
//! каталог уходит в Finalizer, когда он обнулился, — после всех детей
//! (post-order).

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Каталог операции. `pending` — незавершённые дети плюс 1, пока план не
/// закрыл каталог ([`super::Plan::close`]).
pub(crate) struct DirNode<D> {
    pub data: D,
    pub parent: Option<Arc<DirNode<D>>>,
    pending: AtomicU32,
    /// Ребёнок пропущен, не удался или не закрылся чисто.
    dirty: AtomicBool,
}

impl<D> DirNode<D> {
    /// Открытый каталог; ребёнок `parent` (его счётчик растёт).
    pub(super) fn open(parent: Option<&Arc<DirNode<D>>>, data: D) -> Arc<DirNode<D>> {
        if let Some(p) = parent {
            p.add_child();
        }
        Arc::new(DirNode {
            data,
            parent: parent.cloned(),
            pending: AtomicU32::new(1),
            dirty: AtomicBool::new(false),
        })
    }

    /// Все дети готовы без пропусков и ошибок (для Move: можно `rmdir`).
    pub(crate) fn clean(&self) -> bool {
        !self.dirty.load(Ordering::Acquire)
    }

    pub(super) fn add_child(&self) {
        self.pending.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    /// Ребёнок завершён (или план закрыл каталог). `true` — каталог готов к
    /// Finalizer: ровно один вызов видит обнуление.
    pub(super) fn complete_child(&self) -> bool {
        self.pending.fetch_sub(1, Ordering::AcqRel) == 1
    }
}

impl<D> fmt::Debug for DirNode<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirNode")
            .field("pending", &self.pending.load(Ordering::Relaxed))
            .field("clean", &self.clean())
            .finish_non_exhaustive()
    }
}
