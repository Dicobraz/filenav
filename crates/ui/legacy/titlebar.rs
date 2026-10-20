//! Заголовок окна 40 px (S2 §2.1, S2-06): табы каждой панели над её карточкой, справа —
//! кнопки caption 46×40 (Windows, своя рамка). Пустая зона — `HTCAPTION` для ОС.
//! Табы — виджет [`tabs`] (S2-13).

use std::sync::atomic::Ordering;

use egui::{Align2, Color32, Rect, Sense};
use urfin_core::{AppState, Command, Side};
use urfin_i18n::I18n;

use crate::legacy::chrome::{ChromeHandle, PxRect, TitleLayout};
use crate::legacy::fonts::{Font, FontSet};
use crate::legacy::icons;
use crate::legacy::tabs;
use crate::legacy::text::{self, Tx};
use crate::legacy::tokens::{Metric, Role, Tokens};

/// Ширина кнопки caption (Windows 11).
const BUTTON_W: f32 = 46.0;
/// Красная кнопка «закрыть» при наведении — системная, не из темы.
const CLOSE_HOVER: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);

pub(crate) struct TitleBar<'a> {
    pub i18n: &'a I18n,
    pub tokens: &'a Tokens,
    pub fonts: &'a FontSet,
    pub chrome: Option<&'a ChromeHandle>,
    /// Карточки панелей (из раскладки): табы — над ними.
    pub columns: [Option<Rect>; 2],
    /// Состояние наборов табов (прокрутка, перетаскивание).
    pub memo: &'a mut [tabs::TabsMemo; 2],
    /// Меню F9 открыто — меню-бар вместо табов (S2 §2.6).
    pub menu: Option<MenuBar<'a>>,
}

/// Меню-бар в заголовке.
pub(crate) struct MenuBar<'a> {
    pub menus: &'a [crate::legacy::menu::MenuDef],
    pub open: &'a str,
    pub memo: &'a mut crate::legacy::menu::MenuMemo,
}

