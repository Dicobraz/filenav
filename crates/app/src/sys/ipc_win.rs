//! Один экземпляр на Windows (S1-15): именованный mutex и named pipe.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_INBOUND;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

/// Буфер канала: запрос — короткий JSON.
const PIPE_BUFFER: u32 = 64 * 1024;

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

/// SID текущего пользователя строкой (`S-1-5-21-…`); `None` — не удалось.
pub(crate) fn user_sid() -> Option<String> {
    // SAFETY: токен своего процесса открывается на чтение и закрывается через
    // OwnedHandle; буфер TOKEN_USER выделен по размеру из первого вызова и
    // выровнен как u64; строка SID освобождается LocalFree.
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) == 0 {
            return None;
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut len = 0u32;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &raw mut len,
        );
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buf.as_mut_ptr().cast(),
            len,
            &raw mut len,
        ) == 0
        {
            return None;
        }
        let user = &*buf.as_ptr().cast::<TOKEN_USER>();
        let mut s: *mut u16 = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &raw mut s) == 0 {
            return None;
        }
        let mut n = 0;
        while *s.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
        LocalFree(s.cast());
        Some(sid)
    }
}

/// Именованный mutex; `Ok(None)` — уже есть (другой экземпляр). Mutex живёт,
/// пока жив `OwnedHandle`; ОС освобождает его и при падении процесса.
pub(crate) fn create_mutex(name: &str) -> io::Result<Option<OwnedHandle>> {
    let name = wide(name);
    // SAFETY: имя — строка UTF-16 с нулём; без атрибутов безопасности; хэндл
    // сразу переходит в OwnedHandle.
    unsafe {
        let h = CreateMutexW(ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        let exists = GetLastError() == ERROR_ALREADY_EXISTS;
        let h = OwnedHandle::from_raw_handle(h);
        Ok((!exists).then_some(h))
    }
}

/// Экземпляр канала на приём; ждёт клиента. Вернувшийся `File` читается до EOF
/// (клиент закрыл свой конец).
pub(crate) fn pipe_accept(name: &str) -> io::Result<File> {
    let name = wide(name);
    // SAFETY: имя — UTF-16 с нулём; только входящий поток, удалённые клиенты
    // отклоняются; хэндл переходит в File и закрывается им.
    unsafe {
        let h = CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_INBOUND,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            0,
            PIPE_BUFFER,
            0,
            ptr::null(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let file = File::from_raw_handle(h);
        if ConnectNamedPipe(file.as_raw_handle(), ptr::null_mut()) == 0
            && GetLastError() != ERROR_PIPE_CONNECTED
        {
            return Err(io::Error::last_os_error());
        }
        Ok(file)
    }
}

/// Разрешить серверу канала `pipe` вывести своё окно на передний план: право
/// есть у запущенного пользователем процесса, то есть у второго экземпляра.
pub(crate) fn allow_server_foreground(pipe: &File) {
    let mut pid = 0u32;
    // SAFETY: `pipe` — открытый клиентский конец канала; pid пишется в локальную
    // переменную. Ошибки не важны: окно просто не выйдет вперёд.
    unsafe {
        if GetNamedPipeServerProcessId(pipe.as_raw_handle(), &raw mut pid) != 0 {
            AllowSetForegroundWindow(pid);
        }
    }
}
