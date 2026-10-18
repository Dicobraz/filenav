//! S4-10: `stat`, `space`, `roots` на настоящей ФС (каталоги — во временной папке).

use std::path::Path;
use std::time::{Duration, Instant};

use urfin_vfs::{
    CancelToken, Entry, EntryKind, ListOpts, LocalFs, RootKind, Vfs, VfsError, VfsPath,
};

fn list_all(dir: &Path) -> Result<Vec<Entry>, VfsError> {
    let fs = LocalFs::new();
    let cancel = CancelToken::new();
    let mut s = fs.list(&VfsPath::local(dir), &ListOpts::default())?;
    let mut out = Vec::new();
    while s.next_batch(&mut out, &cancel)? {}
    Ok(out)
}

#[test]
fn space_of_temp_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let s = LocalFs::new().space(&VfsPath::local(tmp.path())).unwrap();
    assert!(s.total > 0, "{s:?}");
    assert!(s.avail <= s.total && s.free <= s.total, "{s:?}");
}

/// Системный том — с меткой или местом, вызов укладывается в таймаут.
#[test]
fn roots_include_system_volume() {
    let fs = LocalFs::new();
    let mut roots = Vec::new();
    let t = Instant::now();
    fs.roots(&CancelToken::new(), |r| roots.push(r));
    assert!(
        t.elapsed() < urfin_vfs::ROOTS_TIMEOUT + Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );
    let system = if cfg!(windows) {
        std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\"
    } else {
        "/".into()
    };
    let sys = roots
        .iter()
        .find(|r| r.loc.path == Path::new(&system))
        .unwrap_or_else(|| panic!("no {system} in {roots:?}"));
    assert_eq!(sys.kind, RootKind::Fixed);
    assert!(sys.space.is_some_and(|s| s.total > 0), "{sys:?}");
}

/// Отмена — без опоздавших корней.
#[test]
fn roots_cancelled() {
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut n = 0;
    LocalFs::new().roots(&cancel, |_| n += 1);
    assert_eq!(n, 0);
}

/// Junction (Windows, без прав администратора) или symlink (unix): `link_target`;
/// цель удалена — `LinkBroken` только из `stat`, листинг даёт `LinkDir`.
#[test]
fn link_target_and_broken_link() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let link = tmp.path().join("link");
    if !make_dir_link(&target, &link) {
        eprintln!("cannot create a directory link here, skipped");
        return;
    }
    let fs = LocalFs::new();
    let md = fs.stat(&VfsPath::local(&link), false).unwrap();
    assert_eq!(md.kind, EntryKind::LinkDir);
    let to = md.link_target.expect("link target");
    assert!(to.ends_with("target"), "{}", to.display());
    if cfg!(windows) {
        // IO_REPARSE_TAG_MOUNT_POINT.
        assert_eq!(md.reparse_tag, Some(0xA000_0003));
        assert!(md.nlink.is_some() && md.file_id.is_some_and(|(vol, _)| vol != 0));
    }
    std::fs::remove_dir(&target).unwrap();
    let md = fs.stat(&VfsPath::local(&link), false).unwrap();
    assert_eq!(md.kind, EntryKind::LinkBroken);
    let listed = list_all(tmp.path()).unwrap();
    let e = listed.iter().find(|e| e.name == "link").unwrap();
    assert_ne!(
        e.kind,
        EntryKind::LinkBroken,
        "listing does not resolve links"
    );
}

#[cfg(windows)]
fn make_dir_link(target: &Path, link: &Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(not(windows))]
fn make_dir_link(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

/// Метаданные обычного файла: размер на диске и время изменения метаданных.
#[test]
fn stat_file_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("a.txt");
    std::fs::write(&f, b"hello").unwrap();
    let md = LocalFs::new().stat(&VfsPath::local(&f), true).unwrap();
    assert_eq!((md.kind, md.size), (EntryKind::File, 5));
    assert!(md.ctime.is_some());
    assert!(md.nlink.is_some_and(|n| n >= 1));
    if cfg!(windows) {
        assert!(md.alloc_size.is_some());
        assert_eq!(md.reparse_tag, None);
    }
}
