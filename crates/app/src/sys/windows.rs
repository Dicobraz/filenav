//! Журнал событий Windows (`RegisterEventSourceW` + `ReportEventW`) и DWM.

use std::ptr;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject};
use windows_sys::Win32::System::EventLog::{
    EVENTLOG_ERROR_TYPE, RegisterEventSourceW, ReportEventW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GCLP_HBRBACKGROUND, SetClassLongPtrW};
use windows_sys::core::BOOL;

/// Источник в журнале «Приложение». Регистрацию в реестре
/// (`HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application\FileNav`) делает
/// установщик (S9); без неё Просмотр событий показывает текст с припиской
/// «описание не найдено».
const EVENT_SOURCE: &str = "FileNav";

/// Предел длины одной строки `ReportEventW` (31 839 символов UTF-16).
const MAX_CHARS: usize = 31_000;

/// HANDLE источника как адрес: `HANDLE` — указатель, он не `Send`/`Sync`.
/// 0 — источник не открылся.
fn source() -> usize {
    static HANDLE: OnceLock<usize> = OnceLock::new();
    *HANDLE.get_or_init(|| {
        let name = wide(EVENT_SOURCE);
        // SAFETY: имя — корректная строка UTF-16 с нулём на конце; локальный
        // компьютер (null). Хэндл живёт до конца процесса и не закрывается.
        let h = unsafe { RegisterEventSourceW(ptr::null(), name.as_ptr()) };
        h as usize
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().take(MAX_CHARS).chain(Some(0)).collect()
}

pub(super) fn report_error(id: u32, text: &str) {
    let h = source();
    if h == 0 {
        return;
    }
    let msg = wide(text);
    let strings = [msg.as_ptr()];
    // SAFETY: `h` получен из `RegisterEventSourceW` и не закрывается; одна строка
    // UTF-16 с нулём, живёт до конца вызова; SID и сырые данные не передаются.
    unsafe {
        ReportEventW(
            h as _,
            EVENTLOG_ERROR_TYPE,
            0,
            id,
            ptr::null_mut(),
            1,
            0,
            strings.as_ptr(),
            ptr::null(),
        );
    }
}

pub(super) fn cloak(hwnd: isize, on: bool) {
    set_dwm_bool(hwnd, DWMWA_CLOAK as u32, on);
}

pub(super) fn no_transitions(hwnd: isize) {
    set_dwm_bool(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED as u32, true);
}

fn set_dwm_bool(hwnd: isize, attr: u32, on: bool) {
    let value = BOOL::from(on);
    // SAFETY: `hwnd` — окно этого процесса от winit, живое на время вызова;
    // значение — BOOL, размер передан точно. Ошибка (старая ОС) не важна.
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            attr,
            (&raw const value).cast(),
            size_of::<BOOL>() as u32,
        );
    }
}

pub(super) fn set_background(hwnd: isize, [r, g, b]: [u8; 3]) {
    let color = u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16);
    // SAFETY: `hwnd` — окно этого процесса от winit; кисть создаётся здесь и
    // становится фоном класса окна (winit регистрирует класс на окно). Прежняя
    // кисть — наша (или 0 у winit) и удаляется.
    unsafe {
        let brush = CreateSolidBrush(color);
        if brush.is_null() {
            return;
        }
        let old = SetClassLongPtrW(hwnd as HWND, GCLP_HBRBACKGROUND, brush as isize);
        if old != 0 {
            DeleteObject(old as _);
        }
    }
}
