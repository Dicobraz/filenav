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

pub(crate) struct AppHandler {
    paths: Arc<Paths>,
}

impl AppHandler {
    pub(crate) fn new(paths: Paths) -> Self {
        AppHandler {
            paths: Arc::new(paths),
        }
    }
}

static COMMANDS: [CommandMeta; 3] = [
    meta("app.quit", "Quit", &[], false),
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
            // Чтение файла — не в UI-потоке.
            let paths = self.paths.clone();
            cx.rt
                .spawn(Lane::Bulk, "config.reload", cx.rt.token(), move |tcx| {
                    let (config, warnings) = Config::load(&paths);
                    let apply = msg("config.loaded", move |cx| {
                        cx.state.set_config(Arc::new(config), &warnings);
                    });
                    let _ = tcx.sink.send(apply, &tcx.cancel);
                });
        } else if cmd.id.as_str() == "app.save_session" {
            cx.state.session.force = true;
        }
        // app.quit завершает Core (Core::finish_quit).
        cx.keep_rev();
        Ok(())
    }
}
