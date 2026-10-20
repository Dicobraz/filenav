//! Создание каталогов (S5 §4.14, Far): `a/b/c` — вложенные, `x;y` — несколько.

use std::io;
use std::path::{Component, Path};

use compact_str::CompactString;

/// Ошибка создания: имя из запроса и причина.
#[derive(Debug)]
pub struct MkdirError {
    pub name: CompactString,
    pub error: io::Error,
}

/// Создаёт каталоги из `spec` в `base`. Возвращает имена первого уровня
/// созданного (для курсора), в порядке запроса. Уже существующий каталог — не
/// ошибка (как `mkdir -p`), существующий файл — ошибка.
pub fn mkdir(base: &Path, spec: &str) -> Result<Vec<CompactString>, MkdirError> {
    let mut first = Vec::new();
    for part in spec.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let fail = |error| MkdirError {
            name: part.into(),
            error,
        };
        let rel = Path::new(part);
        let path = base.join(rel);
        std::fs::create_dir_all(&path).map_err(fail)?;
        if !path.is_dir() {
            return Err(fail(io::Error::from(io::ErrorKind::AlreadyExists)));
        }
        // Курсор — на элемент текущего каталога: первый компонент относительного пути.
        if !rel.is_absolute()
            && let Some(Component::Normal(n)) = rel.components().next()
        {
            first.push(n.to_string_lossy().as_ref().into());
        }
    }
    if first.is_empty() && spec.trim_matches([';', ' ']).is_empty() {
        return Err(MkdirError {
            name: CompactString::default(),
            error: io::Error::from(io::ErrorKind::InvalidInput),
        });
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_and_several() {
        let d = tempfile::tempdir().unwrap();
        let made = mkdir(d.path(), "a/b/c; x ;y").unwrap();
        assert_eq!(made, ["a", "x", "y"]);
        assert!(d.path().join("a/b/c").is_dir());
        assert!(d.path().join("x").is_dir());
        assert!(d.path().join("y").is_dir());
    }

    #[test]
    fn existing_dir_is_ok_existing_file_is_error() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("a")).unwrap();
        assert_eq!(mkdir(d.path(), "a").unwrap(), ["a"]);
        std::fs::write(d.path().join("f"), b"1").unwrap();
        let e = mkdir(d.path(), "f").unwrap_err();
        assert_eq!(e.name, "f");
    }

    #[test]
    fn empty_is_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(mkdir(d.path(), " ; ").is_err());
    }
}
