//! Копирование и перенос на настоящих `Core`/`Runtime` и ФС (приёмка S5-05,
//! S5-06): дерево побайтно и со временем изменения, политики конфликтов,
//! перенос в пределах тома и между «томами», отмена посреди файла, обход с
//! маской и ссылками, убитый посреди копии процесс (fault injection);
//! немодальные конфликты с автоответчиком (приёмка S5-07); ошибки,
//! автоповтор и `ops.retry` (приёмка S5-11); журнал, «Продолжить» после
//! убитого процесса и вопрос при выходе (приёмка S5-12).

#![allow(clippy::many_single_char_names, reason = "короткие имена в тестах")]

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use compact_str::CompactString;
use urfin_core::{Command, JobId, JobState, Level, Location, PromptDetail, PromptSpec, UiEvent};

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
            journal_dir: None,
        },
    )
}

struct Run {
    state: JobState,
    /// Вопросов задано (автоответ — «Пропустить»).
    asked: u32,
    toasts: Vec<(Level, String)>,
    report: Arc<Report>,
    id: JobId,
}

fn run(h: &mut H, t: Transfer) -> Run {
    let id = h.start(4, t);
    let mut asked = 0;
    let t0 = Instant::now();
    while h.state(id).is_active() || h.eng.live_threads() > 0 {
        assert!(t0.elapsed() < Duration::from_secs(30), "timeout: job end");
        if has_prompt(h) {
            asked += 1;
            reply(h, "skip", false);
        }
        h.pump();
    }
    let state = h.state(id);
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
        asked,
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
        // Ask: вопрос на каждый занятый файл (автоответ «Пропустить»);
        // SkipIdentical — на отличающиеся.
        let asks = match policy {
            ConflictPolicy::Ask => 4,
            ConflictPolicy::SkipIdentical => 3,
            _ => 0,
        };
        assert_eq!(r.asked, asks, "{policy:?}: questions");
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
/// Finalizer; конфликт при `Ask` — вопрос, «Пропустить» оставляет источник.
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
    assert_eq!(r.asked, 1, "clash asked, answered skip");
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

/// Дочерний процесс fault-injection: копирует (`copy|src|dst`) или переносит
/// как между томами (`move|src|dst`) и не выходит — родитель убивает его
/// посреди работы. Без переменной окружения — ничего.
#[test]
fn kill_child() {
    let Ok(arg) = std::env::var(CHILD_ENV) else {
        return;
    };
    let parts: Vec<&str> = arg.split('|').collect();
    let (mode, src, dst) = (parts[0], parts[1], parts[2]);
    let (src, dst) = (PathBuf::from(src), PathBuf::from(dst));
    let mut h = H::new(64);
    let base = src.parent().unwrap().to_path_buf();
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let o = OpOptions {
        workers: std::num::NonZeroU8::new(4),
        ..OpOptions::default()
    };
    let t = if mode == "move" {
        let t = transfer(Mode::Move, &base, &[&name], &dst, None, o);
        t.info
            .set(Info {
                tol_ns: 0,
                rename: false,
            })
            .unwrap();
        t
    } else {
        let mut t = transfer(Mode::Copy, &base, &[&name], &dst, None, o);
        // `jcopy|src|dst|journals`: с журналом job (S5-12).
        t.journal_dir = parts.get(3).map(PathBuf::from);
        t
    };
    let id = h.start(4, t);
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
        .env(
            CHILD_ENV,
            format!("copy|{}|{}", src.display(), dst.display()),
        )
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

// --- S5-07: немодальные конфликты ---

fn has_prompt(h: &H) -> bool {
    h.core.state().prompts.active().is_some()
}

/// Ответ на открытый вопрос (как кнопка в строке вопроса).
fn reply(h: &mut H, choice: &str, all: bool) -> PromptSpec {
    let p = h.core.state().prompts.active().expect("open prompt");
    let (id, spec) = (i64::try_from(p.id.0).unwrap(), p.spec.clone());
    h.core.dispatch(
        Command::new("prompt.reply")
            .arg("id", id)
            .arg("choice", choice)
            .arg("all", all),
    );
    spec
}

fn snap(h: &H, id: JobId) -> urfin_core::ProgressSnap {
    h.core.state().jobs.get(id).unwrap().progress.snapshot()
}

/// 1000 файлов, из них 100 уже есть в назначении.
fn thousand(d: &Path) -> (PathBuf, PathBuf, Vec<String>) {
    let (src, dst) = (d.join("s"), d.join("d"));
    let names: Vec<String> = (0..1000).map(|i| format!("f{i:04}.txt")).collect();
    for (i, n) in names.iter().enumerate() {
        write(&src.join(n), &bytes(1000 + i, i as u64), old(1_500_000_000));
        if i % 10 == 0 {
            write(&dst.join(n), b"dst", old(1_400_000_000));
        }
    }
    (src, dst, names)
}

/// Ждёт конца job, отвечая на всё, что спросят; число вопросов.
fn finish(h: &mut H, id: JobId, choice: &str) -> u32 {
    let mut asked = 0;
    let t0 = Instant::now();
    while h.state(id).is_active() || h.eng.live_threads() > 0 {
        assert!(t0.elapsed() < Duration::from_secs(30), "timeout: job end");
        if has_prompt(h) {
            asked += 1;
            reply(h, choice, false);
        }
        h.pump();
    }
    asked
}

/// Приёмка S5-07: при Ask 900 свободных копируются, пока вопрос ждёт ответа;
/// один ответ «Заменить для всех» закрывает все 100 конфликтов.
#[test]
fn ask_parks_conflicts_and_replace_all_closes_them() {
    let d = tmp();
    let (src, dst, names) = thousand(d.path());
    let items: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut h = H::new(64);
    let t = transfer(
        Mode::Copy,
        &src,
        &items,
        &dst,
        None,
        opts(ConflictPolicy::Ask),
    );
    let id = h.start(4, t);
    h.pump_until("900 copied while the question waits", |h| {
        has_prompt(h) && snap(h, id).items_done == 900
    });
    assert_eq!(snap(&h, id).parked, 100, "all conflicts parked");
    assert!(matches!(h.state(id), JobState::Waiting(_)));
    assert_eq!(
        std::fs::read(dst.join("f0001.txt")).unwrap(),
        bytes(1001, 1)
    );
    assert_eq!(std::fs::read(dst.join("f0000.txt")).unwrap(), b"dst");
    let spec = reply(&mut h, "overwrite", true);
    assert!(spec.allow_all);
    assert_eq!(spec.choices[usize::from(spec.default)].id, "skip");
    match spec.detail {
        Some(PromptDetail::Conflict {
            src, dst, index, ..
        }) => {
            assert_eq!(index, 1);
            assert_eq!((src.name.as_str(), src.size), ("f0000.txt", 1000));
            assert_eq!(dst.size, 3);
        }
        other => panic!("no conflict detail: {other:?}"),
    }
    let more = finish(&mut h, id, "skip");
    assert_eq!(more, 0, "one answer for all");
    match h.state(id) {
        JobState::Done(s) => assert_eq!((s.items, s.skipped, s.errors), (1000, 0, 0)),
        st => panic!("{st:?}"),
    }
    for i in (0..1000).step_by(10) {
        let n = format!("f{i:04}.txt");
        assert_eq!(
            std::fs::read(dst.join(&n)).unwrap(),
            bytes(1000 + i, i as u64)
        );
    }
    assert_eq!(snap(&h, id).parked, 0);
    no_temps(&dst);
}

/// Приёмка S5-07: «Пропустить для всех» — ни одного лишнего прочитанного байта.
#[test]
fn skip_all_reads_no_extra_bytes() {
    let d = tmp();
    let (src, dst, names) = thousand(d.path());
    let items: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut h = H::new(64);
    let t = transfer(
        Mode::Copy,
        &src,
        &items,
        &dst,
        None,
        opts(ConflictPolicy::Ask),
    );
    let id = h.start(4, t);
    h.pump_until("question", has_prompt);
    reply(&mut h, "skip", true);
    assert_eq!(finish(&mut h, id, "overwrite"), 0, "one answer for all");
    let free: u64 = (0..1000u64).filter(|i| i % 10 != 0).map(|i| 1000 + i).sum();
    match h.state(id) {
        JobState::Done(s) => {
            assert_eq!((s.items, s.skipped), (900, 100));
            assert_eq!(s.bytes, free, "bytes read: only free files");
        }
        st => panic!("{st:?}"),
    }
    let p = snap(&h, id);
    assert_eq!(p.bytes_total, free, "skipped bytes leave the total");
    assert_eq!(std::fs::read(dst.join("f0000.txt")).unwrap(), b"dst");
}

static RACED: AtomicBool = AtomicBool::new(false);

/// Имя занимают между индексом назначения и commit (один раз на процесс).
fn race(dst: &Path) {
    if !RACED.swap(true, Ordering::SeqCst) {
        std::fs::write(dst, b"raced").unwrap();
    }
}

fn big_with_race(d: &Path) -> (PathBuf, PathBuf, Transfer) {
    let (src, dst) = (d.join("s"), d.join("d"));
    write(&src.join("big.bin"), &bytes(3 << 20, 5), old(1_500_000_000));
    std::fs::create_dir_all(&dst).unwrap();
    let mut t = transfer(
        Mode::Copy,
        &src,
        &["big.bin"],
        &dst,
        None,
        opts(ConflictPolicy::Ask),
    );
    t.before_commit = Some(race);
    (src, dst, t)
}

fn temps(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .is_ok_and(|e| e.file_name().to_string_lossy().contains(".fnv~"))
        })
        .count()
}

