//! Панель файлов на GPUI (G-04, S2 §2.1–2.2): карточка с путём, шапкой колонок,
//! строками 24 px и подвалом. Рисуется только видимая страница от `cursor.top` —
//! её держит Core по `panel.viewport` (S0 v3 №3).

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent,
    ScrollWheelEvent, SharedString, div, px,
};
use urfin_core::{AppState, Row, Side, Tab, ViewMode};
use urfin_i18n::FluentArgs;
use urfin_vfs::icons::icon_class;

use super::CoreHost;
use super::theme::Theme;
use crate::fmt_cell::{Cell, ColKind, cell_text};
use crate::tokens::{Metric, Role};

/// Ширина колонок Full: размер и дата (S2 §2.1).
const W_SIZE: f32 = 96.0;
const W_MTIME: f32 = 128.0;
/// Минимальная ширина колонки Brief.
const W_BRIEF: f32 = 220.0;
/// Шрифт иконок ОС (S2 §1.6).
const ICON_FONT: &str = "Segoe Fluent Icons";
/// `..` — стрелка вверх.
const GLYPH_UP: char = '\u{E74A}';

/// Геометрия панели для `panel.viewport`: строк в колонке и колонок.
pub(super) fn grid(theme: &Theme, mode: ViewMode, w: f32, h: f32) -> (u32, u32) {
    let head = if matches!(mode, ViewMode::Full) {
        theme.m(Metric::HColHeader)
    } else {
        0.0
    };
    let list_h = h - theme.m(Metric::HBreadcrumb) - head - theme.m(Metric::HPanelFooter) - 2.0;
    #[allow(clippy::cast_sign_loss, reason = "не меньше 1.0")]
    let rows = (list_h / theme.m(Metric::HRow)).floor().max(1.0) as u32;
    let cols = match mode {
        ViewMode::Brief => (w / W_BRIEF).floor().max(1.0),
        ViewMode::Medium => 2.0,
        _ => 1.0,
    };
    #[allow(clippy::cast_sign_loss, reason = "не меньше 1.0")]
    (rows, cols as u32)
}

/// Карточка панели.
pub(super) fn panel(
    s: &AppState,
    side: Side,
    theme: &Theme,
    rows: u32,
    cols: u32,
    field: Option<&super::field::Field>,
    cx: &Context<CoreHost>,
) -> Div {
    let active = s.active == side;
    let t = s.panel(side).tab();
    let border = if active {
        Role::AccentFill
    } else {
        Role::StrokeCard
    };
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(theme.m(Metric::RadiusOverlay)))
        .border_1()
        .border_color(theme.c(border))
        .bg(theme.c(Role::LayerPanel))
        .child(path_bar(t, theme))
        .when(matches!(t.view_mode, ViewMode::Full), |d| {
            d.child(col_header(side, theme, cx))
        })
        .child(list(t, side, active, theme, rows, cols, field, cx))
        .child(footer(s, t, side, active, theme))
}

fn path_bar(t: &Tab, theme: &Theme) -> Div {
    let path: SharedString = t.loc.path.display().to_string().into();
    div()
        .h(px(theme.m(Metric::HBreadcrumb)))
        .flex_none()
        .flex()
        .items_center()
        .px(px(theme.m(Metric::PadRowX)))
        .border_b_1()
        .border_color(theme.c(Role::StrokeDivider))
        .text_color(theme.c(Role::TextPrimary))
        .child(path)
}

