//! Табы панели в заголовке окна (S2 §2.1, S2-13): набор над своей карточкой,
//! граница — по сплиттеру.
//!
//! Активный таб — `layer.panel` со скруглёнными верхними углами и жирным шрифтом;
//! крестик — у активного и под мышью. Средняя кнопка закрывает, `+` — `tab.new`,
//! перетаскивание — `tab.move`. Не влезают — стрелки прокрутки по краям, активный
//! всегда виден. Табы, крестики, `+` и стрелки — интерактивные зоны для hit-test
//! рамки (S2-06): клик по ним не тащит окно, пустое место заголовка — тащит.

use std::sync::Arc;

use compact_str::CompactString;
use egui::{Galley, Rect, Sense, pos2, vec2};
use urfin_core::{Command, Panel, Side, TabId};

use crate::canvas::Canvas;
use crate::fonts::{Font, FontSet};
use crate::icons;
use crate::panel::side_arg;
use crate::text::Tx;
use crate::tokens::{Metric, Role, Tokens};

/// Ширина таба: не уже и не шире (S2 §2.1, обрезка `…`).
const TAB_MIN: f32 = 96.0;
const TAB_MAX: f32 = 240.0;
/// Крестик закрытия: зона справа в табе.
const CLOSE_W: f32 = 24.0;
/// Кнопки `+` и стрелок прокрутки.
const BUTTON_W: f32 = 32.0;
/// Зазор между табами.
const GAP: f32 = 2.0;
/// Шаг прокрутки стрелкой.
const SCROLL_STEP: f32 = 160.0;

/// Раскладка набора табов в полосе.
#[derive(Clone, Debug, PartialEq)]
pub struct TabsLayout {
    /// `x` и ширина каждого таба (уже с прокруткой).
    pub tabs: Vec<(f32, f32)>,
    /// Видимая часть полосы табов.
    pub view: (f32, f32),
    /// Стрелки прокрутки (влево, вправо), если табы не влезают.
    pub arrows: Option<((f32, f32), (f32, f32))>,
    pub plus: (f32, f32),
    /// Прокрутка, приведённая к допустимой.
    pub scroll: f32,
    pub max_scroll: f32,
}

/// Раскладка табов шириной `natural` в `left..right` с прокруткой `scroll`.
pub fn layout(natural: &[f32], left: f32, right: f32, scroll: f32) -> TabsLayout {
    let widths: Vec<f32> = natural.iter().map(|w| w.clamp(TAB_MIN, TAB_MAX)).collect();
    #[allow(clippy::cast_precision_loss, reason = "табов — десятки")]
    let total = widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32;
    let fits = total + GAP + BUTTON_W <= right - left;
    let (view, arrows, plus) = if fits {
        let plus_l = left + total + GAP;
        ((left, left + total), None, (plus_l, plus_l + BUTTON_W))
    } else {
        let back = (left, left + BUTTON_W);
        let plus = (right - BUTTON_W, right);
        let fwd = (plus.0 - BUTTON_W, plus.0);
        ((back.1, fwd.0), Some((back, fwd)), plus)
    };
    let max_scroll = (total - (view.1 - view.0)).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let mut x = view.0 - scroll;
    let tabs = widths
        .iter()
        .map(|&w| {
            let t = (x, w);
            x += w + GAP;
            t
        })
        .collect();
    TabsLayout {
        tabs,
        view,
        arrows,
        plus,
        scroll,
        max_scroll,
    }
}

/// Прокрутка, при которой таб `i` виден целиком.
pub fn scroll_to(l: &TabsLayout, i: usize) -> f32 {
    let Some(&(x, w)) = l.tabs.get(i) else {
        return l.scroll;
    };
    let (x0, x1) = (x + l.scroll, x + l.scroll + w);
    let width = l.view.1 - l.view.0;
    let left = x0 - l.view.0;
    if left < l.scroll {
        left
    } else if x1 - l.view.0 > l.scroll + width {
        x1 - l.view.0 - width
    } else {
        l.scroll
    }
}

/// Перетаскиваемый таб.
#[derive(Clone, Copy, Debug)]
struct Drag {
    from: usize,
    /// Смещение указателя от левого края таба при захвате.
    grab: f32,
}

/// Состояние набора табов между кадрами.
#[derive(Debug, Default)]
pub(crate) struct TabsMemo {
    key: Vec<(TabId, CompactString, bool)>,
    galleys: Vec<Arc<Galley>>,
    style: (u32, u32),
    pub scroll: f32,
    active: Option<TabId>,
    drag: Option<Drag>,
}

