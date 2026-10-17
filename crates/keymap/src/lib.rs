//! Urfin — keymap и пресеты хоткеев (S3).
//!
//! S3-01: типы клавиш — [`KeyCode`], [`Mods`], [`Stroke`], [`Chord`], [`Os`] — разбор и
//! каноническая форма. S3-02: when-выражения ([`When`]) и флаги ([`FlagRegistry`]).
//! S3-03: файлы TOML и слои ([`Keymap::load`], [`KeymapWarning`]). S3-04: компиляция и
//! поиск ([`Keymap::lookup`], `keys_for`, `keybar`). Конвейер ввода — S3-05.

pub mod compile;
pub mod key;
pub mod keymap;
pub mod when;

pub use compile::{ChordState, EffectiveBinding, KeybarItem, Lookup, Strokes};
pub use key::{Chord, KeyCode, KeyError, KeyErrorKind, Mods, Os, Stroke};
pub use keymap::{
    Binding, Keymap, KeymapCfg, KeymapWarning, Layer, MatchMode, OptionAlt, QsMode, Src, WarnKind,
};
pub use when::{FlagFn, FlagRegistry, When, WhenError, WhenErrorKind};

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
