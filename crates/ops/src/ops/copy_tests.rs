//! Копирование и перенос на настоящих `Core`/`Runtime` и ФС (приёмка S5-05,
//! S5-06): дерево побайтно и со временем изменения, политики конфликтов,
//! перенос в пределах тома и между «томами», отмена посреди файла, обход с
//! маской и ссылками, убитый посреди копии процесс (fault injection).

#![allow(clippy::many_single_char_names, reason = "короткие имена в тестах")]

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use compact_str::CompactString;
use urfin_core::{JobId, JobState, Level, Location, UiEvent};

use super::*;
use crate::engine::tests::H;

fn tmp() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("urfin-").tempdir().unwrap()
}

fn old(secs: u64) -> SystemTime {
    // Не круглое число 100-нс тиков — проверка точности NTFS.
    SystemTime::UNIX_EPOCH + Duration::from_nanos(secs * 1_000_000_000 + 1_234_500)
}

fn set_mtime(p: &Path, t: SystemTime) {
    let f = if p.is_dir() {
        crate::sys::fs::set_dir_mtime(p, t).unwrap();
        return;
    } else {
        std::fs::File::options().write(true).open(p).unwrap()
    };
    f.set_modified(t).unwrap();
}

fn write(p: &Path, data: &[u8], t: SystemTime) {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, data).unwrap();
    set_mtime(p, t);
}

/// Детерминированные «случайные» данные.
fn bytes(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn mtime(p: &Path) -> SystemTime {
    std::fs::symlink_metadata(p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .modified()
        .unwrap()
}

fn hash(p: &Path) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(&std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())));
    h.finish()
}

/// Дерево: относительный путь → (каталог?, хэш, время изменения).
fn snapshot(root: &Path) -> BTreeMap<String, (bool, u64, SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            let ft = e.file_type().unwrap();
            if ft.is_dir() {
                stack.push(p.clone());
                out.insert(rel, (true, 0, mtime(&p)));
            } else {
                out.insert(rel, (false, hash(&p), mtime(&p)));
            }
        }
    }
    out
}

/// Файлов в дереве, без чтения (дерево меняется под ногами).
fn count_files(root: &Path) -> u64 {
    let mut n = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            } else {
                n += 1;
            }
        }
    }
    n
}

fn opts(conflict: ConflictPolicy) -> OpOptions {
    OpOptions {
        conflict,
        ..OpOptions::default()
    }
}

fn transfer(
    mode: Mode,
    base: &Path,
    items: &[&str],
    dest: &Path,
    raw: Option<&str>,
    opts: OpOptions,
) -> Transfer {
    let mask = opts.filter.as_deref().map(|m| Mask::parse(m).unwrap());
    let base = Location::local(base);
    let dest_loc = Location::local(dest);
    Transfer::new(
        mode,
        Start {
            req: OpRequest {
                kind: if mode == Mode::Move {
                    OpKind::Move
                } else {
                    OpKind::Copy
                },
                base,
                items: items.iter().map(|s| CompactString::from(*s)).collect(),
                dest: None,
                opts,
            },
            dest: dest_loc,
            dest_raw: raw.map_or_else(|| dest.display().to_string(), str::to_owned),
            mask,
            small_file: 1 << 20,
        },
    )
}

struct Run {
    state: JobState,
    toasts: Vec<(Level, String)>,
    report: Arc<Report>,
    id: JobId,
}

fn run(h: &mut H, t: Transfer) -> Run {
    let id = h.start(4, t);
    let state = h.wait_done(id);
    // Тост итога приходит сообщением `ops.finished` до `job.state`.
    for _ in 0..5 {
        h.pump();
    }
    let toasts = h
        .core
        .take_ui_events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::Toast { level, text } => Some((level, text)),
            _ => None,
        })
        .collect();
    let report = h.report(id);
    Run {
        state,
        toasts,
        report,
        id,
    }
}