impl TabsMemo {
    /// Подписи — пересчёт только при смене табов, активного, шрифта или масштаба.
    fn refresh(&mut self, ctx: &egui::Context, p: &Panel, fonts: &FontSet, size: f32) {
        let style = (size.to_bits(), ctx.pixels_per_point().to_bits());
        let same = self.style == style
            && self.key.len() == p.tabs.len()
            && self
                .key
                .iter()
                .zip(&p.tabs)
                .enumerate()
                .all(|(i, ((id, title, cur), t))| {
                    *id == t.id && *cur == (i == p.active) && title.as_str() == t.title()
                });
        if same {
            return;
        }
        self.style = style;
        self.key = p
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, CompactString::from(t.title().as_ref()), i == p.active))
            .collect();
        self.galleys = self
            .key
            .iter()
            .map(|(_, title, cur)| {
                let font = fonts.id(if *cur { Font::Strong } else { Font::Ui }, size);
                ctx.fonts_mut(|f| f.layout_no_wrap(title.to_string(), font, egui::Color32::WHITE))
            })
            .collect();
    }
}

/// Входы виджета.
pub(crate) struct TabsCx<'a> {
    pub ui: &'a mut egui::Ui,
    pub tokens: &'a Tokens,
    pub fonts: &'a FontSet,
    pub memo: &'a mut TabsMemo,
}

