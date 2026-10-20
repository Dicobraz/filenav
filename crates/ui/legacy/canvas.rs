//! `Canvas` (S2 §0, S2-07): отрисовка через абстракцию — боевая [`EguiCanvas`] и
//! [`RecordCanvas`], которая пишет список `(вид, прямоугольник, роль, текст)` для
//! insta-снапшотов без GPU. Второй шлюз текстовых API egui после [`crate::legacy::text`]:
//! текст приходит как [`Tx`].

#![allow(
    clippy::disallowed_methods,
    reason = "шлюз текстовых API egui, как ui::text"
)]

use std::fmt::Write as _;
use std::sync::Arc;

use egui::{Align2, CornerRadius, Galley, Painter, Pos2, Rect, Stroke, StrokeKind};
use urfin_i18n::I18n;

use crate::legacy::fonts::{Font, FontSet};
use crate::legacy::text::Tx;
use crate::legacy::tokens::{Role, Tokens};

pub trait Canvas {
    /// Регион раскладки (для записи; в egui ничего не рисует).
    fn region(&mut self, name: &'static str, r: Rect);
    /// Дальнейшее рисование — только внутри `r` (прокручиваемые полосы).
    fn clip(&mut self, r: Rect);
    fn rect(&mut self, r: Rect, role: Role, radius: CornerRadius);
    fn stroke(&mut self, r: Rect, role: Role, radius: CornerRadius, width: f32);
    /// Текст роли `font` кеглем `size`; возвращает занятый прямоугольник.
    #[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
    fn text(
        &mut self,
        pos: Pos2,
        anchor: Align2,
        tx: &Tx<'_>,
        font: Font,
        size: f32,
        role: Role,
    ) -> Rect;
    fn galley(&mut self, pos: Pos2, galley: Arc<Galley>, role: Role);
    /// Однострочный текст от `left_center` не шире `max_width`: лишнее — многоточием.
    #[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
    fn text_clipped(
        &mut self,
        left_center: Pos2,
        max_width: f32,
        tx: &Tx<'_>,
        font: Font,
        size: f32,
        role: Role,
    );
    /// Galley цветом не из ролей (раскраска по правилам пользователя).
    fn tinted(&mut self, pos: Pos2, galley: Arc<Galley>, color: egui::Color32);
    fn icon(&mut self, center: Pos2, glyph: char, size: f32, role: Role);
}

/// Боевая реализация поверх `egui::Painter`.
pub struct EguiCanvas<'a> {
    pub painter: Painter,
    pub tokens: &'a Tokens,
    pub fonts: &'a FontSet,
    pub i18n: &'a I18n,
}

impl std::fmt::Debug for EguiCanvas<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EguiCanvas").finish_non_exhaustive()
    }
}

impl Canvas for EguiCanvas<'_> {
    fn region(&mut self, _name: &'static str, _r: Rect) {}

    fn clip(&mut self, r: Rect) {
        self.painter.set_clip_rect(r);
    }

    fn rect(&mut self, r: Rect, role: Role, radius: CornerRadius) {
        self.painter.rect_filled(r, radius, self.tokens.c(role));
    }

    fn stroke(&mut self, r: Rect, role: Role, radius: CornerRadius, width: f32) {
        self.painter.rect_stroke(
            r,
            radius,
            Stroke::new(width, self.tokens.c(role)),
            StrokeKind::Inside,
        );
    }

    fn text(
        &mut self,
        pos: Pos2,
        anchor: Align2,
        tx: &Tx<'_>,
        font: Font,
        size: f32,
        role: Role,
    ) -> Rect {
        self.painter.text(
            pos,
            anchor,
            tx.resolve(self.i18n).as_ref(),
            self.fonts.id(font, size),
            self.tokens.c(role),
        )
    }

    fn text_clipped(
        &mut self,
        left_center: Pos2,
        max_width: f32,
        tx: &Tx<'_>,
        font: Font,
        size: f32,
        role: Role,
    ) {
        crate::legacy::text::paint_clipped(
            &self.painter,
            self.i18n,
            left_center,
            max_width,
            tx,
            self.fonts.id(font, size),
            self.tokens.c(role),
        );
    }

    fn galley(&mut self, pos: Pos2, galley: Arc<Galley>, role: Role) {
        self.painter
            .galley_with_override_text_color(pos, galley, self.tokens.c(role));
    }

    fn tinted(&mut self, pos: Pos2, galley: Arc<Galley>, color: egui::Color32) {
        self.painter
            .galley_with_override_text_color(pos, galley, color);
    }

    fn icon(&mut self, center: Pos2, glyph: char, size: f32, role: Role) {
        let mut buf = [0u8; 4];
        self.painter.text(
            center,
            Align2::CENTER_CENTER,
            glyph.encode_utf8(&mut buf),
            self.fonts.id(Font::Icons, size),
            self.tokens.c(role),
        );
    }
}

