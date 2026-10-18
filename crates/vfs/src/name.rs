//! Имена (S4 §0, §2.1): обратимое экранирование не-UTF-8, расширение, dotfile.
//!
//! Имя, не представимое в UTF-8 (непарный суррогат UTF-16 на NTFS, невалидный
//! байт на unix), кодируется так: каждый невалидный юнит — символ приватной зоны
//! `U+10F700 + (юнит − 0xD800)` (Windows, до `U+10FEFF`) или `U+10FF00 + байт`
//! (unix, `U+10FF80…U+10FFFF`); записи ставится `Attrs::ESCAPED_NAME`.
//! Валидные имена не меняются и флаг не получают. В экранированном имени
//! настоящий символ из этой зоны тоже экранируется по своим юнитам или байтам —
//! иначе декодирование было бы неоднозначным.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use compact_str::CompactString;

use crate::api::{Attrs, Entry, VfsPath};

const WIDE_BASE: u32 = 0x10_F700;
const BYTE_BASE: u32 = 0x10_FF00;

/// Символ из зоны экранирования.
fn is_escape(c: char) -> bool {
    (WIDE_BASE..=0x10_FFFF).contains(&u32::from(c))
}

/// Может ли имя быть экранированным (символы зоны экранирования): подсказка
/// `ESCAPED_NAME` для `stat_entry` по имени без `Entry`.
pub fn may_be_escaped(name: &str) -> bool {
    name.chars().any(is_escape)
}

/// UTF-16 → имя; `true` — экранировано.
pub fn escape_wide(units: &[u16]) -> (CompactString, bool) {
    if let Ok(s) = String::from_utf16(units) {
        return (s.into(), false);
    }
    let mut out = String::with_capacity(units.len() + 8);
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        let pair = (0xD800..0xDC00).contains(&u)
            && units
                .get(i + 1)
                .is_some_and(|&l| (0xDC00..0xE000).contains(&l));
        if pair {
            let c = char::decode_utf16([u, units[i + 1]])
                .next()
                .and_then(Result::ok)
                .expect("valid surrogate pair");
            if is_escape(c) {
                // Настоящий символ из зоны — юнитами, как непарные.
                out.push(wide_escape(u));
                out.push(wide_escape(units[i + 1]));
            } else {
                out.push(c);
            }
            i += 2;
        } else if (0xD800..0xE000).contains(&u) {
            out.push(wide_escape(u));
            i += 1;
        } else {
            out.push(char::from_u32(u32::from(u)).expect("BMP non-surrogate"));
            i += 1;
        }
    }
    (out.into(), true)
}

fn wide_escape(surrogate: u16) -> char {
    char::from_u32(WIDE_BASE + u32::from(surrogate - 0xD800)).expect("in private plane")
}

/// Обратное к [`escape_wide`].
pub fn unescape_wide(name: &str, escaped: bool) -> Vec<u16> {
    if !escaped {
        return name.encode_utf16().collect();
    }
    let mut out = Vec::with_capacity(name.len());
    for c in name.chars() {
        let v = u32::from(c);
        if (WIDE_BASE..BYTE_BASE).contains(&v) {
            out.push(u16::try_from(v - WIDE_BASE + 0xD800).expect("surrogate range"));
        } else {
            let mut b = [0u16; 2];
            out.extend_from_slice(c.encode_utf16(&mut b));
        }
    }
    out
}

/// Байты имени (unix) → имя; `true` — экранировано.
pub fn escape_bytes(bytes: &[u8]) -> (CompactString, bool) {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.into(), false);
    }
    let mut out = String::with_capacity(bytes.len() + 8);
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if is_escape(c) {
                let mut b = [0u8; 4];
                for &x in c.encode_utf8(&mut b).as_bytes() {
                    out.push(byte_escape(x));
                }
            } else {
                out.push(c);
            }
        }
        for &x in chunk.invalid() {
            out.push(byte_escape(x));
        }
    }
    (out.into(), true)
}

fn byte_escape(b: u8) -> char {
    char::from_u32(BYTE_BASE + u32::from(b)).expect("in private plane")
}

/// Обратное к [`escape_bytes`].
pub fn unescape_bytes(name: &str, escaped: bool) -> Vec<u8> {
    if !escaped {
        return name.as_bytes().to_vec();
    }
    let mut out = Vec::with_capacity(name.len());
    for c in name.chars() {
        let v = u32::from(c);
        if v >= BYTE_BASE + 0x80 {
            out.push(u8::try_from(v - BYTE_BASE).expect("byte range"));
        } else {
            let mut b = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
        }
    }
    out
}

