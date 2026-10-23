//! Менеджер сеансов: выбор shell по умолчанию, cwd, клавиши; пул на настоящих
//! cmd и Windows PowerShell (приёмка S6-10).

use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use urfin_core::{Boot, Command, Core, Services, Waker};

use super::*;
use crate::shells::{PathStyle, ShellKind};

fn spec(id: &str) -> ShellSpec {
    ShellSpec {
        id: id.into(),
        title: id.into(),
        kind: ShellKind::Other,
        program: PathBuf::from(format!("{id}.exe")),
        args: Vec::new(),
        env: Vec::new(),
        integration: false,
        path_style: PathStyle::Windows,
    }
}

fn with_shells(ids: &[&str]) -> Manager {
    let mut m = Manager::new(None);
    m.shells = Some(ids.iter().map(|i| spec(i)).collect());
    m
}

/// №52: настройка → статистика → pwsh → cmd → первый.
#[test]
fn default_shell_order() {
    let cfg = TermConfig::default();
    let id = |m: &Manager, c: &TermConfig| m.default_shell(c).map(|s| s.id.to_string());
    let mut m = with_shells(&["cmd", "powershell", "pwsh", "wsl:Ubuntu"]);
    assert_eq!(id(&m, &cfg).as_deref(), Some("pwsh"), "first run: pwsh 7");
    m.usage.insert("wsl:Ubuntu".into(), 2);
    m.usage.insert("cmd".into(), 2);
    assert_eq!(
        id(&m, &cfg).as_deref(),
        Some("cmd"),
        "most used, tie → list order"
    );
    m.usage.insert("wsl:Ubuntu".into(), 3);
    assert_eq!(id(&m, &cfg).as_deref(), Some("wsl:Ubuntu"));
    let set = TermConfig {
        default_shell: "powershell".into(),
        ..TermConfig::default()
    };
    assert_eq!(id(&m, &set).as_deref(), Some("powershell"), "setting wins");
    let gone = TermConfig {
        default_shell: "fish".into(),
        ..TermConfig::default()
    };
    assert_eq!(
        id(&m, &gone).as_deref(),
        Some("wsl:Ubuntu"),
        "missing → stats"
    );
    let m = with_shells(&["powershell", "cmd"]);
    assert_eq!(id(&m, &cfg).as_deref(), Some("cmd"), "no pwsh → cmd");
    let m = with_shells(&["gitbash"]);
    assert_eq!(id(&m, &cfg).as_deref(), Some("gitbash"));
}

#[test]
fn cwd_is_nearest_local() {
    let local = Location::local(r"C:\work\src");
    assert_eq!(session_cwd(&local), Some(PathBuf::from(r"C:\work\src")));
    let zip = Location {
        backend: "zip".into(),
        path: PathBuf::from("inner/dir"),
        outer: Some(Arc::new(Location::local(r"C:\work\a.zip"))),
    };
    assert_eq!(session_cwd(&zip), Some(PathBuf::from(r"C:\work")));
    let sftp = Location {
        backend: "sftp".into(),
        path: PathBuf::from("/home"),
        outer: None,
    };
    assert_eq!(session_cwd(&sftp), None);
}

#[test]
fn key_names_and_mods() {
    let m = input::Modes::default();
    assert_eq!(key_bytes("Enter", None, m), Some(b"\r".to_vec()));
    assert_eq!(key_bytes("c", Some("ctrl"), m), Some(vec![3]));
    assert_eq!(
        key_bytes("up", Some("Ctrl+Shift"), m),
        Some(b"\x1b[1;6A".to_vec())
    );
    assert_eq!(key_bytes("up", Some("hyper"), m), None);
}

#[test]
fn usage_roundtrip() {
    let d = tempfile::Builder::new().prefix("urfin-").tempdir().unwrap();
    let p = d.path().join("term").join(USAGE_FILE);
    let mut u = BTreeMap::new();
    u.insert(CompactString::from("wsl:Ubuntu"), 3);
    u.insert(CompactString::from("cmd"), 1);
    save_usage(&p, &u).unwrap();
    assert_eq!(load_usage(&p), u);
    assert!(load_usage(&d.path().join("none.toml")).is_empty());
}

#[test]
fn nonce_is_128_bit_hex_and_differs() {
    let (a, b) = (nonce(), nonce());
    assert_eq!(a.len(), 32);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _: Duration) {}
}

struct H {
    core: Core,
    _dir: tempfile::TempDir,
}

