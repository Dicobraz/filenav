//! Быстрая корзина (S5-26, S0-decisions №79): перенос прямо в `<том>\$Recycle.Bin\<SID>\`
//! как `$R<6>` и запись рядом `$I<6>` (формат v2: версия 2, размер, время удаления
//! FILETIME, длина пути в символах с нулём, путь UTF-16), затем `SHChangeNotify` —
//! Проводник видит элемент и восстанавливает его. Только локальные несъёмные тома, где
//! папка корзины пользователя уже есть; остальное (и любая ошибка) — обычным путём
//! (`IFileOperation`, [`super::trash_win`]).

#![allow(unsafe_code)]

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Итог: перенесены и те, что пойдут обычным путём.
#[derive(Debug, Default)]
pub(crate) struct Fast {
    pub moved: Vec<PathBuf>,
    /// Куда легли перенесённые (`$R…`), по порядку `moved`.
    pub stored: Vec<PathBuf>,
    pub rest: Vec<PathBuf>,
}

/// Каталог корзины текущего пользователя на томе `path`, если он годится.
fn bin_for(path: &Path, sid: &str) -> Option<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    // `DRIVE_FIXED` (WinBase.h).
    const DRIVE_FIXED: u32 = 3;
    let root = path.ancestors().last()?.to_path_buf();
    let s = root.as_os_str().to_string_lossy().into_owned();
    // `C:\` — да; UNC и `\\?\` — нет (сеть, особые пути).
    if s.len() != 3 || !s.ends_with(":\\") {
        return None;
    }
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: строка с нулём.
    if unsafe { GetDriveTypeW(wide.as_ptr()) } != DRIVE_FIXED {
        return None;
    }
    let bin = root.join("$Recycle.Bin").join(sid);
    bin.is_dir().then_some(bin)
}

/// SID текущего пользователя строкой (`S-1-5-21-…`).
fn user_sid() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: псевдодескриптор своего процесса; токен закрывается ниже.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return None;
    }
    let mut need = 0u32;
    // SAFETY: запрос размера.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &raw mut need) };
    let mut buf = vec![0u8; need as usize];
    // SAFETY: буфер нужного размера.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr().cast(),
            need,
            &raw mut need,
        )
    };
    let mut out = None;
    if ok != 0 {
        // SAFETY: буфер — TOKEN_USER (выравнивание u8 хватает для чтения через read_unaligned).
        let user: TOKEN_USER = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast()) };
        let mut s: *mut u16 = std::ptr::null_mut();
        // SAFETY: SID из токена; строка освобождается LocalFree.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut s) } != 0 {
            // SAFETY: строка с нулём от системы.
            let len = (0..usize::MAX)
                .take_while(|&i| unsafe { *s.add(i) } != 0)
                .count();
            // SAFETY: len символов.
            let slice = unsafe { std::slice::from_raw_parts(s, len) };
            out = Some(String::from_utf16_lossy(slice));
            // SAFETY: выделено ConvertSidToStringSidW.
            unsafe { LocalFree(s.cast()) };
        }
    }
    // SAFETY: токен открыт выше.
    unsafe { CloseHandle(token) };
    out
}