fn done(r: &Run) -> JobSummary {
    match &r.state {
        JobState::Done(s) => s.clone(),
        st => panic!("job {:?}: {st:?}, failed {:?}", r.id, r.report.failed),
    }
}

fn no_temps(root: &Path) {
    let mut stack = vec![verbatim(root)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            assert!(!name.contains(".fnv~"), "temp file left: {name}");
            if e.file_type().unwrap().is_dir() {
                stack.push(e.path());
            }
        }
    }
}

/// Приёмка S5-06: дерево (мелкие, крупные ≥ `small_file`, пустые файлы и каталоги,
/// скрытый файл, ADS, имя с хвостовой точкой, путь длиннее `MAX_PATH`)
/// копируется побайтно (сверка хэшами), время изменения файлов и каталогов —
/// с точностью NTFS (100 нс).
#[test]
fn copies_tree_byte_identical_with_times() {
    let d = tmp();
    let src = d.path().join("src");
    let t = old(1_100_000_000);
    write(&src.join("a.txt"), b"hello", t);
    write(&src.join("big.bin"), &bytes(3 << 20, 1), t);
    write(&src.join("one-mib.bin"), &bytes(1 << 20, 2), t);
    write(&src.join("empty"), b"", t);
    write(&src.join("sub/deep/x.dat"), &bytes(1_500_000, 3), t);
    write(&src.join("sub/y"), &bytes(70_000, 4), t);
    std::fs::create_dir_all(src.join("emptydir")).unwrap();
    // Скрытый файл и поток Zone.Identifier (Mark-of-the-Web).
    write(&src.join("hidden.cfg"), b"h", t);
    crate::sys::fs::set_attrs(&src.join("hidden.cfg"), crate::sys::fs::ATTR_HIDDEN).unwrap();
    std::fs::write(
        src.join("a.txt:Zone.Identifier"),
        b"[ZoneTransfer]\r\nZoneId=3\r\n",
    )
    .unwrap();
    set_mtime(&src.join("a.txt"), t);
    // Имя с хвостовой точкой и длинный путь — только через `\\?\`.
    let vsrc = verbatim(&src);
    write(&vsrc.join("trail."), b"dot", t);
    let mut long = vsrc.join("long");
    for i in 0..12 {
        long = long.join(format!("segment-number-{i:02}-xxxxxxxxxx"));
    }
    write(&long.join("f.txt"), b"long", t);
    assert!(long.as_os_str().len() > 300);
    // Время каталогов — последним (запись детей его сбивает).
    for dir in snapshot(&vsrc).into_iter().filter(|(_, v)| v.0) {
        set_mtime(&vsrc.join(&dir.0), old(1_000_000_000));
    }
    set_mtime(&vsrc, old(1_000_000_000));
    let dst = d.path().join("dst");
    std::fs::create_dir(&dst).unwrap();

    let mut h = H::new(64);
    let r = run(
        &mut h,
        transfer(
            Mode::Copy,
            d.path(),
            &["src"],
            &dst,
            None,
            OpOptions::default(),
        ),
    );
    let s = done(&r);
    let a = snapshot(&vsrc);
    let b = snapshot(&verbatim(&dst.join("src")));
    assert_eq!(a, b, "tree, hashes and mtimes");
    assert_eq!(s.errors, 0);
    assert_eq!(s.items, a.values().filter(|v| !v.0).count() as u64);
    assert_eq!(
        s.bytes,
        (3 << 20) + (1 << 20) + 1_500_000 + 70_000 + 5 + 1 + 3 + 4
    );
    assert_eq!(mtime(&dst.join("src")), old(1_000_000_000), "top dir mtime");
    let hid = crate::sys::fs::attrs(&dst.join("src/hidden.cfg")).unwrap();
    assert_ne!(hid & crate::sys::fs::ATTR_HIDDEN, 0, "hidden attr");
    let zone = std::fs::read(dst.join("src/a.txt:Zone.Identifier")).unwrap();
    assert!(zone.starts_with(b"[ZoneTransfer]"), "ADS copied");
    no_temps(&dst);
    assert!(r.toasts.is_empty(), "{:?}", r.toasts);
    assert_eq!(h.eng.live_threads(), 0);
}