impl H {
    fn new() -> H {
        let dir = tempfile::Builder::new()
            .prefix("urfin-term-")
            .tempdir()
            .unwrap();
        let root = Location::local(dir.path());
        let mut core = Core::new(
            Boot::new(root.clone(), root),
            Services::null(),
            Arc::new(NoWake),
        );
        crate::register_with(
            &mut core,
            TermConfig::default(),
            Some(dir.path().join("term")),
        );
        H { core, _dir: dir }
    }

    fn mgr(&self) -> &Manager {
        &self.core.state().ext.get::<TermState>().unwrap().sessions
    }

    fn until(&mut self, what: &str, done: impl Fn(&Manager) -> bool) {
        let t0 = Instant::now();
        loop {
            self.core.pump(Duration::from_millis(1));
            if self.core.state().ext.get::<TermState>().is_some() && done(self.mgr()) {
                return;
            }
            assert!(t0.elapsed() < Duration::from_secs(30), "timeout: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn run(&mut self, cmd: Command) {
        self.core.dispatch(cmd);
        self.core.pump(Duration::from_millis(1));
    }

    fn input(&mut self, text: &str) {
        self.run(Command::new("term.input").arg("text", text));
    }
}

fn screen(slot: &Slot) -> String {
    let Some(sh) = slot.shared() else {
        return String::new();
    };
    let vt = sh.vt.lock();
    let g = vt.term.grid();
    let top = -i32::try_from(g.history_size()).unwrap();
    let rows = i32::try_from(g.screen_lines()).unwrap();
    (top..rows)
        .map(|l| {
            let row = &g[Line(l)];
            (0..g.columns())
                .map(|c| row[Column(c)].c)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn active_has(m: &Manager, s: &str) -> bool {
    m.active().is_some_and(|a| screen(a).contains(s))
}

fn running(m: &Manager) -> bool {
    m.active().is_some_and(|a| a.state == SlotState::Running)
}

/// Приёмка S6-10 (без WSL: старт ВМ в тесте — десятки секунд): прогрев, переключение
/// cmd ↔ powershell с сохранением вывода, `exit` → строка и перезапуск по Enter,
/// выход при `ping -t` → вопрос.
#[test]
fn pool_switch_exit_restart_and_quit_veto() {
    let mut h = H::new();
    // Прогрев: shell по умолчанию стартует сам; без pwsh 7 на стенде — cmd.
    h.until("warm-up session", running);
    let warm = h.mgr().active().unwrap().shell.clone();
    let has_pwsh = h.mgr().shells().iter().any(|s| s.id == "pwsh");
    assert_eq!(warm, if has_pwsh { "pwsh" } else { "cmd" });
    let first = h.mgr().active().unwrap().id;
    h.run(Command::new("term.shell").arg("id", "cmd"));
    h.until("cmd active", running);
    let cmd_id = h.mgr().active().unwrap().id;
    if warm == "cmd" {
        assert_eq!(cmd_id, first, "reuse the warm cmd session");
    }
    h.input("echo URFIN_ONE\r");
    h.until("cmd output", |m| active_has(m, "URFIN_ONE"));

    h.run(Command::new("term.shell").arg("id", "powershell"));
    h.until("powershell running", |m| {
        running(m) && m.active().is_some_and(|a| a.shell == "powershell")
    });
    h.input("echo URFIN_TWO\r");
    h.until("powershell output", |m| active_has(m, "URFIN_TWO"));

    h.run(Command::new("term.shell").arg("id", "cmd"));
    assert_eq!(h.mgr().active().unwrap().id, cmd_id);
    assert!(active_has(h.mgr(), "URFIN_ONE"), "cmd output kept");
    let ps = h
        .mgr()
        .slots()
        .iter()
        .find(|s| s.shell == "powershell")
        .unwrap();
    assert!(screen(ps).contains("URFIN_TWO"), "background output kept");
    let ps_id = ps.id;
    h.run(Command::new("term.next"));
    assert_ne!(h.mgr().active().unwrap().id, cmd_id);

    // exit → строка «процесс завершён», Enter — новый сеанс того же shell.
    h.run(Command::new("term.shell").arg("id", "cmd"));
    h.input("exit 3\r");
    h.until("cmd exited", |m| {
        m.active()
            .is_some_and(|a| a.state == SlotState::Exited(Some(3)))
    });
    let line = tr("term-exited", &[("code", "3")]);
    assert!(
        active_has(h.mgr(), &line),
        "{}",
        screen(h.mgr().active().unwrap())
    );
    h.run(Command::new("term.key").arg("key", "enter"));
    h.until("cmd restarted", running);
    let restarted = h.mgr().active().unwrap();
    assert_eq!(restarted.shell, "cmd");
    assert_ne!(restarted.id, cmd_id);

    // ping -t → выход спрашивает; kill — вопроса нет.
    h.until("prompt after restart", |m| active_has(m, ">"));
    h.input("ping -t 127.0.0.1\r");
    h.until("busy", |m| m.active().is_some_and(Slot::busy));
    assert!(matches!(h.core.quit_decision(), QuitDecision::Veto(_)));
    h.run(Command::new("term.kill"));
    h.until("killed", |m| {
        m.active()
            .is_some_and(|a| matches!(a.state, SlotState::Exited(_)))
    });
    assert!(h.mgr().slots().iter().any(|s| s.id == ps_id));
    assert!(matches!(h.core.quit_decision(), QuitDecision::Allow));
    assert!(h.mgr().slots().is_empty(), "allowed quit closes the pool");
}

fn panels_events(core: &mut Core) -> Vec<Option<bool>> {
    core.take_ui_events()
        .into_iter()
        .filter_map(|e| match e {
            urfin_core::UiEvent::Ext { id, args } if id.as_str() == "term.panels" => {
                Some(args.bool("show"))
            }
            _ => None,
        })
        .collect()
}

/// Приёмка S6-15/S6-08 на настоящем cmd: размер области — у PTY и сетки; строка
/// набирается командами `cmdline.*` и уходит в shell по submit; alt-screen прячет
/// панели, выход из него — возвращает.
#[test]
fn submit_area_and_alt_screen_autoshow() {
    let mut h = H::new();
    h.until("warm-up session", running);
    h.run(Command::new("term.shell").arg("id", "cmd"));
    h.until("cmd active", running);
    h.run(Command::new("term.resize").arg("cols", 100).arg("rows", 40));
    h.until("area applied", |m| {
        m.area()
            == Some(PtySize {
                cols: 100,
                rows: 40,
            })
            && m.active().and_then(Slot::shared).is_some_and(|s| {
                s.size()
                    == PtySize {
                        cols: 100,
                        rows: 40,
                    }
            })
    });

    assert!(crate::cmdline_empty(h.core.state()));
    h.run(Command::new("cmdline.insert").arg("text", "echo URFIN_SUBMIT"));
    assert!(!crate::cmdline_empty(h.core.state()));
    h.run(Command::new("cmdline.exec"));
    assert!(crate::cmdline_empty(h.core.state()));
    h.until("submitted output", |m| active_has(m, "URFIN_SUBMIT"));

    let _ = panels_events(&mut h.core);
    let shared = h.mgr().active().and_then(Slot::shared).unwrap().clone();
    shared.inject(b"\x1b[?1049h");
    let t0 = Instant::now();
    let mut seen = Vec::new();
    while seen.is_empty() && t0.elapsed() < Duration::from_secs(5) {
        h.core.pump(Duration::from_millis(1));
        seen = panels_events(&mut h.core);
    }
    assert_eq!(seen, [Some(false)], "alt-screen hides panels");
    shared.inject(b"\x1b[?1049l");
    let t0 = Instant::now();
    seen.clear();
    while seen.is_empty() && t0.elapsed() < Duration::from_secs(5) {
        h.core.pump(Duration::from_millis(1));
        seen = panels_events(&mut h.core);
    }
    assert_eq!(seen, [Some(true)], "leaving alt-screen returns panels");

    // Многострочная вставка в cmd (без bracketed paste): сначала вопрос; «Вставить» —
    // строки в shell, панели гаснут. Одна строка с переводом в конце — в cmdline.
    h.run(Command::new("cmdline.paste").arg("text", "echo URFIN_L1\r\necho URFIN_L2\r\n"));
    assert!(
        crate::cmdline_empty(h.core.state()),
        "multi-line skips cmdline"
    );
    let id = h
        .core
        .state()
        .prompts
        .active()
        .map(|p| p.id)
        .expect("question");
    let _ = panels_events(&mut h.core);
    h.run(
        Command::new("prompt.reply")
            .arg("id", i64::try_from(id.0).unwrap())
            .arg("choice", "paste"),
    );
    h.until("pasted lines ran", |m| {
        active_has(m, "URFIN_L1") && active_has(m, "URFIN_L2")
    });
    assert_eq!(panels_events(&mut h.core), [Some(false)], "terminal shown");
    h.run(Command::new("cmdline.paste").arg("text", "dir\r\n"));
    assert_eq!(
        h.core.state().ext.get::<TermState>().unwrap().line.text(),
        "dir"
    );
}
