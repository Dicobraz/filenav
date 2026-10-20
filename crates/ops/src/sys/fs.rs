//! Файловые вызовы копирования и переноса (S5-06): `CopyFileExW` с прогрессом
//! и отменой, `MoveFileExW` с заменой и без, атрибуты, копия ссылки (symlink,
//! junction) как ссылки. Пути в ОС — verbatim (`\\?\`, `urfin_vfs::path`).

#![allow(unsafe_code)]

use std::io;
use std::path::Path;

/// Отмена `CopyFileExW` из колбэка (`ERROR_REQUEST_ABORTED`).
pub const ERR_ABORTED: i32 = 1235;
/// Разные тома у `MoveFileExW` без копирования (`ERROR_NOT_SAME_DEVICE`).
pub const ERR_NOT_SAME_DEVICE: i32 = 17;

/// Атрибуты (`FILE_ATTRIBUTE_*`), которые переносим на каталоги.
pub const ATTR_READONLY: u32 = 0x1;
pub const ATTR_HIDDEN: u32 = 0x2;
pub const ATTR_SYSTEM: u32 = 0x4;
pub const ATTR_ARCHIVE: u32 = 0x20;

/// Как копировать файл.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyFlags {
    /// Назначение не должно существовать (`AlreadyExists`).
    pub excl: bool,
    /// Ссылку — как ссылку, без разыменования.
    pub symlink: bool,
}

/// Копирует данные, альтернативные потоки, атрибуты и время изменения (как
/// Проводник). `progress(done)` — после каждого чанка основного потока, байт
/// с начала файла; `false` — отмена (ошибка с кодом [`ERR_ABORTED`],
/// недописанное назначение система удаляет сама). Пауза — блокировкой
/// внутри `progress`.
pub fn copy_file(
    src: &Path,
    dst: &Path,
    flags: CopyFlags,
    progress: &mut dyn FnMut(u64) -> bool,
) -> io::Result<()> {
    imp::copy_file(src, dst, flags, progress)
}

/// Переименование или перенос в пределах тома. Без `replace` занятое имя —
/// `AlreadyExists`; другой том — [`ERR_NOT_SAME_DEVICE`].
pub fn move_file(src: &Path, dst: &Path, replace: bool) -> io::Result<()> {
    imp::move_file(src, dst, replace)
}

/// Атрибуты (`FILE_ATTRIBUTE_*`) по пути, без разыменования ссылки.
pub fn attrs(p: &Path) -> io::Result<u32> {
    imp::attrs(p)
}

/// Выставить атрибуты; 0 → `NORMAL`.
pub fn set_attrs(p: &Path, attrs: u32) -> io::Result<()> {
    imp::set_attrs(p, attrs)
}

/// Копия ссылки (symlink, junction) `src` в `dst` (не существует): сама
/// ссылка с той же целью, цель не трогается.
pub fn copy_link(src: &Path, dst: &Path, dir: bool) -> io::Result<()> {
    imp::copy_link(src, dst, dir)
}

/// Время изменения каталога (после всех его детей, S5 §3.6).
pub fn set_dir_mtime(p: &Path, t: std::time::SystemTime) -> io::Result<()> {
    imp::open_dir_attrs(p)?.set_modified(t)
}

