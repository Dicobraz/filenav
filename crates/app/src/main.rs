//! FileNav — бинарь `filenav`.
//!
//! Старт (S1 §2.9, S1-13): каталоги → конфиг → логи → обработчик паник → сессия →
//! Core → окно eframe. Листинг — `FakeListing` с генерируемым деревом до S4.
//!
//! Флаги: `--smoke` — без окна: Core, 1000 команд, shutdown, выход 0 (CI);
//! `--bench-startup` — окно, JSON с длительностями этапов старта после первого
//! кадра и выход (S0 v5 №10); только в debug: `--crash-test` — паника для
//! проверки crash-файла и баннера, `--error-test` — ошибка в лог (и в журнал
//! Windows).

// Release — оконное приложение без консоли; debug — с консолью для логов.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod boot;
mod sys;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use filenav_core::fake::{FakeListing, FakeTree};
use filenav_core::{
    Boot, Command, Config, ConfigWarning, Core, Level, Location, Paths, Services, Session, UiEvent,
    Waker,
};

/// Версия приложения из `Cargo.toml`.
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let text = format!("FileNav {} failed to start: {e:#}", version());
            eprintln!("{text}");
            tracing::error!(target: "filenav::startup", "{e:#}");
            sys::event_log(sys::EventId::StartupFailed, &text);
            ExitCode::FAILURE
        }
    }
}

/// Длительности этапов старта (имена — контракт S0 v5 №10). Отсчёт — от начала
/// `main`; от создания процесса (`GetProcessTimes`) — S9.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Startup {
    process: Instant,
    paths: Duration,
    config: Duration,
    tracing: Duration,
    session: Duration,
    core: Duration,
    window: Duration,
    first_frame: Duration,
    bench: bool,
}

impl Startup {
    fn json(&self) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        format!(
            "{{\"startup.paths\":{:.3},\"startup.config\":{:.3},\"startup.tracing\":{:.3},\
             \"startup.session\":{:.3},\"startup.core\":{:.3},\"startup.window\":{:.3},\
             \"startup.first_frame\":{:.3}}}",
            ms(self.paths),
            ms(self.config),
            ms(self.tracing),
            ms(self.session),
            ms(self.core),
            ms(self.window),
            ms(self.first_frame),
        )
    }
}

fn run() -> anyhow::Result<()> {
    let process = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);

    // До подписчика tracing спаны не пишутся: длительность меряем сами.
    let t = Instant::now();
    let paths = Paths::detect();
    paths.ensure().context("cannot create app directories")?;
    let t_paths = t.elapsed();
    let t = Instant::now();
    if let Err(e) = Config::write_default_if_missing(&paths) {
        eprintln!("cannot write default config: {e}");
    }
    let (config, warnings) = Config::load(&paths);
    let t_config = t.elapsed();
    let t = Instant::now();
    let _log = boot::init_tracing(&paths, &config.log)?;
    boot::install_panic_hook(&paths);
    let t_tracing = t.elapsed();
    tracing::info!(target: "startup.paths", elapsed = ?t_paths, portable = paths.portable);
    tracing::info!(target: "startup.config", elapsed = ?t_config, warnings = warnings.len());
    tracing::info!(version = version(), ?args, "starting");

    // Только debug: проверка записи ошибки в журнал Windows.
    if cfg!(debug_assertions) && flag("--error-test") {
        tracing::error!(test = "error-test", "error test");
        return Ok(());
    }
    // Только debug: проверка crash-файла и баннера (S1-12).
    assert!(
        !(cfg!(debug_assertions) && flag("--crash-test")),
        "crash test"
    );

    let crashed = boot::take_crash_marker(&paths);
    if flag("--smoke") {
        println!("filenav {}", version());
        return smoke(paths, config, warnings, crashed);
    }

    let t = Instant::now();
    let session = {
        let _s = tracing::info_span!("startup.session").entered();
        Session::load(&paths)
    };
    let t_session = t.elapsed();
    let root = demo_root();
    let mut boot = Boot::new(root.clone(), root);
    boot.paths = paths;
    boot.config = config;
    boot.warnings = warnings;
    boot.session = session;
    boot.crashed_last_time = crashed;
    let startup = Startup {
        process,
        paths: t_paths,
        config: t_config,
        tracing: t_tracing,
        session: t_session,
        core: Duration::ZERO,
        window: Duration::ZERO,
        first_frame: Duration::ZERO,
        bench: flag("--bench-startup"),
    };
    window::run(boot, startup)
}