fn col_header(side: Side, theme: &Theme, cx: &Context<CoreHost>) -> Div {
    let i18n = urfin_i18n::current();
    // Клик по колонке — сортировка по ней (повтор — обратная, S2 §2.1).
    let cell = |key: &str, sort: &'static str, w: Option<f32>| {
        let d = div()
            .id(SharedString::from(format!("col-{sort}-{side:?}")))
            .px_1()
            .child(SharedString::from(i18n.get(key)))
            .on_click(cx.listener(move |h, _: &ClickEvent, w, cx| {
                h.run_all(
                    vec![
                        urfin_core::Command::new("panel.focus")
                            .arg("side", super::chrome::side_name(side)),
                        urfin_core::Command::new("panel.sort").arg("key", sort),
                    ],
                    w,
                    cx,
                );
            }));
        match w {
            Some(w) => d.w(px(w)).flex_none().flex().justify_end(),
            None => d.flex_1().min_w_0(),
        }
    };
    div()
        .h(px(theme.m(Metric::HColHeader)))
        .flex_none()
        .flex()
        .items_center()
        .px(px(theme.m(Metric::PadRowX)))
        .text_size(px(theme.m(Metric::TypeCaption)))
        .text_color(theme.c(Role::TextSecondary))
        .border_b_1()
        .border_color(theme.c(Role::StrokeDivider))
        .child(div().w(px(theme.m(Metric::WIcon) + 8.0)).flex_none())
        .child(cell("ui-col-name", "name", None))
        .child(cell("ui-col-size", "size", Some(W_SIZE)))
        .child(cell("ui-col-mtime", "mtime", Some(W_MTIME)))
}

/// Видимая страница: Full/Wide — одна колонка, Brief/Medium — колонки сверху вниз.
#[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
fn list(
    t: &Tab,
    side: Side,
    active: bool,
    theme: &Theme,
    rows: u32,
    cols: u32,
    field: Option<&super::field::Field>,
    cx: &Context<CoreHost>,
) -> Div {
    let total = t.rows();
    let top = t.cursor.top;
    let rows = rows.max(1);
    let mut grid = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_row()
        .overflow_hidden()
        .on_scroll_wheel(cx.listener(move |h, ev: &ScrollWheelEvent, w, cx| {
            h.wheel(side, ev, w, cx);
        }));
    for c in 0..cols.max(1) {
        let mut col = div().flex_1().min_w_0().flex().flex_col();
        for r in 0..rows {
            let i = top + c * rows + r;
            if i >= total {
                break;
            }
            col = col.child(row(t, side, i, active, theme, field, cx));
        }
        grid = grid.child(col);
    }
    grid
}

