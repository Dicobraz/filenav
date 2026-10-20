//! Цвет и тема токенов: `Color32` — sRGB с премультиплицированной альфой (крейт
//! `ecolor`, цвета токенов совпадают бит в бит с egui-версией), [`Theme`] — светлая
//! или тёмная.

pub use ecolor::Color32;

/// Тема оформления.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}
