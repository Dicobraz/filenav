//! Старт приложения (S1 §2.9): логирование, обработчик паник, маркер падения.

use std::backtrace::Backtrace;
use std::fs;
use std::io::IsTerminal;
use std::panic::PanicHookInfo;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use filenav_core::{LogCfg, Paths};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Переменная окружения с фильтром `tracing`; важнее `[log] level` из конфига.
pub const LOG_ENV: &str = "FILENAV_LOG";

/// Маркер падения в `state_dir`; внутри — путь к crash-файлу.
const CRASH_MARKER: &str = "crashed";

/// Логи: `log_dir/filenav.YYYY-MM-DD.log` (ротация раз в сутки, не больше
/// `keep_files` файлов, неблокирующая запись) и stderr в debug-сборках.
/// Фильтр — `FILENAV_LOG`, иначе `cfg.level`. Guard держать до выхода: он
/// дописывает буфер.
pub fn init_tracing(paths: &Paths, cfg: &LogCfg) -> anyhow::Result<WorkerGuard> {
    let filter = match std::env::var(LOG_ENV) {
        Ok(v) if !v.trim().is_empty() => {
            EnvFilter::try_new(&v).with_context(|| format!("{LOG_ENV}={v}"))?
        }
        _ => EnvFilter::try_new(cfg.level.as_str()).or_else(|_| EnvFilter::try_new("info"))?,
    };
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("filenav")
        .filename_suffix("log")
        .max_log_files(usize::from(cfg.keep_files.max(1)))
        .build(&paths.log_dir)
        .with_context(|| format!("log dir {}", paths.log_dir.display()))?;
    let (file, guard) = tracing_appender::non_blocking(appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_ansi(false)
        .with_thread_names(true);
    let stderr_layer = cfg!(debug_assertions).then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(std::io::stderr().is_terminal())
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .context("tracing already initialized")?;
    Ok(guard)
}

/// Паника в любом потоке: `log_dir/crash-<unix-ms>.txt` (сообщение, место,
/// бэктрейс, версия, ОС) и маркер `state_dir/crashed`; следующий старт покажет
/// баннер. Отчёты никуда не отправляются (офлайн). Затем — прежний hook.
pub fn install_panic_hook(paths: &Paths) {
    let paths = paths.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        match write_crash(&paths, info) {
            Ok(file) => tracing::error!(crash = %file.display(), "panic: {info}"),
            Err(e) => tracing::error!(error = %e, "panic: {info} (crash file not written)"),
        }
        previous(info);
    }));
}

fn write_crash(paths: &Paths, info: &PanicHookInfo<'_>) -> std::io::Result<PathBuf> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    fs::create_dir_all(&paths.log_dir)?;
    fs::create_dir_all(&paths.state_dir)?;
    let file = paths.log_dir.join(format!("crash-{ms}.txt"));
    let thread = std::thread::current();
    let text = format!(
        "FileNav {version} crashed\n\
         os: {os} {arch}\n\
         thread: {thread}\n\
         message: {message}\n\
         location: {location}\n\
         time (unix ms): {ms}\n\n\
         backtrace:\n{backtrace}\n",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        thread = thread.name().unwrap_or("<unnamed>"),
        message = panic_message(info),
        location = info
            .location()
            .map_or_else(|| "<unknown>".to_owned(), ToString::to_string),
        backtrace = Backtrace::force_capture(),
    );
    fs::write(&file, text)?;
    fs::write(
        paths.state_dir.join(CRASH_MARKER),
        file.to_string_lossy().as_bytes(),
    )?;
    Ok(file)
}

fn panic_message<'a>(info: &'a PanicHookInfo<'_>) -> &'a str {
    let p = info.payload();
    p.downcast_ref::<&str>()
        .copied()
        .or_else(|| p.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic>")
}

/// Маркер прошлого падения: путь к crash-файлу; маркер удаляется.
pub fn take_crash_marker(paths: &Paths) -> Option<PathBuf> {
    let marker = paths.state_dir.join(CRASH_MARKER);
    let text = fs::read_to_string(&marker).ok()?;
    if let Err(e) = fs::remove_file(&marker) {
        tracing::warn!(error = %e, "cannot remove crash marker");
    }
    let file = PathBuf::from(text.trim());
    Some(if file.as_os_str().is_empty() {
        paths.log_dir.clone()
    } else {
        file
    })
}
