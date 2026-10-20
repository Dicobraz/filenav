//! Панель файлов на GPUI (G-04, S2 §2.1–2.2): карточка с путём, шапкой колонок,
//! строками 24 px и подвалом. Рисуется только видимая страница от `cursor.top` —
//! её держит Core по `panel.viewport` (S0 v3 №3).

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, FontWeight, SharedString, div, px};
use urfin_core::{AppState, Row, Side, Tab, ViewMode};
use urfin_i18n::FluentArgs;
use urfin_vfs::icons::icon_class;

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
pub(super) fn panel(s: &AppState, side: Side, theme: &Theme, rows: u32, cols: u32) -> Div {
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
            d.child(col_header(theme))
        })
        .child(list(t, active, theme, rows, cols))
        .child(footer(t, theme))
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

fn col_header(theme: &Theme) -> Div {
    let i18n = urfin_i18n::current();
    let cell = |key: &str, w: Option<f32>| {
        let d = div().px_1().child(SharedString::from(i18n.get(key)));
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
        .child(cell("ui-col-name", None))
        .child(cell("ui-col-size", Some(W_SIZE)))
        .child(cell("ui-col-mtime", Some(W_MTIME)))
}

/// Видимая страница: Full/Wide — одна колонка, Brief/Medium — колонки сверху вниз.
fn list(t: &Tab, active: bool, theme: &Theme, rows: u32, cols: u32) -> Div {
    let total = t.rows();
    let top = t.cursor.top;
    let rows = rows.max(1);
    let mut grid = div().flex_1().min_h_0().flex().flex_row().overflow_hidden();
    for c in 0..cols.max(1) {
        let mut col = div().flex_1().min_w_0().flex().flex_col();
        for r in 0..rows {
            let i = top + c * rows + r;
            if i >= total {
                break;
            }
            col = col.child(row(t, i, active, theme));
        }
        grid = grid.child(col);
    }
    grid
}

fn row(t: &Tab, i: u32, active: bool, theme: &Theme) -> AnyElement {
    let cursor = i == t.cursor.row;
    let full = matches!(t.view_mode, ViewMode::Full | ViewMode::Wide);
    let mut d = div()
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
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .when(marked, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(SharedString::from(name)),
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

fn footer(t: &Tab, theme: &Theme) -> Div {
    let i18n = urfin_i18n::current();
    let mut args = FluentArgs::new();
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