/// Конфликты: каждая политика на одном наборе (такой же, старее, новее, нет).
#[test]
fn conflict_policies() {
    let cases: [(ConflictPolicy, &[&str], &[&str]); 7] = [
        // (политика, заменены, пропущены)
        (ConflictPolicy::Skip, &[], &["same", "older", "newer"]),
        (ConflictPolicy::Overwrite, &["same", "older", "newer"], &[]),
        (
            ConflictPolicy::OverwriteIfNewer,
            &["older"],
            &["same", "newer"],
        ),
        (
            ConflictPolicy::OverwriteIfDiffers,
            &["older", "newer"],
            &["same"],
        ),
        (
            ConflictPolicy::SkipIdentical,
            &[],
            &["same", "older", "newer"],
        ),
        (ConflictPolicy::Ask, &[], &["same", "older", "newer"]),
        (
            ConflictPolicy::RenameExisting(RenameScheme::Counter),
            &["same", "older", "newer"],
            &[],
        ),
    ];
    let mut h = H::new(64);
    for (policy, replaced, skipped) in cases {
        let d = tmp();
        let (src, dst) = (d.path().join("s"), d.path().join("d"));
        let t = old(1_500_000_000);
        for n in ["same", "older", "newer", "new", "big"] {
            let data = if n == "big" {
                bytes(2 << 20, 9)
            } else {
                format!("src-{n}").into_bytes()
            };
            write(&src.join(n), &data, t);
        }
        write(&dst.join("same"), b"src-same", t);
        write(&dst.join("older"), b"dst-older!", old(1_400_000_000));
        write(&dst.join("newer"), b"dst-newer!", old(1_600_000_000));
        write(&dst.join("big"), b"dst-big", old(1_400_000_000));
        let items = ["same", "older", "newer", "new", "big"];
        let r = run(
            &mut h,
            transfer(Mode::Copy, &src, &items, &dst, None, opts(policy)),
        );
        let s = done(&r);
        let content = |n: &str| std::fs::read(dst.join(n)).unwrap();
        for n in ["same", "older", "newer"] {
            let from_src = content(n) == format!("src-{n}").into_bytes();
            if replaced.contains(&n) {
                assert!(from_src, "{policy:?}: {n} must be replaced");
                assert_eq!(mtime(&dst.join(n)), t, "{policy:?}: {n} mtime");
            } else if n != "same" {
                assert!(!from_src, "{policy:?}: {n} must stay");
            }
        }
        assert_eq!(content("new"), b"src-new", "{policy:?}");
        let n_skipped = skipped.len() as u64
            + u64::from(!matches!(
                policy,
                ConflictPolicy::Overwrite
                    | ConflictPolicy::OverwriteIfNewer
                    | ConflictPolicy::OverwriteIfDiffers
                    | ConflictPolicy::RenameExisting(_)
            ));
        assert_eq!(s.skipped, n_skipped, "{policy:?}: skipped (big included)");
        if matches!(policy, ConflictPolicy::RenameExisting(_)) {
            assert_eq!(
                std::fs::read(dst.join("older.bak~1")).unwrap(),
                b"dst-older!"
            );
            assert_eq!(std::fs::read(dst.join("big.bak~1")).unwrap(), b"dst-big");
        }
        let asked = r.toasts.iter().any(|(l, _)| *l == Level::Warn);
        let asks = matches!(policy, ConflictPolicy::Ask | ConflictPolicy::SkipIdentical);
        assert_eq!(asked, asks, "{policy:?}: Ask → skip toast {:?}", r.toasts);
        no_temps(&dst);
    }
}

