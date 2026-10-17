//! Локальная ФС. Листинг: Windows — `NtQueryDirectoryFile` (S4-07), запасные —
//! `FindFirstFileExW` и `std::fs`; Linux/macOS — пока `std::fs` (S4-08/09). Место
//! и корни — S4-10, наблюдение — S4-12/13.

pub(crate) mod std_fs;

use std::path::PathBuf;

use crate::api::{
    Attrs, Caps, Entry, ListOpts, ListStream, Metadata, Space, Vfs, VfsError, VfsPath, VfsRead,
    WatchGuard, WatchTx,
};

/// Способ листинга; `Auto` — самый быстрый из доступных с откатом на запасные.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ListMode {
    #[default]
    Auto,
    /// Windows: только `NtQueryDirectoryFile`.
    Nt,
    /// Windows: только `FindFirstFileExW`.
    Find,
    /// `std::fs::read_dir` — эталон.
    Std,
}

impl ListMode {
    /// `FILENAV_VFS_LIST=nt|find|std` — принудительно (диагностика, тесты).
    pub fn from_env() -> ListMode {
        match std::env::var("FILENAV_VFS_LIST").as_deref() {
            Ok("nt") => ListMode::Nt,
            Ok("find") => ListMode::Find,
            Ok("std") => ListMode::Std,
            _ => ListMode::Auto,
        }
    }
}

#[derive(Debug, Default)]
pub struct LocalFs {
    mode: ListMode,
}

impl LocalFs {
    /// Способ листинга — из `FILENAV_VFS_LIST`, иначе `Auto`.
    pub fn new() -> Self {
        LocalFs {
            mode: ListMode::from_env(),
        }
    }

    pub fn with_mode(mode: ListMode) -> Self {
        LocalFs { mode }
    }

    /// Способ листинга (вне Windows — всегда std).
    pub fn mode(&self) -> ListMode {
        self.mode
    }
}

impl Vfs for LocalFs {
    fn scheme(&self) -> &'static str {
        "local"
    }

    fn caps(&self, _at: &VfsPath) -> Caps {
        let case = if cfg!(target_os = "linux") {
            Caps::CASE_SENSITIVE
        } else {
            0
        };
        Caps(Caps::SEEK | Caps::LOCAL_PATH | case)
    }

    fn list(&self, dir: &VfsPath, opts: &ListOpts) -> Result<Box<dyn ListStream>, VfsError> {
        #[cfg(windows)]
        {
            use crate::sys::windows as w;
            match self.mode {
                ListMode::Auto | ListMode::Nt => {
                    if let Some(l) = w::nt_list(&dir.path)? {
                        return Ok(Box::new(l));
                    }
                    if self.mode == ListMode::Nt {
                        return Err(VfsError::Unsupported("NtQueryDirectoryFile"));
                    }
                    tracing::debug!(dir = %dir.path.display(), "NtQueryDirectoryFile unsupported, FindFirstFileExW");
                    return Ok(Box::new(w::find_list(&dir.path)?));
                }
                ListMode::Find => return Ok(Box::new(w::find_list(&dir.path)?)),
                ListMode::Std => {}
            }
        }
        Ok(Box::new(std_fs::list(&dir.path, opts)?))
    }

    fn stat(&self, p: &VfsPath, follow: bool) -> Result<Metadata, VfsError> {
        std_fs::stat(&p.path, follow)
    }

    fn stat_entry(&self, dir: &VfsPath, name: &str, attrs_hint: Attrs) -> Result<Entry, VfsError> {
        std_fs::stat_entry(&dir.path, name, attrs_hint.has(Attrs::ESCAPED_NAME))
    }

    fn open_read(&self, p: &VfsPath) -> Result<Box<dyn VfsRead>, VfsError> {
        Ok(Box::new(std_fs::open_read(&p.path)?))
    }

    fn space(&self, _at: &VfsPath) -> Result<Space, VfsError> {
        Err(VfsError::Unsupported("space: S4-10"))
    }

    fn local_path(&self, p: &VfsPath) -> Option<PathBuf> {
        Some(p.path.clone())
    }

    fn watch(&self, _dir: &VfsPath, _tx: WatchTx) -> Result<WatchGuard, VfsError> {
        Err(VfsError::Unsupported("watch: S4-12"))
    }
}
