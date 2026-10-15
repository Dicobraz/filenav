//! `app.*`. Выход: опрос handlers (`Handler::on_quit`) делает Core после этой
//! команды; перечитывание конфига — S1-10, запись сессии — S1-11.

use super::meta;
use crate::bus::Handler;
use crate::cmd::{CmdError, Command, CommandMeta};
use crate::msg::Cx;

pub(crate) struct AppHandler;

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
        // app.quit завершает Core (Core::finish_quit); остальное — до S1-10/S1-11 без действия.
        tracing::debug!(cmd = cmd.id.as_str(), "app command");
        cx.keep_rev();
        Ok(())
    }
}
