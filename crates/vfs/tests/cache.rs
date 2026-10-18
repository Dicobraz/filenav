//! Кэш каталогов с настоящим core (S4-16): возврат в недавний каталог.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use urfin_core::{
    Attrs, Boot, Command, Core, Entry, ListingStatus, Location, Row, Services, Side, Waker,
};
use urfin_vfs::{
    Caps, ListOpts, ListStream, LocalFs, Metadata, Space, Vfs, VfsCfg, VfsError, VfsListing,
    VfsPath, VfsRead, VfsRegistry, WatchGuard, WatchTx,
};

struct NoWake;
impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

/// `LocalFs`, считающий чтения каталогов.
struct Counting {
    inner: LocalFs,
    lists: Arc<AtomicU32>,
}

impl Vfs for Counting {
    fn scheme(&self) -> &'static str {
        "local"
    }
    fn caps(&self, at: &VfsPath) -> Caps {
        self.inner.caps(at)
    }
    fn list(&self, dir: &VfsPath, opts: &ListOpts) -> Result<Box<dyn ListStream>, VfsError> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.inner.list(dir, opts)
    }
    fn stat(&self, p: &VfsPath, follow: bool) -> Result<Metadata, VfsError> {
        self.inner.stat(p, follow)
    }
    fn stat_entry(&self, d: &VfsPath, n: &str, a: Attrs) -> Result<Entry, VfsError> {
        self.inner.stat_entry(d, n, a)
    }
    fn open_read(&self, p: &VfsPath) -> Result<Box<dyn VfsRead>, VfsError> {
        self.inner.open_read(p)
    }
    fn space(&self, at: &VfsPath) -> Result<Space, VfsError> {
        self.inner.space(at)
    }
    fn local_path(&self, p: &VfsPath) -> Option<PathBuf> {
        self.inner.local_path(p)
    }
    fn watch(&self, dir: &VfsPath, tx: WatchTx) -> Result<WatchGuard, VfsError> {
        self.inner.watch(dir, tx)
    }
}

fn setup(start: &Path, other: &Path, cfg: VfsCfg) -> (Core, Arc<VfsListing>, Arc<AtomicU32>) {
    let lists = Arc::new(AtomicU32::new(0));
    let mut reg = VfsRegistry::default();
    reg.register(Arc::new(Counting {
        inner: LocalFs::new(),
        lists: Arc::clone(&lists),
    }));
    let listing = Arc::new(VfsListing::new(Arc::new(reg), cfg));
    let services = Services {
        listing: listing.clone(),
        open_resolvers: Vec::new(),
    };
    let core = Core::new(
        Boot::new(Location::local(start), Location::local(other)),
        services,
        Arc::new(NoWake),
    );
    (core, listing, lists)
}