#[allow(clippy::too_many_lines, reason = "строка слева направо")]
#[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
fn row(
    t: &Tab,
    side: Side,
    i: u32,
    active: bool,
    theme: &Theme,
    field: Option<&super::field::Field>,
    cx: &Context<CoreHost>,
) -> AnyElement {
    let cursor = i == t.cursor.row;
    let full = matches!(t.view_mode, ViewMode::Full | ViewMode::Wide);
    // Имя строки для `panel.cursor.to` (у `..` — нет).
    let target = match t.row(i) {
        Some(Row::Entry { entry, .. }) => Some(entry.name.to_string()),
        _ => None,
    };
    let right = target.clone();
    let mut d = div()
        .id(SharedString::from(format!("row-{side:?}-{i}")))
        .on_click(cx.listener(move |h, ev: &ClickEvent, w, cx| {
            h.row_click(
                side,
                target.as_deref(),
                ev.click_count(),
                ev.modifiers().control,
                w,
                cx,
            );
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |h, _: &MouseDownEvent, w, cx| {
                h.row_right_click(side, right.as_deref(), w, cx);
            }),
        )
        .h(px(theme.m(Metric::HRow)))
        .flex_none()
        .flex()
        .items_center()
        .mx_1()
        .px(px(theme.m(Metric::PadRowX) - 4.0))
        .rounded(px(theme.m(Metric::RadiusControl)));
    let (glyph, name, marked, color) = match t.row(i) {
        Some(Row::Parent) => (GLYPH_UP, "..".to_owned(), false, None),
        Some(Row::Entry { idx, entry }) => {
            let g = crate::icons::glyph(icon_class(&entry.name, entry.kind, entry.attrs));
            let group = theme.hl.classify(entry);
            let color = theme.hl.color(group, &theme.tokens);
            (g, entry.name.to_string(), t.selection.contains(idx), color)
        }
        None => return div().into_any_element(),
    };
    if cursor {
        d = d.bg(theme.c(if active {
            Role::FillCursor
        } else {
            Role::FillCursorPassive
        }));
    } else if marked {
        d = d.bg(theme.c(Role::AccentSubtle));
    }
    let text = if marked {
        theme.c(Role::AccentText)
    } else {
        color.map_or_else(|| theme.c(Role::TextPrimary), super::theme::hsla)
    };
    d = d
        .text_color(text)
        .child(
            div()
                .w(px(theme.m(Metric::WIcon) + 8.0))
                .flex_none()
                .font_family(ICON_FONT)
                .text_size(px(theme.m(Metric::WIcon)))
                .text_color(theme.c(if marked {
                    Role::AccentText
                } else {
                    Role::TextSecondary
                }))
                .child(SharedString::from(glyph.to_string())),
        )
        .child(
            match super::field::rename_input(field.filter(|_| cursor), t) {
                // Переименование на месте (S2-16): поле вместо имени.
                Some(input) => div().flex_1().min_w_0().child(input).into_any_element(),
                None => div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when(marked, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .child(SharedString::from(name))
                    .into_any_element(),
            },
        );
    if full && let Some(Row::Entry { entry, .. }) = t.row(i) {
        let mut cell = Cell::new();
        let mut col = |kind: ColKind, w: f32| {
            cell.clear();
            cell_text(kind, entry, &theme.fmt, &mut cell);
            div()
                .w(px(w))
                .flex_none()
                .flex()
                .justify_end()
                .text_color(theme.c(Role::TextSecondary))
                .child(SharedString::from(cell.as_str().to_owned()))
        };
        d = d.child(col(ColKind::Size, W_SIZE));
        if matches!(t.view_mode, ViewMode::Full) {
            d = d.child(col(ColKind::Mtime, W_MTIME));
        }
    }
    d.into_any_element()
}

/// Подвал: итоги или выделение; при быстром поиске — маска и число совпадений,
/// при незаконченном аккорде — его начало.
fn footer(s: &AppState, t: &Tab, side: Side, active: bool, theme: &Theme) -> Div {
    let i18n = urfin_i18n::current();
    let mut args = FluentArgs::new();
    let keys = s.ext.get::<urfin_keymap::KeymapView>();
    if let Some(q) = keys.and_then(|k| k.qs.as_ref()).filter(|q| q.side == side) {
        args.set("count", u64::from(q.matches));
        let miss = q.miss_at.is_some();
        return div()
            .h(px(theme.m(Metric::HPanelFooter)))
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px(px(theme.m(Metric::PadRowX)))
            .bg(theme.c(Role::FillControlInput))
            .border_b_2()
            .border_color(theme.c(if miss {
                Role::CriticalFg
            } else {
                Role::AccentFill
            }))
            .child(
                div()
                    .font_family("Segoe Fluent Icons")
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from("\u{E721}")),
            )
            .child(div().flex_1().child(SharedString::from(q.mask.to_string())))
            .child(
                div()
                    .text_size(px(theme.m(Metric::TypeCaption)))
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(i18n.fmt("ui-qs-matches", &args))),
            );
    }
    if active && let Some(chord) = keys.and_then(|k| k.pending.as_deref()) {
        args.set("chord", chord.to_owned());
        return div()
            .h(px(theme.m(Metric::HPanelFooter)))
            .flex_none()
            .flex()
            .items_center()
            .px(px(theme.m(Metric::PadRowX)))
            .border_t_1()
            .border_color(theme.c(Role::StrokeDivider))
            .text_size(px(theme.m(Metric::TypeCaption)))
            .child(SharedString::from(i18n.fmt("ui-chord-pending", &args)));
    }
    let text = if t.selection.is_empty() {
        let st = &t.listing.stats;
        args.set("count", u64::from(st.files + st.dirs));
        args.set("bytes", urfin_i18n::fmt::bytes_with(&i18n, st.bytes));
        i18n.fmt("ui-footer-items", &args)
    } else {
        args.set("count", u64::from(t.selection.len()));
        args.set(
            "bytes",
            urfin_i18n::fmt::bytes_with(&i18n, t.selection.bytes()),
        );
        i18n.fmt("ui-footer-selected", &args)
    };
    div()
        .h(px(theme.m(Metric::HPanelFooter)))
        .flex_none()
        .flex()
        .items_center()
        .px(px(theme.m(Metric::PadRowX)))
        .border_t_1()
        .border_color(theme.c(Role::StrokeDivider))
        .text_size(px(theme.m(Metric::TypeCaption)))
        .text_color(theme.c(Role::TextSecondary))
        .child(SharedString::from(text))
}
