//! `VfsListing` — `core::ListingService` поверх `Vfs` (S4 §2.2).
//!
//! Загрузка: задача на `Lane::Interactive`, `ListStream` → `Batcher` → `Chunk`.
//! Первый чанк отсортирован внутри себя (верх списка сразу правильный), дальше
//! `visible` — в порядке прихода, полный `Order` — на конце (геометрический
//! пересорт по ходу — S4-06), затем `Done`. Ошибка → `Failed`. Отмена (навигация)
//! — тихий выход, после неё ничего не отправляется.

use std::sync::Arc;
use std::time::Instant;

use serde::Deserialize;
use urfin_core::{
    ArrangeReq, ArrangeSpec, Entry, ListingService, ListingUpdate, LoadReq, Runtime, SortKey,
    TaskCx, listing_msg,
};

use crate::api::{ListOpts, VfsError, VfsRegistry};
use crate::batch::{Batcher, Resort};
use crate::sort::{ArrangeOpts, arrange, visible};

/// Секция `[vfs]` конфига (S4 §2.2). Поля кэша и watcher'а читают S4-12…16.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct VfsCfg {
    pub cache_dirs: u16,
    pub cache_entries: u32,
    /// Меньше записей — полный refresh вместо патча.
    pub patch_threshold: u32,
    pub debounce_ms: u16,
    pub max_latency_ms: u16,
    pub poll_interval_ms: u32,
    /// Решение владельца №8: на Windows dotfiles видны.
    pub dotfiles_hidden_on_windows: bool,
    /// Решение владельца №10.
    pub natural_sort: bool,
    /// 0 — авто: 4 на SSD, 1 на HDD или сети.
    pub dir_size_threads: u8,
}

impl Default for VfsCfg {
    fn default() -> Self {
        VfsCfg {
            cache_dirs: 16,
            cache_entries: 200_000,
            patch_threshold: 5000,
            debounce_ms: 50,
            max_latency_ms: 250,
            poll_interval_ms: 2000,
            dotfiles_hidden_on_windows: false,
            natural_sort: true,
            dir_size_threads: 0,
        }
    }
}

impl VfsCfg {
    /// Параметры порядка для `arrange`.
    pub fn arrange_opts(&self) -> ArrangeOpts {
        ArrangeOpts {
            natural: self.natural_sort,
            dotfiles_hidden: !cfg!(windows) || self.dotfiles_hidden_on_windows,
        }
    }
}

pub struct VfsListing {
    reg: Arc<VfsRegistry>,
    cfg: VfsCfg,
}

impl VfsListing {
    pub fn new(reg: Arc<VfsRegistry>, cfg: VfsCfg) -> VfsListing {
        VfsListing { reg, cfg }
    }
}

impl std::fmt::Debug for VfsListing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfsListing")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

impl ListingService for VfsListing {
    fn load(&self, req: LoadReq, rt: &Runtime) {
        let reg = Arc::clone(&self.reg);
        let opts = self.cfg.arrange_opts();
        rt.spawn(
            urfin_core::Lane::Interactive,
            "vfs.list",
            req.cancel.clone(),
            move |cx| load(&reg, &req, opts, &cx),
        );
    }

    fn arrange(&self, req: ArrangeReq, rt: &Runtime) {
        let opts = self.cfg.arrange_opts();
        rt.spawn_cpu("vfs.arrange", req.cancel.clone(), move |cx| {
            if cx.cancel.is_cancelled() {
                return;
            }
            let order = arrange(&req.entries, &req.view, opts);
            let u = ListingUpdate::Order {
                order: Arc::new(order),
                for_len: u32::try_from(req.entries.len()).unwrap_or(u32::MAX),
                seq: req.seq,
            };
            let _ = cx
                .sink
                .send(listing_msg(req.tab, req.generation, u), &cx.cancel);
        });
    }
}

