//! Пути (S4 §0, §2.1): нормализация и verbatim-форма Windows.

use std::path::{Path, PathBuf};

/// Абсолютный нормализованный путь по правилам S0 v2 №6 (их реализует
/// `Location::local`); без обращения к ФС — относительный путь достраивается от
/// текущего каталога (`std::path::absolute`), symlink не разрешаются.
pub fn normalize_local(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    filenav_core::Location::local(abs).path
}

/// Verbatim-путь Windows для системных вызовов: `\\?\C:\…`, `\\?\UNC\srv\share\…`,
/// NUL на конце. Без лимита `MAX_PATH`, с хвостовыми точками и пробелами в
/// именах. `\\?\` отключает разбор `.`/`..` в Win32, поэтому они разрешаются
/// здесь, лексически. Без аллокаций для путей до 263 символов.
#[cfg(windows)]
pub fn to_verbatim(p: &Path, buf: &mut smallvec::SmallVec<[u16; 264]>) {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};

    fn push_str(buf: &mut smallvec::SmallVec<[u16; 264]>, s: &str) {
        buf.extend(s.encode_utf16());
    }

    buf.clear();
    let mut root = 0;
    for c in p.components() {
        match c {
            Component::Prefix(pc) => {
                match pc.kind() {
                    Prefix::Disk(d) | Prefix::VerbatimDisk(d) => {
                        push_str(buf, r"\\?\");
                        buf.push(u16::from(d.to_ascii_uppercase()));
                        buf.push(u16::from(b':'));
                    }
                    Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                        push_str(buf, r"\\?\UNC\");
                        buf.extend(server.encode_wide());
                        buf.push(u16::from(b'\\'));
                        buf.extend(share.encode_wide());
                    }
                    Prefix::Verbatim(x) => {
                        push_str(buf, r"\\?\");
                        buf.extend(x.encode_wide());
                    }
                    Prefix::DeviceNS(x) => {
                        push_str(buf, r"\\.\");
                        buf.extend(x.encode_wide());
                    }
                }
                root = buf.len();
            }
            Component::RootDir => {
                buf.push(u16::from(b'\\'));
                root = buf.len();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                // До предыдущего разделителя, но не выше корня.
                if buf.len() > root {
                    let cut = buf[root..]
                        .iter()
                        .rposition(|&u| u == u16::from(b'\\'))
                        .map_or(root, |i| root + i);
                    buf.truncate(cut);
                }
            }
            Component::Normal(n) => {
                if buf.len() > root && buf.last() != Some(&u16::from(b'\\')) {
                    buf.push(u16::from(b'\\'));
                }
                buf.extend(n.encode_wide());
            }
        }
    }
    buf.push(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_makes_absolute() {
        let n = normalize_local(Path::new("a/../b"));
        assert!(n.is_absolute(), "{n:?}");
        assert!(n.ends_with("b"));
        assert!(!n.to_string_lossy().contains(".."));
    }

    #[cfg(windows)]
    fn v(p: &str) -> String {
        let mut b = smallvec::SmallVec::new();
        to_verbatim(Path::new(p), &mut b);
        assert_eq!(b.last(), Some(&0), "NUL-terminated");
        String::from_utf16(&b[..b.len() - 1]).unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_cases() {
        assert_eq!(v(r"C:\"), r"\\?\C:\");
        assert_eq!(v(r"c:\a\..\b"), r"\\?\C:\b");
        assert_eq!(v(r"C:\a\.\b\..\..\.."), r"\\?\C:\");
        assert_eq!(v("C:/x/y"), r"\\?\C:\x\y");
        assert_eq!(v(r"\\srv\share\dir\f.txt"), r"\\?\UNC\srv\share\dir\f.txt");
        // Корень шары — с разделителем: без него каталог не открыть.
        assert_eq!(v(r"\\srv\share"), r"\\?\UNC\srv\share\");
        assert_eq!(v(r"\\?\C:\already"), r"\\?\C:\already");
        assert_eq!(v(r"\\?\UNC\s\sh\x"), r"\\?\UNC\s\sh\x");
        // Хвостовые точки и пробелы сохраняются: verbatim даёт к ним доступ.
        assert_eq!(v(r"C:\dir\name. "), r"\\?\C:\dir\name. ");
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_long_path() {
        let long = format!(r"C:\{}", vec!["a".repeat(100); 4].join(r"\"));
        assert!(long.len() > 260);
        let out = v(&long);
        assert_eq!(out, format!(r"\\?\{long}"));
        // Реальный каталог глубже MAX_PATH создаётся и читается через verbatim.
        let tmp = tempfile::tempdir().unwrap();
        let mut deep = tmp.path().to_path_buf();
        for _ in 0..4 {
            deep.push("d".repeat(80));
        }
        let mut b = smallvec::SmallVec::new();
        to_verbatim(&deep, &mut b);
        let vp = std::path::PathBuf::from(String::from_utf16(&b[..b.len() - 1]).unwrap());
        assert!(deep.as_os_str().len() > 260);
        std::fs::create_dir_all(&vp).unwrap();
        assert!(std::fs::metadata(&vp).unwrap().is_dir());
    }
}
