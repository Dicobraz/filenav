//! `LocalFs` на временном каталоге (S4-01).

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use filenav_core::{Attrs, CancelToken, Entry, EntryKind, Location};
use filenav_vfs::{ListOpts, LocalFs, Vfs, VfsError, VfsRegistry};

fn list_all(dir: &Path, opts: &ListOpts) -> Result<Vec<Entry>, VfsError> {
    let fs = LocalFs::new();
    let mut s = fs.list(&Location::local(dir), opts)?;
    let mut out = Vec::new();
    let cancel = CancelToken::new();
    while s.next_batch(&mut out, &cancel)? {}
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn by_name<'a>(v: &'a [Entry], name: &str) -> &'a Entry {
    v.iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("{name} not listed: {v:?}"))
}

#[test]
fn lists_files_dirs_sizes_and_mtime() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    fs::write(d.join("a.txt"), b"hello").unwrap();
    fs::write(d.join("empty"), b"").unwrap();
    fs::create_dir(d.join("sub")).unwrap();
    fs::write(d.join(".dot"), b"x").unwrap();
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    fs::File::options()
        .write(true)
        .open(d.join("a.txt"))
        .unwrap()
        .set_modified(t)
        .unwrap();

    // Маленький первый батч: стрим отдаёт записи несколькими вызовами.
    let v = list_all(
        d,
        &ListOpts {
            first_batch_hint: 1,
            ..ListOpts::default()
        },
    )
    .unwrap();
    let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, [".dot", "a.txt", "empty", "sub"]);

    let a = by_name(&v, "a.txt");
    assert_eq!((a.kind, a.size), (EntryKind::File, 5));
    assert_eq!(a.mtime, 1_700_000_000_000_000_000);
    assert_eq!(by_name(&v, "empty").size, 0);
    let sub = by_name(&v, "sub");
    assert_eq!((sub.kind, sub.size), (EntryKind::Dir, 0));
    assert!(by_name(&v, ".dot").attrs.has(Attrs::DOTFILE));
    assert!(!a.attrs.has(Attrs::DOTFILE));
    assert!(!a.attrs.has(Attrs::HIDDEN));
}

#[cfg(windows)]
#[test]
fn hidden_attribute_on_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("h.txt");
    fs::write(&f, b"x").unwrap();
    let ok = std::process::Command::new("attrib")
        .arg("+h")
        .arg(&f)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let v = list_all(tmp.path(), &ListOpts::default()).unwrap();
    assert!(by_name(&v, "h.txt").attrs.has(Attrs::HIDDEN));
}

#[test]
fn errors_map_to_vfs_error() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");
    assert!(matches!(
        list_all(&missing, &ListOpts::default()),
        Err(VfsError::NotFound(p)) if p == missing
    ));
    let file = tmp.path().join("f");
    fs::write(&file, b"").unwrap();
    assert!(
        matches!(
            list_all(&file, &ListOpts::default()),
            Err(VfsError::NotDir(_))
        ),
        "{:?}",
        list_all(&file, &ListOpts::default())
    );
    let fs = LocalFs::new();
    assert!(matches!(
        fs.stat(&Location::local(&missing), true),
        Err(VfsError::NotFound(_))
    ));
}

#[cfg(unix)]
#[test]
fn denied_maps_to_denied() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("locked");
    fs::create_dir(&d).unwrap();
    fs::set_permissions(&d, fs::Permissions::from_mode(0o000)).unwrap();
    let r = list_all(&d, &ListOpts::default());
    fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
    // root читает всё — тогда проверять нечего.
    if r.is_ok() {
        return;
    }
    assert!(matches!(r, Err(VfsError::Denied(_))), "{r:?}");
}

