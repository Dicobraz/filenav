//! Режимы панели на GPUI (S2 §2.4–2.5, D-03): дерево, «Сведения» и быстрый просмотр —
//! по `docs/design/screens/screen-tree.png`, `-info.png`, `-quickview.png`. Данные
//! готовит handler (`views::sync`): дерево каталогов, том, содержимое файла под курсором
//! соседней панели; здесь — только отрисовка видимой страницы.

use std::fmt::Write as _;

use gpui_kit::prelude::*;
use gpui_kit::{Div, SharedString, div, px, relative};
use urfin_core::{AppState, PanelMode, Row, Side};
use urfin_i18n::FluentArgs;
use urfin_vfs::volume::Media;

use super::theme::Theme;
use crate::UiState;
use crate::config::IconMode;
use crate::tokens::{Metric, Role};
use crate::views::{Preview, PreviewState, TreeState};

const ICON_FONT: &str = "Segoe Fluent Icons";
const GLYPH_FOLDER: &str = "\u{E8B7}";
const GLYPH_FILE: &str = "\u{E7C3}";
/// Байт в строке hex-дампа.
const HEX_ROW: usize = 16;
/// Отступ уровня дерева и направляющая в нём (D-03).
const TREE_INDENT: f32 = 16.0;
const TREE_GUIDE_X: f32 = 7.0;
/// Номера строк быстрого просмотра и высота строки.
const QV_GUTTER: f32 = 40.0;
const QV_ROW: f32 = 20.0;
const SMALL: f32 = 12.5;

/// Тело панели в режиме `mode` (не `Files`): шапка как путь панели и содержимое.
pub(super) fn body(s: &AppState, side: Side, theme: &Theme, rows: u32) -> Div {
    let st = s.ext.get::<UiState>();
    let i = usize::from(side == Side::Right);
    let i18n = urfin_i18n::current();
    let active = s.active == side;
    let icons = st.map_or(IconMode::Mono, |u| u.icons);
    let (title, right, content) = match s.panel(side).tab().mode {
        PanelMode::Tree => {
            let t = st.and_then(|u| u.tree[i].as_ref());
            let root = t
                .and_then(|t| t.nodes.first())
                .map_or_else(String::new, |n| n.loc.path.display().to_string());
            (
                root,
                i18n.get("ui-tree-label"),
                t.map_or_else(div, |t| tree(t, theme, rows, active, icons)),
            )
        }
        PanelMode::Info => (
            i18n.get("ui-info-title"),
            String::new(),
            info(s, side, theme),
        ),
        PanelMode::QView | PanelMode::Files => (
            i18n.get("ui-qv-title"),
            preview_kind(s, side),
            st.and_then(|u| u.preview[i].as_ref())
                .map_or_else(div, |p| preview(p, theme, rows)),
        ),
    };
    let head = div()
        .h(px(theme.m(Metric::HBreadcrumb)))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(theme.m(Metric::PadRowX)))
        .text_size(px(SMALL))
        .border_b_1()
        .border_color(theme.c(Role::StrokeDivider))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(theme.c(if active {
                    Role::TextPrimary
                } else {
                    Role::TextSecondary
                }))
                .when(active, |d| {
                    d.border_b_2()
                        .border_color(theme.c(Role::AccentFill))
                        .pb(px(3.0))
                        .mt(px(5.0))
                })
                .child(SharedString::from(title)),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.c(Role::TextSecondary))
                .child(SharedString::from(right)),
        );
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(head)
        .child(content)
}

/// Дерево: видимые узлы страницей вокруг курсора; отступ 16 на уровень с направляющими
/// `border`, ▸/▾, значок папки; курсор — заливка во всю строку (№66).
fn tree(t: &TreeState, theme: &Theme, rows: u32, active: bool, icons: IconMode) -> Div {
    let vis = t.visible();
    let cur = vis
        .iter()
        .position(|&n| t.nodes[n].loc == t.cursor)
        .unwrap_or(0);
    let page = usize::try_from(rows.max(1)).unwrap_or(1);
    let top = cur
        .saturating_sub(page / 2)
        .min(vis.len().saturating_sub(page));
    let row_h = theme.m(Metric::HRow);
    let mut list = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .pt(px(4.0))
        .overflow_hidden();
    for (k, &n) in vis.iter().enumerate().skip(top).take(page) {
        let node = &t.nodes[n];
        let on = k == cur;
        let fg = |r: Role| theme.c(if on && active { Role::CursorText } else { r });
        let chevron = if node.empty {
            ""
        } else if node.expanded {
            "▾"
        } else {
            "▸"
        };
        // Направляющие: по линии 1 px в каждом уровне отступа.
        let mut indent = div()
            .flex_none()
            .h_full()
            .w(px(f32::from(node.depth) * TREE_INDENT))
            .relative();
        for d in 0..node.depth {
            indent = indent.child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(f32::from(d) * TREE_INDENT + TREE_GUIDE_X))
                    .w(px(1.0))
                    .h_full()
                    .bg(theme.c(Role::StrokeDivider)),
            );
        }
        let mut row = div()
            .h(px(row_h))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(theme.m(Metric::PadRowX)))
            .child(indent)
            .child(
                div()
                    .w(px(14.0))
                    .flex_none()
                    .text_center()
                    .text_size(px(10.0))
                    .text_color(fg(Role::TextSecondary))
                    .child(SharedString::from(chevron)),
            );
        if icons != IconMode::None {
            row = row.child(
                div()
                    .w(px(14.0))
                    .flex_none()
                    .font_family(ICON_FONT)
                    .text_size(px(14.0))
                    .text_color(fg(if icons == IconMode::Color {
                        Role::AccentText
                    } else {
                        Role::TextSecondary
                    }))
                    .child(SharedString::from(GLYPH_FOLDER)),
            );
        }
        row = row.child(
            div()
                .min_w_0()
                .ml(px(4.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(fg(Role::TextPrimary))
                .when(node.depth == 0 || on, |d| {
                    d.font_weight(gpui_kit::FontWeight::MEDIUM)
                })
                .child(SharedString::from(node.name.to_string())),
        );
        if on {
            row = row.bg(theme.c(if active {
                Role::FillCursor
            } else {
                Role::FillCursorPassive
            }));
        }
        list = list.child(row);
    }
    list
}