/// Табы панели `side` в полосе `area` заголовка. Интерактивные зоны — в `interactive`.
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "табы, крестики, перетаскивание и прокрутка — одна полоса"
)]
pub(crate) fn show(
    c: &mut dyn Canvas,
    cx: &mut TabsCx<'_>,
    p: &Panel,
    side: Side,
    area: Rect,
    out: &mut Vec<Command>,
    interactive: &mut Vec<Rect>,
) {
    let tk = cx.tokens;
    let ctx = cx.ui.ctx().clone();
    let body = tk.m(Metric::TypeBody);
    let pad = tk.m(Metric::SpaceM);
    let tab_h = tk.m(Metric::HTab);
    let top = area.bottom() - tab_h;
    cx.memo.refresh(&ctx, p, cx.fonts, body);
    let natural: Vec<f32> = cx
        .memo
        .galleys
        .iter()
        .map(|g| g.size().x + pad + CLOSE_W)
        .collect();
    let mut l = layout(&natural, area.left(), area.right(), cx.memo.scroll);
    // Активный сменился (Ctrl+Tab, новый таб) — прокрутить к нему.
    let active_id = p.tab().id;
    if cx.memo.active != Some(active_id) {
        cx.memo.active = Some(active_id);
        l = layout(&natural, area.left(), area.right(), scroll_to(&l, p.active));
    }
    cx.memo.scroll = l.scroll;
    let sid = side_arg(side);
    let row = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0, top), pos2(x1, area.bottom()));
    let view = row(l.view.0, l.view.1);
    let radius = tk.radius(Metric::RadiusControl);
    let r_top = tk.radius(Metric::RadiusOverlay).nw;
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let dragging = cx.memo.drag;

    // Табы — только в видимой части полосы (стрелки прокрутки не перекрываются).
    c.clip(view);
    for (i, &(x, w)) in l.tabs.iter().enumerate() {
        let rect = row(x, x + w);
        let vis = rect.intersect(view);
        if vis.width() <= 0.0 {
            continue;
        }
        let current = i == p.active;
        let resp = cx.ui.interact(
            vis,
            cx.ui.scope_id().with(("tab", sid, i)),
            Sense::click_and_drag(),
        );
        interactive.push(vis);
        // Перетаскиваемый — под указателем, на своём месте пусто.
        let shown = match (dragging, pointer) {
            (Some(d), Some(at)) if d.from == i => rect.translate(vec2(at.x - d.grab - x, 0.0)),
            _ => rect,
        };
        if current {
            c.rect(
                shown,
                Role::LayerPanel,
                egui::CornerRadius {
                    nw: r_top,
                    ne: r_top,
                    sw: 0,
                    se: 0,
                },
            );
        } else if resp.hovered() {
            c.rect(shown.shrink2(vec2(0.0, 4.0)), Role::FillSubtleHover, radius);
        }
        let g = Arc::clone(&cx.memo.galleys[i]);
        let room = w - pad - CLOSE_W;
        let role = if current {
            Role::TextPrimary
        } else {
            Role::TextSecondary
        };
        let text_at = pos2(shown.left() + pad, shown.center().y);
        if g.size().x <= room {
            c.galley(pos2(text_at.x, text_at.y - g.size().y / 2.0), g, role);
        } else {
            c.text_clipped(
                text_at,
                room,
                &Tx::user(cx.memo.key[i].1.as_str()),
                if current { Font::Strong } else { Font::Ui },
                body,
                role,
            );
        }
        // Крестик — у активного и под мышью (S2 §2.1).
        let close = Rect::from_center_size(
            pos2(shown.right() - CLOSE_W / 2.0 - 2.0, shown.center().y),
            vec2(20.0, 20.0),
        );
        let close_resp = cx.ui.interact(
            close.intersect(view),
            cx.ui.scope_id().with(("tab-close", sid, i)),
            Sense::click(),
        );
        if current || resp.hovered() || close_resp.hovered() {
            if close_resp.hovered() {
                c.rect(close, Role::FillSubtleHover, radius);
            }
            c.icon(close.center(), icons::CANCEL, 10.0, Role::TextSecondary);
        }
        if close_resp.clicked() || resp.middle_clicked() {
            close_tab(p, i, sid, out);
        } else if resp.clicked() && !current {
            out.push(goto(i, sid));
        }
        if resp.drag_started() {
            let grab = pointer.map_or(w / 2.0, |at| at.x - x);
            cx.memo.drag = Some(Drag { from: i, grab });
        }
        if resp.drag_stopped()
            && let (Some(d), Some(at)) = (cx.memo.drag.take(), pointer)
        {
            let to = drop_index(&l, at.x);
            if to != d.from {
                if d.from != p.active {
                    out.push(goto(d.from, sid));
                }
                out.push(
                    Command::new("tab.move")
                        .arg("side", sid)
                        .arg("n", i64::try_from(to + 1).unwrap_or(1)),
                );
            }
        }
    }

    c.clip(area);
    if let Some((back, fwd)) = l.arrows {
        for (b, glyph, delta) in [
            (back, icons::CHEVRON_LEFT, -SCROLL_STEP),
            (fwd, icons::CHEVRON_RIGHT, SCROLL_STEP),
        ] {
            let rect = row(b.0, b.1);
            let resp = cx.ui.interact(
                rect,
                cx.ui.scope_id().with(("tab-scroll", sid, glyph)),
                Sense::click(),
            );
            interactive.push(rect);
            let enabled = if delta < 0.0 {
                l.scroll > 0.0
            } else {
                l.scroll < l.max_scroll
            };
            if resp.hovered() && enabled {
                c.rect(rect.shrink2(vec2(0.0, 4.0)), Role::FillSubtleHover, radius);
            }
            let role = if enabled {
                Role::TextSecondary
            } else {
                Role::TextDisabled
            };
            c.icon(rect.center(), glyph, 12.0, role);
            if resp.clicked() && enabled {
                cx.memo.scroll = (l.scroll + delta).clamp(0.0, l.max_scroll);
                ctx.request_repaint();
            }
        }
        // Колесо над табами — прокрутка полосы.
        let over = pointer.is_some_and(|at| view.contains(at));
        if over {
            let dy = ctx.input(|i| i.smooth_scroll_delta.y + i.smooth_scroll_delta.x);
            if dy != 0.0 {
                cx.memo.scroll = (l.scroll - dy).clamp(0.0, l.max_scroll);
            }
        }
    }

    let plus = row(l.plus.0, l.plus.1);
    let resp = cx.ui.interact(
        plus,
        cx.ui.scope_id().with(("tab-new", sid)),
        Sense::click(),
    );
    interactive.push(plus);
    if resp.hovered() {
        c.rect(plus.shrink2(vec2(0.0, 4.0)), Role::FillSubtleHover, radius);
    }
    c.icon(plus.center(), icons::ADD, 12.0, Role::TextSecondary);
    if resp.clicked() {
        out.push(Command::new("tab.new").arg("side", sid));
    }
}

fn goto(i: usize, side: &'static str) -> Command {
    Command::new("tab.goto")
        .arg("side", side)
        .arg("n", i64::try_from(i + 1).unwrap_or(1))
}

/// Закрыть таб `i`: `tab.close` закрывает активный, поэтому чужой — через `tab.goto`
/// и возврат к прежнему активному (его номер сдвигается, если закрыт левее).
fn close_tab(p: &Panel, i: usize, side: &'static str, out: &mut Vec<Command>) {
    if i == p.active {
        out.push(Command::new("tab.close").arg("side", side));
        return;
    }
    out.push(goto(i, side));
    out.push(Command::new("tab.close").arg("side", side));
    let back = if p.active > i { p.active - 1 } else { p.active };
    out.push(goto(back, side));
}

/// Куда встанет перетаскиваемый таб: позиция по центрам табов.
fn drop_index(l: &TabsLayout, x: f32) -> usize {
    l.tabs
        .iter()
        .filter(|(tx, w)| x > tx + w / 2.0)
        .count()
        .min(l.tabs.len().saturating_sub(1))
}

#[cfg(test)]
#[path = "tabs_tests.rs"]
mod tests;