/// Имя ОС → имя записи; `true` — экранировано (`Attrs::ESCAPED_NAME`).
pub fn from_os(name: &OsStr) -> (CompactString, bool) {
    if let Some(s) = name.to_str() {
        return (s.into(), false);
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let units: Vec<u16> = name.encode_wide().collect();
        escape_wide(&units)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        escape_bytes(name.as_bytes())
    }
}

/// Имя ОС из имени записи (обратное к экранированию).
pub fn to_os(name: &str, escaped: bool) -> Cow<'_, OsStr> {
    if !escaped {
        return Cow::Borrowed(OsStr::new(name));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        Cow::Owned(OsString::from_wide(&unescape_wide(name, true)))
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Cow::Owned(OsString::from_vec(unescape_bytes(name, true)))
    }
}

/// Путь записи `e` в каталоге `dir` (с учётом `ESCAPED_NAME`).
pub fn join_os(dir: &VfsPath, e: &Entry) -> PathBuf {
    dir.path
        .join(to_os(&e.name, e.attrs.has(Attrs::ESCAPED_NAME)))
}

/// Расширение: после последней `.`; у dotfile без второй точки — пусто.
pub fn ext(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i + 1..],
        _ => "",
    }
}

/// Имя начинается с `.` (кроме `.` и `..`).
pub fn is_dotfile(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn wide_roundtrip(units in proptest::collection::vec(any::<u16>(), 0..40)) {
            let (name, esc) = escape_wide(&units);
            prop_assert_eq!(unescape_wide(&name, esc), units.clone());
            prop_assert_eq!(esc, String::from_utf16(&units).is_err());
        }

        #[test]
        fn bytes_roundtrip(bytes in proptest::collection::vec(any::<u8>(), 0..40)) {
            let (name, esc) = escape_bytes(&bytes);
            prop_assert_eq!(unescape_bytes(&name, esc), bytes.clone());
            prop_assert_eq!(esc, std::str::from_utf8(&bytes).is_err());
        }

        /// Настоящие символы зоны экранирования рядом с невалидными юнитами.
        #[test]
        fn wide_roundtrip_with_private_plane(
            parts in proptest::collection::vec(
                prop_oneof![
                    (0x10_F700u32..=0x10_FFFF).prop_map(|v| char::from_u32(v).unwrap().to_string()),
                    "[a-zа-яё0-9.]{0,3}",
                ],
                0..8,
            ),
            lone in 0xD800u16..0xDC00,
        ) {
            let mut units: Vec<u16> = parts.concat().encode_utf16().collect();
            units.push(lone);
            let (name, esc) = escape_wide(&units);
            prop_assert!(esc);
            prop_assert_eq!(unescape_wide(&name, esc), units);
        }

        #[test]
        fn bytes_roundtrip_with_private_plane(
            s in "[a-z\u{10F700}-\u{10FFFF}]{0,6}",
            bad in 0x80u8..=0xFF,
        ) {
            let mut bytes = s.into_bytes();
            bytes.push(bad);
            let (name, esc) = escape_bytes(&bytes);
            prop_assert_eq!(unescape_bytes(&name, esc), bytes);
        }
    }

    #[test]
    fn valid_names_unchanged() {
        for n in ["a.txt", "Ёлка", "😀", "\u{10F800}", ""] {
            let w: Vec<u16> = n.encode_utf16().collect();
            assert_eq!(escape_wide(&w), (n.into(), false));
            assert_eq!(escape_bytes(n.as_bytes()), (n.into(), false));
            assert_eq!(to_os(n, false), OsStr::new(n));
        }
    }

    #[test]
    fn lone_surrogate_and_bad_byte() {
        let (n, e) = escape_wide(&[u16::from(b'a'), 0xD800, u16::from(b'b')]);
        assert!(e);
        assert_eq!(n, "a\u{10F700}b");
        let (n, e) = escape_bytes(b"a\xFFb");
        assert!(e);
        assert_eq!(n, "a\u{10FFFF}b");
    }

    #[test]
    fn os_roundtrip_on_this_platform() {
        #[cfg(windows)]
        let raw = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x61, 0xDC00, 0x62])
        };
        #[cfg(unix)]
        let raw = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"a\x80b".to_vec())
        };
        let (name, esc) = from_os(&raw);
        assert!(esc);
        assert_eq!(to_os(&name, esc), raw.as_os_str());
    }

    #[test]
    fn ext_and_dotfile() {
        assert_eq!(ext("a.tar.gz"), "gz");
        assert_eq!(ext("noext"), "");
        assert_eq!(ext(".bashrc"), "");
        assert_eq!(ext(".config.toml"), "toml");
        assert_eq!(ext("a."), "");
        assert!(is_dotfile(".git"));
        assert!(!is_dotfile("a.b") && !is_dotfile(".") && !is_dotfile(".."));
    }
}
