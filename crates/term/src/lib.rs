//! Urfin — терминал и командная строка (S6).
//!
//! Каркас S6-01: handlers `term` и `cmdline` с полным реестром команд (S6 §9;
//! имена `cmdline.*` — как в пресетах keymap S3), секция `[term]`, состояние в
//! `AppState.ext`. Команды пока отвечают «ещё не сделано» — их наполняют
//! S6-02…S6-22 (`docs/TASKS-M3.md`).

pub mod config;
pub mod shells;
pub mod sys;
pub mod vt;

pub use config::{CompleteConfig, HistoryConfig, ShellConfig, TermConfig};

use urfin_core::{ArgSpec, CmdError, Command, CommandMeta, Core, Cx, Handler, ValueKind};

const fn arg(name: &'static str, kind: ValueKind, required: bool) -> ArgSpec {
    ArgSpec {
        name,
        kind,
        required,
    }
}

const fn meta(
    id: &'static str,
    title: &'static str,
    category: &'static str,
    args: &'static [ArgSpec],
    repeatable: bool,
) -> CommandMeta {
    CommandMeta {
        id,
        title,
        category,
        args,
        repeatable,
    }
}

const NONE: &[ArgSpec] = &[];

/// `term.*` (S6 §9).
static TERM_COMMANDS: [CommandMeta; 19] = [
    meta("term.toggle", "Show or hide panels", "term", NONE, false),
    meta(
        "term.shell",
        "Switch shell",
        "term",
        &[arg("id", ValueKind::Str, true)],
        false,
    ),
    meta("term.shell_menu", "Shell menu", "term", NONE, false),
    meta(
        "term.new",
        "New session",
        "term",
        &[arg("shell", ValueKind::Str, false)],
        false,
    ),
    meta("term.next", "Next session", "term", NONE, false),
    meta("term.prev", "Previous session", "term", NONE, false),
    meta(
        "term.close",
        "Close session",
        "term",
        &[arg("id", ValueKind::Int, false)],
        false,
    ),
    meta("term.restart", "Restart session", "term", NONE, false),
    meta("term.kill", "Kill process", "term", NONE, false),
    meta("term.focus", "Focus terminal", "term", NONE, false),
    meta(
        "term.input",
        "Send text",
        "term",
        &[arg("text", ValueKind::Str, true)],
        false,
    ),
    meta(
        "term.key",
        "Send key",
        "term",
        &[
            arg("key", ValueKind::Str, true),
            arg("mods", ValueKind::Str, false),
        ],
        true,
    ),
    meta(
        "term.scroll",
        "Scroll output",
        "term",
        &[
            arg("lines", ValueKind::Int, false),
            arg("pages", ValueKind::Int, false),
        ],
        true,
    ),
    meta("term.copy", "Copy selection", "term", NONE, false),
    meta("term.paste", "Paste", "term", NONE, false),
    meta("term.select_all", "Select all output", "term", NONE, false),
    meta("term.clear", "Clear output", "term", NONE, false),
    meta(
        "term.open",
        "Open file",
        "term",
        &[
            arg("path", ValueKind::Str, true),
            arg("verb", ValueKind::Str, false),
        ],
        false,
    ),
    meta(
        "term.exec",
        "Run command",
        "term",
        &[
            arg("cmd", ValueKind::Str, true),
            arg("cwd", ValueKind::Str, false),
        ],
        false,
    ),
];

