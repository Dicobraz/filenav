//! Демо-дерево для `--screens`: одно и то же на любой машине (имена, размеры, атрибуты),
//! чтобы снимки сравнивались между прогонами. Каталог — `%TEMP%\urfin-screens\tree`,
//! пересоздаётся при каждом запуске.

use std::io;
use std::path::PathBuf;

const DIRS: [&str; 6] = [
    "Документы",
    "Фото",
    "Projects",
    "Projects/urfin",
    "Projects/postman",
    ".git",
];

/// (имя, размер в байтах).
const FILES: [(&str, usize); 14] = [
    ("Отчёт за квартал.pdf", 284_311),
    ("photo-2026-10-21.jpg", 3_145_728),
    ("archive.zip", 1_048_576),
    ("build.ps1", 2_048),
    ("README.md", 6_311),
    ("notes.txt", 812),
    (
        "Очень длинное имя файла, которое не помещается в колонку и обрезается многоточием.docx",
        41_200,
    ),
    ("setup.exe", 12_582_912),
    ("data.json", 19_500),
    ("Cargo.toml", 1_311),
    ("main.rs", 9_800),
    ("Projects/urfin/Cargo.lock", 120_000),
    ("Projects/urfin/CLAUDE.md", 4_100),
    ("Projects/postman/todo.txt", 300),
];

/// Скрытый (атрибут Hidden) и только для чтения.
const HIDDEN: &str = "desktop.ini";
const READ_ONLY: &str = "license.txt";

pub fn create() -> io::Result<PathBuf> {
    // `%LOCALAPPDATA%\Temp`, а не `temp_dir()`: тот бывает в 8.3-форме (`A81F7~1.ANT`).
    let base = std::env::var_os("LOCALAPPDATA")
        .map_or_else(std::env::temp_dir, |d| PathBuf::from(d).join("Temp"));
    let root = base.join(r"urfin-screens\tree");
    if root.exists() {
        // Read-only мешает удалению: снять и удалить заново.
        let ro = root.join(READ_ONLY);
        if let Ok(m) = std::fs::metadata(&ro) {
            let mut p = m.permissions();
            #[allow(
                clippy::permissions_set_readonly_false,
                reason = "пересоздание демо-дерева"
            )]
            p.set_readonly(false);
            let _ = std::fs::set_permissions(&ro, p);
        }
        std::fs::remove_dir_all(&root)?;
    }
    for d in DIRS {
        std::fs::create_dir_all(root.join(d))?;
    }
    for (name, size) in FILES {
        std::fs::write(root.join(name), vec![b'u'; size])?;
    }
    hidden(&root.join(HIDDEN))?;
    let ro = root.join(READ_ONLY);
    std::fs::write(&ro, b"MIT")?;
    let mut p = std::fs::metadata(&ro)?.permissions();
    p.set_readonly(true);
    std::fs::set_permissions(&ro, p)?;
    Ok(root)
}

#[cfg(windows)]
fn hidden(path: &std::path::Path) -> io::Result<()> {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_ATTRIBUTE_HIDDEN`.
    const HIDDEN_ATTR: u32 = 0x2;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .attributes(HIDDEN_ATTR)
        .open(path)?
        .write_all(b"[.ShellClassInfo]")
}

#[cfg(not(windows))]
fn hidden(path: &std::path::Path) -> io::Result<()> {
    std::fs::write(path, b"[.ShellClassInfo]")
}