/// Приёмка S5-07: гонка (файл создан после индекса) → вопрос с данными во
/// временном файле, «Заменить» — rename без повторной копии. Отмена на вопросе
/// убирает временный файл — отдельный процесс (`RACED` — на процесс), поэтому в
/// этом же тесте: вторая job.
#[test]
fn race_parks_with_temp_and_replace_renames_it() {
    let d = tmp();
    let (src, dst, t) = big_with_race(d.path());
    let mut h = H::new(64);
    let id = h.start(4, t);
    h.pump_until("race question", has_prompt);
    assert_eq!(temps(&dst), 1, "data waits in a temp file");
    assert_eq!(std::fs::read(dst.join("big.bin")).unwrap(), b"raced");
    reply(&mut h, "overwrite", false);
    assert_eq!(finish(&mut h, id, "skip"), 0);
    match h.state(id) {
        JobState::Done(s) => assert_eq!((s.items, s.bytes), (1, 3 << 20), "copied once"),
        st => panic!("{st:?}"),
    }
    assert_eq!(hash(&dst.join("big.bin")), hash(&src.join("big.bin")));
    no_temps(&dst);

    // Отмена (Esc) на вопросе гонки: temp удалён, занятый файл цел, потоков 0.
    RACED.store(false, Ordering::SeqCst);
    let d2 = tmp();
    let (_, dst2, t2) = big_with_race(d2.path());
    let id2 = h.start(4, t2);
    h.pump_until("race question", has_prompt);
    let pid = i64::try_from(h.core.state().prompts.active().unwrap().id.0).unwrap();
    h.core
        .dispatch(Command::new("prompt.cancel").arg("id", pid));
    assert_eq!(h.wait_done(id2), JobState::Cancelled);
    assert_eq!(std::fs::read(dst2.join("big.bin")).unwrap(), b"raced");
    assert_eq!(temps(&dst2), 0, "temp removed on cancel");
}

