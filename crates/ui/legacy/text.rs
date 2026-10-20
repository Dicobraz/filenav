//! Единственный модуль ui, где можно звать текстовые API egui (`clippy.toml`,
//! `disallowed-methods`). Остальной код передаёт сюда [`Tx`]: ключ Fluent или данные
//! пользователя — сырая строка интерфейса не пройдёт мимо i18n (S2 §7).

#![allow(clippy::disallowed_methods, reason = "модуль-шлюз текстовых API egui")]

use std::borrow::Cow;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Response, RichText, Ui};
use urfin_i18n::{FluentArgs, I18n};

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

/// Подпись с цветом и шрифтом по выбору (`ui-strong` — через `FontSet::id`).
pub fn label_styled(
    ui: &mut Ui,
    i18n: &I18n,
    tx: &Tx<'_>,
    color: Option<Color32>,
    font: Option<FontId>,
) -> Response {
    let mut rt = RichText::new(tx.resolve(i18n).as_ref());
    if let Some(c) = color {
        rt = rt.color(c);
    }
    if let Some(f) = font {
        rt = rt.font(f);
    }
    ui.label(rt)
}

/// Всплывающая подсказка у `resp` при наведении.
pub fn tooltip(resp: &Response, i18n: &I18n, tx: &Tx<'_>) {
    let _ = resp.clone().on_hover_text(tx.resolve(i18n).into_owned());
}

/// Кнопка.
pub fn button(ui: &mut Ui, i18n: &I18n, tx: &Tx<'_>) -> Response {
    ui.button(tx.resolve(i18n).as_ref())
}

/// Однострочный текст слева направо от `left_center`, не шире `max_width`: лишнее —
/// многоточием (S2 §5). Возвращает занятый прямоугольник.
#[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
pub fn paint_clipped(
    painter: &Painter,
    i18n: &I18n,
    left_center: Pos2,
    max_width: f32,
    tx: &Tx<'_>,
    font: FontId,
    color: Color32,
) -> Rect {
    let mut job = LayoutJob::single_section(
        tx.resolve(i18n).into_owned(),
        TextFormat::simple(font, color),
    );
    job.wrap = TextWrapping {
        max_width: max_width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = painter.layout_job(job);
    let pos = Pos2::new(left_center.x, left_center.y - galley.size().y / 2.0);
    let rect = Rect::from_min_size(pos, galley.size());
    painter.galley(pos, galley, color);
    rect
}

/// Число с эмуляцией tabular-цифр (S2 §0, §5): каждая цифра — по центру слота `slot`.
/// `right_center` — правый край. Для шрифтов с табличными цифрами — обычный текст.
#[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
pub fn paint_tabular(
    painter: &Painter,
    right_center: Pos2,
    text: &str,
    font: &FontId,
    color: Color32,
    slot: f32,
) -> Rect {
    let ctx = painter.ctx().clone();
    let width = |c: char| ctx.fonts_mut(|f| f.glyph_width(font, c));
    let (glyphs, total) = crate::legacy::fonts::tabular_layout(text, slot, width);
    let left = right_center.x - total;
    let mut buf = [0u8; 4];
    for (c, x) in glyphs {
        painter.text(
            Pos2::new(left + x, right_center.y),
            Align2::LEFT_CENTER,
            c.encode_utf8(&mut buf),
            font.clone(),
            color,
        );
    }
    let h = ctx.fonts_mut(|f| f.row_height(font));
    Rect::from_min_size(
        Pos2::new(left, right_center.y - h / 2.0),
        egui::vec2(total, h),
    )
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