/// Местное время для имён `RenameScheme::Timestamp`: `2026-10-20 23-18-05`.
pub fn local_stamp() -> String {
    imp::local_stamp()
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::path::Path;

    use smallvec::SmallVec;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        CALLBACK_CHUNK_FINISHED, COPY_FILE_COPY_SYMLINK, COPY_FILE_FAIL_IF_EXISTS, CopyFileExW,
        FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        GetFileAttributesW, INVALID_FILE_ATTRIBUTES, LPPROGRESS_ROUTINE_CALLBACK_REASON,
        MAXIMUM_REPARSE_DATA_BUFFER_SIZE, MOVEFILE_REPLACE_EXISTING, MoveFileExW, PROGRESS_CANCEL,
        PROGRESS_CONTINUE, SetFileAttributesW,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FSCTL_GET_REPARSE_POINT, FSCTL_SET_REPARSE_POINT};
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    use super::CopyFlags;

    const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
    const GENERIC_WRITE: u32 = 0x4000_0000;

    fn wide(p: &Path) -> SmallVec<[u16; 264]> {
        let mut b = SmallVec::new();
        urfin_vfs::path::to_verbatim(p, &mut b);
        b
    }

    type Progress<'a> = &'a mut dyn FnMut(u64) -> bool;

    #[allow(clippy::too_many_arguments, reason = "сигнатура LPPROGRESS_ROUTINE")]
    unsafe extern "system" fn on_chunk(
        _total: i64,
        _total_done: i64,
        _stream_size: i64,
        stream_done: i64,
        stream: u32,
        reason: LPPROGRESS_ROUTINE_CALLBACK_REASON,
        _src: HANDLE,
        _dst: HANDLE,
        data: *const c_void,
    ) -> u32 {
        if stream != 1 || reason != CALLBACK_CHUNK_FINISHED {
            return PROGRESS_CONTINUE;
        }
        // SAFETY: `data` — указатель на `Progress` из `copy_file`, живой всё
        // время вызова `CopyFileExW`; колбэк зовётся в том же потоке.
        let f = unsafe { &mut *data.cast::<Progress<'_>>().cast_mut() };
        let done = u64::try_from(stream_done).unwrap_or(0);
        // Паника не должна пересечь границу FFI: отмена копии.
        match catch_unwind(AssertUnwindSafe(|| f(done))) {
            Ok(true) => PROGRESS_CONTINUE,
            _ => PROGRESS_CANCEL,
        }
    }

    pub fn copy_file(
        src: &Path,
        dst: &Path,
        flags: CopyFlags,
        mut progress: Progress<'_>,
    ) -> io::Result<()> {
        let (s, d) = (wide(src), wide(dst));
        let mut f = 0;
        if flags.excl {
            f |= COPY_FILE_FAIL_IF_EXISTS;
        }
        if flags.symlink {
            f |= COPY_FILE_COPY_SYMLINK;
        }
        let data: *mut Progress<'_> = &raw mut progress;
        // SAFETY: строки с NUL живут до конца вызова; `data` указывает на
        // локальную переменную, переживающую вызов.
        let ok = unsafe {
            CopyFileExW(
                s.as_ptr(),
                d.as_ptr(),
                Some(on_chunk),
                data.cast::<c_void>().cast_const(),
                std::ptr::null_mut(),
                f,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn move_file(src: &Path, dst: &Path, replace: bool) -> io::Result<()> {
        let (s, d) = (wide(src), wide(dst));
        let f = if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
        // SAFETY: строки с NUL живут до конца вызова.
        if unsafe { MoveFileExW(s.as_ptr(), d.as_ptr(), f) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn attrs(p: &Path) -> io::Result<u32> {
        let w = wide(p);
        // SAFETY: строка с NUL.
        let a = unsafe { GetFileAttributesW(w.as_ptr()) };
        if a == INVALID_FILE_ATTRIBUTES {
            return Err(io::Error::last_os_error());
        }
        Ok(a)
    }

    pub fn set_attrs(p: &Path, attrs: u32) -> io::Result<()> {
        let w = wide(p);
        let a = if attrs == 0 {
            FILE_ATTRIBUTE_NORMAL
        } else {
            attrs
        };
        // SAFETY: строка с NUL.
        if unsafe { SetFileAttributesW(w.as_ptr(), a) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn open_dir_attrs(p: &Path) -> io::Result<File> {
        const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
        OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(p)
    }

    fn open_reparse(p: &Path, write: bool) -> io::Result<File> {
        OpenOptions::new()
            .access_mode(if write { GENERIC_WRITE } else { 0 })
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(p)
    }

    /// Буфер `REPARSE_DATA_BUFFER` ссылки.
    fn reparse_data(src: &Path) -> io::Result<Vec<u8>> {
        let f = open_reparse(src, false)?;
        let mut buf = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize];
        let mut got = 0u32;
        // SAFETY: хэндл открыт; буфер `buf.len()` байт; синхронный вызов.
        let ok = unsafe {
            DeviceIoControl(
                f.as_raw_handle() as HANDLE,
                FSCTL_GET_REPARSE_POINT,
                std::ptr::null(),
                0,
                buf.as_mut_ptr().cast(),
                MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
                &raw mut got,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(got as usize);
        Ok(buf)
    }

    fn set_reparse(dst: &Path, data: &[u8]) -> io::Result<()> {
        let f = open_reparse(dst, true)?;
        let len = u32::try_from(data.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
        let mut got = 0u32;
        // SAFETY: хэндл открыт на запись; `data` — буфер от
        // FSCTL_GET_REPARSE_POINT; синхронный вызов.
        let ok = unsafe {
            DeviceIoControl(
                f.as_raw_handle() as HANDLE,
                FSCTL_SET_REPARSE_POINT,
                data.as_ptr().cast(),
                len,
                std::ptr::null_mut(),
                0,
                &raw mut got,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn copy_link(src: &Path, dst: &Path, dir: bool) -> io::Result<()> {
        let data = reparse_data(src)?;
        let tag = data
            .get(..4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        if tag == IO_REPARSE_TAG_SYMLINK {
            // Symlink — через std (без прав администратора в режиме
            // разработчика); относительная цель остаётся относительной.
            let target = std::fs::read_link(src)?;
            return if dir {
                std::os::windows::fs::symlink_dir(target, dst)
            } else {
                std::os::windows::fs::symlink_file(target, dst)
            };
        }
        // Junction и прочие теги — тем же буфером.
        if dir {
            std::fs::create_dir(dst)?;
        } else {
            File::create_new(dst)?;
        }
        set_reparse(dst, &data).inspect_err(|_| {
            let _ = if dir {
                std::fs::remove_dir(dst)
            } else {
                std::fs::remove_file(dst)
            };
        })
    }

    pub fn local_stamp() -> String {
        // SAFETY: структура на стеке заполняется системой.
        let t = unsafe {
            let mut t = std::mem::zeroed();
            GetLocalTime(&raw mut t);
            t
        };
        format!(
            "{:04}-{:02}-{:02} {:02}-{:02}-{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
}

/// Не Windows (S0-decisions №57: не собирается и не проверяется) — std.
#[cfg(not(windows))]
mod imp {
    use std::io;
    use std::path::Path;

    use super::CopyFlags;

    pub fn copy_file(
        src: &Path,
        dst: &Path,
        flags: CopyFlags,
        progress: &mut dyn FnMut(u64) -> bool,
    ) -> io::Result<()> {
        if flags.excl && std::fs::symlink_metadata(dst).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let n = std::fs::copy(src, dst)?;
        progress(n);
        Ok(())
    }

    pub fn move_file(src: &Path, dst: &Path, replace: bool) -> io::Result<()> {
        if !replace && std::fs::symlink_metadata(dst).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        std::fs::rename(src, dst)
    }

    pub fn open_dir_attrs(p: &Path) -> io::Result<std::fs::File> {
        std::fs::File::open(p)
    }

    pub fn attrs(_p: &Path) -> io::Result<u32> {
        Ok(0)
    }

    pub fn set_attrs(_p: &Path, _attrs: u32) -> io::Result<()> {
        Ok(())
    }

    pub fn copy_link(src: &Path, dst: &Path, _dir: bool) -> io::Result<()> {
        std::os::unix::fs::symlink(std::fs::read_link(src)?, dst)
    }

    pub fn local_stamp() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
            .to_string()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn old() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)
    }

    /// Признак завершённости (S5 §6.1): пока `CopyFileExW` пишет данные, время
    /// изменения назначения ещё не исходное — убитая посреди копия не
    /// выглядит готовой.
    #[test]
    fn mtime_is_set_after_data() {
        let d = tempfile::Builder::new().prefix("urfin-").tempdir().unwrap();
        let (src, dst) = (d.path().join("big"), d.path().join("copy"));
        std::fs::write(&src, vec![7u8; 48 << 20]).unwrap();
        let f = std::fs::File::options().write(true).open(&src).unwrap();
        f.set_modified(old()).unwrap();
        drop(f);
        let mut seen = Vec::new();
        copy_file(&src, &dst, CopyFlags::default(), &mut |done| {
            if done < 40 << 20 {
                let m = std::fs::metadata(&dst).unwrap().modified().unwrap();
                seen.push(m == old());
            }
            true
        })
        .unwrap();
        assert!(!seen.is_empty(), "no chunk callbacks");
        assert!(
            seen.iter().all(|s| !s),
            "source mtime before data end: {seen:?}"
        );
        let m = std::fs::metadata(&dst).unwrap().modified().unwrap();
        assert_eq!(m, old());
        assert_eq!(std::fs::read(&dst).unwrap(), std::fs::read(&src).unwrap());
    }

    #[test]
    fn cancel_removes_partial_and_excl_refuses() {
        let d = tempfile::Builder::new().prefix("urfin-").tempdir().unwrap();
        let (src, dst) = (d.path().join("big"), d.path().join("copy"));
        std::fs::write(&src, vec![1u8; 32 << 20]).unwrap();
        let e = copy_file(&src, &dst, CopyFlags::default(), &mut |_| false).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(ERR_ABORTED));
        assert!(!dst.exists(), "partial copy left behind");
        std::fs::write(&dst, b"x").unwrap();
        let flags = CopyFlags {
            excl: true,
            symlink: false,
        };
        let e = copy_file(&src, &dst, flags, &mut |_| true).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&dst).unwrap(), b"x");
    }

    #[test]
    fn move_noreplace_and_replace() {
        let d = tempfile::Builder::new().prefix("urfin-").tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let e = move_file(&a, &b, false).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        move_file(&a, &b, true).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"a");
        assert!(!a.exists());
    }

    #[test]
    fn junction_is_copied_as_link() {
        let d = tempfile::Builder::new().prefix("urfin-").tempdir().unwrap();
        let target = d.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("f"), b"1").unwrap();
        let link = d.path().join("link");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(ok, "mklink /J");
        let copy = d.path().join("copy");
        copy_link(&link, &copy, true).unwrap();
        assert!(
            std::fs::symlink_metadata(&copy)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(copy.join("f")).unwrap(), b"1");
        assert_eq!(attrs(&copy).unwrap() & 0x400, 0x400, "reparse point");
    }
}
