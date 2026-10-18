//! `WatchHub` с настоящим core (S4-14): изменение каталога → `panel.refresh`,
//! жизненный цикл наблюдения по табам. На Windows — `ReadDirectoryChangesExW`,
//! на unix пока опрос (S4-13: `notify` — позже), интервал в тесте короткий.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use urfin_core::{Boot, Command, Core, ListingStatus, Location, Row, Services, Side, TabId, Waker};
use urfin_vfs::{VfsCfg, VfsListing, VfsRegistry};

struct NoWake;
impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

fn setup(left: &Path, right: &Path) -> (Core, Arc<VfsListing>) {
    let cfg = VfsCfg {
        poll_interval_ms: 100,
        ..VfsCfg::default()
    };
    let listing = Arc::new(VfsListing::new(Arc::new(VfsRegistry::with_local()), cfg));
    let services = Services {
        listing: listing.clone(),
        open_resolvers: Vec::new(),
    };
    let core = Core::new(
        Boot::new(Location::local(left), Location::local(right)),
        services,
        Arc::new(NoWake),
    );
    (core, listing)
}

fn pump_until(core: &mut Core, what: &str, done: impl Fn(&Core) -> bool) -> Duration {
    let t0 = Instant::now();
    while !done(core) {
        assert!(t0.elapsed() < Duration::from_secs(10), "timeout: {what}");
        core.pump(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(1));
    }
    t0.elapsed()
}

fn names(core: &Core, side: Side) -> Vec<String> {
    let t = core.state().panel(side).tab();
    (0..t.rows())
        .filter_map(|r| match t.row(r) {
            Some(Row::Entry { entry, .. }) => Some(entry.name.to_string()),
            _ => None,
        })
        .collect()
}

fn complete(core: &Core, side: Side) -> bool {
    matches!(
        core.state().panel(side).tab().listing.status,
        ListingStatus::Complete
    )
}

fn tab_id(core: &Core, side: Side) -> TabId {
    core.state().panel(side).tab().id
}

fn has(core: &Core, side: Side, name: &str) -> bool {
    names(core, side).iter().any(|n| n == name)
}

/// Приёмка: p50 «событие → обновлённый листинг» < 100 мс.
#[test]
fn created_file_appears_quickly() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let (mut core, listing) = setup(a.path(), b.path());
    pump_until(&mut core, "initial", |c| {
        complete(c, Side::Left) && complete(c, Side::Right)
    });
    pump_until(&mut core, "watched", |_| listing.watched_tabs().len() == 2);
    let mut times = Vec::new();
    for i in 0..9 {
        let name = format!("new{i}.txt");
        let t0 = Instant::now();
        fs::write(a.path().join(&name), b"x").unwrap();
        pump_until(&mut core, &name, |c| has(c, Side::Left, &name));
        times.push(t0.elapsed());
    }
    times.sort();
    let p50 = times[times.len() / 2];
    eprintln!("event → listing: p50 {p50:?}, all {times:?}");
    // На стенде p50 ≈ debounce 50 мс + листинг; на CI — с запасом.
    assert!(
        p50 < Duration::from_millis(if cfg!(windows) { 100 } else { 400 }),
        "{times:?}"
    );
    // Чужой каталог не трогается.
    assert_eq!(names(&core, Side::Right), Vec::<String>::new());
}

/// Изменения во время чтения каталога не теряются: наблюдение ставится до листинга.
#[test]
fn rename_and_delete_are_reflected() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    fs::write(a.path().join("old.txt"), b"x").unwrap();
    fs::write(a.path().join("doomed.txt"), b"x").unwrap();
    let (mut core, listing) = setup(a.path(), b.path());
    pump_until(&mut core, "initial", |c| complete(c, Side::Left));
    let left = tab_id(&core, Side::Left);
    pump_until(&mut core, "watched", |_| {
        listing.watched_tabs().contains(&left)
    });
    fs::rename(a.path().join("old.txt"), a.path().join("new.txt")).unwrap();
    fs::remove_file(a.path().join("doomed.txt")).unwrap();
    pump_until(&mut core, "rename+delete", |c| {
        names(c, Side::Left) == ["new.txt"]
    });
}

