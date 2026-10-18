//! Локальная ФС. Листинг: Windows — `NtQueryDirectoryFile` (S4-07), запасные —
//! `FindFirstFileExW` и `std::fs`; Linux/macOS — пока `std::fs` (S4-08/09). Место
//! и корни — S4-10, наблюдение — S4-12/13.

pub(crate) mod std_fs;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use compact_str::CompactString;

use crate::api::{
    Attrs, CancelToken, Caps, Entry, ListOpts, ListStream, Metadata, RootInfo, RootKind, Space,
    Vfs, VfsError, VfsPath, VfsRead, WatchGuard, WatchTx,
};

/// Сколько ждать меток и места томов: медленные (отключённый сетевой диск) выдаются
/// без них (S4 §3.1).
pub const ROOTS_TIMEOUT: Duration = Duration::from_millis(1500);

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
    /// `URFIN_VFS_LIST=nt|find|std` — принудительно (диагностика, тесты).
    pub fn from_env() -> ListMode {
        match std::env::var("URFIN_VFS_LIST").as_deref() {
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
    /// Способ листинга — из `URFIN_VFS_LIST`, иначе `Auto`.
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

    /// Корни (диски, тома). Список — сразу; метки и место — по потоку на корень, не
    /// дольше [`ROOTS_TIMEOUT`]: опоздавшие выдаются без них. Порядок — по готовности.
    pub fn roots(&self, cancel: &CancelToken, mut out: impl FnMut(RootInfo)) {
        let list = platform_roots();
        let (tx, rx) = mpsc::channel();
        for (i, (path, _)) in list.iter().enumerate() {
            let (tx, path) = (tx.clone(), path.clone());
            let spawned = std::thread::Builder::new()
                .name("volume-info".into())
                .spawn(move || {
                    let label = root_label(&path);
                    let space = space_of(&path).ok();
                    let _ = tx.send((i, label, space));
                });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "volume-info thread");
            }
        }
        drop(tx);
        let deadline = Instant::now() + ROOTS_TIMEOUT;
        let mut done = vec![false; list.len()];
        let info = |i: usize, label: Option<String>, space: Option<Space>| RootInfo {
            loc: VfsPath::local(&list[i].0),
            label: label.map(CompactString::from).unwrap_or_default(),
            kind: list[i].1,
            space,
        };
        while done.iter().any(|d| !d) && !cancel.is_cancelled() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match rx.recv_timeout(left.min(Duration::from_millis(50))) {
                Ok((i, label, space)) => {
                    done[i] = true;
                    out(info(i, label, space));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        if cancel.is_cancelled() {
            return;
        }
        // Опоздавшие — без метки и места; их потоки дорабатывают сами.
        for i in (0..list.len()).filter(|i| !done[*i]) {
            tracing::debug!(root = %list[i].0.display(), "volume info timed out");
            out(info(i, None, None));
        }
    }
}

/// Корни без обращения к томам: путь и тип.
fn platform_roots() -> Vec<(PathBuf, RootKind)> {
    #[cfg(windows)]
    {
        crate::sys::win_meta::drives()
            .into_iter()
            .map(|(p, k)| (PathBuf::from(p), k))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut v = vec![(PathBuf::from("/"), RootKind::Fixed)];
        v.extend(unix_mounts());
        v
    }
}

/// Linux: съёмные и сетевые точки монтирования (`/media`, `/mnt`, `/run/media`);
/// macOS: `/Volumes/*`.
#[cfg(not(windows))]
fn unix_mounts() -> Vec<(PathBuf, RootKind)> {
    let mut v = Vec::new();
    if cfg!(target_os = "macos") {
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            v.extend(rd.flatten().map(|e| (e.path(), RootKind::Other)));
        }
        return v;
    }
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return v;
    };
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let (Some(_dev), Some(at), Some(fs)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        // В /proc/mounts пробел в пути — `\040`.
        let at = at.replace("\\040", " ");
        if !["/media/", "/mnt/", "/run/media/"]
            .iter()
            .any(|p| at.starts_with(p))
        {
            continue;
        }
        let kind = if matches!(fs, "nfs" | "nfs4" | "cifs" | "smb3" | "fuse.sshfs") {
            RootKind::Network
        } else {
            RootKind::Removable
        };
        v.push((PathBuf::from(at), kind));
    }
    v
}

fn root_label(p: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        crate::sys::win_meta::label(p)
    }
    #[cfg(not(windows))]
    {
        p.file_name().map(|n| n.to_string_lossy().into_owned())
    }
}

/// Место на томе с путём `p`.
pub(crate) fn space_of(p: &Path) -> Result<Space, VfsError> {
    #[cfg(windows)]
    {
        crate::sys::win_meta::space(p)
    }
    #[cfg(not(windows))]
    {
        let s = rustix::fs::statvfs(p).map_err(|e| VfsError::from_io(p, e.into()))?;
        let unit = s.f_frsize;
        Ok(Space {
            total: s.f_blocks.saturating_mul(unit),
            free: s.f_bfree.saturating_mul(unit),
            avail: s.f_bavail.saturating_mul(unit),
        })
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

    fn space(&self, at: &VfsPath) -> Result<Space, VfsError> {
        space_of(&at.path)
    }

    fn local_path(&self, p: &VfsPath) -> Option<PathBuf> {
        Some(p.path.clone())
    }

    fn watch(&self, _dir: &VfsPath, _tx: WatchTx) -> Result<WatchGuard, VfsError> {
        Err(VfsError::Unsupported("watch: S4-12"))
    }
}
