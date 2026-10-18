//! Urfin — keymap и пресеты хоткеев (S3).
//!
//! S3-01: типы клавиш — [`KeyCode`], [`Mods`], [`Stroke`], [`Chord`], [`Os`] — разбор и
//! каноническая форма. S3-02: when-выражения ([`When`]) и флаги ([`FlagRegistry`]).
//! S3-03: файлы TOML и слои ([`Keymap::load`], [`KeymapWarning`]). S3-04: компиляция и
//! поиск ([`Keymap::lookup`], `keys_for`, `keybar`). S3-05: конвейер ввода
//! ([`KeyInput::process`]).

pub mod compile;
pub mod handler;
pub mod input;
pub mod key;
pub mod keymap;
pub mod when;

pub use compile::{ChordState, EffectiveBinding, KeybarItem, Lookup, Strokes};
pub use handler::{KeymapView, QuickSearch, register};
pub use input::{InputOutcome, KeyInput, KeyNote, KeyNotes};
pub use key::{Chord, KeyCode, KeyError, KeyErrorKind, Mods, Os, Stroke};
pub use keymap::{
    Binding, Keymap, KeymapCfg, KeymapWarning, Layer, MatchMode, OptionAlt, QsMode, Src, WarnKind,
};
pub use when::{FlagFn, FlagRegistry, When, WhenError, WhenErrorKind};

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
