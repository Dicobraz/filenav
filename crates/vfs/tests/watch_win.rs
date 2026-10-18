//! Наблюдение за каталогом на Windows (S4-12): `ReadDirectoryChangesExW` + IOCP.

#![cfg(windows)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use urfin_core::{EntryKind, Location};
use urfin_vfs::{LocalFs, Vfs, VfsError, WatchEvent, WatchGuard, WatchId, WatchTx};

/// Приёмка: событие приходит за ≤ 50 мс (с запасом на загруженный CI — в `recv`).
const LATENCY: Duration = Duration::from_millis(50);

fn watch(dir: &Path) -> (WatchGuard, Receiver<(WatchId, WatchEvent)>) {
    let (tx, rx) = unbounded();
    let g = LocalFs::new()
        .watch(&Location::local(dir), WatchTx { id: WatchId(7), tx })
        .expect("watch");
    (g, rx)
}

/// Следующее событие; время от `t0` — в ответе.
fn next(rx: &Receiver<(WatchId, WatchEvent)>, t0: Instant) -> (WatchEvent, Duration) {
    let (id, ev) = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("event within 5 s");
    assert_eq!(id, WatchId(7));
    (ev, t0.elapsed())
}

/// Пропустить `Modified` (запись файла их даёт пачкой) до события другого вида.
fn next_not_modified(rx: &Receiver<(WatchId, WatchEvent)>, t0: Instant) -> (WatchEvent, Duration) {
    loop {
        let r = next(rx, t0);
        if !matches!(r.0, WatchEvent::Modified(..)) {
            return r;
        }
    }
}

fn drain(rx: &Receiver<(WatchId, WatchEvent)>) {
    while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}
}

#[test]
fn create_modify_rename_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let (_g, rx) = watch(d);
    let mut slow = Vec::new();

    let t0 = Instant::now();
    fs::write(d.join("a.txt"), b"hello").unwrap();
    let (ev, dt) = next(&rx, t0);
    slow.push(("create", dt));
    match ev {
        WatchEvent::Added(name, Some(e)) => {
            assert_eq!(name, "a.txt");
            assert_eq!(e.name, "a.txt");
            assert_eq!(e.kind, EntryKind::File);
        }
        other => panic!("expected Added with Entry: {other:?}"),
    }
    drain(&rx);

    let t0 = Instant::now();
    fs::write(d.join("a.txt"), b"hello, world").unwrap();
    let (ev, dt) = next(&rx, t0);
    slow.push(("modify", dt));
    let WatchEvent::Modified(name, entry) = ev else {
        panic!("expected Modified: {ev:?}");
    };
    assert_eq!(name, "a.txt");
    drain(&rx);
    // Размер в Entry — последнего события пачки; проверяем, что Entry вообще есть.
    assert!(entry.is_some());

    let t0 = Instant::now();
    fs::rename(d.join("a.txt"), d.join("b.txt")).unwrap();
    let (ev, dt) = next_not_modified(&rx, t0);
    slow.push(("rename", dt));
    match ev {
        WatchEvent::Renamed { from, to, entry } => {
            assert_eq!((from.as_str(), to.as_str()), ("a.txt", "b.txt"));
            assert_eq!(entry.expect("entry").size, 12);
        }
        other => panic!("expected Renamed: {other:?}"),
    }
    drain(&rx);

    let t0 = Instant::now();
    fs::create_dir(d.join("sub")).unwrap();
    let (ev, dt) = next(&rx, t0);
    slow.push(("mkdir", dt));
    match ev {
        WatchEvent::Added(name, Some(e)) => {
            assert_eq!(name, "sub");
            assert_eq!(e.kind, EntryKind::Dir);
        }
        other => panic!("expected Added dir: {other:?}"),
    }
    drain(&rx);

    let t0 = Instant::now();
    fs::remove_file(d.join("b.txt")).unwrap();
    let (ev, dt) = next_not_modified(&rx, t0);
    slow.push(("delete", dt));
    match ev {
        WatchEvent::Removed(name) => assert_eq!(name, "b.txt"),
        other => panic!("expected Removed: {other:?}"),
    }

    // Подкаталоги не наблюдаются (NonRecursive).
    fs::write(d.join("sub").join("inner.txt"), b"x").unwrap();
    while let Ok((_, ev)) = rx.recv_timeout(Duration::from_millis(200)) {
        let name = match &ev {
            WatchEvent::Added(n, _) | WatchEvent::Removed(n) | WatchEvent::Modified(n, _) => n,
            other => panic!("unexpected {other:?}"),
        };
        // Изменение содержимого `sub` — это Modified самого `sub`.
        assert_eq!(name, "sub", "{ev:?}");
    }

    eprintln!("latencies: {slow:?}");
    let over: Vec<_> = slow.iter().filter(|(_, dt)| *dt > LATENCY).collect();
    assert!(
        over.len() <= 1,
        "events slower than {LATENCY:?}: {over:?} (all: {slow:?})"
    );
}