impl TitleBar<'_> {
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, s: &AppState, out: &mut Vec<Command>) {
        let t = self.tokens;
        let bar = ui.max_rect();
        let gap = t.m(Metric::GapPanels);
        let buttons_w = if self.chrome.is_some() {
            BUTTON_W * 3.0
        } else {
            0.0
        };
        let mut interactive = Vec::new();
        // Табы — над своими карточками, не заходя на кнопки caption.
        let limit = bar.right() - buttons_w - gap;
        if let Some(m) = self.menu.as_mut() {
            let left = self
                .columns
                .iter()
                .flatten()
                .map(egui::Rect::left)
                .fold(limit, f32::min);
            let area =
                Rect::from_min_max(egui::pos2(left, bar.top()), egui::pos2(limit, bar.bottom()));
            let mut c = crate::legacy::canvas::EguiCanvas {
                painter: ui.painter_at(area),
                tokens: self.tokens,
                fonts: self.fonts,
                i18n: self.i18n,
            };
            crate::legacy::menu::bar(
                &mut c,
                ui,
                self.i18n,
                self.tokens,
                self.fonts,
                m.menus,
                m.open,
                m.memo,
                area,
                out,
                &mut interactive,
            );
        }
        let show_tabs = self.menu.is_none();
        for (side, card) in [Side::Left, Side::Right]
            .into_iter()
            .zip(self.columns)
            .filter(|_| show_tabs)
        {
            if let Some(card) = card {
                let area = Rect::from_min_max(
                    egui::pos2(card.left(), bar.top()),
                    egui::pos2(card.right().min(limit), bar.bottom()),
                );
                self.tabs(ui, s, side, area, out, &mut interactive);
            }
        }
        if let Some(chrome) = self.chrome {
            self.buttons(ui, bar, chrome, &mut interactive);
            let ppp = ui.ctx().pixels_per_point();
            let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            let max_rect = button_rect(bar, 1);
            chrome.shared.set_layout(TitleLayout {
                caption_h: PxRect::from_egui(bar, ppp).bottom,
                width: PxRect::from_egui(ui.ctx().content_rect(), ppp).right,
                resize: if maximized {
                    0
                } else {
                    PxRect::from_egui(
                        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(0.0, t.m(Metric::SpaceS))),
                        ppp,
                    )
                    .bottom
                },
                max_button: PxRect::from_egui(max_rect, ppp),
                interactive: interactive
                    .iter()
                    .map(|r| PxRect::from_egui(*r, ppp))
                    .collect(),
            });
        }
    }

    fn tabs(
        &mut self,
        ui: &mut egui::Ui,
        s: &AppState,
        side: Side,
        area: Rect,
        out: &mut Vec<Command>,
        interactive: &mut Vec<Rect>,
    ) {
        let mut c = crate::legacy::canvas::EguiCanvas {
            painter: ui.painter_at(area),
            tokens: self.tokens,
            fonts: self.fonts,
            i18n: self.i18n,
        };
        let mut cx = tabs::TabsCx {
            ui,
            tokens: self.tokens,
            fonts: self.fonts,
            memo: &mut self.memo[side.idx()],
        };
        tabs::show(&mut c, &mut cx, s.panel(side), side, area, out, interactive);
    }

    fn buttons(
        &self,
        ui: &mut egui::Ui,
        bar: Rect,
        chrome: &ChromeHandle,
        interactive: &mut Vec<Rect>,
    ) {
        let t = self.tokens;
        let ctx = ui.ctx().clone();
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let glyph_font = self.fonts.id(Font::Icons, 10.0);
        let painter = ui.painter_at(bar);
        let max_glyph = if maximized {
            icons::CHROME_RESTORE
        } else {
            icons::CHROME_MAXIMIZE
        };
        for (i, glyph) in [icons::CHROME_MINIMIZE, max_glyph, icons::CHROME_CLOSE]
            .into_iter()
            .enumerate()
        {
            let rect = button_rect(bar, i);
            let (hover, pressed, clicked) = if i == 1 {
                // «Развернуть» — HTMAXBUTTON: состояние ведёт оконная процедура.
                (
                    chrome.shared.max_hover.load(Ordering::Relaxed),
                    chrome.shared.max_pressed.load(Ordering::Relaxed),
                    false,
                )
            } else {
                let resp = ui.interact(rect, ui.scope_id().with(("caption", i)), Sense::CLICK);
                interactive.push(rect);
                (
                    resp.hovered(),
                    resp.is_pointer_button_down_on(),
                    resp.clicked(),
                )
            };
            let close = i == 2;
            let (fill, fg) = match (close, hover, pressed) {
                (true, true, false) => (CLOSE_HOVER, Color32::WHITE),
                (true, _, true) => (CLOSE_HOVER.gamma_multiply(0.9), Color32::WHITE),
                (false, _, true) => (t.c(Role::FillSubtlePressed), t.c(Role::TextSecondary)),
                (false, true, false) => (t.c(Role::FillSubtleHover), t.c(Role::TextPrimary)),
                _ => (Color32::TRANSPARENT, t.c(Role::TextPrimary)),
            };
            painter.rect_filled(rect, 0.0, fill);
            let mut buf = [0u8; 4];
            text::paint(
                &painter,
                self.i18n,
                rect.center(),
                Align2::CENTER_CENTER,
                &Tx::user(&*glyph.encode_utf8(&mut buf)),
                glyph_font.clone(),
                fg,
            );
            if clicked {
                ctx.send_viewport_cmd(if close {
                    egui::ViewportCommand::Close
                } else {
                    egui::ViewportCommand::Minimized(true)
                });
            }
        }
    }
}

/// Кнопка caption `i` (0 — свернуть, 1 — развернуть, 2 — закрыть) у правого края.
fn button_rect(bar: Rect, i: usize) -> Rect {
    let from_right = [2.0, 1.0, 0.0][i.min(2)];
    let right = bar.right() - BUTTON_W * from_right;
    Rect::from_min_max(
        egui::pos2(right - BUTTON_W, bar.top()),
        egui::pos2(right, bar.bottom()),
    )
}
