//! Наблюдение опросом (S4-13): изменения ловятся за один интервал.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use urfin_core::{CancelToken, Entry, Location};
use urfin_vfs::{ListOpts, LocalFs, Poller, Vfs, WatchEvent};

const BASE: Duration = Duration::from_millis(100);

fn listing(fs: &LocalFs, loc: &Location) -> Vec<Entry> {
    let mut s = fs.list(loc, &ListOpts::default()).unwrap();
    let mut out = Vec::new();
    while s.next_batch(&mut out, &CancelToken::new()).unwrap() {}
    out
}

fn wait_due(p: &Poller) {
    let left = p.due().saturating_duration_since(Instant::now());
    std::thread::sleep(left);
}

fn summary(ev: &[WatchEvent]) -> Vec<String> {
    let mut v: Vec<String> = ev
        .iter()
        .map(|e| match e {
            WatchEvent::Added(n, Some(_)) => format!("+{n}"),
            WatchEvent::Removed(n) => format!("-{n}"),
            WatchEvent::Modified(n, Some(_)) => format!("~{n}"),
            other => format!("{other:?}"),
        })
        .collect();
    v.sort();
    v
}

#[test]
fn changes_are_seen_within_one_interval() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    fs::write(d.join("keep"), b"1").unwrap();
    fs::write(d.join("grow"), b"1").unwrap();
    fs::write(d.join("gone"), b"1").unwrap();
    let local = LocalFs::new();
    let loc = Location::local(d);
    let entries = listing(&local, &loc);
    let t0 = Instant::now();
    let mut p = Poller::new(
        Arc::new(LocalFs::new()),
        loc,
        &entries,
        BASE,
        Duration::ZERO,
        t0,
    );
    assert_eq!(p.due(), t0 + BASE);

    // Без изменений — пусто.
    wait_due(&p);
    assert!(
        p.poll(&CancelToken::new(), Instant::now())
            .unwrap()
            .is_empty()
    );

    fs::write(d.join("grow"), b"1234").unwrap();
    fs::remove_file(d.join("gone")).unwrap();
    fs::write(d.join("new"), b"").unwrap();
    fs::create_dir(d.join("sub")).unwrap();
    // Интервал, по которому назначен этот опрос (после опроса он пересчитается
    // по времени листинга).
    let interval = p.interval();
    let changed = Instant::now();
    // Опрос назначен не позже интервала после изменения. Проверяем расписание,
    // а не фактический старт: `sleep` на раннере macOS просыпается с опозданием
    // (docs/macos-notes.md, «Таймеры»).
    assert!(p.due() <= changed + interval);
    wait_due(&p);
    let ev = p.poll(&CancelToken::new(), Instant::now()).unwrap();
    assert_eq!(summary(&ev), ["+new", "+sub", "-gone", "~grow"]);

    // Снимок обновлён: повторный опрос пуст.
    wait_due(&p);
    assert!(
        p.poll(&CancelToken::new(), Instant::now())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn removed_dir_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("victim");
    fs::create_dir(&d).unwrap();
    let loc = Location::local(&d);
    let mut p = Poller::new(
        Arc::new(LocalFs::new()),
        loc,
        &[],
        BASE,
        Duration::ZERO,
        Instant::now(),
    );
    fs::remove_dir(&d).unwrap();
    let ev = p.poll(&CancelToken::new(), Instant::now()).unwrap();
    assert!(matches!(ev.as_slice(), [WatchEvent::Gone]), "{ev:?}");
}

/// Медленный листинг — реже опрос: интервал не меньше 10 × время чтения.
#[test]
fn interval_follows_listing_time() {
    let tmp = tempfile::tempdir().unwrap();
    let loc = Location::local(tmp.path());
    let p = Poller::new(
        Arc::new(LocalFs::new()),
        loc,
        &[],
        BASE,
        Duration::from_millis(40),
        Instant::now(),
    );
    assert_eq!(p.interval(), Duration::from_millis(400));
}