// --- S5-08: перенос ---

/// Приёмка S5-08: перенос между томами, процесс убит посреди работы — каждый
/// файл целиком хотя бы в одном месте (источник удаляется только после fsync
/// копии и сверки размера).
#[test]
fn killed_move_loses_nothing() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }
    let d = tmp();
    let src = d.path().join("src");
    let t = old(1_300_000_000);
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
    let before = snapshot(&src);
    let dst = d.path().join("dst");
    std::fs::create_dir(&dst).unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(exe)
        .args(["--exact", "ops::copy::tests::kill_child", "--nocapture"])
        .env(
            CHILD_ENV,
            format!("move|{}|{}", src.display(), dst.display()),
        )
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
    let total = count_files(&src);
    while count_files(&src) > total * 9 / 10 && t0.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        !dst.with_extension("finished").exists(),
        "move finished before kill: make the data bigger"
    );
    let (mut moved, mut kept) = (0, 0);
    for (rel, (dir, h, m)) in &before {
        if *dir {
            continue;
        }
        let at_src = src.join(rel);
        let at_dst = dst.join("src").join(rel);
        let whole = |p: &Path| p.exists() && hash(p) == *h && mtime(p) == *m;
        if whole(&at_src) {
            kept += 1;
        } else {
            assert!(
                whole(&at_dst),
                "{rel}: lost — not whole in source nor in copy"
            );
            moved += 1;
        }
    }
    eprintln!("killed move: {moved} moved, {kept} kept of {total}");
    assert!(moved > 0 && kept > 0);
}

