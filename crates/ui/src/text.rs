//! Единственный модуль ui, где можно звать текстовые API egui (`clippy.toml`,
//! `disallowed-methods`). Остальной код передаёт сюда [`Tx`]: ключ Fluent или данные
//! пользователя — сырая строка интерфейса не пройдёт мимо i18n (S2 §7).

#![allow(clippy::disallowed_methods, reason = "модуль-шлюз текстовых API egui")]

use std::borrow::Cow;

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Response, RichText, Ui};
use filenav_i18n::{FluentArgs, I18n};

/// Текст для показа.
#[derive(Debug)]
pub enum Tx<'a> {
    /// Ключ Fluent без аргументов (`ui-loading`, `keybar-ops-copy`).
    Key(Cow<'static, str>),
    /// Ключ с аргументами.
    Args(&'static str, FluentArgs<'a>),
    /// Данные, а не текст интерфейса: имена, пути, ошибки ОС, уже переведённые строки core.
    User(Cow<'a, str>),
}

impl<'a> Tx<'a> {
    pub fn key(key: &'static str) -> Self {
        Tx::Key(Cow::Borrowed(key))
    }

    pub fn user(text: impl Into<Cow<'a, str>>) -> Self {
        Tx::User(text.into())
    }

    /// Строка на активном языке.
    pub fn resolve(&self, i18n: &I18n) -> Cow<'_, str> {
        match self {
            Tx::Key(key) => Cow::Owned(i18n.get(key)),
            Tx::Args(key, args) => Cow::Owned(i18n.fmt(key, args)),
            Tx::User(text) => Cow::Borrowed(text),
        }
    }
}

/// Подпись.
pub fn label(ui: &mut Ui, i18n: &I18n, tx: &Tx<'_>) -> Response {
    ui.label(tx.resolve(i18n).as_ref())
}

/// Подпись с цветом и моноширинным шрифтом по выбору.
pub fn label_styled(
    ui: &mut Ui,
    i18n: &I18n,
    tx: &Tx<'_>,
    color: Option<Color32>,
    strong: bool,
) -> Response {
    let mut rt = RichText::new(tx.resolve(i18n).as_ref());
    if let Some(c) = color {
        rt = rt.color(c);
    }
    if strong {
        rt = rt.strong();
    }
    ui.label(rt)
}

/// Кнопка.
pub fn button(ui: &mut Ui, i18n: &I18n, tx: &Tx<'_>) -> Response {
    ui.button(tx.resolve(i18n).as_ref())
}

/// Текст painter-ом (строки списков, keybar). Возвращает занятый прямоугольник.
pub fn paint(
    painter: &Painter,
    i18n: &I18n,
    pos: Pos2,
    anchor: Align2,
    tx: &Tx<'_>,
    font: FontId,
    color: Color32,
) -> Rect {
    painter.text(pos, anchor, tx.resolve(i18n).as_ref(), font, color)
}
