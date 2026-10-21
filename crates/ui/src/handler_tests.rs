//! Настройки `ui.*` в `config.toml` и обратно в [`UiState`] (S2-22).

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use urfin_core::{Boot, Command, Config, Core, Location, Paths, Services, Waker};

use crate::state::UiState;

struct NoWake;
impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

/// Core с handler `ui`; конфиг — из `paths`, как при старте приложения.
fn core(paths: &Paths) -> Core {
    let (config, warnings) = Config::load(paths);
    assert_eq!(warnings, vec![]);
    let root = Location::local(paths.config_dir.join("files"));
    let mut boot = Boot::new(root.clone(), root);
    boot.paths = paths.clone();
    boot.config = config;
    let mut core = Core::new(boot, Services::null(), Arc::new(NoWake));
    crate::register(&mut core);
    core.pump(Duration::from_millis(5));
    core
}

fn ui(core: &Core) -> &UiState {
    core.state()
        .ext
        .get::<UiState>()
        .expect("UiState after first pump")
}

const USER: &str = "# мой конфиг\n[ui]\ntheme = \"dark\"   # тёмная\nzoom = 120\n";

#[test]
fn config_seeds_state_and_commands_write_back() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(tmp.path());
    fs::write(paths.config_file(), USER).unwrap();
    let mut c = core(&paths);
    let st = ui(&c);
    assert_eq!(
        (st.theme.as_str(), st.zoom, st.split),
        ("system-dark", 120, 0.5)
    );

    for cmd in [
        "ui.theme.set id=system-light",
        "ui.zoom delta=1",
        "ui.split delta=1",
        "ui.columns set=dev",
        "ui.lang lang=ru",
        "ui.lang lang=xx-unknown",
    ] {
        c.dispatch(Command::parse(cmd).unwrap());
        c.pump(Duration::from_millis(5));
    }
    urfin_i18n::set_language("en");
    let st = ui(&c);
    assert_eq!((st.theme.as_str(), st.zoom), ("system-light", 130));
    // Выход пишет отложенное сразу, не дожидаясь debounce.
    let _ = c.shutdown(Duration::from_millis(500));
    let text = fs::read_to_string(paths.config_file()).unwrap();
    assert!(
        text.starts_with("# мой конфиг\n[ui]\ntheme = \"system-light\"   # тёмная\nzoom = 130\n"),
        "{text}"
    );
    assert!(text.contains("split = 0.55\n"), "{text}");
    assert!(text.contains("columns = \"dev\"\n"), "{text}");
    assert!(
        text.contains("[general]\nlanguage = \"ru\"\n"),
        "unknown language is not saved: {text}"
    );

    // Следующий запуск — с сохранёнными значениями.
    let c = core(&paths);
    let st = ui(&c);
    assert_eq!(st.theme, "system-light");
    assert_eq!(st.zoom, 130);
    assert!((st.split - 0.55).abs() < 1e-6);
    assert_eq!(st.columns.as_deref(), Some("dev"));
    assert_eq!(c.state().config.general.language, "ru");
}

#[test]
fn reload_config_applies_edited_ui_section() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(tmp.path());
    let mut c = core(&paths);
    assert_eq!(ui(&c).theme, "auto");
    fs::write(
        paths.config_file(),
        "[ui]\ntheme = \"dark\"\nzoom = 999\nsplit = 0.05\n",
    )
    .unwrap();
    c.dispatch(Command::new("app.reload_config"));
    for _ in 0..400 {
        c.pump(Duration::from_millis(5));
        if ui(&c).theme == "system-dark" {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let st = ui(&c);
    assert_eq!(st.theme, "system-dark");
    assert_eq!(st.zoom, 300, "clamped to the zoom range");
    assert!((st.split - 0.1).abs() < 1e-6, "clamped to the split range");
}