/// Скрытый таб не наблюдается; при показе — перечитывается.
#[test]
fn hidden_tab_refreshes_when_shown() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let (mut core, listing) = setup(a.path(), b.path());
    pump_until(&mut core, "initial", |c| complete(c, Side::Left));
    let first = tab_id(&core, Side::Left);
    pump_until(&mut core, "watched", |_| {
        listing.watched_tabs().contains(&first)
    });

    // Новый таб слева уходит в b — первый скрыт.
    core.dispatch(Command::new("tab.new"));
    core.dispatch(Command::new("panel.go").arg("path", Location::local(b.path())));
    pump_until(&mut core, "second tab", |c| {
        tab_id(c, Side::Left) != first && complete(c, Side::Left)
    });
    pump_until(&mut core, "first unwatched", |_| {
        !listing.watched_tabs().contains(&first)
    });

    fs::write(a.path().join("while-hidden.txt"), b"x").unwrap();
    std::thread::sleep(Duration::from_millis(150));
    core.pump(Duration::from_millis(10));

    core.dispatch(Command::new("tab.prev"));
    assert_eq!(tab_id(&core, Side::Left), first);
    pump_until(&mut core, "refreshed on show", |c| {
        has(c, Side::Left, "while-hidden.txt")
    });
    pump_until(&mut core, "watched again", |_| {
        listing.watched_tabs().contains(&first)
    });
}

#[test]
fn closing_tab_drops_watch() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let (mut core, listing) = setup(a.path(), b.path());
    pump_until(&mut core, "initial", |c| complete(c, Side::Left));
    core.dispatch(Command::new("tab.new"));
    let second = tab_id(&core, Side::Left);
    pump_until(&mut core, "second watched", |_| {
        listing.watched_tabs().contains(&second)
    });
    core.dispatch(Command::new("tab.close"));
    pump_until(&mut core, "second unwatched", |_| {
        !listing.watched_tabs().contains(&second)
    });
}

/// Каталог таба удалён — таб уходит в ближайший существующий родитель.
#[test]
fn deleted_directory_goes_up() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let deep = a.path().join("x").join("y");
    fs::create_dir_all(&deep).unwrap();
    let (mut core, listing) = setup(&deep, b.path());
    pump_until(&mut core, "initial", |c| complete(c, Side::Left));
    let left = tab_id(&core, Side::Left);
    pump_until(&mut core, "watched", |_| {
        listing.watched_tabs().contains(&left)
    });
    fs::remove_dir_all(a.path().join("x")).unwrap();
    let here = Location::local(a.path());
    pump_until(&mut core, "went up", |c| {
        let t = c.state().panel(Side::Left).tab();
        t.loc == here && complete(c, Side::Left)
    });
}

/// Шторм в видимом каталоге: не больше ~1 перечитывания в секунду (S4 §7).
#[test]
fn storm_is_rate_limited() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let (mut core, listing) = setup(a.path(), b.path());
    pump_until(&mut core, "initial", |c| complete(c, Side::Left));
    let left = tab_id(&core, Side::Left);
    pump_until(&mut core, "watched", |_| {
        listing.watched_tabs().contains(&left)
    });
    let generation = |c: &Core| c.state().panel(Side::Left).tab().listing.generation;
    let g0 = generation(&core);
    let t0 = Instant::now();
    let mut i = 0u32;
    // 3 с непрерывных созданий.
    while t0.elapsed() < Duration::from_secs(3) {
        for _ in 0..20 {
            fs::write(a.path().join(format!("s{i}")), b"").unwrap();
            i += 1;
        }
        core.pump(Duration::from_millis(1));
    }
    let refreshes = generation(&core).0 - g0.0;
    eprintln!("storm: {i} files in 3 s, {refreshes} refreshes");
    // Первые ~0,75 с — по max_latency (до 3), дальше — не чаще раза в секунду.
    assert!(refreshes <= 3 + 3, "{refreshes} refreshes");
    // После шторма список догоняет.
    let n = i as usize;
    pump_until(&mut core, "caught up", |c| names(c, Side::Left).len() == n);
}