/// `RenameNew`: существующие остаются, новые — `имя (2).ext`, затем `(3)`.
#[test]
fn rename_new_picks_free_names() {
    let d = tmp();
    let (src, dst) = (d.path().join("s"), d.path().join("d"));
    let t = old(1_500_000_000);
    write(&src.join("r.txt"), b"new", t);
    write(&dst.join("r.txt"), b"old", t);
    write(&dst.join("r (2).txt"), b"old2", t);
    let mut h = H::new(64);
    let policy = ConflictPolicy::RenameNew(RenameScheme::Counter);
    done(&run(
        &mut h,
        transfer(Mode::Copy, &src, &["r.txt"], &dst, None, opts(policy)),
    ));
    assert_eq!(std::fs::read(dst.join("r.txt")).unwrap(), b"old");
    assert_eq!(std::fs::read(dst.join("r (2).txt")).unwrap(), b"old2");
    assert_eq!(std::fs::read(dst.join("r (3).txt")).unwrap(), b"new");
}

/// Назначение по Far: файл под новым именем, завершающий разделитель —
/// создать каталог, «сам в себя» и «внутрь себя» — ошибки в тосте.
#[test]
fn destination_semantics_and_self_copy() {
    let d = tmp();
    let t = old(1_500_000_000);
    write(&d.path().join("f.txt"), b"f", t);
    write(&d.path().join("dir/x"), b"x", t);
    let mut h = H::new(64);
    let renamed = d.path().join("g.txt");
    done(&run(
        &mut h,
        transfer(
            Mode::Copy,
            d.path(),
            &["f.txt"],
            &renamed,
            None,
            OpOptions::default(),
        ),
    ));
    assert_eq!(std::fs::read(&renamed).unwrap(), b"f");
    let into = d.path().join("new-dir");
    let raw = format!("{}\\", into.display());
    done(&run(
        &mut h,
        transfer(
            Mode::Copy,
            d.path(),
            &["f.txt"],
            &into,
            Some(&raw),
            OpOptions::default(),
        ),
    ));
    assert_eq!(std::fs::read(into.join("f.txt")).unwrap(), b"f");

    let r = run(
        &mut h,
        transfer(
            Mode::Copy,
            d.path(),
            &["f.txt"],
            d.path(),
            None,
            OpOptions::default(),
        ),
    );
    assert_eq!(done(&r).errors, 1, "onto itself");
    assert!(r.toasts.iter().any(|(l, _)| *l == Level::Error));
    let r = run(
        &mut h,
        transfer(
            Mode::Copy,
            d.path(),
            &["dir"],
            &d.path().join("dir/sub"),
            None,
            OpOptions::default(),
        ),
    );
    assert_eq!(done(&r).errors, 1, "into itself");
    assert!(!d.path().join("dir/sub/dir").exists());
}

/// Перенос в пределах тома: каталог — одним rename (времена и содержимое те
/// же), в существующий — по детям с удалением пустого источника; смена только
/// регистра имени.
#[test]
fn move_same_volume() {
    let d = tmp();
    let t = old(1_500_000_000);
    let (src, dst) = (d.path().join("s"), d.path().join("d"));
    write(&src.join("a/f1"), b"1", t);
    write(&src.join("a/sub/f2"), &bytes(2 << 20, 5), t);
    write(&src.join("b/f3"), b"3", t);
    write(&src.join("top.txt"), b"t", t);
    write(&dst.join("b/keep"), b"k", t);
    let before = snapshot(&src.join("a"));
    let mut h = H::new(64);
    let r = run(
        &mut h,
        transfer(
            Mode::Move,
            &src,
            &["a", "b", "top.txt"],
            &dst,
            None,
            OpOptions::default(),
        ),
    );
    let s = done(&r);
    assert_eq!(s.errors, 0, "{:?}", r.report.failed);
    assert_eq!(snapshot(&dst.join("a")), before, "renamed as a whole");
    assert_eq!(std::fs::read(dst.join("b/f3")).unwrap(), b"3");
    assert_eq!(std::fs::read(dst.join("b/keep")).unwrap(), b"k");
    assert_eq!(std::fs::read(dst.join("top.txt")).unwrap(), b"t");
    assert_eq!(std::fs::read_dir(&src).unwrap().count(), 0, "sources gone");
    assert_eq!(s.bytes, 0, "rename moves no data");

    write(&dst.join("case.txt"), b"c", t);
    let to = dst.join("CASE.TXT");
    done(&run(
        &mut h,
        transfer(
            Mode::Move,
            &dst,
            &["case.txt"],
            &to,
            None,
            OpOptions::default(),
        ),
    ));
    let names: Vec<String> = std::fs::read_dir(&dst)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"CASE.TXT".to_owned()), "{names:?}");
}