/// Приёмка S5-08: перенос каталога в пределах тома — один rename, < 50 мс
/// независимо от числа файлов (дерево глубины 6). Лучший из трёх переносов
/// туда и обратно: сразу после записи 2000 файлов ОС (антивирус, сброс кэша)
/// изредка задерживает первый rename на сотню мс.
#[test]
fn move_same_volume_tree_is_one_rename() {
    let d = tmp();
    let (a, b) = (d.path().join("a"), d.path().join("b"));
    let t = old(1_500_000_000);
    for i in 0..2000u64 {
        let rel = format!(
            "a{}/b{}/c{}/d{}/e{}/f{i}.txt",
            i % 2,
            i % 3,
            i % 4,
            i % 5,
            i % 6
        );
        write(&a.join("tree").join(rel), &bytes(1024 + i as usize, i), t);
    }
    let before = snapshot(&a.join("tree"));
    std::fs::create_dir(&b).unwrap();
    let mut h = H::new(64);
    let mut best = Duration::MAX;
    for (from, to) in [(&a, &b), (&b, &a), (&a, &b)] {
        let r = run(
            &mut h,
            transfer(Mode::Move, from, &["tree"], to, None, OpOptions::default()),
        );
        let s = done(&r);
        assert_eq!(s.items, 1, "the directory moves with one rename");
        assert!(!from.join("tree").exists());
        best = best.min(r.report.elapsed);
    }
    assert!(
        best < Duration::from_millis(50),
        "same-volume move took {best:?}"
    );
    assert_eq!(snapshot(&b.join("tree")), before);
}

/// Приёмка S5-08: запаркованный ребёнок (вопрос открыт) не даёт удалить
/// исходный каталог при межтомном переносе; после «Заменить» каталог удалён.
#[test]
fn parked_child_keeps_source_dir() {
    let d = tmp();
    let (src, dst) = (d.path().join("s"), d.path().join("d"));
    let t = old(1_500_000_000);
    for i in 0..20 {
        write(
            &src.join(format!("t/f{i}.txt")),
            format!("src-{i}").as_bytes(),
            t,
        );
    }
    write(&dst.join("t/f7.txt"), b"dst", old(1_400_000_000));
    let tr = transfer(Mode::Move, &src, &["t"], &dst, None, OpOptions::default());
    tr.info
        .set(Info {
            tol_ns: 0,
            rename: false,
        })
        .unwrap();
    let mut h = H::new(64);
    let id = h.start(4, tr);
    h.pump_until("19 moved, question open", |h| {
        has_prompt(h) && snap(h, id).items_done == 19
    });
    // Пока вопрос открыт: в источнике только конфликтный файл, каталог на месте.
    let left: Vec<String> = snapshot(&src).into_keys().collect();
    assert_eq!(left, ["t", "t\\f7.txt"], "{left:?}");
    reply(&mut h, "overwrite", false);
    assert_eq!(finish(&mut h, id, "skip"), 0);
    assert!(matches!(h.state(id), JobState::Done(_)));
    assert_eq!(std::fs::read(dst.join("t/f7.txt")).unwrap(), b"src-7");
    assert!(
        !src.join("t").exists(),
        "source dir removed after the answer"
    );
}

