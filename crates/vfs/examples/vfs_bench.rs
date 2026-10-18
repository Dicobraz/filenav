//! Метрики листинга S4 §7 в JSON (S4-11):
//! `cargo run -p urfin-vfs --example vfs_bench --release -- <каталог> [--runs 5] [--json <файл>]`.
//!
//! - `stream`: `Vfs::list` без core — `t_first` (первый батч), `t_done` (конец каталога);
//! - `core`: `panel.go` через `VfsListing` и `Core` — `t_first` (первые записи в табе),
//!   `t_sorted` (`Complete`: всё прочитано, отсортировано, итоговый `Order`);
//! - `peak_ws`/`peak_private`: пик памяти процесса после прогонов; `listing_peak_*` — он
//!   же минус пик до первого листинга (процесс, рантайм, пустой `Core`).
//!
//! Первый прогон — прогрев, в медиану не входит. `--core-only` — без `stream`: пик памяти
//! тогда — только листинг в табе (метрика §7 «peak RSS листинга»). `core.heap_peak_mb` —
//! пик кучи за прогон по считающему аллокатору (без кода и стеков ОС).

#![allow(unsafe_code, reason = "считающий GlobalAlloc — обёртка над System")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use urfin_core::{Boot, CancelToken, Command, Core, ListingStatus, Location, Services, Waker};
use urfin_vfs::{ListOpts, LocalFs, Vfs, VfsCfg, VfsListing, VfsRegistry};

/// Счётчик кучи: текущий объём и пик (S9 заведёт `count-alloc`, здесь — свой).
struct Counting;

