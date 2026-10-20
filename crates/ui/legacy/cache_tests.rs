#![allow(
    unsafe_code,
    reason = "тестовый GlobalAlloc (S0 v5 №11) — обёртка над System"
)]

use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::cell::Cell as StdCell;

use urfin_core::{Attrs, Entry, EntryKind, Gen, Listing, Location, Tab, TabId};

use super::*;
use crate::legacy::fmt_cell::FmtCfg;
use crate::legacy::highlight::Highlighter;
use crate::legacy::panel::{LineCx, fill_lines};
use crate::legacy::sys::Zone;

struct Counting;

thread_local! {
    static ON: StdCell<bool> = const { StdCell::new(false) };
    static COUNT: StdCell<usize> = const { StdCell::new(0) };
}

// SAFETY: делегирует System; счётчик — потоковый `Cell` без аллокаций.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
        if ON.with(StdCell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        // SAFETY: тот же контракт.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
        // SAFETY: тот же контракт.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: AllocLayout, new: usize) -> *mut u8 {
        if ON.with(StdCell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        // SAFETY: тот же контракт.
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ON.with(|o| o.set(true));
    f();
    ON.with(|o| o.set(false));
    COUNT.with(StdCell::get)
}

fn tab(n: u32) -> Tab {
    let mut t = Tab::new(TabId(7), Location::local(std::env::temp_dir().join("rc")));
    let entries: Vec<Entry> = (0..n)
        .map(|i| Entry {
            name: format!("file_{i:05}.txt").into(),
            size: u64::from(i) * 1000,
            mtime: 1_791_969_120_000_000_000,
            attrs: Attrs::default(),
            kind: EntryKind::File,
        })
        .collect();
    t.listing = Listing::with_entries(entries, (0..n).collect(), Gen(1));
    t
}

/// Кадр без окна: внутри `f` шрифты контекста готовы.
fn in_frame(ctx: &egui::Context, f: impl FnMut(&mut egui::Ui)) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, f);
    out.textures_delta.clear();
}

struct Fixture {
    ctx: egui::Context,
    fmt: FmtCfg,
    hl: Highlighter,
    cache: RowCache,
    lines: Vec<crate::legacy::panel::Line>,
}

impl Fixture {
    fn new() -> Self {
        let i18n = urfin_i18n::Catalog::embedded()
            .build(&"en".parse().unwrap())
            .0;
        Self {
            ctx: egui::Context::default(),
            fmt: FmtCfg::new(&i18n, false, Zone::utc()),
            hl: Highlighter::default(),
            cache: RowCache::new(),
            lines: Vec::new(),
        }
    }

    /// Кадр: строки `page` от `cursor.top`; число аллокаций в `fill_lines`.
    fn frame(&mut self, t: &Tab, page: u32) -> usize {
        let mut n = 0;
        let ctx = self.ctx.clone();
        in_frame(&ctx, |_| {
            let cx = LineCx {
                ctx: &ctx,
                fmt: &self.fmt,
                hl: &self.hl,
                font: egui::FontId::proportional(14.0),
                name_w: 300.0,
                digit_slot: Some(8.0),
                size: true,
                mtime: false,
            };
            n = allocations(|| fill_lines(&mut self.lines, &mut self.cache, &cx, t, page));
        });
        self.cache.end_frame();
        n
    }
}

#[test]
fn repeat_frame_hits_without_allocations() {
    let mut fx = Fixture::new();
    let t = tab(1000);
    fx.frame(&t, 40);
    let after_first = fx.cache.stats;
    // Строка 0 — `..` (только имя), дальше 39 × (имя + размер).
    assert_eq!(after_first.misses, 79);
    let allocs = fx.frame(&t, 40);
    let second = fx.cache.stats;
    assert_eq!(
        second.misses, after_first.misses,
        "повторный кадр — 100 % попаданий"
    );
    assert_eq!(second.hits - after_first.hits, 79);
    assert_eq!(allocs, 0, "аллокаций на повторном кадре");
    assert_eq!(fx.lines.len(), 40);
}

#[test]
fn patch_update_misses_only_changed_cells() {
    let mut fx = Fixture::new();
    let mut t = tab(100);
    fx.frame(&t, 30);
    let before = fx.cache.stats.misses;
    // `Patch.update`: тот же элемент, новый размер; поколение то же.
    std::sync::Arc::make_mut(&mut t.listing.entries)[5].size += 1;
    fx.frame(&t, 30);
    assert_eq!(
        fx.cache.stats.misses - before,
        2,
        "имя и размер одной записи"
    );
}

#[test]
fn size_stays_bounded_while_scrolling() {
    let mut fx = Fixture::new();
    let mut t = tab(5000);
    let page = 40;
    for frame in 0..1000u32 {
        t.cursor.top = (frame * 7) % (5000 - page);
        fx.frame(&t, page);
        assert!(
            fx.cache.len() <= 4 * 2 * page as usize + 2 * page as usize,
            "кадр {frame}: {}",
            fx.cache.len()
        );
    }
    // В покое — не больше 4 × видимых ячеек (2 на строку).
    assert!(
        fx.cache.len() <= 4 * 2 * page as usize,
        "{}",
        fx.cache.len()
    );
}

#[test]
fn epochs_reset_everything() {
    let mut fx = Fixture::new();
    let t = tab(10);
    fx.frame(&t, 10);
    assert!(!fx.cache.is_empty());
    fx.cache.sync_epochs(0, 1);
    assert!(fx.cache.is_empty(), "смена стиля");
    fx.frame(&t, 10);
    fx.cache.sync_epochs(5, 1);
    assert!(fx.cache.is_empty(), "смена языка");
}

#[test]
fn tabular_layout_aligns_digits() {
    let mut fx = Fixture::new();
    let mut t = tab(2);
    std::sync::Arc::make_mut(&mut t.listing.entries)[0].size = 111;
    std::sync::Arc::make_mut(&mut t.listing.entries)[1].size = 888;
    fx.frame(&t, 3);
    let sizes: Vec<f32> = fx
        .lines
        .iter()
        .filter_map(|l| l.size.as_ref().map(|s| s.2))
        .collect();
    assert_eq!(sizes.len(), 2);
    assert!((sizes[0] - sizes[1]).abs() < 0.01, "{sizes:?}");
}

#[test]
fn fingerprint_tracks_fields() {
    let e = Entry {
        name: "a".into(),
        size: 1,
        mtime: 2,
        attrs: Attrs::default(),
        kind: EntryKind::File,
    };
    let base = fingerprint(&e);
    for changed in [
        Entry {
            size: 9,
            ..e.clone()
        },
        Entry {
            mtime: 9,
            ..e.clone()
        },
        Entry {
            attrs: Attrs(Attrs::HIDDEN),
            ..e.clone()
        },
        Entry {
            kind: EntryKind::Dir,
            ..e.clone()
        },
        Entry {
            name: "b".into(),
            ..e.clone()
        },
    ] {
        assert_ne!(fingerprint(&changed), base);
    }
}