/// «Сведения» (D-03): значок 48, имя, тип и точный размер объекта под курсором соседней
/// панели; таблица 120 / остаток; черта; том с полосой заполнения и сводка папки.
#[allow(clippy::too_many_lines, reason = "блоки сведений сверху вниз")]
#[allow(
    clippy::many_single_char_names,
    reason = "s, t, a — состояние, вкладка, аргументы"
)]
fn info(s: &AppState, side: Side, theme: &Theme) -> Div {
    let i18n = urfin_i18n::current();
    let t = s.panel(side.other()).tab();
    let bytes = |n: u64| urfin_i18n::fmt::bytes_with(&i18n, n);
    let mut pairs: Vec<(String, String)> =
        vec![(i18n.get("ui-info-path"), t.loc.path.display().to_string())];
    let (name, kind, glyph) = match t.current() {
        Some(Row::Entry { entry, .. }) => {
            let dir = entry.kind.is_dir_like();
            let kind = if dir {
                i18n.get("ui-info-kind-dir")
            } else {
                format!(
                    "{} · {}",
                    i18n.get("ui-info-kind-file"),
                    urfin_i18n::fmt::bytes_exact_with(&i18n, entry.size)
                )
            };
            let mut cell = crate::fmt_cell::Cell::default();
            crate::fmt_cell::cell_text(
                crate::fmt_cell::ColKind::Mtime,
                entry,
                &theme.fmt,
                &mut cell,
            );
            pairs.push((i18n.get("ui-info-mtime"), cell.as_str().trim().to_owned()));
            cell.clear();
            crate::fmt_cell::cell_text(
                crate::fmt_cell::ColKind::Attrs,
                entry,
                &theme.fmt,
                &mut cell,
            );
            if !cell.as_str().trim().is_empty() {
                pairs.push((i18n.get("ui-info-attrs"), cell.as_str().trim().to_owned()));
            }
            (
                entry.name.to_string(),
                kind,
                if dir { GLYPH_FOLDER } else { GLYPH_FILE },
            )
        }
        _ => (
            t.loc.path.file_name().map_or_else(
                || t.loc.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            i18n.get("ui-info-kind-dir"),
            GLYPH_FOLDER,
        ),
    };
    if !t.selection.is_empty() {
        pairs.push((
            i18n.get("ui-info-selected"),
            format!("{} · {}", t.selection.len(), bytes(t.selection.bytes())),
        ));
    }
    let mut grid = div().flex().flex_col().gap(px(7.0)).text_size(px(SMALL));
    for (k, v) in pairs {
        grid = grid.child(
            div()
                .flex()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(120.0))
                        .flex_none()
                        .text_color(theme.c(Role::TextSecondary))
                        .child(SharedString::from(k)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(v)),
                ),
        );
    }
    let mut col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(18.0))
        .pt(px(20.0))
        .px(px(20.0))
        .pb(px(16.0))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .gap(px(14.0))
                .items_center()
                .child(
                    div()
                        .size(px(48.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.0))
                        .bg(theme.c(Role::Header))
                        .border_1()
                        .border_color(theme.c(Role::StrokeDivider))
                        .font_family(ICON_FONT)
                        .text_size(px(24.0))
                        .text_color(theme.c(Role::AccentText))
                        .child(SharedString::from(glyph)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(SharedString::from(name)),
                        )
                        .child(
                            div()
                                .text_color(theme.c(Role::TextSecondary))
                                .child(SharedString::from(kind)),
                        ),
                ),
        )
        .child(grid)
        .child(div().h(px(1.0)).bg(theme.c(Role::StrokeDivider)));
    let i = usize::from(side == Side::Right);
    let vol = s
        .ext
        .get::<UiState>()
        .and_then(|u| u.info[i].as_ref())
        .and_then(|inf| inf.volume.as_ref());
    let mut a = FluentArgs::new();
    a.set("dirs", u64::from(t.listing.stats.dirs));
    a.set("files", u64::from(t.listing.stats.files));
    a.set("bytes", bytes(t.listing.stats.bytes));
    a.set("path", t.loc.path.display().to_string());
    let summary = i18n.fmt("ui-info-summary", &a);
    let mut volume = div().flex().flex_col().gap(px(8.0)).text_size(px(SMALL));
    if let Some(Ok(v)) = vol {
        let media = match v.media {
            Media::Net => format!(" · {}", i18n.get("ui-info-media-net")),
            Media::Usb => format!(" · {}", i18n.get("ui-info-media-usb")),
            _ => String::new(),
        };
        let root = v.root.display().to_string();
        let root = root.trim_end_matches(['\\', '/']).to_owned();
        let label = if v.label.is_empty() {
            i18n.get("ui-info-volume-local")
        } else {
            v.label.clone()
        };
        let mut line =
            div().flex().justify_between().child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(SharedString::from(root)),
                    )
                    .child(div().text_color(theme.c(Role::TextSecondary)).child(
                        SharedString::from(format!("{label} · {}{media}", v.fs_name)),
                    )),
            );
        let mut used = None;
        if let Some(sp) = v.space {
            let mut fa = FluentArgs::new();
            fa.set("free", bytes(sp.avail));
            fa.set("total", bytes(sp.total));
            line = line.child(
                div()
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(i18n.fmt("ui-info-free-of", &fa))),
            );
            #[allow(clippy::cast_precision_loss, reason = "доля для полосы")]
            let k = if sp.total > 0 {
                1.0 - sp.avail as f32 / sp.total as f32
            } else {
                0.0
            };
            used = Some(k.clamp(0.0, 1.0));
        }
        volume = volume.child(line);
        if let Some(k) = used {
            volume = volume.child(
                div()
                    .h(px(6.0))
                    .rounded(px(3.0))
                    .bg(theme.c(Role::StrokeDivider))
                    .overflow_hidden()
                    .child(div().h_full().w(relative(k)).bg(theme.c(if k > 0.75 {
                        Role::Modified
                    } else {
                        Role::AccentFill
                    }))),
            );
        }
    }
    volume = volume.child(
        div()
            .text_color(theme.c(Role::TextSecondary))
            .child(SharedString::from(summary)),
    );
    col = col.child(volume);
    col
}

