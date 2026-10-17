//! Заголовок окна 40 px (S2 §2.1, S2-06): табы каждой панели над её карточкой, справа —
//! кнопки caption 46×40 (Windows, своя рамка). Пустая зона — `HTCAPTION` для ОС.
//! Табы пока простые (подпись, клик); крестик, `+` и перетаскивание — S2-13.

use std::sync::atomic::Ordering;

use egui::{Align2, Color32, Rect, Sense};
use filenav_core::{AppState, Command, Side};
use filenav_i18n::I18n;

use crate::chrome::{ChromeHandle, PxRect, TitleLayout};
use crate::fonts::{Font, FontSet};
use crate::icons;
use crate::panel::side_arg;
use crate::text::{self, Tx};
use crate::tokens::{Metric, Role, Tokens};

/// Ширина кнопки caption (Windows 11).
const BUTTON_W: f32 = 46.0;
/// Красная кнопка «закрыть» при наведении — системная, не из темы.
const CLOSE_HOVER: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);

pub(crate) struct TitleBar<'a> {
    pub i18n: &'a I18n,
    pub tokens: &'a Tokens,
    pub fonts: &'a FontSet,
    pub chrome: Option<&'a ChromeHandle>,
    /// Доля левой панели и зазор — как у карточек.
    pub split: f32,
}

impl TitleBar<'_> {
    pub(crate) fn show(&self, ui: &mut egui::Ui, s: &AppState, out: &mut Vec<Command>) {
        let t = self.tokens;
        let bar = ui.max_rect();
        let gap = t.m(Metric::GapPanels);
        let buttons_w = if self.chrome.is_some() {
            BUTTON_W * 3.0
        } else {
            0.0
        };
        let mut interactive = Vec::new();
        // Табы — над карточками: та же раскладка, что у панелей.
        let content = bar
            .with_max_x(bar.right() - buttons_w)
            .shrink2(egui::vec2(gap, 0.0));
        let left_w = ((bar.width() - 3.0 * gap) * self.split).max(0.0);
        let columns = [
            Rect::from_min_max(
                content.min,
                egui::pos2(content.left() + left_w, content.bottom()),
            ),
            Rect::from_min_max(
                egui::pos2(content.left() + left_w + gap, content.top()),
                content.max,
            ),
        ];
        for (side, area) in [Side::Left, Side::Right].into_iter().zip(columns) {
            self.tabs(ui, s, side, area, out, &mut interactive);
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

    #[allow(clippy::many_single_char_names, reason = "геометрия: t, p, s, x, w, f")]
    fn tabs(
        &self,
        ui: &mut egui::Ui,
        s: &AppState,
        side: Side,
        area: Rect,
        out: &mut Vec<Command>,
        interactive: &mut Vec<Rect>,
    ) {
        let t = self.tokens;
        let p = s.panel(side);
        let font = self.fonts.id(Font::Ui, t.m(Metric::TypeBody));
        let strong = self.fonts.id(Font::Strong, t.m(Metric::TypeBody));
        let top = area.top() + (t.m(Metric::HTitlebar) - t.m(Metric::HTab));
        let pad = t.m(Metric::SpaceM);
        let mut x = area.left();
        let painter = ui.painter_at(area);
        for (i, tab) in p.tabs.iter().enumerate() {
            let current = i == p.active;
            let title = tab.title();
            let f = if current { &strong } else { &font };
            let text_w = ui.ctx().fonts_mut(|fs| {
                fs.layout_no_wrap(title.to_string(), f.clone(), Color32::WHITE)
                    .size()
                    .x
            });
            let w = (text_w + 2.0 * pad).min(240.0).min(area.right() - x);
            if w <= pad {
                break;
            }
            let rect = Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, t.m(Metric::HTab)));
            let resp = ui.interact(rect, ui.id().with(("tab", side_arg(side), i)), Sense::CLICK);
            if current {
                let r = t.radius(Metric::RadiusOverlay).nw;
                painter.rect_filled(
                    rect,
                    egui::CornerRadius {
                        nw: r,
                        ne: r,
                        sw: 0,
                        se: 0,
                    },
                    t.c(Role::LayerPanel),
                );
            } else if resp.hovered() {
                painter.rect_filled(
                    rect.shrink2(egui::vec2(0.0, 4.0)),
                    t.radius(Metric::RadiusControl),
                    t.c(Role::FillSubtleHover),
                );
            }
            text::paint_clipped(
                &painter,
                self.i18n,
                egui::pos2(rect.left() + pad, rect.center().y),
                w - 2.0 * pad,
                &Tx::user(title.as_ref()),
                f.clone(),
                t.c(if current {
                    Role::TextPrimary
                } else {
                    Role::TextSecondary
                }),
            );
            if resp.clicked() {
                out.push(
                    Command::new("tab.goto")
                        .arg("n", i64::try_from(i + 1).unwrap_or(1))
                        .arg("side", side_arg(side)),
                );
            }
            interactive.push(rect);
            x += w + t.m(Metric::SpaceXxs);
        }
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
                let resp = ui.interact(rect, ui.id().with(("caption", i)), Sense::CLICK);
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
