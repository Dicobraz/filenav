//! Имена и пути назначения (S5 §4.11, S5-03; только Windows, S0-decisions №57).
//!
//! Новое имя проверяется до копирования (`IssueKind::InvalidName`): запрещённые
//! символы, зарезервированные имена, хвостовые точки и пробелы, длина компонента.
//! Существующие источники с такими именами (`CON`, `a.`, `a `) читаются и
//! удаляются через verbatim-путь [`os_path`] — `\\?\` отключает разбор имени в Win32.
//! Длинные пути `std::fs` на Windows оборачивает в `\\?\` сам.

use std::path::{Path, PathBuf};

use compact_str::CompactString;

/// Чем имя не подходит для создания на NTFS/FAT/exFAT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameIssue {
    Empty,
    /// `<>:"/\|?*` и управляющие символы.
    BadChar(char),
    /// `CON`, `PRN`, `AUX`, `NUL`, `COM1`…`COM9`, `LPT1`…`LPT9` (с любым расширением).
    Reserved,
    /// Проводник и большинство программ такие имена не откроют.
    TrailingDotOrSpace,
    /// Больше 255 единиц UTF-16.
    TooLong,
}

const BAD: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Проверка нового имени (компонента пути).
pub fn check_name(name: &str) -> Result<(), NameIssue> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(NameIssue::Empty);
    }
    if let Some(c) = name
        .chars()
        .find(|&c| BAD.contains(&c) || u32::from(c) < 32)
    {
        return Err(NameIssue::BadChar(c));
    }
    if name.ends_with(['.', ' ']) {
        return Err(NameIssue::TrailingDotOrSpace);
    }
    if is_reserved(name) {
        return Err(NameIssue::Reserved);
    }
    if name.encode_utf16().count() > 255 {
        return Err(NameIssue::TooLong);
    }
    Ok(())
}

/// Зарезервированное имя устройства: основа до первой точки (и хвостовых пробелов).
fn is_reserved(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let up = base.to_ascii_uppercase();
    match up.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => {
            let cut = up.char_indices().nth(3).map_or(up.len(), |(i, _)| i);
            let (head, tail) = up.split_at(cut);
            (head == "COM" || head == "LPT")
                && matches!(
                    tail,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
        }
    }
}

/// «Исправить автоматически» (S5 §2.2, `InvalidName`): запрещённые символы → `_`,
/// хвостовые точки и пробелы — прочь, зарезервированное имя — с `_` на конце.
pub fn fix_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if BAD.contains(&c) || u32::from(c) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    if s.is_empty() {
        s.push('_');
    }
    if is_reserved(&s) {
        let dot = s.find('.').unwrap_or(s.len());
        s.insert(dot, '_');
    }
    s
}

/// Ключ имени для поиска коллизий на ФС без учёта регистра (NTFS сравнивает по
/// таблице верхнего регистра — простое посимвольное преобразование).
pub fn name_key(name: &str) -> CompactString {
    name.chars()
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect()
}

/// Verbatim-путь `\\?\…` для системных вызовов: имена с хвостовыми точками и
/// пробелами, `CON`, пути длиннее `MAX_PATH`. `.` и `..` разрешаются лексически.
pub fn os_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut buf = smallvec::SmallVec::new();
        urfin_vfs::path::to_verbatim(p, &mut buf);
        let len = buf.len().saturating_sub(1);
        PathBuf::from(std::ffi::OsString::from_wide(&buf[..len]))
    }
    #[cfg(not(windows))]
    {
        p.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert_eq!(check_name("a.txt"), Ok(()));
        assert_eq!(check_name(""), Err(NameIssue::Empty));
        assert_eq!(check_name("a:b"), Err(NameIssue::BadChar(':')));
        assert_eq!(check_name("a\tb"), Err(NameIssue::BadChar('\t')));
        assert_eq!(check_name("a."), Err(NameIssue::TrailingDotOrSpace));
        assert_eq!(check_name("a "), Err(NameIssue::TrailingDotOrSpace));
        assert_eq!(check_name("con"), Err(NameIssue::Reserved));
        assert_eq!(check_name("Nul.txt"), Err(NameIssue::Reserved));
        assert_eq!(check_name("COM1"), Err(NameIssue::Reserved));
        assert_eq!(check_name("com10"), Ok(()));
        assert_eq!(check_name("console"), Ok(()));
        assert_eq!(check_name(&"я".repeat(256)), Err(NameIssue::TooLong));
    }

    #[test]
    fn fixes() {
        assert_eq!(fix_name("a:b?.txt"), "a_b_.txt");
        assert_eq!(fix_name("name. . "), "name");
        assert_eq!(fix_name("con.txt"), "con_.txt");
        assert_eq!(fix_name("..."), "_");
        for n in ["a:b", "con", "x. ", "lpt3.log"] {
            assert_eq!(check_name(&fix_name(n)), Ok(()), "{n}");
        }
    }

    #[test]
    fn case_collisions() {
        assert_eq!(name_key("Readme.TXT"), name_key("README.txt"));
        assert_eq!(name_key("Ёлка"), name_key("ёЛКА"));
        assert_ne!(name_key("a"), name_key("b"));
    }

    /// Путь 1000 символов, `CON`, `a.`, `a ` создаются, читаются и удаляются (S5-03).
    #[cfg(windows)]
    #[test]
    fn long_and_special_paths() {
        let d = tempfile::tempdir().unwrap();
        let mut deep = d.path().to_path_buf();
        while deep.as_os_str().len() < 1000 {
            deep.push("x".repeat(60));
        }
        let p = os_path(&deep);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("f"), b"1").unwrap();
        assert_eq!(std::fs::read(p.join("f")).unwrap(), b"1");
        for name in ["CON", "a.", "a "] {
            let f = os_path(&d.path().join(name));
            std::fs::write(&f, name).unwrap();
            assert_eq!(std::fs::read(&f).unwrap(), name.as_bytes(), "{name}");
            std::fs::remove_file(&f).unwrap();
        }
        remove_dir_all::remove_dir_all(os_path(d.path()).join("x".repeat(60))).unwrap();
    }
}