/// Перенос между томами: копия и удаление источника (принудительно — без
/// второго тома на стенде), read-only источник тоже удаляется, каталоги — в
/// Finalizer; конфликт при `Ask` оставляет источник.
#[test]
fn move_cross_volume_copies_and_deletes() {
    let d = tmp();
    let t = old(1_500_000_000);
    let (src, dst) = (d.path().join("s"), d.path().join("d"));
    write(&src.join("t/a"), b"a", t);
    write(&src.join("t/deep/b"), &bytes(1 << 21, 6), t);
    write(&src.join("t/ro"), b"ro", t);
    crate::sys::fs::set_attrs(&src.join("t/ro"), crate::sys::fs::ATTR_READONLY).unwrap();
    write(&src.join("t/clash"), b"src", t);
    write(&dst.join("t/clash"), b"dst", t);
    let before = snapshot(&src.join("t"));
    let tr = transfer(Mode::Move, &src, &["t"], &dst, None, OpOptions::default());
    tr.info
        .set(Info {
            tol_ns: 0,
            rename: false,
        })
        .unwrap();
    let mut h = H::new(64);
    let r = run(&mut h, tr);
    let s = done(&r);
    assert_eq!((s.errors, s.skipped), (0, 1), "{:?}", r.report.failed);
    let after = snapshot(&dst.join("t"));
    for (k, v) in &before {
        if k != "clash" && !v.0 {
            assert_eq!(after.get(k), Some(v), "{k}");
        }
    }
    assert_eq!(std::fs::read(dst.join("t/clash")).unwrap(), b"dst");
    // Источник: остался только конфликтный файл (и его каталог).
    let left: Vec<String> = snapshot(&src).into_keys().collect();
    assert_eq!(left, ["t", "t\\clash"], "{left:?}");
    assert!(
        r.toasts.iter().any(|(l, _)| *l == Level::Warn),
        "asked → skipped"
    );
}

/// Отмена посреди крупного файла: job `Cancelled`, ни назначения, ни
/// временного файла.
#[test]
fn cancel_mid_file() {
    let d = tmp();
    let src = d.path().join("huge.bin");
    let f = std::fs::File::create(&src).unwrap();
    f.set_len(768 << 20).unwrap();
    drop(f);
    let dst = d.path().join("out");
    std::fs::create_dir(&dst).unwrap();
    let mut h = H::new(64);
    let id = h.start(
        2,
        transfer(
            Mode::Copy,
            d.path(),
            &["huge.bin"],
            &dst,
            None,
            OpOptions::default(),
        ),
    );
    let progress = |h: &H| {
        h.core
            .state()
            .jobs
            .get(id)
            .unwrap()
            .progress
            .bytes_done
            .load(Ordering::Relaxed)
    };
    h.pump_until("first chunk", |h| {
        progress(h) > 0 || !h.state(id).is_active()
    });
    let at = progress(&h);
    let t0 = Instant::now();
    h.cmd("job.cancel", id);
    let st = h.wait_done(id);
    eprintln!("cancelled at {at} bytes in {:?}", t0.elapsed());
    assert_eq!(st, JobState::Cancelled, "copied too fast to cancel? {at}");
    assert!(at < 768 << 20);
    assert_eq!(
        std::fs::read_dir(&dst).unwrap().count(),
        0,
        "no partial or temp file"
    );
}

