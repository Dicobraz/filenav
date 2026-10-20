//! Платформенная часть ops. `unsafe` — только здесь (CLAUDE.md).

#[cfg(windows)]
mod windows;

pub(crate) mod fs;

/// Как открыть файл.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    /// Ассоциация ОС.
    Default,
    /// От имени администратора.
    Elevated,
    /// Диалог «Открыть с помощью».
    With,
    /// Показать в Проводнике (выделить файл).
    Reveal,
}

/// Открыть файл средствами ОС. Вызывать вне UI-потока: `ShellExecute` может ждать.
pub fn open(path: &std::path::Path, how: Open) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        windows::open(path, how)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, how);
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}
