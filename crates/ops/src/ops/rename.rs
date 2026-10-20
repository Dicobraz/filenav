//! Переименование одного элемента в его каталоге (S5 §4.14). Существующее имя
//! не затирается (`std::fs::rename` на Windows заменил бы файл); смена только
//! регистра — прямой вызов. Перенос в другой каталог — `ops.move` (S5-08).

use std::io;
use std::path::Path;

/// Имя с разделителем каталогов — это перенос, не переименование.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}

/// Переименовать `from` в `to` внутри `dir`.
pub fn rename(dir: &Path, from: &str, to: &str) -> io::Result<()> {
    if !is_plain_name(to) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    if from == to {
        return Ok(());
    }
    let (src, dst) = (dir.join(from), dir.join(to));
    if from.to_lowercase() == to.to_lowercase() {
        // Та же запись другим регистром: «существует» — это она сама.
        return crate::sys::rename(&src, &dst, true);
    }
    crate::sys::rename(&src, &dst, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_file_and_dir() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), b"1").unwrap();
        rename(d.path(), "a.txt", "b.txt").unwrap();
        assert!(d.path().join("b.txt").is_file());
        std::fs::create_dir(d.path().join("dir")).unwrap();
        rename(d.path(), "dir", "dir2").unwrap();
        assert!(d.path().join("dir2").is_dir());
    }

    #[test]
    fn does_not_overwrite() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a"), b"a").unwrap();
        std::fs::write(d.path().join("b"), b"b").unwrap();
        let e = rename(d.path(), "a", "b").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(d.path().join("b")).unwrap(), b"b");
        assert!(d.path().join("a").is_file());
    }

    #[test]
    fn case_only() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("readme"), b"1").unwrap();
        rename(d.path(), "readme", "README").unwrap();
        let names: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["README"]);
    }

    #[test]
    fn rejects_paths() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a"), b"1").unwrap();
        assert!(rename(d.path(), "a", "x/y").is_err());
        assert!(rename(d.path(), "a", "..").is_err());
    }
}