fn pump_until(core: &mut Core, what: &str, done: impl Fn(&Core) -> bool) {
    let t0 = Instant::now();
    while !done(core) {
        assert!(t0.elapsed() < Duration::from_secs(10), "timeout: {what}");
        core.pump(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn names(core: &Core) -> Vec<String> {
    let t = core.state().panel(Side::Left).tab();
    (0..t.rows())
        .filter_map(|r| match t.row(r) {
            Some(Row::Entry { entry, .. }) => Some(entry.name.to_string()),
            _ => None,
        })
        .collect()
}

fn at(core: &Core, dir: &Path) -> bool {
    let t = core.state().panel(Side::Left).tab();
    t.loc == Location::local(dir) && matches!(t.listing.status, ListingStatus::Complete)
}

fn cursor(core: &Core) -> Option<String> {
    match core.state().panel(Side::Left).tab().current() {
        Some(Row::Entry { entry, .. }) => Some(entry.name.to_string()),
        _ => None,
    }
}

fn go(core: &mut Core, dir: &Path) {
    core.dispatch(Command::new("panel.go").arg("path", Location::local(dir)));
    pump_until(core, "navigated", |c| at(c, dir));
}

/// Дождаться, пока хаб положит каталог в кэш (обработка `Left` асинхронна).
fn wait_cached(core: &mut Core, listing: &VfsListing, dirs: usize) {
    pump_until(core, "cached", |_| listing.cache_stats().0 >= dirs);
}

fn tree() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    for i in 0..20 {
        fs::write(a.join(format!("a{i:02}.txt")), b"x").unwrap();
    }
    fs::write(b.join("b.txt"), b"x").unwrap();
    (tmp, a, b)
}

/// Приёмка: возврат в неизменённый локальный каталог — без чтения (`list` = 0).
#[cfg(windows)]
#[test]
fn fresh_return_reads_nothing() {
    let (tmp, a, b) = tree();
    let (mut core, listing, lists) = setup(&a, tmp.path(), VfsCfg::default());
    pump_until(&mut core, "initial", |c| at(c, &a));
    let left = core.state().panel(Side::Left).tab().id;
    pump_until(&mut core, "watched", |_| {
        listing.watched_tabs().contains(&left)
    });
    let first = names(&core);
    go(&mut core, &b);
    wait_cached(&mut core, &listing, 1);
    let before = lists.load(Ordering::SeqCst);
    go(&mut core, &a);
    assert_eq!(
        lists.load(Ordering::SeqCst),
        before,
        "fresh cache hit must not list"
    );
    assert_eq!(names(&core), first);
}

/// Каталог изменился, пока таб был в другом: сразу кэш, затем перечитанный
/// список; курсор остаётся на той же записи.
#[test]
fn stale_return_shows_cache_then_refreshes() {
    let (tmp, a, b) = tree();
    let (mut core, listing, lists) = setup(&a, tmp.path(), VfsCfg::default());
    pump_until(&mut core, "initial", |c| at(c, &a));
    go(&mut core, &b);
    wait_cached(&mut core, &listing, 1);
    fs::write(a.join("a99-new.txt"), b"x").unwrap();
    // Дать наблюдению кэша (Windows) заметить изменение.
    std::thread::sleep(Duration::from_millis(100));
    core.pump(Duration::from_millis(5));
    let before = lists.load(Ordering::SeqCst);
    core.dispatch(Command::new("panel.go").arg("path", Location::local(&a)));
    // Первым — кэш, без нового файла.
    pump_until(&mut core, "cached listing", |c| at(c, &a));
    let shown = names(&core);
    assert!(
        !shown.iter().any(|n| n == "a99-new.txt"),
        "cache first: {shown:?}"
    );
    core.dispatch(Command::new("panel.cursor.down").arg("n", 5));
    let cur = cursor(&core);
    // Затем — тихое перечитывание.
    pump_until(&mut core, "refreshed", |c| {
        names(c).iter().any(|n| n == "a99-new.txt")
    });
    assert!(lists.load(Ordering::SeqCst) > before, "stale hit re-reads");
    assert_eq!(cursor(&core), cur, "cursor kept on the same name");
}

#[test]
fn entry_limit_is_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs: Vec<PathBuf> = (0..5)
        .map(|i| {
            let d = tmp.path().join(format!("d{i}"));
            fs::create_dir(&d).unwrap();
            for k in 0..30 {
                fs::write(d.join(format!("f{k}")), b"").unwrap();
            }
            d
        })
        .collect();
    let cfg = VfsCfg {
        cache_entries: 70,
        ..VfsCfg::default()
    };
    let (mut core, listing, _) = setup(&dirs[0], tmp.path(), cfg);
    pump_until(&mut core, "initial", |c| at(c, &dirs[0]));
    for d in &dirs[1..] {
        go(&mut core, d);
        let left = core.state().panel(Side::Left).tab().id;
        pump_until(&mut core, "watched", |_| {
            listing.watched_tabs().contains(&left)
        });
    }
    wait_cached(&mut core, &listing, 2);
    let (n, entries) = listing.cache_stats();
    assert!(entries <= 70, "{n} dirs, {entries} entries");
    assert_eq!(n, 2);
}