/// Содержимое `$I` v2.
pub(crate) fn info_v2(original: &Path, size: u64, deleted: SystemTime) -> Vec<u8> {
    let path: Vec<u16> = original.as_os_str().encode_wide().chain(Some(0)).collect();
    // FILETIME: сотни наносекунд от 1601-01-01.
    let unix = deleted
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let ft = u64::try_from(unix.as_nanos() / 100).unwrap_or(0) + 116_444_736_000_000_000;
    let mut out = Vec::with_capacity(28 + path.len() * 2);
    out.extend_from_slice(&2u64.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&ft.to_le_bytes());
    out.extend_from_slice(&u32::try_from(path.len()).unwrap_or(0).to_le_bytes());
    for c in path {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out
}

/// Имя `$R…`/`$I…`: 6 знаков из времени и счётчика, расширение — как у файла.
fn names(path: &Path, n: u32, is_dir: bool) -> (String, String) {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        ^ (u128::from(n) << 64)
        ^ u128::from(std::process::id());
    let mut x = seed;
    let tag: String = (0..6)
        .map(|_| {
            let c = ABC[(x % 36) as usize] as char;
            x /= 36;
            c
        })
        .collect();
    let ext = if is_dir {
        String::new()
    } else {
        path.extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default()
    };
    (format!("$R{tag}{ext}"), format!("$I{tag}{ext}"))
}

/// Перенести `items` в корзину напрямую; что не вышло — в `rest`.
pub(crate) fn recycle(items: &[PathBuf]) -> Fast {
    let mut out = Fast::default();
    let Some(sid) = user_sid() else {
        out.rest = items.to_vec();
        return out;
    };
    let now = SystemTime::now();
    let mut touched: Vec<PathBuf> = Vec::new();
    for (k, p) in items.iter().enumerate() {
        let moved = bin_for(p, &sid).and_then(|bin| {
            let meta = std::fs::symlink_metadata(p).ok()?;
            let is_dir = meta.is_dir();
            for attempt in 0..4u32 {
                let (r, i) = names(p, u32::try_from(k).unwrap_or(0) * 4 + attempt, is_dir);
                let (rp, ip) = (bin.join(&r), bin.join(&i));
                if rp.exists() || ip.exists() {
                    continue;
                }
                let size = if is_dir { 0 } else { meta.len() };
                std::fs::write(&ip, info_v2(p, size, now)).ok()?;
                if std::fs::rename(p, &rp).is_err() {
                    let _ = std::fs::remove_file(&ip);
                    return None;
                }
                if !touched.contains(&bin) {
                    touched.push(bin.clone());
                }
                return Some(rp);
            }
            None
        });
        if let Some(at) = moved {
            out.moved.push(p.clone());
            out.stored.push(at);
        } else {
            out.rest.push(p.clone());
        }
    }
    notify(&touched, &out.moved);
    out
}

/// Сказать Shell: элементы удалены, папки корзин обновились.
fn notify(bins: &[PathBuf], moved: &[PathBuf]) {
    use windows_sys::Win32::UI::Shell::{
        SHCNE_DELETE, SHCNE_RMDIR, SHCNE_UPDATEDIR, SHCNF_PATHW, SHChangeNotify,
    };
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() };
    for p in moved {
        let w = wide(p);
        let ev = if p.extension().is_none() && !p.exists() {
            SHCNE_RMDIR
        } else {
            SHCNE_DELETE
        };
        // SAFETY: путь с нулём; Shell копирует данные.
        unsafe {
            SHChangeNotify(
                ev.cast_signed(),
                SHCNF_PATHW,
                w.as_ptr().cast(),
                std::ptr::null(),
            );
        };
    }
    for b in bins {
        let w = wide(b);
        // SAFETY: то же.
        unsafe {
            SHChangeNotify(
                SHCNE_UPDATEDIR.cast_signed(),
                SHCNF_PATHW,
                w.as_ptr().cast(),
                std::ptr::null(),
            );
        };
    }
}

/// Отключена ли быстрая корзина политикой (`ExperimentalFastTrash` = 0, №66).
pub(crate) fn policy_off() -> bool {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
    };
    let key: Vec<u16> = r"Software\Policies\Aahz\Urfin"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = "ExperimentalFastTrash"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut v = 0u32;
    let mut len = 4u32;
    // SAFETY: буфер DWORD.
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut v).cast(),
            &raw mut len,
        )
    };
    r == 0 && v == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_v2_layout() {
        let p = Path::new(r"C:\a\b.txt");
        let t = SystemTime::UNIX_EPOCH;
        let b = info_v2(p, 42, t);
        assert_eq!(&b[0..8], &2u64.to_le_bytes());
        assert_eq!(&b[8..16], &42u64.to_le_bytes());
        assert_eq!(&b[16..24], &116_444_736_000_000_000u64.to_le_bytes());
        assert_eq!(&b[24..28], &11u32.to_le_bytes());
        assert_eq!(b.len(), 28 + 11 * 2);
    }

    #[test]
    fn names_keep_extension() {
        let (r, i) = names(Path::new(r"C:\x\report.xlsx"), 0, false);
        assert!(
            r.starts_with("$R") && Path::new(&r).extension().is_some_and(|e| e == "xlsx"),
            "{r}"
        );
        assert!(
            i.starts_with("$I") && Path::new(&i).extension().is_some_and(|e| e == "xlsx"),
            "{i}"
        );
        assert_eq!(r[2..], i[2..]);
        let (r, _) = names(Path::new(r"C:\x\dir.d"), 1, true);
        assert_eq!(r.len(), 8);
    }

    #[test]
    fn network_and_odd_paths_are_not_fast() {
        assert!(bin_for(Path::new(r"\\server\share\f.txt"), "S-1").is_none());
        assert!(bin_for(Path::new(r"\\?\C:\f.txt"), "S-1").is_none());
    }

    /// Настоящая корзина пользователя: временный файл уходит в `$Recycle.Bin\<SID>`,
    /// рядом `$I` с исходным путём; затем возвращается (`#[ignore]`: трогает корзину).
    #[test]
    #[ignore = "пишет в корзину текущего пользователя (и убирает за собой)"]
    fn moves_to_recycle_bin_and_back() {
        let d = tempfile::Builder::new()
            .prefix("urfin-fast-")
            .tempdir()
            .unwrap();
        let f = d.path().join("probe.txt");
        std::fs::write(&f, b"urfin").unwrap();
        let out = recycle(std::slice::from_ref(&f));
        assert_eq!(out.moved, vec![f.clone()], "rest: {:?}", out.rest);
        assert!(!f.exists());
        let r = &out.stored[0];
        let name = r.file_name().unwrap().to_string_lossy().into_owned();
        let i = r.with_file_name(name.replacen("$R", "$I", 1));
        let info = std::fs::read(&i).unwrap();
        assert_eq!(info[24..], info_v2(&f, 5, SystemTime::now())[24..]);
        std::fs::rename(r, &f).unwrap();
        std::fs::remove_file(&i).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"urfin");
    }
}
