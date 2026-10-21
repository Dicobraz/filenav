//! Кто держит файл (S0-decisions №69): Restart Manager (`RmGetList`) по пути — процессы,
//! открывшие файл, с именем exe и PID. Зовётся из потока вопросов ops (не UI). Для
//! сетевых путей не спрашивается: процесс на чужой машине не узнать.
#![allow(unsafe_code)]

use std::path::Path;

/// Процесс, открывший файл: имя exe (`EXCEL.EXE`) и PID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub exe: String,
    pub pid: u32,
}

/// Процессы, держащие `path`; пусто — никто, ошибка Restart Manager или сетевой путь.
#[cfg(windows)]
pub fn holders(path: &Path) -> Vec<Holder> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::RestartManager::{
        CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources,
        RmStartSession,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };

    let s = path.as_os_str().to_string_lossy();
    if s.starts_with(r"\\") && !s.starts_with(r"\\?\") {
        return Vec::new();
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    let mut session = 0u32;
    // SAFETY: буфер ключа — CCH_RM_SESSION_KEY + 1 символов, как требует RmStartSession.
    if unsafe { RmStartSession(&raw mut session, 0, key.as_mut_ptr()) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    let files = [wide.as_ptr()];
    // SAFETY: один путь, завершённый нулём; живёт до конца функции.
    let reg = unsafe {
        RmRegisterResources(
            session,
            1,
            files.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        )
    };
    if reg == ERROR_SUCCESS {
        let mut need = 0u32;
        let mut have = 0u32;
        let mut reasons = 0u32;
        // Сначала — сколько; потом — сами записи (за это время число могло вырасти).
        // SAFETY: нулевой буфер с have = 0 — законный запрос размера.
        let r = unsafe {
            RmGetList(
                session,
                &raw mut need,
                &raw mut have,
                std::ptr::null_mut(),
                &raw mut reasons,
            )
        };
        if (r == ERROR_MORE_DATA || r == ERROR_SUCCESS) && need > 0 {
            have = need + 2;
            // SAFETY: RM_PROCESS_INFO — простая C-структура, нули — допустимое значение.
            let mut list: Vec<RM_PROCESS_INFO> = vec![unsafe { std::mem::zeroed() }; have as usize];
            // SAFETY: буфер на `have` записей.
            let r = unsafe {
                RmGetList(
                    session,
                    &raw mut need,
                    &raw mut have,
                    list.as_mut_ptr(),
                    &raw mut reasons,
                )
            };
            if r == ERROR_SUCCESS {
                for info in list.iter().take(have as usize) {
                    let pid = info.Process.dwProcessId;
                    let mut exe = String::new();
                    // SAFETY: только чтение имени образа; дескриптор закрывается.
                    unsafe {
                        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                        if !h.is_null() {
                            let mut buf = [0u16; 1024];
                            let mut len = buf.len() as u32;
                            if QueryFullProcessImageNameW(
                                h,
                                PROCESS_NAME_WIN32,
                                buf.as_mut_ptr(),
                                &raw mut len,
                            ) != 0
                            {
                                let full = String::from_utf16_lossy(&buf[..len as usize]);
                                exe = full
                                    .rsplit(['\\', '/'])
                                    .next()
                                    .unwrap_or_default()
                                    .to_uppercase();
                            }
                            CloseHandle(h);
                        }
                    }
                    if exe.is_empty() {
                        // Нет прав на процесс — имя приложения из Restart Manager.
                        let n = info.strAppName.iter().position(|&c| c == 0).unwrap_or(0);
                        exe = String::from_utf16_lossy(&info.strAppName[..n]);
                    }
                    out.push(Holder { exe, pid });
                }
            }
        }
    }
    // SAFETY: сессия открыта выше.
    unsafe { RmEndSession(session) };
    out
}

#[cfg(not(windows))]
pub fn holders(_path: &Path) -> Vec<Holder> {
    Vec::new()
}

#[cfg(all(test, windows))]
mod tests {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    use super::*;

    /// Свой процесс держит файл без общего доступа — Restart Manager его называет.
    #[test]
    fn own_process_is_reported() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("locked.bin");
        std::fs::write(&p, b"x").unwrap();
        let _f = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&p)
            .unwrap();
        let h = holders(&p);
        assert!(h.iter().any(|h| h.pid == std::process::id()), "{h:?}");
        assert_eq!(holders(Path::new(r"\\server\share\f.txt")).len(), 0);
    }
}
