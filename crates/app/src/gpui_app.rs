//! Окно (ADR-0018): Core с handlers приложения, вторые запуски (S1-15) — в его `pump`;
//! окно и цикл — [`urfin_ui::gpui_shell`].

use std::time::Instant;

use urfin_core::Core;
use urfin_keymap::{FlagRegistry, KeymapCfg};
use urfin_ui::gpui_shell::{self, Signal};

use urfin_core::{Boot, Command};

use crate::Startup;

/// Всё, что окну нужно от `main`.
pub struct Launch {
    pub boot: Boot,
    pub startup: Startup,
    /// Запросы вторых запусков (S1-15); `None` — отдельный экземпляр.
    pub ipc: Option<crate::ipc::Inbox>,
    /// Пути из своей командной строки: новые табы после восстановления сессии.
    pub open: Vec<Command>,
    /// `--bench <каталог>`: сценарий замера на этом каталоге (G-07).
    pub bench: Option<std::path::PathBuf>,
}

pub fn run(launch: Launch) {
    let Launch {
        boot,
        mut startup,
        ipc,
        open,
        bench,
    } = launch;
    let geom = boot.session.as_ref().and_then(|s| s.window);
    let services = crate::vfs_services(&boot.config);
    let (config, paths) = (boot.config.clone(), boot.paths.clone());
    let (signal, wake) = Signal::new();
    let mut core = {
        let _s = tracing::info_span!("startup.core").entered();
        let t = Instant::now();
        let mut core = Core::new(boot, services, signal.clone());
        crate::register_handlers(&mut core, &config);
        startup.core = t.elapsed();
        core
    };
    // Конвейер клавиш keymap (S3) — в окно GPUI.
    let (cfg, warning) = config.section::<KeymapCfg>("keymap");
    if let Some(w) = warning {
        tracing::warn!(key = %w.key, msg = %w.msg, "[keymap] section ignored");
    }
    let keys = urfin_keymap::register(&mut core, paths, cfg, FlagRegistry::with_builtin());
    let (ui_cfg, _) = config.section::<urfin_ui::config::UiCfg>("ui");
    let theme = gpui_shell::Theme::new(&urfin_ui::sys::probe_system(), &ui_cfg);
    for c in open {
        core.dispatch(c);
    }
    // Бенч: обе панели — на каталог замера, курсор в начало.
    if let Some(dir) = &bench {
        for side in ["left", "right"] {
            core.dispatch(
                Command::new("panel.go")
                    .arg("side", side)
                    .arg("path", urfin_core::Location::local(dir.clone())),
            );
        }
    }
    if let Some(inbox) = &ipc {
        let s = signal.clone();
        inbox.set_waker(move || urfin_core::Waker::wake(&*s));
    }
    // Второй запуск: пути — новыми табами, окно — вперёд.
    let poll = Box::new(move |core: &mut Core| {
        let Some(inbox) = &ipc else {
            return false;
        };
        let mut activate = false;
        for req in inbox.try_iter() {
            for c in req.commands() {
                core.dispatch(c);
            }
            activate = true;
        }
        activate
    });
    // Первый кадр: длительности старта в лог; `--bench-startup` — JSON и выход (S0 v5 №10).
    let window_at = std::time::Instant::now();
    let first_frame = Box::new(move || {
        startup.window = window_at.elapsed();
        startup.first_frame = startup.process.elapsed();
        tracing::info!(target: "startup", json = %startup.json(), "first frame");
        if startup.bench {
            println!("{}", startup.json());
        }
        startup.bench
    });
    gpui_shell::run(
        gpui_shell::Launch {
            core,
            wake,
            signal,
            poll,
            keys,
            theme,
            first_frame: Some(first_frame),
            bench: bench.map(|_| urfin_ui::bench::Bench::new((1200.0, 760.0))),
        },
        geom,
    );
}