/// Запись отрисовки.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub kind: &'static str,
    pub rect: Rect,
    pub role: Option<Role>,
    pub text: Option<String>,
}

/// Пишет операции вместо рисования; ширина текста оценивается (0,5 кегля на символ).
#[derive(Debug)]
pub struct RecordCanvas<'a> {
    pub i18n: &'a I18n,
    pub ops: Vec<Op>,
}

impl<'a> RecordCanvas<'a> {
    pub fn new(i18n: &'a I18n) -> Self {
        Self {
            i18n,
            ops: Vec::new(),
        }
    }

    fn push(&mut self, kind: &'static str, rect: Rect, role: Option<Role>, text: Option<String>) {
        self.ops.push(Op {
            kind,
            rect,
            role,
            text,
        });
    }

    /// insta-YAML: по операции на запись.
    pub fn to_yaml(&self) -> String {
        let mut out = String::new();
        for op in &self.ops {
            let r = op.rect;
            let _ = write!(
                out,
                "- {}: [{}, {}, {}, {}]",
                op.kind,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            );
            if let Some(role) = op.role {
                let _ = write!(out, " {}", role.name());
            }
            if let Some(t) = &op.text {
                let _ = write!(out, " {t:?}");
            }
            out.push('\n');
        }
        out
    }
}

impl Canvas for RecordCanvas<'_> {
    fn region(&mut self, name: &'static str, r: Rect) {
        self.push("region", r, None, Some(name.to_owned()));
    }

    fn clip(&mut self, r: Rect) {
        self.push("clip", r, None, None);
    }

    fn rect(&mut self, r: Rect, role: Role, _radius: CornerRadius) {
        self.push("rect", r, Some(role), None);
    }

    fn stroke(&mut self, r: Rect, role: Role, _radius: CornerRadius, _width: f32) {
        self.push("stroke", r, Some(role), None);
    }

    fn text(
        &mut self,
        pos: Pos2,
        anchor: Align2,
        tx: &Tx<'_>,
        _font: Font,
        size: f32,
        role: Role,
    ) -> Rect {
        let text = tx.resolve(self.i18n).into_owned();
        #[allow(clippy::cast_precision_loss, reason = "оценка ширины")]
        let size_v = egui::vec2(text.chars().count() as f32 * size * 0.5, size);
        let r = anchor.anchor_size(pos, size_v);
        self.push("text", r, Some(role), Some(text));
        r
    }

    fn galley(&mut self, pos: Pos2, galley: Arc<Galley>, role: Role) {
        let r = Rect::from_min_size(pos, galley.size());
        self.push("galley", r, Some(role), Some(galley.text().to_owned()));
    }

    fn text_clipped(
        &mut self,
        left_center: Pos2,
        max_width: f32,
        tx: &Tx<'_>,
        _font: Font,
        size: f32,
        role: Role,
    ) {
        let text = tx.resolve(self.i18n).into_owned();
        let r = Rect::from_min_size(
            Pos2::new(left_center.x, left_center.y - size / 2.0),
            egui::vec2(max_width, size),
        );
        self.push("text", r, Some(role), Some(text));
    }

    fn tinted(&mut self, pos: Pos2, galley: Arc<Galley>, color: egui::Color32) {
        let r = Rect::from_min_size(pos, galley.size());
        let [cr, cg, cb, ca] = color.to_srgba_unmultiplied();
        let text = if ca == 255 {
            format!("{} #{cr:02X}{cg:02X}{cb:02X}", galley.text())
        } else {
            format!("{} #{ca:02X}{cr:02X}{cg:02X}{cb:02X}", galley.text())
        };
        self.push("galley", r, None, Some(text));
    }

    fn icon(&mut self, center: Pos2, glyph: char, size: f32, role: Role) {
        let r = Rect::from_center_size(center, egui::vec2(size, size));
        self.push(
            "icon",
            r,
            Some(role),
            Some(format!("U+{:04X}", u32::from(glyph))),
        );
    }
}
