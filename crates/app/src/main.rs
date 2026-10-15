//! FileNav — бинарь `filenav`.
//!
//! Старт (S1 §2.9): каталоги → конфиг → логи → обработчик паник → Core. Окно
//! eframe, цикл кадра и `windows_subsystem` — S1-13.
//!
//! Флаги: `--smoke` — headless-прогон Core для CI (выход 0); только в debug:
//! `--crash-test` — паника для проверки crash-файла и баннера, `--error-test` —
//! ошибка в лог (и в журнал Windows).

mod boot;
mod sys;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use filenav_core::{Boot, Config, Core, Location, Paths, Services, Waker};

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

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    println!("filenav {}", version());

    // До подписчика tracing спаны не пишутся: длительность меряем сами.
    let t0 = Instant::now();
    let paths = Paths::detect();
    paths.ensure().context("cannot create app directories")?;
    let t_paths = t0.elapsed();
    let t1 = Instant::now();
    if let Err(e) = Config::write_default_if_missing(&paths) {
        eprintln!("cannot write default config: {e}");
    }
    let (config, warnings) = Config::load(&paths);
    let t_config = t1.elapsed();
    let _log = boot::init_tracing(&paths, &config.log)?;
    boot::install_panic_hook(&paths);
    // Стабильные имена этапов старта — контракт (S0 v5 №10).
    tracing::info!(target: "startup.paths", elapsed = ?t_paths, portable = paths.portable);
    tracing::info!(target: "startup.config", elapsed = ?t_config, warnings = warnings.len());
    tracing::debug!(?args, "starting");

    // Только debug: проверка записи ошибки в журнал Windows.
    if cfg!(debug_assertions) && flag("--error-test") {
        tracing::error!(test = "error-test", "error test");
    }
    // Только debug: проверка crash-файла и баннера (S1-12).
    assert!(
        !(cfg!(debug_assertions) && flag("--crash-test")),
        "crash test"
    );

    let crashed = boot::take_crash_marker(&paths);
    if flag("--smoke") {
        smoke(paths, config, warnings, crashed);
    }
    Ok(())
}

/// Ничего не будит: в `--smoke` pump качается вручную.
struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

/// Headless-прогон Core: старт, загрузка табов, завершение. Сессию не читает и
/// не перезаписывает чужую: пишет её в свои каталоги, как обычный запуск.
fn smoke(
    paths: Paths,
    config: Config,
    warnings: Vec<filenav_core::ConfigWarning>,
    crashed: Option<std::path::PathBuf>,
) {
    let root = if cfg!(windows) { r"C:\" } else { "/" };
    let mut boot = Boot::new(Location::local(root), Location::local(root));
    boot.paths = paths;
    boot.config = config;
    boot.warnings = warnings;
    boot.crashed_last_time = crashed;
    let mut core = {
        let _s = tracing::info_span!("startup.core").entered();
        Core::new(boot, Services::null(), Arc::new(NoWake))
    };
    // NullListing отвечает сразу: двадцати кадров хватает на загрузку обоих табов.
    for _ in 0..20 {
        core.pump(Duration::from_millis(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(b) = &core.state().banner {
        println!("banner: {}", b.text);
    }
    tracing::debug!("smoke done");
    let _ = core.shutdown(Duration::from_millis(500));
    println!("smoke ok");
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}
