//! Платформенный слой приложения (S0 v5 №5: `unsafe` разрешён только здесь).

#[cfg(windows)]
#[allow(unsafe_code, reason = "WinAPI: журнал событий, DWM")]
mod windows;

use raw_window_handle::HasWindowHandle;

/// Скрыть окно средствами DWM (cloak) или вернуть его. Скрытое так окно для
/// системы видимо: разворачивается и рисуется, но на экран не выводится — старт
/// без мигания. На других ОС — ничего.
pub fn cloak(window: &impl HasWindowHandle, on: bool) {
    #[cfg(windows)]
    if let Ok(h) = window.window_handle()
        && let raw_window_handle::RawWindowHandle::Win32(h) = h.as_raw()
    {
        windows::cloak(h.hwnd.get(), on);
    }
    #[cfg(not(windows))]
    let _ = (window, on);
}

/// Без анимации разворота/свёртывания для этого окна: DWM плавно смешивает старый
/// и новый снимок окна ~250 мс, и текст панелей двоится. На других ОС — ничего.
pub fn no_transitions(window: &impl HasWindowHandle) {
    #[cfg(windows)]
    if let Ok(h) = window.window_handle()
        && let raw_window_handle::RawWindowHandle::Win32(h) = h.as_raw()
    {
        windows::no_transitions(h.hwnd.get());
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Фон окна до первого кадра — цвет `rgb`: при развороте ОС заливает им новую
/// область, пока не пришёл кадр нового размера (иначе — белым). На других ОС —
/// ничего.
pub fn set_background(window: &impl HasWindowHandle, rgb: [u8; 3]) {
    #[cfg(windows)]
    if let Ok(h) = window.window_handle()
        && let raw_window_handle::RawWindowHandle::Win32(h) = h.as_raw()
    {
        windows::set_background(h.hwnd.get(), rgb);
    }
    #[cfg(not(windows))]
    let _ = (window, rgb);
}

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
