//! Модель команд: одна форма на проводе — `Command { id, args }` (ADR-0002).
//!
//! Три представления одной команды:
//! - типизированное: [`Command::new`]`("panel.go").arg("path", loc)`;
//! - текст для cmdline и палитры: `panel.go path="C:\x y"` ([`Command::parse`], `Display`);
//! - TOML для keymap: `"panel.up"` или `{ cmd = "panel.go", path = "C:\\x" }` (serde).

mod args;
mod id;
mod registry;
mod serde_impl;
mod text;
mod value;

pub use args::{Args, SideRef};
pub use id::CommandId;
pub use registry::{ArgSpec, CommandMeta, CommandRegistry};
pub use value::{Value, ValueKind};

use compact_str::CompactString;

/// Источник команды.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Origin {
    Key,
    Mouse,
    Palette,
    Cmdline,
    #[default]
    Internal,
    Remote,
}

/// Команда. `origin` не входит в текстовую и TOML-формы (при разборе — `Internal`).
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub args: Args,
    pub origin: Origin,
}

impl Command {
    /// Команда без аргументов. Паникует на невалидном id — для литералов в коде.
    pub fn new(id: &'static str) -> Self {
        Self::with_id(CommandId::from_static(id))
    }

    pub(crate) fn with_id(id: CommandId) -> Self {
        Command {
            id,
            args: Args::default(),
            origin: Origin::default(),
        }
    }

    #[must_use]
    pub fn arg(mut self, k: &str, v: impl Into<Value>) -> Self {
        self.args = self.args.with(k, v);
        self
    }

    #[must_use]
    pub fn from(mut self, o: Origin) -> Self {
        self.origin = o;
        self
    }

    /// Текстовая форма для cmdline/палитры: `panel.go path="C:\x"`.
    ///
    /// Синтаксис: `id` и пары `key=value` через пробелы. Значение:
    /// - `"…"` — строка как есть, обратный слэш не экранирует (`"D:\"` — это `D:\`);
    ///   кавычка внутри удваивается: `"a""b"` → `a"b`;
    /// - `true` / `false` — `Bool`, `-?[0-9]+` — `Int`;
    /// - `[v, v, …]` — `List` (элементы — любые значения, в том числе списки);
    /// - иначе слово до пробела (в списке — до `,` или `]`) — `Str`.
    ///
    /// `Loc` текстом не задаётся: строка превращается в `Location` в [`Args::loc`].
    pub fn parse(s: &str) -> Result<Self, CmdError> {
        text::parse(s)
    }
}

/// Ошибка команды.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CmdError {
    #[error("unknown command {0}")]
    Unknown(CompactString),
    #[error("bad id {0}")]
    BadId(CompactString),
    /// Ошибка аргумента: отсутствует, неверный тип или синтаксис. `name` пуст,
    /// если ошибка синтаксиса не привязана к ключу.
    #[error("arg {name}: {msg}")]
    Arg {
        name: CompactString,
        msg: CompactString,
    },
    /// «Нельзя сейчас»; показывается тостом.
    #[error("{0}")]
    Rejected(CompactString),
}

impl CmdError {
    pub(crate) fn arg(name: &str, msg: impl Into<CompactString>) -> Self {
        CmdError::Arg {
            name: name.into(),
            msg: msg.into(),
        }
    }
}

/// Имя аргумента: `[a-z0-9_]+`, кроме зарезервированного `cmd` (ключ id в TOML-форме).
pub(crate) fn valid_key(k: &str) -> bool {
    !k.is_empty()
        && k != "cmd"
        && k.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