/// Корень демо-дерева `FakeListing` (настоящая ФС — S4).
fn demo_root() -> Location {
    Location::local(if cfg!(windows) {
        r"C:\FakeListing"
    } else {
        "/FakeListing"
    })
}

/// Листинг до S4: генерируемое дерево, чанки как у S4.
pub(crate) fn demo_services() -> Services {
    Services {
        listing: Arc::new(FakeListing::new(2048).with_tree(FakeTree {
            dirs: 12,
            files: 300,
            max_depth: 6,
        })),
        open_resolvers: Vec::new(),
    }
}

/// Ничего не будит: в `--smoke` pump качается вручную.
struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

/// Команды `--smoke`: навигация, табы, сортировка, выделение, refresh. Enter —
/// только на каталог (строка 2 — первый каталог: `..`, каталоги первыми);
/// открытие файлов — S5.
const SMOKE_SCRIPT: [&str; 20] = [
    "panel.cursor.home",
    "panel.cursor.down n=2",
    "panel.enter",
    "panel.cursor.end",
    "sel.toggle",
    "panel.sort key=size",
    "panel.up",
    "tab.new",
    "panel.switch",
    "panel.cursor.page_down",
    "sel.all",
    "panel.cursor.home",
    "panel.enter",
    "panel.back",
    "tab.close side=left",
    "panel.switch",
    "panel.sort key=name",
    "sel.none",
    "panel.hidden.toggle",
    "panel.refresh",
];

/// Headless-прогон без окна: Core с `FakeListing`, 1000 команд, shutdown.
/// Сессию пользователя не читает и не перезаписывает (`state_dir` — временный).
/// Ошибка любой команды — провал прогона.
fn smoke(
    paths: Paths,
    config: Config,
    warnings: Vec<ConfigWarning>,
    crashed: Option<PathBuf>,
) -> anyhow::Result<()> {
    let root = demo_root();
    let mut boot = Boot::new(root.clone(), root);
    boot.paths = Paths {
        state_dir: std::env::temp_dir().join("filenav-smoke-state"),
        ..paths
    };
    boot.config = config;
    boot.warnings = warnings;
    boot.crashed_last_time = crashed;
    let mut core = Core::new(boot, demo_services(), Arc::new(NoWake));
    let pump = |core: &mut Core| {
        for _ in 0..3 {
            core.pump(Duration::from_millis(5));
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    pump(&mut core);
    if let Some(b) = &core.state().banner {
        println!("banner: {}", b.text);
    }
    let mut n = 0;
    let mut errors = Vec::new();
    while n < 1000 {
        for text in SMOKE_SCRIPT {
            core.dispatch(Command::parse(text).expect("smoke script"));
            n += 1;
        }
        pump(&mut core);
        errors.extend(core.take_ui_events().into_iter().filter_map(|e| match e {
            UiEvent::Toast {
                level: Level::Error | Level::Warn,
                text,
            } => Some(text),
            _ => None,
        }));
    }
    tracing::debug!(commands = n, errors = errors.len(), "smoke done");
    let _ = core.shutdown(Duration::from_millis(500));
    anyhow::ensure!(
        errors.is_empty(),
        "smoke: {} errors, first: {}",
        errors.len(),
        errors[0]
    );
    println!("smoke ok: {n} commands");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}
