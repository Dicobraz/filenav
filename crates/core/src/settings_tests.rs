//! Запись настроек через Core (S2-22): debounce, выход, `app.reload_config`, битый файл.

use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::bus::{Boot, Core};
use crate::cmd::Command;
use crate::loc::Location;
use crate::msg::Waker;
use crate::paths::Paths;
use crate::services::Services;
use crate::settings;
use crate::state::{Level, UiEvent};

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

fn core(tmp: &tempfile::TempDir) -> (Core, Paths) {
    let paths = Paths::in_dir(tmp.path().join("cfg"));
    let root = Location::local(tmp.path().join("files"));
    let mut boot = Boot::new(root.clone(), root);
    boot.paths = paths.clone();
    let mut core = Core::new(boot, Services::null(), Arc::new(NoWake));
    core.pump(Duration::from_millis(5));
    (core, paths)
}

/// Как из задачи: `Msg` через sink, применяется в pump.
fn set(core: &mut Core, path: &[&str], v: impl Into<settings::Value>) {
    core.rt().sink().try_send(settings::set(path, v)).unwrap();
    core.pump(Duration::from_millis(5));
}

/// Pump, пока не выполнится условие (запись — на `Lane::Bulk`).
fn pump_until(core: &mut Core, mut f: impl FnMut(&mut Core) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f(core) {
        assert!(Instant::now() < deadline, "condition not reached");
        core.pump(Duration::from_millis(5));
        thread::sleep(Duration::from_millis(5));
    }
}

fn read(paths: &Paths) -> String {
    fs::read_to_string(paths.config_file()).unwrap_or_default()
}

#[test]
fn ten_changes_in_a_second_write_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, paths) = core(&tmp);
    assert_eq!(c.settings_debounce, Duration::from_secs(1));
    let started = Instant::now();
    for i in 0..10i64 {
        let due = started + Duration::from_millis(60) * u32::try_from(i).unwrap();
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        set(&mut c, &["ui", "zoom"], 100 + i * 10);
    }
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "series took {:?}",
        started.elapsed()
    );
    assert_eq!(c.settings_flushes, 0, "no write while changes keep coming");
    assert!(!paths.config_file().exists());
    thread::sleep(Duration::from_millis(1100));
    c.pump(Duration::from_millis(5));
    assert_eq!(c.settings_flushes, 1);
    pump_until(&mut c, |_| read(&paths).contains("zoom = 190"));
    c.pump(Duration::from_millis(5));
    assert_eq!(c.settings_flushes, 1, "nothing changed since");
    let _ = c.shutdown(Duration::from_millis(500));
    assert_eq!(read(&paths).matches("zoom =").count(), 1);
}

#[test]
fn exit_writes_pending_now() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, paths) = core(&tmp);
    set(&mut c, &["ui", "theme"], "dark");
    assert_eq!(c.settings_flushes, 0);
    let _ = c.shutdown(Duration::from_millis(500));
    assert!(
        read(&paths).ends_with("[ui]\ntheme = \"dark\"\n"),
        "{}",
        read(&paths)
    );
}

#[test]
fn reload_config_sees_pending_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, _paths) = core(&tmp);
    set(&mut c, &["general", "language"], "ru");
    set(&mut c, &["keymap", "preset"], "dn");
    let before = Arc::clone(&c.state().config);
    c.dispatch(Command::new("app.reload_config"));
    pump_until(&mut c, |c| !Arc::ptr_eq(&before, &c.state().config));
    let cfg = &c.state().config;
    assert_eq!(cfg.general.language, "ru");
    assert_eq!(cfg.ext["keymap"]["preset"].as_str(), Some("dn"));
    assert_eq!(
        c.settings_flushes, 0,
        "written by the reload, not the timer"
    );
    assert!(c.state().banner.is_none());
}

#[test]
fn broken_config_is_kept_and_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, paths) = core(&tmp);
    fs::create_dir_all(&paths.config_dir).unwrap();
    let bad = "[ui\ntheme = \"light\"\n";
    fs::write(paths.config_file(), bad).unwrap();
    c.settings_debounce = Duration::ZERO;
    set(&mut c, &["ui", "theme"], "dark");
    let mut toast = None;
    pump_until(&mut c, |c| {
        for e in c.take_ui_events() {
            if let UiEvent::Toast {
                level: Level::Warn,
                text,
            } = e
            {
                toast = Some(text);
            }
        }
        toast.is_some()
    });
    let toast = toast.unwrap();
    assert!(toast.contains("config.toml"), "{toast}");
    assert_eq!(read(&paths), bad);
    let _ = c.shutdown(Duration::from_millis(500));
    assert_eq!(read(&paths), bad, "exit does not retry dropped changes");
}
