use filenav_core::{Attrs, Entry, EntryKind};
use jiff::tz::{Offset, TimeZone};
use proptest::prelude::*;

use super::*;

fn cfg(tag: &str) -> FmtCfg {
    let i18n = filenav_i18n::Catalog::embedded()
        .build(&tag.parse().unwrap())
        .0;
    FmtCfg::new(&i18n, false, TimeZone::fixed(Offset::UTC))
}

fn entry(name: &str, size: u64, attrs: u32, kind: EntryKind) -> Entry {
    Entry {
        name: name.into(),
        size,
        // 2026-10-14 09:12 UTC.
        mtime: 1_791_969_120 * 1_000_000_000,
        attrs: Attrs(attrs),
        kind,
    }
}

fn file(name: &str, size: u64) -> Entry {
    entry(name, size, Attrs::ARCHIVE, EntryKind::File)
}

fn text(col: ColKind, e: &Entry, c: &FmtCfg) -> String {
    let mut out = Cell::new();
    cell_text(col, e, c, &mut out);
    out.as_str().to_owned()
}

/// Строки мокапов S2 §2.1 и §2.3 (ru). Размеры меньше 10 в мокапе даны с одной цифрой
/// после запятой («1,2 КБ»), а текст §2.1 и приёмка S2-01 — 3 значащие цифры («1,00 КБ»);
/// сверяются значения, где мокап и правило сходятся.
#[test]
fn matches_mockups() {
    let c = cfg("ru");
    let rows = [
        (
            entry(
                ".gitignore",
                118,
                Attrs::HIDDEN | Attrs::ARCHIVE,
                EntryKind::File,
            ),
            ["118 Б", "Файл GITIGNORE", "HA"],
        ),
        (file("Cargo.lock", 48_213), ["47,1 КБ", "Файл LOCK", "A"]),
        (file("LICENSE-APACHE", 10_854), ["10,6 КБ", "Файл", "A"]),
        (file("LICENSE-MIT", 1023), ["1023 Б", "Файл", "A"]),
        (
            entry(
                "setup.exe",
                14_260_000,
                Attrs::READONLY | Attrs::ARCHIVE,
                EntryKind::File,
            ),
            ["13,6 МБ", "Приложение", "RA"],
        ),
        (
            file("backup.7z", 1_024_000_000),
            ["977 МБ", "Архив 7z", "A"],
        ),
        (entry("crates", 0, 0, EntryKind::Dir), ["", "Папка", "D"]),
    ];
    for (e, [size, ty, attrs]) in rows {
        assert_eq!(text(ColKind::Size, &e, &c), size, "{}", e.name);
        assert_eq!(text(ColKind::Type, &e, &c), ty, "{}", e.name);
        assert_eq!(text(ColKind::Attrs, &e, &c), attrs, "{}", e.name);
        assert_eq!(text(ColKind::Mtime, &e, &c), "14.10.26 09:12");
        assert_eq!(text(ColKind::Name, &e, &c), e.name.as_str());
    }
    assert_eq!(text(ColKind::Ext, &file("a.tar.gz", 1), &c), "gz");
    assert_eq!(
        text(ColKind::Ext, &entry("x.d", 0, 0, EntryKind::Dir), &c),
        ""
    );
}

#[test]
fn english_and_iso() {
    let c = cfg("en");
    let e = file("Cargo.lock", 48_213);
    assert_eq!(text(ColKind::Size, &e, &c), "47.1 KB");
    assert_eq!(text(ColKind::Type, &e, &c), "LOCK File");
    assert_eq!(text(ColKind::Mtime, &e, &c), "10/14/26 9:12 AM");
    assert_eq!(text(ColKind::Type, &file("a.zip", 1), &c), "zip Archive");
    let i18n = filenav_i18n::Catalog::embedded()
        .build(&"en".parse().unwrap())
        .0;
    let iso = FmtCfg::new(&i18n, true, TimeZone::fixed(Offset::UTC));
    assert_eq!(text(ColKind::Mtime, &e, &iso), "2026-10-14 09:12");
}

#[test]
fn unknown_time_and_dir_sizes() {
    let c = cfg("ru");
    let mut e = file("a", 5);
    e.mtime = i64::MIN;
    assert_eq!(text(ColKind::Mtime, &e, &c), "");
    assert_eq!(text(ColKind::Ctime, &e, &c), "");
    let mut d = entry("d", 3 * 1024 * 1024, 0, EntryKind::Dir);
    assert_eq!(text(ColKind::Size, &d, &c), "", "не посчитан — пусто");
    d.attrs = Attrs(Attrs::SIZE_KNOWN);
    assert_eq!(text(ColKind::Size, &d, &c), "3,00 МБ");
    assert_eq!(
        text(ColKind::Type, &entry("l", 0, 0, EntryKind::LinkBroken), &c),
        "Ярлык"
    );
}

#[test]
fn cell_text_truncates_on_char_boundary() {
    let mut t = CellText::<8>::new();
    let _ = t.write_str("абвгд");
    assert!(t.is_truncated());
    assert_eq!(t.as_str(), "аб…");
    t.clear();
    let _ = t.write_str("abc");
    assert_eq!(t.as_str(), "abc");
    assert!(!t.is_truncated());
}

#[test]
fn size_matches_i18n_bytes() {
    let i18n = filenav_i18n::Catalog::embedded()
        .build(&"ru".parse().unwrap())
        .0;
    let c = FmtCfg::new(&i18n, false, TimeZone::UTC);
    for n in [0, 1, 1023, 1024, 48_213, 1 << 30, u64::MAX] {
        assert_eq!(
            text(ColKind::Size, &file("f", n), &c),
            filenav_i18n::fmt::bytes_with(&i18n, n)
        );
    }
}

proptest! {
    #[test]
    fn never_panics_and_fits(
        name in "\\PC{0,200}",
        size: u64,
        mtime: i64,
        attrs: u32,
        kind in 0usize..6,
    ) {
        let kinds = [
            EntryKind::File,
            EntryKind::Dir,
            EntryKind::LinkFile,
            EntryKind::LinkDir,
            EntryKind::LinkBroken,
            EntryKind::Other,
        ];
        let e = Entry { name: name.as_str().into(), size, mtime, attrs: Attrs(attrs), kind: kinds[kind] };
        let c = cfg("ru");
        let mut out = Cell::new();
        for col in [
            ColKind::Name, ColKind::Ext, ColKind::Type, ColKind::Size,
            ColKind::Mtime, ColKind::Ctime, ColKind::Atime, ColKind::Attrs,
        ] {
            cell_text(col, &e, &c, &mut out);
            prop_assert!(out.len() <= 128);
            prop_assert!(std::str::from_utf8(out.as_str().as_bytes()).is_ok());
        }
    }

    #[test]
    fn size_cell_equals_bytes(n: u64) {
        let i18n = filenav_i18n::Catalog::embedded().build(&"en".parse().unwrap()).0;
        let c = FmtCfg::new(&i18n, false, TimeZone::UTC);
        prop_assert_eq!(text(ColKind::Size, &file("f", n), &c), filenav_i18n::fmt::bytes_with(&i18n, n));
    }
}
