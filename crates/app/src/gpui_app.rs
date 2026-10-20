//! Запуск интерфейса на GPUI (`urfin --ui=gpui`, ADR-0018, G-02): Core с теми же
//! handlers, что у egui-окна, вторые запуски (S1-15) — в его `pump`; окно и цикл —
//! [`urfin_ui::gpui_shell`].

use std::time::Instant;

use urfin_core::Core;
use urfin_keymap::{FlagRegistry, KeymapCfg};
use urfin_ui::gpui_shell::{self, Signal};

use crate::window::Launch;

pub fn run(launch: Launch) {
    let Launch {
        boot,
        mut startup,
        ipc,
        open,
        ..
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
    // Команды keymap — в реестре (конвейер клавиш GPUI — G-03).
    let (cfg, warning) = config.section::<KeymapCfg>("keymap");
    if let Some(w) = warning {
        tracing::warn!(key = %w.key, msg = %w.msg, "[keymap] section ignored");
    }
    let _keys = urfin_keymap::register(&mut core, paths, cfg, FlagRegistry::with_builtin());
    for c in open {
        core.dispatch(c);
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
    tracing::info!(core_ms = startup.core.as_millis(), "gpui window");
    gpui_shell::run(
        gpui_shell::Launch {
            core,
            wake,
            signal,
            poll,
        },
        geom,
    );
}
