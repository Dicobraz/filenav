//! Urfin — keymap и пресеты хоткеев (S3).
//!
//! S3-01: типы клавиш — [`KeyCode`], [`Mods`], [`Stroke`], [`Chord`], [`Os`] — разбор и
//! каноническая форма. When-выражения, TOML-слои, компиляция и конвейер ввода —
//! S3-02…S3-05.

pub mod key;

pub use key::{Chord, KeyCode, KeyError, KeyErrorKind, Mods, Os, Stroke};

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