/// Тип файла под курсором соседней панели для шапки просмотра (`TOML`).
fn preview_kind(s: &AppState, side: Side) -> String {
    match s.panel(side.other()).tab().current() {
        Some(Row::Entry { entry, .. }) => std::path::Path::new(entry.name.as_str())
            .extension()
            .map(|e| e.to_string_lossy().to_uppercase())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Быстрый просмотр: номера строк 40, строки 20, моно 12,5; hex — первая страница;
/// каталог и ошибка — строкой.
fn preview(p: &PreviewState, theme: &Theme, rows: u32) -> Div {
    let row_h = QV_ROW;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "строк на страницу"
    )]
    let page = (f32::from(u16::try_from(rows.max(1)).unwrap_or(u16::MAX)) * theme.m(Metric::HRow)
        / row_h) as usize;
    let mut col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .py(px(6.0))
        .font_family("Cascadia Mono")
        .text_size(px(SMALL));
    let gutter = |n: String| {
        div()
            .w(px(QV_GUTTER))
            .flex_none()
            .pr(px(12.0))
            .text_right()
            .text_color(theme.c(Role::TextSecondary))
            .child(SharedString::from(n))
    };
    let row = || {
        div()
            .h(px(row_h))
            .flex_none()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .overflow_hidden()
    };
    match &p.content {
        None => col = col.child(row().child(gutter(String::new())).child("…")),
        Some(Preview::Text { text, .. }) => {
            for (n, l) in text.lines().take(page).enumerate() {
                col = col.child(
                    row()
                        .child(gutter((n + 1).to_string()))
                        .child(SharedString::from(l.replace('\t', "    "))),
                );
            }
        }
        Some(Preview::Hex(bytes)) => {
            for (n, chunk) in bytes.chunks(HEX_ROW).take(page).enumerate() {
                let hex = chunk.iter().fold(String::new(), |mut acc, b| {
                    let _ = write!(acc, "{b:02X} ");
                    acc
                });
                let ascii: String = chunk
                    .iter()
                    .map(|&b| {
                        if b.is_ascii_graphic() {
                            char::from(b)
                        } else {
                            '.'
                        }
                    })
                    .collect();
                col = col.child(row().px(px(12.0)).child(SharedString::from(format!(
                    "{:08X}  {hex:<48} {ascii}",
                    n * HEX_ROW
                ))));
            }
        }
        Some(Preview::Dir | Preview::Nothing) => {}
        Some(Preview::Error(e)) => {
            col = col.child(
                row()
                    .px(px(12.0))
                    .text_color(theme.c(Role::CriticalFg))
                    .child(SharedString::from(e.to_string())),
            );
        }
    }
    col
}