/// Поток листинга. Ошибки — `Failed`, отмена — молча.
fn load(reg: &VfsRegistry, req: &LoadReq, opts: ArrangeOpts, cx: &TaskCx) {
    let t0 = Instant::now();
    let send = |u: ListingUpdate| {
        cx.sink
            .send(listing_msg(req.tab, req.generation, u), &cx.cancel)
            .is_ok()
    };
    match stream(reg, req, opts, cx, &send, t0) {
        Ok(Some((all, resort))) => {
            if req.view.sort.key != SortKey::Unsorted
                && resort.final_due(all.len())
                && !send_order(&all, req, opts, &send)
            {
                return;
            }
            tracing::debug!(target: "list.sorted", elapsed = ?t0.elapsed(), entries = all.len());
            if send(ListingUpdate::Done) {
                tracing::debug!(target: "list.done", elapsed = ?t0.elapsed(), entries = all.len(), loc = %req.loc.path.display());
            }
        }
        // Отменено: ничего не отправляем.
        Ok(None) | Err(VfsError::Cancelled) => {}
        Err(e) => {
            if !cx.cancel.is_cancelled() {
                tracing::debug!(error = %e, loc = %req.loc.path.display(), "listing failed");
                send(ListingUpdate::Failed(e.to_string().into()));
            }
        }
    }
}

/// Полный порядок `all`; `false` — отправка не прошла (отмена).
fn send_order(
    all: &[Entry],
    req: &LoadReq,
    opts: ArrangeOpts,
    send: &impl Fn(ListingUpdate) -> bool,
) -> bool {
    let order = arrange(all, &req.view, opts);
    send(ListingUpdate::Order {
        order: Arc::new(order),
        for_len: u32::try_from(all.len()).unwrap_or(u32::MAX),
        seq: req.seq,
    })
}

/// Чтение и чанки с геометрическим пересортом: на CHUNK·2^k записей — полный
/// `Order` до следующего чанка (core принимает `Order` только при
/// `for_len == entries.len()`), сортировка — rayon внутри. `Ok(None)` —
/// отменено или отправка не прошла (отмена).
fn stream(
    reg: &VfsRegistry,
    req: &LoadReq,
    opts: ArrangeOpts,
    cx: &TaskCx,
    send: &impl Fn(ListingUpdate) -> bool,
    t0: Instant,
) -> Result<Option<(Vec<Entry>, Resort)>, VfsError> {
    let sorting = req.view.sort.key != SortKey::Unsorted;
    let mut resort = Resort::default();
    let fs = reg.get(&req.loc)?;
    let mut s = fs.list(&req.loc, &ListOpts::default())?;
    let mut all: Vec<Entry> = Vec::with_capacity(s.size_hint().unwrap_or(0) as usize);
    let mut pending: Vec<Entry> = Vec::new();
    let mut batcher = Batcher::default();
    loop {
        let more = s.next_batch(&mut pending, &cx.cancel)?;
        if cx.cancel.is_cancelled() {
            return Ok(None);
        }
        if !batcher.should_flush(pending.len(), !more) {
            if more {
                continue;
            }
            break;
        }
        let first = batcher.is_first();
        let visible = chunk_visible(&pending, all.len(), &req.view, opts, first);
        all.extend(pending.iter().cloned());
        if !send(ListingUpdate::Chunk {
            entries: std::mem::take(&mut pending),
            visible,
        }) {
            return Ok(None);
        }
        if first {
            tracing::debug!(target: "list.first_chunk", elapsed = ?t0.elapsed(), entries = all.len());
        }
        batcher.flushed();
        if sorting && more && resort.due(all.len()) {
            if !send_order(&all, req, opts, send) {
                return Ok(None);
            }
            resort.sorted(all.len());
        }
        if !more {
            break;
        }
    }
    Ok(Some((all, resort)))
}

/// `visible` чанка: абсолютные индексы прошедших hidden и фильтр. Первый чанк —
/// отсортирован (его `base` = 0), остальные — в порядке прихода.
fn chunk_visible(
    chunk: &[Entry],
    base: usize,
    view: &ArrangeSpec,
    opts: ArrangeOpts,
    first: bool,
) -> Vec<u32> {
    if first && base == 0 {
        return arrange(chunk, view, opts);
    }
    let base = u32::try_from(base).unwrap_or(u32::MAX);
    (0u32..)
        .zip(chunk)
        .filter(|(_, e)| visible(e, view, opts))
        .map(|(i, _)| base + i)
        .collect()
}
