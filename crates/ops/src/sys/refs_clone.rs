//! Клон блоков `ReFS` / Dev Drive (S5-15, S5 §4.1): копия на том же томе без переноса
//! данных — `FSCTL_DUPLICATE_EXTENTS_TO_FILE` кусками по гигабайту (лимит вызова меньше 4 гигабайт,
//! выравнивание по кластеру). Перед этим — разреженность и integrity streams как у
//! источника, длина — с округлением до кластера; после — точный размер, время,
//! атрибуты и `Zone.Identifier`. Не вышло (`ERROR_NOT_SUPPORTED`, лимит ссылок на блок,
//! integrity не совпала) — `Ok(false)`: копия обычным путём. Недоделанный клон удаляется.

#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::IO::DeviceIoControl;

/// Кусок одного вызова клона.
const CHUNK: u64 = 1 << 30;

const FSCTL_SET_SPARSE: u32 = 0x0009_00C4;
const FSCTL_GET_INTEGRITY_INFORMATION: u32 = 0x0009_027C;
const FSCTL_SET_INTEGRITY_INFORMATION: u32 = 0x0009_C280;
const FSCTL_DUPLICATE_EXTENTS_TO_FILE: u32 = 0x0009_8344;
const ATTR_SPARSE: u32 = 0x200;
/// Ошибки «клон здесь нельзя» — обычная копия.
const NOT_SUPPORTED: [i32; 4] = [
    50,   // ERROR_NOT_SUPPORTED
    1,    // ERROR_INVALID_FUNCTION
    87,   // ERROR_INVALID_PARAMETER
    4401, // ERROR_BLOCK_TOO_MANY_REFERENCES
];

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct IntegrityGet {
    checksum_algorithm: u16,
    reserved: u16,
    flags: u32,
    checksum_chunk_size: u32,
    cluster_size: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct IntegritySet {
    checksum_algorithm: u16,
    reserved: u16,
    flags: u32,
}

#[repr(C)]
struct DuplicateExtents {
    file_handle: HANDLE,
    source_offset: i64,
    target_offset: i64,
    byte_count: i64,
}

fn ioctl<I, O>(h: HANDLE, code: u32, input: Option<&I>, output: Option<&mut O>) -> io::Result<()> {
    let mut ret = 0u32;
    let (ip, il) = input.map_or((std::ptr::null(), 0), |i| {
        (std::ptr::from_ref(i).cast(), size_of::<I>() as u32)
    });
    let (op, ol) = output.map_or((std::ptr::null_mut(), 0), |o| {
        (std::ptr::from_mut(o).cast(), size_of::<O>() as u32)
    });
    // SAFETY: буферы — живые структуры нужного размера на время вызова.
    let ok =
        unsafe { DeviceIoControl(h, code, ip, il, op, ol, &raw mut ret, std::ptr::null_mut()) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Клонировать `src` в новый `dst` (тот же том `ReFS`). `Ok(false)` — клон тут не
/// работает (обычная копия), `dst` не остаётся.
pub(crate) fn clone_file(src: &Path, dst: &Path, cluster: u32) -> io::Result<bool> {
    let r = try_clone(src, dst, u64::from(cluster.max(4096)));
    match r {
        Ok(()) => Ok(true),
        Err(e) => {
            let _ = std::fs::remove_file(dst);
            if e.raw_os_error().is_some_and(|c| NOT_SUPPORTED.contains(&c)) {
                Ok(false)
            } else {
                Err(e)
            }
        }
    }
}

fn try_clone(src: &Path, dst: &Path, cluster: u64) -> io::Result<()> {
    let from = File::open(src)?;
    let meta = from.metadata()?;
    let size = meta.len();
    let to = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(dst)?;
    let (hs, hd) = (from.as_raw_handle() as HANDLE, to.as_raw_handle() as HANDLE);
    let attrs = super::fs::attrs(src).unwrap_or(0);
    if attrs & ATTR_SPARSE != 0 {
        ioctl::<(), ()>(hd, FSCTL_SET_SPARSE, None, None)?;
    }
    // Integrity streams: клон требует одинакового состояния.
    let mut got = IntegrityGet::default();
    if ioctl::<(), _>(hs, FSCTL_GET_INTEGRITY_INFORMATION, None, Some(&mut got)).is_ok() {
        let set = IntegritySet {
            checksum_algorithm: got.checksum_algorithm,
            reserved: 0,
            flags: got.flags,
        };
        ioctl::<_, ()>(hd, FSCTL_SET_INTEGRITY_INFORMATION, Some(&set), None)?;
    }
    let rounded = size.div_ceil(cluster) * cluster;
    to.set_len(rounded)?;
    let mut off = 0u64;
    while off < rounded {
        let n = CHUNK.min(rounded - off);
        let d = DuplicateExtents {
            file_handle: hs,
            source_offset: i64::try_from(off).unwrap_or(i64::MAX),
            target_offset: i64::try_from(off).unwrap_or(i64::MAX),
            byte_count: i64::try_from(n).unwrap_or(i64::MAX),
        };
        ioctl::<_, ()>(hd, FSCTL_DUPLICATE_EXTENTS_TO_FILE, Some(&d), None)?;
        off += n;
    }
    to.set_len(size)?;
    to.set_modified(meta.modified()?)?;
    drop(to);
    let zone = |p: &Path| {
        let mut s = p.as_os_str().to_owned();
        s.push(":Zone.Identifier");
        std::path::PathBuf::from(s)
    };
    if let Ok(z) = std::fs::read(zone(src)) {
        let _ = std::fs::write(zone(dst), z);
    }
    if attrs != 0 {
        let _ = super::fs::set_attrs(dst, attrs);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    /// На NTFS (стенд, CI) клон не работает — `Ok(false)`, назначения нет.
    #[test]
    fn ntfs_falls_back() {
        let d = tempfile::Builder::new().prefix("urfin-").tempdir().unwrap();
        let (src, dst) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&src, vec![1u8; 70_000]).unwrap();
        let fs = urfin_vfs::volume::of(d.path()).map(|v| v.fs);
        if fs.is_ok_and(|f| f == urfin_vfs::volume::FsKind::Refs) {
            return;
        }
        assert!(!clone_file(&src, &dst, 4096).unwrap());
        assert!(!dst.exists());
    }

    /// Dev Drive: файл в 5 гигабайт клонируется несколькими вызовами и совпадает
    /// (`URFIN_DEVDRIVE=<каталог на Dev Drive>`).
    #[test]
    #[ignore = "нужен Dev Drive (`ReFS`): URFIN_DEVDRIVE=<каталог>"]
    fn dev_drive_5gib_clone_is_identical() {
        let dir = std::path::PathBuf::from(std::env::var("URFIN_DEVDRIVE").unwrap());
        let (src, dst) = (
            dir.join("urfin-clone-src.bin"),
            dir.join("urfin-clone-dst.bin"),
        );
        let _ = std::fs::remove_file(&dst);
        {
            use std::io::Write;
            let mut f = File::create(&src).unwrap();
            let block: Vec<u8> = (0..(1u32 << 20)).map(|i| (i % 253) as u8).collect();
            for _ in 0..5 * 1024 {
                f.write_all(&block).unwrap();
            }
        }
        let cluster = urfin_vfs::volume::of(&dir).unwrap().cluster;
        let started = std::time::Instant::now();
        assert!(clone_file(&src, &dst, cluster).unwrap(), "clone on ReFS");
        eprintln!("5 GiB clone: {:?}", started.elapsed());
        assert_eq!(std::fs::metadata(&dst).unwrap().len(), 5 << 30);
        let (mut left, mut right) = (File::open(&src).unwrap(), File::open(&dst).unwrap());
        let (mut lbuf, mut rbuf) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
        loop {
            let got = left.read(&mut lbuf).unwrap();
            assert_eq!(got, right.read(&mut rbuf).unwrap());
            if got == 0 {
                break;
            }
            assert_eq!(lbuf[..got], rbuf[..got]);
        }
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }
}