/// Замер (не приёмка) S5-08: перенос датасета B2-размера в пределах тома и
/// обратно. `cargo nextest run --release -p urfin-ops bench_move --run-ignored only --no-capture`.
#[test]
#[ignore = "замер на стенде"]
fn bench_move_same_volume() {
    let n: usize = std::env::var("URFIN_BENCH_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    let spec = urfin_testdata::TreeSpec {
        n,
        seed: 2,
        layout: urfin_testdata::Layout::Nav { dirs: n / 100 },
    };
    let src = urfin_testdata::dataset("urfin", &format!("move-{n}"), &spec).unwrap();
    let base = src.parent().unwrap().to_path_buf();
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let side = base.join(format!("{name}.moved"));
    std::fs::create_dir_all(&side).unwrap();
    let mut h = H::new(64);
    for (from, to) in [(base.clone(), side.clone()), (side.clone(), base.clone())] {
        let r = run(
            &mut h,
            transfer(Mode::Move, &from, &[&name], &to, None, OpOptions::default()),
        );
        let s = done(&r);
        eprintln!(
            "move {n}: {} item(s) in {:?} (job), {}",
            s.items,
            r.report.elapsed,
            to.display()
        );
    }
    assert!(src.exists());
}

// --- S5-11: ошибки и повтор ---

/// Держатель файла (как другая программа): открыт без общего доступа, пока не
/// отпущен.
struct Holder(
    Option<std::sync::mpsc::Sender<()>>,
    Option<std::thread::JoinHandle<()>>,
);

impl Holder {
    /// Держит `p` до `release` или `for_` (что раньше).
    fn hold(p: &Path, for_: Duration) -> Holder {
        use std::os::windows::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(p)
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let h = std::thread::spawn(move || {
            let _ = rx.recv_timeout(for_);
            drop(f);
        });
        Holder(Some(tx), Some(h))
    }

    fn release(&mut self) {
        drop(self.0.take());
        if let Some(h) = self.1.take() {
            h.join().unwrap();
        }
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        self.release();
    }
}

/// Приёмка S5-11: sharing violation снимается автоповтором, если держатель
/// отпустил файл за 1 с; вопроса нет.
#[test]
fn sharing_violation_clears_with_retry() {
    let d = tmp();
    let (src, dst) = (d.path().join("s"), d.path().join("d"));
    let data = bytes(50_000, 3);
    write(&src.join("held.bin"), &data, old(1_500_000_000));
    std::fs::create_dir(&dst).unwrap();
    let _holder = Holder::hold(&src.join("held.bin"), Duration::from_millis(700));
    let mut h = H::new(64);
    let r = run(
        &mut h,
        transfer(
            Mode::Copy,
            &src,
            &["held.bin"],
            &dst,
            None,
            OpOptions::default(),
        ),
    );
    let s = done(&r);
    assert_eq!((s.items, s.errors, r.asked), (1, 0, 0));
    assert!(
        r.report.elapsed >= Duration::from_millis(200),
        "retried after a pause"
    );
    assert_eq!(std::fs::read(dst.join("held.bin")).unwrap(), data);
}

/// Приёмка S5-11: ошибка не прошла за автоповторы → вопрос по классу;
/// «Пропустить» — в отчёт; `ops.retry` (новая job) копирует только упавшие.
#[test]
fn retry_copies_only_failed() {
    let d = tmp();
    let (src, dst) = (d.path().join("s"), d.path().join("d"));
    let t = old(1_500_000_000);
    for i in 0..5 {
        write(
            &src.join(format!("t/f{i}.txt")),
            format!("src-{i}").as_bytes(),
            t,
        );
    }
    std::fs::create_dir(&dst).unwrap();
    let mut holder = Holder::hold(&src.join("t/f2.txt"), Duration::from_secs(60));
    let mut h = H::new(64);
    h.core.register(Box::new(
        crate::OpsHandler::new(crate::OpsConfig::default()),
    ));
    let t1 = transfer(Mode::Copy, &src, &["t"], &dst, None, OpOptions::default());
    let id = h.start(4, t1);
    h.pump_until("question after auto-retries", has_prompt);
    let spec = reply(&mut h, "skip", false);
    assert_eq!(spec.choices[0].id, "retry");
    assert!(spec.body.contains("f2.txt"), "{}", spec.body);
    assert_eq!(finish(&mut h, id, "skip"), 0);
    match h.state(id) {
        JobState::Done(s) => assert_eq!((s.items, s.errors), (4, 1)),
        st => panic!("{st:?}"),
    }
    assert!(!dst.join("t/f2.txt").exists());
    // Изменим уже скопированный: повтор не должен его трогать.
    std::fs::write(dst.join("t/f0.txt"), b"mine").unwrap();
    holder.release();
    for _ in 0..5 {
        h.pump();
    }
    h.core
        .dispatch(Command::new("ops.retry").arg("id", i64::try_from(id.0).unwrap()));
    h.pump_until("retry job", |h| {
        h.core.state().jobs.iter().any(|j| j.id.0 > id.0)
    });
    let id2 = h
        .core
        .state()
        .jobs
        .iter()
        .map(|j| j.id)
        .max_by_key(|j| j.0)
        .unwrap();
    h.pump_until("retry job end", |h| !h.state(id2).is_active());
    match h.state(id2) {
        JobState::Done(s) => assert_eq!((s.items, s.errors), (1, 0), "only the failed one"),
        st => panic!("{st:?}"),
    }
    assert_eq!(std::fs::read(dst.join("t/f2.txt")).unwrap(), b"src-2");
    assert_eq!(std::fs::read(dst.join("t/f0.txt")).unwrap(), b"mine");
}

