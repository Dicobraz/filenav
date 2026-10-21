//! `app.*`. Выход: опрос handlers (`Handler::on_quit`) делает Core после этой
//! команды; запись сессии — Core в конце pump.

use std::sync::Arc;

use super::meta;
use crate::bus::Handler;
use crate::cmd::{CmdError, Command, CommandMeta};
use crate::config::Config;
use crate::msg::{Cx, msg};
use crate::paths::Paths;
use crate::runtime::Lane;
use crate::settings::{self, Writer};

pub(crate) struct AppHandler {
    paths: Arc<Paths>,
    /// Запись настроек: перед перечитыванием конфига отложенное пишется (S2-22).
    settings: Arc<Writer>,
}

impl AppHandler {
    pub(crate) fn new(paths: Paths, settings: Arc<Writer>) -> Self {
        AppHandler {
            paths: Arc::new(paths),
            settings,
        }
    }
}

static COMMANDS: [CommandMeta; 4] = [
    meta("app.quit", "Quit", &[], false),
    meta("app.dismiss_banner", "Dismiss banner", &[], false),
    meta("app.reload_config", "Reload configuration", &[], false),
    meta("app.save_session", "Save session", &[], false),
];

impl Handler for AppHandler {
    fn namespace(&self) -> &'static str {
        "app"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        if cmd.id.as_str() == "app.reload_config" {
            // Чтение файла — не в UI-потоке. Сначала отложенные настройки из UI: иначе
            // перечитанный файл вернул бы их старые значения.
            let (paths, settings) = (self.paths.clone(), Arc::clone(&self.settings));
            settings.enqueue(cx.state.settings.take());
            cx.rt
                .spawn(Lane::Bulk, "config.reload", cx.rt.token(), move |tcx| {
                    let (saved, (config, warnings)) = settings.flush_then(|| Config::load(&paths));
                    if let Err(e) = saved {
                        let _ = tcx
                            .sink
                            .send(settings::failed(settings.file(), &e), &tcx.cancel);
                    }
                    let apply = msg("config.loaded", move |cx| {
                        cx.state.set_config(Arc::new(config), &warnings);
                    });
                    let _ = tcx.sink.send(apply, &tcx.cancel);
                });
        } else if cmd.id.as_str() == "app.save_session" {
            cx.state.session.force = true;
        } else if cmd.id.as_str() == "app.dismiss_banner" {
            cx.state.dismiss_banner();
            return Ok(());
        }
        // app.quit завершает Core (Core::finish_quit).
        cx.keep_rev();
        Ok(())
    }
}