/// Ссылки — где ОС даёт их создать (Windows: режим разработчика или админ).
#[test]
fn symlinks_are_typed() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    fs::write(d.join("target.txt"), b"abc").unwrap();
    fs::create_dir(d.join("tdir")).unwrap();
    #[cfg(unix)]
    let made = {
        use std::os::unix::fs::symlink;
        symlink(d.join("target.txt"), d.join("lf")).is_ok()
            && symlink(d.join("tdir"), d.join("ld")).is_ok()
            && symlink(d.join("nowhere"), d.join("lb")).is_ok()
    };
    #[cfg(windows)]
    let made = {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        symlink_file(d.join("target.txt"), d.join("lf")).is_ok()
            && symlink_dir(d.join("tdir"), d.join("ld")).is_ok()
    };
    if !made {
        eprintln!("symlinks are not allowed here, skipped");
        return;
    }
    let v = list_all(d, &ListOpts::default()).unwrap();
    assert_eq!(by_name(&v, "lf").kind, EntryKind::LinkFile);
    assert_eq!(by_name(&v, "ld").kind, EntryKind::LinkDir);
    #[cfg(unix)]
    assert_eq!(by_name(&v, "lb").kind, EntryKind::LinkBroken);

    let fs = LocalFs::new();
    let m = fs.stat(&Location::local(d.join("lf")), false).unwrap();
    assert_eq!(m.kind, EntryKind::LinkFile);
    assert!(m.link_target.is_some());
    let m = fs.stat(&Location::local(d.join("lf")), true).unwrap();
    assert_eq!((m.kind, m.size), (EntryKind::File, 3));
}

/// Junction создаётся без прав администратора — Windows-ветка типа ссылки.
#[cfg(windows)]
#[test]
fn junction_is_link_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    fs::create_dir(d.join("tdir")).unwrap();
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(d.join("j"))
        .arg(d.join("tdir"))
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "mklink /J failed");
    let v = list_all(d, &ListOpts::default()).unwrap();
    let j = by_name(&v, "j");
    assert_eq!(j.kind, EntryKind::LinkDir);
    assert!(j.attrs.has(Attrs::REPARSE));
}

#[test]
fn stat_entry_open_read_and_registry() {
    use std::io::Read;
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("f.bin"), b"0123456789").unwrap();
    let dir = Location::local(tmp.path());
    let reg = VfsRegistry::with_local();
    let fs = reg.get(&dir).unwrap();
    assert_eq!(fs.scheme(), "local");
    let e = fs.stat_entry(&dir, "f.bin", Attrs::default()).unwrap();
    assert_eq!(
        (e.kind, e.size, e.name.as_str()),
        (EntryKind::File, 10, "f.bin")
    );
    let mut r = fs
        .open_read(&Location::local(tmp.path().join("f.bin")))
        .unwrap();
    assert_eq!(r.len(), Some(10));
    let mut s = String::new();
    r.read_to_string(&mut s).unwrap();
    assert_eq!(s, "0123456789");
    assert_eq!(fs.local_path(&dir), Some(tmp.path().to_path_buf()));

    let mut zip = Location::local(tmp.path());
    zip.backend = "zip".into();
    assert!(matches!(reg.get(&zip), Err(VfsError::Unsupported(_))));
}

#[test]
fn cancelled_stream_stops() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..10 {
        fs::write(tmp.path().join(format!("f{i}")), b"").unwrap();
    }
    let fs = LocalFs::new();
    let mut s = fs
        .list(&Location::local(tmp.path()), &ListOpts::default())
        .unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut out = Vec::new();
    assert!(matches!(
        s.next_batch(&mut out, &cancel),
        Err(VfsError::Cancelled)
    ));
    assert_eq!(out.len(), 0);
}

/// Имя не в UTF-8 на настоящей ФС: листинг даёт экранированное имя с флагом, по
/// нему запись снова находится (S4-02).
#[test]
fn non_utf8_name_roundtrips_through_fs() {
    use std::ffi::OsString;
    let tmp = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    let raw = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0x61, 0xD800, 0x62])
    };
    #[cfg(unix)]
    let raw = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b"a\xFFb".to_vec())
    };
    if fs::write(tmp.path().join(&raw), b"12").is_err() {
        eprintln!("this file system rejects non-UTF-8 names, skipped");
        return;
    }
    let v = list_all(tmp.path(), &ListOpts::default()).unwrap();
    assert_eq!(v.len(), 1);
    let e = &v[0];
    assert!(e.attrs.has(Attrs::ESCAPED_NAME), "{e:?}");
    let dir = Location::local(tmp.path());
    assert_eq!(filenav_vfs::name::join_os(&dir, e), tmp.path().join(&raw));
    let again = LocalFs::new().stat_entry(&dir, &e.name, e.attrs).unwrap();
    assert_eq!((again.size, again.name.as_str()), (2, e.name.as_str()));
    assert!(again.attrs.has(Attrs::ESCAPED_NAME));
}