// --- S5-12: журнал и восстановление ---

fn journals(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |rd| {
        rd.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "jnl"))
            .count()
    })
}

/// Приёмка S5-12: копия убита (kill -9) около половины → «Продолжить» доводит
/// до конца без повторного копирования готовых файлов: байт не больше остатка.
#[test]
fn killed_copy_continues_without_recopy() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }
    let d = tmp();
    let src = d.path().join("src");
    let t = old(1_300_000_000);
    let mut total_bytes = 0u64;
    for i in 0..400u64 {
        let size = match i % 8 {
            0 => (3 << 20) + i as usize,
            1 => (1 << 20) + 17,
            _ => 300_000 + 1_000 * i as usize,
        };
        total_bytes += size as u64;
        write(
            &src.join(format!("d{}/f{i}.bin", i % 10)),
            &bytes(size, i),
            t,
        );
    }
    let (dst, jdir) = (d.path().join("dst"), d.path().join("jobs"));
    std::fs::create_dir(&dst).unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(exe)
        .args(["--exact", "ops::copy::tests::kill_child", "--nocapture"])
        .env(
            CHILD_ENV,
            format!(
                "jcopy|{}|{}|{}",
                src.display(),
                dst.display(),
                jdir.display()
            ),
        )
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
    let total = count_files(&src);
    while count_files(&dst) < total / 2 && t0.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        !dst.with_extension("finished").exists(),
        "copy finished before kill: make the data bigger"
    );
    let srcs = snapshot(&src);
    let ready: u64 = snapshot(&dst.join("src"))
        .iter()
        .filter(|(rel, (dir, h, m))| !dir && srcs.get(*rel).is_some_and(|s| s.1 == *h && s.2 == *m))
        .map(|(rel, _)| std::fs::metadata(src.join(rel)).unwrap().len())
        .sum();
    // Как при старте: журнал прерванной job, «Продолжить».
    let found = journal::scan(&jdir);
    assert_eq!(found.len(), 1, "interrupted journal");
    let i = &found[0];
    i.remove_temps();
    std::fs::remove_file(&i.path).unwrap();
    let start = recovered(&i.header, 1 << 20, Some(jdir.clone())).unwrap();
    let mut h = H::new(64);
    let r = run(&mut h, Transfer::new(Mode::Copy, start));
    let s = done(&r);
    assert_eq!(r.asked, 0, "continue asks nothing");
    let left = total_bytes - ready;
    eprintln!(
        "continue: ready {ready} of {total_bytes}, copied {} (left {left})",
        s.bytes
    );
    assert!(
        ready > 0 && s.bytes <= left,
        "copied {} > left {left}",
        s.bytes
    );
    for (rel, (dir, h, m)) in &srcs {
        if !dir {
            let p = dst.join("src").join(rel);
            assert_eq!((hash(&p), mtime(&p)), (*h, *m), "{rel}");
        }
    }
    no_temps(&dst);
    assert_eq!(journals(&jdir), 0, "finished job removes its journal");
}