static HEAP_NOW: AtomicUsize = AtomicUsize::new(0);
static HEAP_PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: делегирует в `System`, только считает байты.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: контракт `GlobalAlloc::alloc` передаётся как есть.
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = HEAP_NOW.fetch_add(l.size(), Relaxed) + l.size();
            HEAP_PEAK.fetch_max(now, Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: контракт `GlobalAlloc::dealloc` передаётся как есть.
        unsafe { System.dealloc(p, l) };
        HEAP_NOW.fetch_sub(l.size(), Relaxed);
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // SAFETY: контракт `GlobalAlloc::realloc` передаётся как есть.
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            // Пик считаем как «старый + новый»: realloc может копировать.
            let now = HEAP_NOW.fetch_add(new, Relaxed) + new;
            HEAP_PEAK.fetch_max(now, Relaxed);
            HEAP_NOW.fetch_sub(l.size(), Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Пик кучи за `f` сверх объёма на входе.
fn heap_peak_during<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = HEAP_NOW.load(Relaxed);
    HEAP_PEAK.store(base, Relaxed);
    let r = f();
    (r, HEAP_PEAK.load(Relaxed).saturating_sub(base))
}

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

/// `Vfs::list` до конца: время первого батча и всего, число записей.
fn stream(loc: &Location) -> (Duration, Duration, usize) {
    let fs = LocalFs::new();
    let cancel = CancelToken::new();
    let t0 = Instant::now();
    let mut s = fs.list(loc, &ListOpts::default()).expect("list");
    let mut out = Vec::new();
    let mut first = None;
    while s.next_batch(&mut out, &cancel).expect("next_batch") {
        if first.is_none() && !out.is_empty() {
            first = Some(t0.elapsed());
        }
    }
    let done = t0.elapsed();
    (first.unwrap_or(done), done, out.len())
}

/// Через core: от `panel.go` до первых записей и до `Complete`.
fn via_core(core: &mut Core, loc: &Location) -> (Duration, Duration) {
    let t0 = Instant::now();
    core.dispatch(Command::new("panel.go").arg("path", loc.clone()));
    let mut first = None;
    loop {
        core.pump(Duration::from_millis(2));
        let s = core.state();
        let t = s.panel(s.active).tab();
        if first.is_none() && !t.listing.entries.is_empty() {
            first = Some(t0.elapsed());
        }
        if matches!(t.listing.status, ListingStatus::Complete) {
            let done = t0.elapsed();
            return (first.unwrap_or(done), done);
        }
        if matches!(t.listing.status, ListingStatus::Failed(_)) {
            panic!("listing failed: {:?}", t.listing.status);
        }
        std::hint::spin_loop();
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut dir, mut runs, mut json, mut core_only) = (None, 5usize, None, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--core-only" => core_only = true,
            "--runs" => {
                runs = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--runs <n>");
            }
            "--json" => json = Some(PathBuf::from(args.next().expect("--json <файл>"))),
            d => dir = Some(PathBuf::from(d)),
        }
    }
    let dir = dir.expect("каталог");
    let loc = Location::local(&dir);
    let empty_dir = std::env::temp_dir().join("urfin-bench-empty");
    std::fs::create_dir_all(&empty_dir).expect("empty dir");
    let empty = Location::local(&empty_dir);

    // Пик до листинга: процесс, рантайм, пустой Core — вычитается из итогового.
    let services = Services {
        listing: Arc::new(VfsListing::new(
            Arc::new(VfsRegistry::with_local()),
            VfsCfg::default(),
        )),
        open_resolvers: Vec::new(),
    };
    let mut core = Core::new(
        Boot::new(empty.clone(), empty.clone()),
        services,
        Arc::new(NoWake),
    );
    core.pump(Duration::from_millis(50));
    let (base_ws, base_private) = urfin_vfs::diag::peak_memory().unwrap_or((0, None));
    let (mut s_first, mut s_done) = (Vec::new(), Vec::new());
    let mut n = 0;
    let stream_runs = if core_only { 0 } else { runs + 1 };
    for i in 0..stream_runs {
        let (f, d, count) = stream(&loc);
        n = count;
        if i > 0 {
            s_first.push(ms(f));
            s_done.push(ms(d));
        }
    }
    let mb = |b: u64| f64::from(u32::try_from(b / 1024).unwrap_or(u32::MAX)) / 1024.0;
    let (mut c_first, mut c_sorted, mut c_heap) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..=runs {
        // Прежний листинг освобождается до замера.
        core.dispatch(Command::new("panel.go").arg("path", empty.clone()));
        core.pump(Duration::from_millis(50));
        let ((f, d), heap) = heap_peak_during(|| via_core(&mut core, &loc));
        if i > 0 {
            c_first.push(ms(f));
            c_sorted.push(ms(d));
            c_heap.push(mb(u64::try_from(heap).unwrap_or(u64::MAX)));
        }
    }
    if core_only {
        let s = core.state();
        n = s.panel(s.active).tab().listing.entries.len();
    }
    let (ws, private) = urfin_vfs::diag::peak_memory().unwrap_or((0, None));
    let stream_json = if core_only {
        "null".to_owned()
    } else {
        format!(
            "{{\"t_first_ms\":{:.2},\"t_done_ms\":{:.2}}}",
            median(s_first),
            median(s_done)
        )
    };
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"dir\":{:?},\"entries\":{n},\"runs\":{runs},\"os\":{:?},\
         \"stream\":{stream_json},\
         \"core\":{{\"t_first_ms\":{:.2},\"t_sorted_ms\":{:.2},\"heap_peak_mb\":{:.1}}},\
         \"peak_ws_mb\":{:.1},\"peak_private_mb\":{},\"listing_peak_ws_mb\":{:.1},\"listing_peak_private_mb\":{}}}",
        dir.display().to_string(),
        std::env::consts::OS,
        median(c_first),
        median(c_sorted),
        median(c_heap),
        mb(ws),
        private.map_or_else(|| "null".to_owned(), |p| format!("{:.1}", mb(p))),
        mb(ws.saturating_sub(base_ws)),
        private.zip(base_private).map_or_else(
            || "null".to_owned(),
            |(p, b)| format!("{:.1}", mb(p.saturating_sub(b)))
        ),
    );
    println!("{out}");
    if let Some(p) = json {
        std::fs::write(&p, format!("{out}\n")).expect("write json");
    }
}
