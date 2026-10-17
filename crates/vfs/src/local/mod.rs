//! Локальная ФС. Пока всё — через `std::fs` (`std_fs`); fast path ОС — S4-07…09,
//! место и корни — S4-10, наблюдение — S4-12/13.

pub(crate) mod std_fs;

use std::path::PathBuf;

use crate::api::{
    Attrs, Caps, Entry, ListOpts, ListStream, Metadata, Space, Vfs, VfsError, VfsPath, VfsRead,
    WatchGuard, WatchTx,
};

#[derive(Debug, Default)]
pub struct LocalFs {}

impl LocalFs {
    pub fn new() -> Self {
        LocalFs {}
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