/// Приёмка S5-12 (§6.4): выход при идущей копии — вопрос; «Прервать и выйти»
/// отменяет job и оставляет журнал для «Продолжить».
#[test]
fn quit_with_running_copy_asks_and_keeps_journal() {
    let d = tmp();
    let (src, dst, jdir) = (
        d.path().join("s"),
        d.path().join("d"),
        d.path().join("jobs"),
    );
    for i in 0..50 {
        write(
            &src.join(format!("t/f{i}.bin")),
            &bytes(400_000, i),
            old(1_500_000_000),
        );
    }
    std::fs::create_dir(&dst).unwrap();
    let mut h = H::new(64);
    h.core.register(Box::new(
        crate::OpsHandler::new(crate::OpsConfig::default()),
    ));
    let mut t = transfer(Mode::Copy, &src, &["t"], &dst, None, OpOptions::default());
    t.journal_dir = Some(jdir.clone());
    let id = h.start(4, t);
    h.cmd("job.pause", id);
    h.pump_until("journal created", |_| journals(&jdir) == 1);
    h.core.dispatch(Command::new("app.quit"));
    h.pump_until("quit question", has_prompt);
    let spec = reply(&mut h, "quit", false);
    assert_eq!(
        spec.choices
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["wait", "quit"]
    );
    h.pump_until("job cancelled", |h| !h.state(id).is_active());
    assert_eq!(h.state(id), JobState::Cancelled);
    h.pump_until("engine threads gone", |h| h.eng.live_threads() == 0);
    let found = journal::scan(&jdir);
    assert_eq!(found.len(), 1, "journal kept for Continue");
    assert_eq!(found[0].header.req.items, [CompactString::from("t")]);
}

/// S5-12 сквозь handler: при старте — баннер о прерванной job, «Продолжить»
/// доводит копию, журнал удаляется; «Очистить» удаляет журнал и temp.
#[test]
fn startup_banner_continue_and_clear() {
    let d = tmp();
    let (src, dst, jdir) = (
        d.path().join("s"),
        d.path().join("d"),
        d.path().join("jobs"),
    );
    for i in 0..5 {
        write(
            &src.join(format!("t/f{i}.txt")),
            format!("src-{i}").as_bytes(),
            old(1_500_000_000),
        );
    }
    std::fs::create_dir(&dst).unwrap();
    // Две «прерванные» job: заголовок есть, `Done` нет.
    let header = |dest: &Path| journal::Header {
        v: journal::VERSION,
        req: OpRequest {
            kind: OpKind::Copy,
            base: Location::local(&src),
            items: vec![CompactString::from("t")],
            dest: None,
            opts: OpOptions::default(),
        },
        dest: dest.to_path_buf(),
        dest_raw: dest.display().to_string(),
        started: 1,
        pid: 1,
    };
    let orphan = dst.join(".x.fnv~zz");
    std::fs::write(&orphan, b"partial").unwrap();
    let j1 = Journal::create(&jdir, header(&dst)).unwrap();
    j1.keep();
    let mut h2 = header(&d.path().join("other"));
    h2.started = 2;
    let j2 = Journal::create(&jdir, h2).unwrap();
    j2.append_now(&Record::Temp(orphan.clone()));
    j2.keep();
    let mut h = H::new(64);
    h.core.register(Box::new(crate::OpsHandler::with_journal(
        crate::OpsConfig::default(),
        Some(jdir.clone()),
    )));
    let banner = |h: &H| h.core.state().banner.clone();
    h.pump_until("banner", |h| banner(h).is_some());
    let b = banner(&h).unwrap();
    assert!(b.text.contains(&dst.display().to_string()), "{}", b.text);
    assert_eq!(b.actions.len(), 2);
    h.core.dispatch(b.actions[0].1.clone()); // Продолжить
    h.pump_until("copy done", |_| dst.join("t/f4.txt").exists());
    // Следующая прерванная — свой баннер; «Очистить».
    h.pump_until("second banner", |h| {
        banner(h).is_some_and(|b| b.text.contains("other"))
    });
    let b = banner(&h).unwrap();
    h.core.dispatch(b.actions[1].1.clone()); // Очистить
    h.pump_until("journals gone, jobs done", |h| {
        journals(&jdir) == 0 && !h.core.state().jobs.any_active()
    });
    assert!(!orphan.exists(), "temp of the cleared job removed");
    assert!(banner(&h).is_none());
    for i in 0..5 {
        assert_eq!(
            std::fs::read(dst.join(format!("t/f{i}.txt"))).unwrap(),
            format!("src-{i}").into_bytes()
        );
    }
}
