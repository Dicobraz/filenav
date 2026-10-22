//! `ConPTY` на настоящем `cmd.exe` (приёмка S6-02).

use std::io::Read;
use std::sync::mpsc;
use std::time::Instant;

use super::*;

fn cmd(args: &[&str]) -> SpawnSpec {
    SpawnSpec {
        program: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        args: args.iter().map(OsString::from).collect(),
        cwd: None,
        env: vec![("URFIN_TEST".into(), "1".into())],
    }
}

const SIZE: PtySize = PtySize { cols: 80, rows: 24 };

/// Читатель conout до EOF в своём потоке.
fn drain(mut r: File) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => return out,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
    })
}

/// Приёмка: `cmd /c echo hi` даёт `hi` и код 0.
#[test]
fn echo_hi() {
    let pty = spawn(&cmd(&["/c", "echo hi %URFIN_TEST%"]), SIZE).unwrap();
    let reader = drain(pty.reader);
    let code = pty.control.wait(Some(Duration::from_secs(10)));
    assert_eq!(code, Some(0));
    pty.control.close();
    drop(pty.writer);
    let out = String::from_utf8_lossy(&reader.join().unwrap()).into_owned();
    assert!(out.contains("hi 1"), "{out:?}");
}

/// Хэндлов после затихания: минимум из нескольких замеров (системный пул потоков
/// и conhost освобождают своё асинхронно, разовый замер ловит их на полпути).
fn settled_handle_count() -> u32 {
    (0..5)
        .map(|_| {
            std::thread::sleep(Duration::from_millis(20));
            handle_count()
        })
        .min()
        .unwrap_or(0)
}

/// Занятость всех ядер системы между замерами `system_times`, %.
fn busy_percent(a: (u64, u64), b: (u64, u64)) -> u64 {
    let total = b.1 - a.1;
    if total == 0 {
        return 0;
    }
    // Дискретность счётчиков: простой за короткий интервал бывает чуть больше общего.
    100u64.saturating_sub(b.0.saturating_sub(a.0) * 100 / total)
}

/// Приёмка: 100 циклов spawn/close без зависаний (каждый ≤ 1 с) и утечек хэндлов.
///
/// Цикл — это запуск `cmd.exe` и conhost (≈50 мс): когда чужая нагрузка занимает
/// все ядра, он честно идёт 1–2 с. Зависание и медленное закрытие — это ожидание,
/// система при нём простаивает. Поэтому цикл дольше 1 с прощается, только если
/// ядра были заняты (≥ 75 %), и то не дольше 10 с. Соседей в nextest у теста нет
/// (`.config/nextest.toml`).
#[test]
fn hundred_cycles_no_leaks() {
    let one = || {
        let t0 = Instant::now();
        let pty = spawn(&cmd(&["/c", "exit 3"]), SIZE).unwrap();
        let reader = drain(pty.reader);
        assert_eq!(pty.control.wait(Some(Duration::from_secs(5))), Some(3));
        pty.control.close();
        drop(pty.writer);
        reader.join().unwrap();
        t0.elapsed()
    };
    // Прогрев: ленивые хэндлы системы (пул потоков, загрузчик).
    for _ in 0..3 {
        one();
    }
    let before = settled_handle_count();
    for i in 0..100 {
        let s0 = system_times();
        let t = one();
        let busy = busy_percent(s0, system_times());
        assert!(
            t < Duration::from_secs(1) || (busy >= 75 && t < Duration::from_secs(10)),
            "cycle {i}: {t:?}, system CPU busy {busy}%"
        );
    }
    let after = settled_handle_count();
    // Утечка хоть одного хэндла за цикл — это +100; запас 10 — на шум системы.
    assert!(after <= before + 10, "handles {before} → {after}");
}

/// Приёмка: kill убивает дерево (`cmd /c ping -t`), «занят» видит ребёнка.
#[test]
fn kill_tree_and_busy() {
    let pty = spawn(&cmd(&["/c", "ping -t 127.0.0.1"]), SIZE).unwrap();
    let reader = drain(pty.reader);
    let t0 = Instant::now();
    while !pty.control.busy() {
        assert!(t0.elapsed() < Duration::from_secs(10), "ping did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    pty.control.kill_tree().unwrap();
    assert!(
        pty.control.wait(Some(Duration::from_secs(5))).is_some(),
        "killed"
    );
    let t1 = Instant::now();
    while pty.control.active_processes() > 0 {
        assert!(t1.elapsed() < Duration::from_secs(5), "tree still alive");
        std::thread::sleep(Duration::from_millis(20));
    }
    pty.control.close();
    drop(pty.writer);
    reader.join().unwrap();
}

/// Приёмка: close при непрочитанном потоке вывода не виснет (читатель есть, но
/// вывод идёт непрерывно).
#[test]
fn close_while_output_flows() {
    let pty = spawn(&cmd(&["/c", r"dir /s C:\Windows\System32"]), SIZE).unwrap();
    let reader = drain(pty.reader);
    std::thread::sleep(Duration::from_millis(200));
    let t0 = Instant::now();
    pty.control.kill_tree().unwrap();
    pty.control.close();
    drop(pty.writer);
    reader.join().unwrap();
    assert!(
        t0.elapsed() < Duration::from_secs(3),
        "close took {:?}",
        t0.elapsed()
    );
}

/// Колбэк выхода (`RegisterWaitForSingleObject`) получает код.
#[test]
fn on_exit_callback() {
    let pty = spawn(&cmd(&["/c", "exit 7"]), SIZE).unwrap();
    let reader = drain(pty.reader);
    let (tx, rx) = mpsc::channel();
    let watch = pty
        .control
        .on_exit(move |code| {
            let _ = tx.send(code);
        })
        .unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(7));
    drop(watch);
    pty.control.close();
    drop(pty.writer);
    reader.join().unwrap();
}

#[test]
fn quoting_crt_rules() {
    let q = |s: &str| {
        let mut v = Vec::new();
        quote_arg(OsStr::new(s), &mut v);
        String::from_utf16(&v).unwrap()
    };
    assert_eq!(q("plain"), "plain");
    assert_eq!(q("a b"), "\"a b\"");
    assert_eq!(q(""), "\"\"");
    assert_eq!(q("say \"hi\""), r#""say \"hi\"""#);
    assert_eq!(q(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    assert_eq!(q(r"no\space\"), r"no\space\");
}
