//! Windows: метаданные по хэндлу, место на томе, корни (S4 §3.1, S4-10).
//!
//! `stat`: `CreateFileW(FILE_READ_ATTRIBUTES, share all, BACKUP_SEMANTICS
//! [| OPEN_REPARSE_POINT])` и `GetFileInformationByHandleEx` (Basic, Standard,
//! `AttributeTag`, `Id`) — время изменения метаданных, размер на диске, число жёстких
//! ссылок, тег reparse и `FileId` без чтения содержимого. Место — `GetDiskFreeSpaceExW`.
//! Корни — `GetLogicalDrives`/`GetDriveTypeW` сразу, метки и место — в потоке на
//! диск (`roots` в `local`).

use std::mem::{MaybeUninit, size_of};
use std::path::Path;
use std::ptr;

use smallvec::SmallVec;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileAttributeTagInfo,
    FileBasicInfo, FileIdInfo, FileStandardInfo, GetDiskFreeSpaceExW, GetDriveTypeW,
    GetLogicalDrives, GetVolumeInformationW, OPEN_EXISTING,
};

use crate::api::{RootKind, Space, VfsError};

/// Сверх std: что даёт хэндл.
#[derive(Clone, Copy, Debug)]
pub struct Extra {
    pub change: i64,
    pub alloc: u64,
    pub nlink: u32,
    pub reparse_tag: Option<u32>,
    pub file_id: (u64, u128),
}

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

fn wide(p: &Path) -> SmallVec<[u16; 264]> {
    let mut b = SmallVec::new();
    crate::path::to_verbatim(p, &mut b);
    b
}

fn info<T>(
    h: windows_sys::Win32::Foundation::HANDLE,
    class: FILE_INFO_BY_HANDLE_CLASS,
) -> Option<T> {
    let mut v = MaybeUninit::<T>::uninit();
    // SAFETY: буфер — `T` нужного размера для класса; хэндл открыт.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
            h,
            class,
            v.as_mut_ptr().cast(),
            u32::try_from(size_of::<T>()).ok()?,
        )
    };
    // SAFETY: при успехе ОС заполнила структуру.
    (ok != 0).then(|| unsafe { v.assume_init() })
}

/// Метаданные по хэндлу; `None` — открыть не удалось (нет прав на атрибуты и т. п.):
/// тогда остаётся то, что дал std.
pub fn stat_extra(p: &Path, follow: bool) -> Option<Extra> {
    let path = wide(p);
    let flags = FILE_FLAG_BACKUP_SEMANTICS
        | if follow {
            0
        } else {
            FILE_FLAG_OPEN_REPARSE_POINT
        };
    // SAFETY: путь — NUL-терминированный UTF-16; хэндл закрывается ниже.
    let h = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let basic: Option<FILE_BASIC_INFO> = info(h, FileBasicInfo);
    let std_info: Option<FILE_STANDARD_INFO> = info(h, FileStandardInfo);
    let tag: Option<FILE_ATTRIBUTE_TAG_INFO> = info(h, FileAttributeTagInfo);
    let id: Option<FILE_ID_INFO> = info(h, FileIdInfo);
    // SAFETY: хэндл открыт выше и закрывается один раз.
    unsafe { CloseHandle(h) };
    Some(Extra {
        change: basic.map_or(i64::MIN, |b| filetime_nanos(b.ChangeTime)),
        alloc: std_info.map_or(0, |s| u64::try_from(s.AllocationSize).unwrap_or(0)),
        nlink: std_info.map_or(1, |s| s.NumberOfLinks),
        reparse_tag: tag
            .filter(|t| t.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
            .map(|t| t.ReparseTag),
        file_id: id.map_or((0, 0), |i| {
            (
                i.VolumeSerialNumber,
                u128::from_le_bytes(i.FileId.Identifier),
            )
        }),
    })
}

/// FILETIME (100 нс от 1601) → нс от Unix epoch.
fn filetime_nanos(t: i64) -> i64 {
    const EPOCH_DIFF: i64 = 116_444_736_000_000_000;
    if t <= 0 {
        return i64::MIN;
    }
    (t - EPOCH_DIFF).saturating_mul(100)
}

/// Корень/каталог как обычный путь с `\` в конце (для `GetDiskFreeSpaceExW` и
/// `GetVolumeInformationW` verbatim-форма не нужна).
fn dir_wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut v: Vec<u16> = p.as_os_str().encode_wide().collect();
    if v.last() != Some(&u16::from(b'\\')) {
        v.push(u16::from(b'\\'));
    }
    v.push(0);
    v
}

/// Место на томе, где лежит `p`.
pub fn space(p: &Path) -> Result<Space, VfsError> {
    let path = dir_wide(p);
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: путь NUL-терминирован, выходы — валидные `u64`.
    let ok = unsafe {
        GetDiskFreeSpaceExW(path.as_ptr(), &raw mut avail, &raw mut total, &raw mut free)
    };
    if ok == 0 {
        // SAFETY: без аргументов.
        let code = unsafe { GetLastError() };
        return Err(VfsError::from_io(
            p,
            std::io::Error::from_raw_os_error(code.cast_signed()),
        ));
    }
    Ok(Space { total, free, avail })
}

/// Буквы дисков (`C:\`) с типом — быстро, без обращения к томам.
pub fn drives() -> Vec<(String, RootKind)> {
    // SAFETY: без аргументов.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| {
            let root = format!("{}:\\", char::from(b'A' + i));
            let w: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            // SAFETY: NUL-терминированная строка корня.
            let kind = match unsafe { GetDriveTypeW(w.as_ptr()) } {
                2 => RootKind::Removable,
                3 => RootKind::Fixed,
                4 => RootKind::Network,
                5 => RootKind::Optical,
                6 => RootKind::Ram,
                _ => RootKind::Other,
            };
            (root, kind)
        })
        .collect()
}

/// Метка тома (может ждать: сеть, оптический привод) — вызывается в своём потоке.
pub fn label(root: &Path) -> Option<String> {
    let path = dir_wide(root);
    let mut buf = [0u16; 261];
    // SAFETY: путь NUL-терминирован, буфер метки — 261 символ (MAX_PATH + 1).
    let ok = unsafe {
        GetVolumeInformationW(
            path.as_ptr(),
            buf.as_mut_ptr(),
            u32::try_from(buf.len()).ok()?,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}