#[test]
fn rename_out_is_removed_and_in_is_added() {
    let tmp = tempfile::tempdir().unwrap();
    let watched = tmp.path().join("w");
    let other = tmp.path().join("o");
    fs::create_dir(&watched).unwrap();
    fs::create_dir(&other).unwrap();
    fs::write(other.join("in.txt"), b"1").unwrap();
    fs::write(watched.join("out.txt"), b"2").unwrap();
    let (_g, rx) = watch(&watched);

    let t0 = Instant::now();
    fs::rename(watched.join("out.txt"), other.join("out.txt")).unwrap();
    match next_not_modified(&rx, t0).0 {
        WatchEvent::Removed(n) => assert_eq!(n, "out.txt"),
        other => panic!("expected Removed: {other:?}"),
    }
    fs::rename(other.join("in.txt"), watched.join("in.txt")).unwrap();
    match next_not_modified(&rx, t0).0 {
        WatchEvent::Added(n, Some(e)) => {
            assert_eq!(n, "in.txt");
            assert_eq!(e.size, 1);
        }
        other => panic!("expected Added: {other:?}"),
    }
}

#[test]
fn unicode_names() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let (_g, rx) = watch(d);
    fs::write(d.join("отчёт 🎉.txt"), b"").unwrap();
    match next(&rx, Instant::now()).0 {
        WatchEvent::Added(n, Some(e)) => {
            assert_eq!(n, "отчёт 🎉.txt");
            assert_eq!(e.name, n);
        }
        other => panic!("{other:?}"),
    }
}

/// Пачка без потерь: либо все имена, либо `Overflow` (тогда hub перечитает каталог).
fn burst(n: usize) {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let (_g, rx) = watch(d);
    for i in 0..n {
        fs::write(d.join(format!("f{i:06}")), b"").unwrap();
    }
    let mut seen = HashSet::new();
    let mut overflow = false;
    while let Ok((_, ev)) = rx.recv_timeout(Duration::from_millis(500)) {
        match ev {
            WatchEvent::Added(name, _) => {
                seen.insert(name);
            }
            WatchEvent::Overflow => overflow = true,
            WatchEvent::Modified(..) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    eprintln!("burst {n}: {} names, overflow: {overflow}", seen.len());
    assert!(
        overflow || seen.len() == n,
        "lost events without Overflow: {} of {n}",
        seen.len()
    );
}

#[test]
fn burst_5k_without_loss() {
    burst(5_000);
}

/// Приёмка S4-12 на 100k (≈ 20 с на NTFS): `cargo nextest run -p urfin-vfs burst_100k --run-ignored all`.
#[test]
#[ignore = "долгий: 100k файлов"]
fn burst_100k_without_loss() {
    burst(100_000);
}

#[test]
fn deleted_dir_gives_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("victim");
    fs::create_dir(&d).unwrap();
    fs::write(d.join("x"), b"").unwrap();
    let (_g, rx) = watch(&d);
    fs::remove_dir_all(&d).unwrap();
    let t0 = Instant::now();
    loop {
        match next(&rx, t0).0 {
            WatchEvent::Gone => break,
            WatchEvent::Removed(_) | WatchEvent::Modified(..) => {}
            other => panic!("expected Gone: {other:?}"),
        }
    }
}

#[test]
fn dropping_guard_releases_handle() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("before");
    fs::create_dir(&d).unwrap();
    let (g, rx) = watch(&d);
    drop(g);
    let renamed = tmp.path().join("after");
    fs::rename(&d, &renamed).unwrap();
    fs::remove_dir(&renamed).unwrap();
    // После drop событий нет: канал закрыт (WatchTx ушёл вместе с наблюдением).
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "no events after drop"
    );
}

#[test]
fn many_watches_and_restart() {
    for _ in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        let dirs: Vec<_> = (0..8)
            .map(|i| {
                let p = tmp.path().join(format!("d{i}"));
                fs::create_dir(&p).unwrap();
                p
            })
            .collect();
        let watches: Vec<_> = dirs.iter().map(|p| watch(p)).collect();
        for (p, (_, rx)) in dirs.iter().zip(&watches) {
            fs::write(p.join("x"), b"").unwrap();
            match next(rx, Instant::now()).0 {
                WatchEvent::Added(n, _) => assert_eq!(n, "x"),
                other => panic!("{other:?}"),
            }
        }
        // Все гарды уходят — поток останавливается; следующая итерация поднимет его заново.
        drop(watches);
    }
}

#[test]
fn missing_dir_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, _rx) = unbounded();
    let r = LocalFs::new().watch(
        &Location::local(tmp.path().join("nope")),
        WatchTx { id: WatchId(1), tx },
    );
    assert!(matches!(r, Err(VfsError::NotFound(_))), "{r:?}");
}
