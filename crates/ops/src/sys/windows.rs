//! Windows: открытие файлов — `ShellExecuteExW` (S5 §2.5 `file.open`).

#![allow(unsafe_code)]

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::Open;

fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn open(path: &Path, how: Open) -> std::io::Result<()> {
    // Расширения оболочки (ассоциации, «Открыть с помощью») ждут STA; поток
    // задачи — наш, повторная инициализация безвредна.
    // SAFETY: инициализация COM текущего потока без указателей.
    unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
    }
    let (verb, file, params) = match how {
        Open::Default => (None, wide(path), None),
        Open::Elevated => (Some(wide("runas")), wide(path), None),
        Open::With => (Some(wide("openas")), wide(path), None),
        Open::Reveal => {
            let mut p = std::ffi::OsString::from("/select,\"");
            p.push(path.as_os_str());
            p.push("\"");
            (None, wide("explorer.exe"), Some(wide(p)))
        }
    };
    let dir = path.parent().map(wide);
    // SAFETY: структура на стеке, строки живут до конца вызова.
    let ok = unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0);
        info.fMask = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        info.lpVerb = verb.as_ref().map_or(std::ptr::null(), Vec::as_ptr);
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ref().map_or(std::ptr::null(), Vec::as_ptr);
        info.lpDirectory = dir.as_ref().map_or(std::ptr::null(), Vec::as_ptr);
        info.nShow = SW_SHOWNORMAL;
        ShellExecuteExW(&raw mut info)
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