/// `cmdline.*` (S6 §9; имена — как в пресетах keymap S3).
static CMDLINE_COMMANDS: [CommandMeta; 26] = [
    meta(
        "cmdline.insert",
        "Insert text",
        "cmdline",
        &[arg("text", ValueKind::Str, true)],
        false,
    ),
    meta("cmdline.cursor.left", "Cursor left", "cmdline", NONE, true),
    meta(
        "cmdline.cursor.right",
        "Cursor right",
        "cmdline",
        NONE,
        true,
    ),
    meta("cmdline.word.left", "Word left", "cmdline", NONE, true),
    meta("cmdline.word.right", "Word right", "cmdline", NONE, true),
    meta("cmdline.home", "Line start", "cmdline", NONE, false),
    meta("cmdline.end", "Line end", "cmdline", NONE, false),
    meta("cmdline.backspace", "Delete left", "cmdline", NONE, true),
    meta("cmdline.delete", "Delete right", "cmdline", NONE, true),
    meta(
        "cmdline.del.word_left",
        "Delete word left",
        "cmdline",
        NONE,
        true,
    ),
    meta(
        "cmdline.del.word_right",
        "Delete word right",
        "cmdline",
        NONE,
        true,
    ),
    meta(
        "cmdline.del.to_end",
        "Delete to end",
        "cmdline",
        NONE,
        false,
    ),
    meta("cmdline.clear", "Clear line", "cmdline", NONE, false),
    meta("cmdline.undo", "Undo", "cmdline", NONE, true),
    meta("cmdline.redo", "Redo", "cmdline", NONE, true),
    meta(
        "cmdline.exec",
        "Run command",
        "cmdline",
        &[arg("detached", ValueKind::Bool, false)],
        false,
    ),
    meta("cmdline.history", "Command history", "cmdline", NONE, false),
    meta(
        "cmdline.history.prev",
        "Previous command",
        "cmdline",
        NONE,
        true,
    ),
    meta(
        "cmdline.history.next",
        "Next command",
        "cmdline",
        NONE,
        true,
    ),
    meta("cmdline.complete", "Complete", "cmdline", NONE, false),
    meta(
        "cmdline.complete_native",
        "Shell completion",
        "cmdline",
        NONE,
        false,
    ),
    meta(
        "cmdline.insert_name",
        "Insert name",
        "cmdline",
        &[
            arg("what", ValueKind::Str, true),
            arg("side", ValueKind::Str, false),
        ],
        false,
    ),
    meta(
        "cmdline.insert_selection",
        "Insert selected names",
        "cmdline",
        NONE,
        false,
    ),
    meta(
        "cmdline.apply",
        "Apply to selection",
        "cmdline",
        NONE,
        false,
    ),
    meta("cmdline.copy", "Copy command line", "cmdline", NONE, false),
    meta(
        "cmdline.paste",
        "Paste",
        "cmdline",
        &[arg("text", ValueKind::Str, false)],
        false,
    ),
];

/// Состояние терминала в `AppState.ext` (S6 §1): сеансы появятся в S6-10.
#[derive(Debug, Default)]
pub struct TermState {
    pub config: TermConfig,
}

fn not_yet(cmd: &Command) -> CmdError {
    let mut a = urfin_i18n::FluentArgs::new();
    a.set("what", cmd.id.as_str().to_owned());
    CmdError::Rejected(urfin_i18n::current().fmt("term-not-yet", &a).into())
}

/// Handler `term`.
#[derive(Debug, Default)]
pub struct TermHandler {
    /// Конфиг до первого pump: тогда уходит в `TermState` (у `Core` нет доступа
    /// к состоянию до цикла).
    pending: Option<TermConfig>,
}

impl TermHandler {
    pub fn new(config: TermConfig) -> TermHandler {
        TermHandler {
            pending: Some(config),
        }
    }
}

impl Handler for TermHandler {
    fn namespace(&self) -> &'static str {
        "term"
    }

    fn on_pump(&mut self, cx: &mut Cx<'_>) {
        if let Some(config) = self.pending.take() {
            cx.state.ext.insert(TermState { config });
        }
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &TERM_COMMANDS
    }

    fn handle(&mut self, cmd: &Command, _cx: &mut Cx<'_>) -> Result<(), CmdError> {
        Err(not_yet(cmd))
    }
}

/// Handler `cmdline`.
#[derive(Debug, Default)]
pub struct CmdlineHandler;

impl Handler for CmdlineHandler {
    fn namespace(&self) -> &'static str {
        "cmdline"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &CMDLINE_COMMANDS
    }

    fn handle(&mut self, cmd: &Command, _cx: &mut Cx<'_>) -> Result<(), CmdError> {
        Err(not_yet(cmd))
    }
}

/// Регистрирует handlers `term` и `cmdline`, состояние — в `AppState.ext`.
/// Конфиг — из `config.section("term")`.
pub fn register(core: &mut Core, config: TermConfig) {
    core.register(Box::new(TermHandler::new(config)));
    core.register(Box::new(CmdlineHandler));
}

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }

    /// Приёмка S6-01: реестр видит все команды §9, `validate` проходит.
    #[test]
    fn registry_has_all_commands() {
        let mut reg = urfin_core::CommandRegistry::new();
        reg.register(&TERM_COMMANDS).unwrap();
        reg.register(&CMDLINE_COMMANDS).unwrap();
        for c in [
            "term.shell id=pwsh",
            "term.key key=Up mods=ctrl",
            "cmdline.insert text=dir",
            "cmdline.insert_name what=path side=left",
            "cmdline.exec detached=true",
        ] {
            let c = Command::parse(c).unwrap();
            assert_eq!(reg.validate(&c), Ok(()), "{c}");
        }
        for id in [
            "term.toggle",
            "term.exec",
            "term.key",
            "cmdline.exec",
            "cmdline.insert_name",
            "cmdline.del.to_end",
        ] {
            assert!(reg.get(id).is_some(), "{id}");
        }
    }
}
