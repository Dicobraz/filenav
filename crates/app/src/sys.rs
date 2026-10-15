//! Платформенный слой приложения (S0 v5 №5: `unsafe` разрешён только здесь).

#[cfg(windows)]
#[allow(unsafe_code, reason = "WinAPI: журнал событий Windows")]
mod windows;

/// Коды событий FileNav в журнале Windows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventId {
    /// Паника: crash-файл записан.
    Crash = 1000,
    /// Ошибка уровня Error из логов.
    Error = 1001,
    /// Приложение не смогло стартовать.
    StartupFailed = 1002,
}

/// Записать ошибку в журнал Windows «Приложение». На других ОС — ничего.
/// Ошибки записи игнорируются: журнал — дополнительный канал, файл лога главный.
pub fn event_log(id: EventId, text: &str) {
    #[cfg(windows)]
    windows::report_error(id as u32, text);
    #[cfg(not(windows))]
    let _ = (id, text);
}
