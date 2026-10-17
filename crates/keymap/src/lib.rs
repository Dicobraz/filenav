//! Urfin — keymap и пресеты хоткеев (S3).
//!
//! S3-01: типы клавиш — [`KeyCode`], [`Mods`], [`Stroke`], [`Chord`], [`Os`] — разбор и
//! каноническая форма. S3-02: when-выражения ([`When`]) и флаги ([`FlagRegistry`]).
//! TOML-слои, компиляция и конвейер ввода — S3-03…S3-05.

pub mod key;
pub mod when;

pub use key::{Chord, KeyCode, KeyError, KeyErrorKind, Mods, Os, Stroke};
pub use when::{FlagFn, FlagRegistry, When, WhenError, WhenErrorKind};

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