fn mklink_j(link: &Path, target: &Path) {
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(ok, "mklink /J");
}

/// Обход (S5-05): маска — только к файлам (каталоги создаются), ссылка при
/// `Preserve` копируется ссылкой, при `Skip` — нет, при `Follow` цикл через
/// junction на предка не зацикливает обход.
#[test]
fn walker_mask_and_links() {
    let d = tmp();
    let t = old(1_500_000_000);
    let src = d.path().join("s");
    write(&src.join("a.txt"), b"a", t);
    write(&src.join("b.log"), b"b", t);
    write(&src.join("sub/c.txt"), b"c", t);
    write(&src.join("sub/d.log"), b"d", t);
    let target = d.path().join("target");
    write(&target.join("t.txt"), b"t", t);
    mklink_j(&src.join("j"), &target);
    mklink_j(&src.join("sub").join("up"), &src);
    let mut h = H::new(64);

    let dst = d.path().join("masked");
    std::fs::create_dir(&dst).unwrap();
    let o = OpOptions {
        filter: Some("*.txt".into()),
        links: LinkMode::Skip,
        ..OpOptions::default()
    };
    done(&run(
        &mut h,
        transfer(Mode::Copy, d.path(), &["s"], &dst, None, o),
    ));
    let got: Vec<String> = snapshot(&dst).into_keys().collect();
    assert_eq!(got, ["s", "s\\a.txt", "s\\sub", "s\\sub\\c.txt"]);

    let dst = d.path().join("preserve");
    std::fs::create_dir(&dst).unwrap();
    done(&run(
        &mut h,
        transfer(
            Mode::Copy,
            d.path(),
            &["s"],
            &dst,
            None,
            OpOptions::default(),
        ),
    ));
    let j = dst.join("s/j");
    assert!(
        std::fs::symlink_metadata(&j)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(j.join("t.txt")).unwrap(), b"t", "same target");
    assert!(
        std::fs::symlink_metadata(dst.join("s/sub/up"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    let dst = d.path().join("follow");
    std::fs::create_dir(&dst).unwrap();
    let o = OpOptions {
        links: LinkMode::Follow,
        ..OpOptions::default()
    };
    let r = run(
        &mut h,
        transfer(Mode::Copy, d.path(), &["s"], &dst, None, o),
    );
    let s = done(&r);
    assert_eq!(
        std::fs::read(dst.join("s/j/t.txt")).unwrap(),
        b"t",
        "followed"
    );
    assert!(
        !std::fs::symlink_metadata(dst.join("s/j"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(s.skipped, 1, "cycle s/sub/up skipped");
    assert!(!dst.join("s/sub/up").exists());
}

const CHILD_ENV: &str = "URFIN_COPY_KILL_CHILD";

/// Дочерний процесс fault-injection: копирует `src` в `dst` и не выходит
/// (родитель убивает его посреди копии). Без переменной окружения — ничего.
#[test]
fn kill_child() {
    let Ok(dirs) = std::env::var(CHILD_ENV) else {
        return;
    };
    let (src, dst) = dirs.split_once('|').unwrap();
    let (src, dst) = (PathBuf::from(src), PathBuf::from(dst));
    let mut h = H::new(64);
    let base = src.parent().unwrap().to_path_buf();
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let o = OpOptions {
        workers: std::num::NonZeroU8::new(4),
        ..OpOptions::default()
    };
    let id = h.start(4, transfer(Mode::Copy, &base, &[&name], &dst, None, o));
    std::fs::write(dst.with_extension("started"), b"1").unwrap();
    let st = h.wait_done(id);
    // Сюда доходит, только если родитель не успел: тест родителя это видит.
    std::fs::write(dst.with_extension("finished"), format!("{st:?}")).unwrap();
}

/// Приёмка S5-06: процесс убит (`TerminateProcess`, как kill -9) посреди
/// копии — ни одного файла с исходным временем изменения и неверным
/// содержимым; недописанные — только временные `.…fnv~…` или с чужим временем.
#[test]
fn killed_copy_leaves_no_false_complete_files() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }
    let d = tmp();
    let src = d.path().join("src");
    let t = old(1_300_000_000);
    // Мелкие (сразу под своим именем) и крупные (через временный) вперемешку.
    for i in 0..400u64 {
        let size = match i % 8 {
            0 => (3 << 20) + i as usize,
            1 => (1 << 20) + 17,
            _ => 300_000 + 1_000 * i as usize,
        };
        write(
            &src.join(format!("d{}/f{i}.bin", i % 10)),
            &bytes(size, i),
            t,
        );
    }
    let dst = d.path().join("dst");
    std::fs::create_dir(&dst).unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(exe)
        .args(["--exact", "ops::copy::tests::kill_child", "--nocapture"])
        .env(CHILD_ENV, format!("{}|{}", src.display(), dst.display()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let started = dst.with_extension("started");
    let t0 = Instant::now();
    while !started.exists() {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "child did not start"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // Дать накопировать немного (кэш ОС копирует быстро): убить в начале.
    let total = count_files(&src);
    let mut seen = 0;
    while seen < total / 20 && t0.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(20));
        seen = count_files(&dst);
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        !dst.with_extension("finished").exists(),
        "copy finished before kill: make the data bigger"
    );
    let srcs = snapshot(&src);
    let (mut complete, mut partial, mut temps) = (0, 0, 0);
    for (rel, (dir, h, m)) in snapshot(&dst.join("src")) {
        if dir {
            continue;
        }
        if rel.contains(".fnv~") {
            temps += 1;
            continue;
        }
        let (_, sh, sm) = srcs[&rel];
        if m == sm {
            assert_eq!(h, sh, "{rel}: source mtime but wrong content");
            complete += 1;
        } else {
            partial += 1;
        }
    }
    eprintln!("killed: {complete} complete, {partial} partial, {temps} temp of {total}");
    assert!(complete > 0 && complete < total);
}

/// Замер (не приёмка): копия датасета `copy-<N>` (`urfin-testdata`, по
/// умолчанию 10k файлов в 100 каталогах; `URFIN_BENCH_N`) в `%TEMP%`.
/// `cargo nextest run --release -p urfin-ops bench_copy --run-ignored only --no-capture`.
#[test]
#[ignore = "замер, запускается руками"]
fn bench_copy_dataset() {
    let n: usize = std::env::var("URFIN_BENCH_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);
    let spec = urfin_testdata::TreeSpec {
        n,
        seed: 1,
        layout: urfin_testdata::Layout::Nav { dirs: n / 100 },
    };
    let src = urfin_testdata::dataset("urfin", &format!("copy-{n}"), &spec).unwrap();
    let d = tmp();
    let mut h = H::new(64);
    let base = src.parent().unwrap().to_path_buf();
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let t0 = Instant::now();
    let r = run(
        &mut h,
        transfer(
            Mode::Copy,
            &base,
            &[&name],
            d.path(),
            None,
            OpOptions::default(),
        ),
    );
    let dt = t0.elapsed();
    let s = done(&r);
    #[allow(clippy::cast_precision_loss, reason = "замер")]
    let rate = s.items as f64 / dt.as_secs_f64();
    eprintln!(
        "copy {n}: {} files, {} bytes in {dt:?} ({rate:.0} files/s), src {}",
        s.items,
        s.bytes,
        src.display()
    );
    assert_eq!(count_files(&src), count_files(&d.path().join(&name)));
}
