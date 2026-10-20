//! Режимы панели на GPUI (S2 §2.4–2.5, G-05): дерево, «Сведения» и быстрый просмотр.
//! Данные готовит handler (`views::sync`): дерево каталогов, том, содержимое файла под
//! курсором соседней панели; здесь — только отрисовка видимой страницы.

use std::fmt::Write as _;

use gpui_kit::prelude::*;
use gpui_kit::{Div, SharedString, div, px, relative};
use urfin_core::{AppState, PanelMode, Row, Side};
use urfin_i18n::FluentArgs;
use urfin_vfs::volume::Media;

use super::theme::Theme;
use crate::UiState;
use crate::tokens::{Metric, Role};
use crate::views::{Preview, PreviewState, TreeState};

const ICON_FONT: &str = "Segoe Fluent Icons";
const GLYPH_FOLDER: &str = "\u{E8B7}";
const GLYPH_OPEN: &str = "\u{E70D}";
const GLYPH_CLOSED: &str = "\u{E76C}";
/// Байт в строке hex-дампа.
const HEX_ROW: usize = 16;

/// Тело карточки в режиме `mode` (не `Files`).
pub(super) fn body(s: &AppState, side: Side, theme: &Theme, rows: u32) -> Div {
    let st = s.ext.get::<UiState>();
    let i = usize::from(side == Side::Right);
    let mode = s.panel(side).tab().mode;
    let (title, content) = match mode {
        PanelMode::Tree => (
            urfin_i18n::current().get("ui-tree-title"),
            st.and_then(|u| u.tree[i].as_ref())
                .map_or_else(div, |t| tree(t, theme, rows)),
        ),
        PanelMode::Info => (
            urfin_i18n::current().get("ui-info-title"),
            info(s, side, theme),
        ),
        PanelMode::QView | PanelMode::Files => (
            preview_title(s, side),
            st.and_then(|u| u.preview[i].as_ref())
                .map_or_else(div, |p| preview(p, theme, rows)),
        ),
    };
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(theme.m(Metric::HBreadcrumb)))
                .flex_none()
                .flex()
                .items_center()
                .px(px(theme.m(Metric::PadRowX)))
                .border_b_1()
                .border_color(theme.c(Role::StrokeDivider))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(SharedString::from(title)),
        )
        .child(content)
}

fn line(theme: &Theme) -> Div {
    div()
        .h(px(theme.m(Metric::HRow)))
        .flex_none()
        .flex()
        .items_center()
        .px(px(theme.m(Metric::PadRowX)))
}

/// Дерево: видимые узлы страницей вокруг курсора, отступ по глубине.
fn tree(t: &TreeState, theme: &Theme, rows: u32) -> Div {
    let vis = t.visible();
    let cur = vis
        .iter()
        .position(|&n| t.nodes[n].loc == t.cursor)
        .unwrap_or(0);
    let page = usize::try_from(rows.max(1)).unwrap_or(1);
    let top = cur
        .saturating_sub(page / 2)
        .min(vis.len().saturating_sub(page));
    let mut list = div().flex_1().min_h_0().flex().flex_col().overflow_hidden();
    for (k, &n) in vis.iter().enumerate().skip(top).take(page) {
        let node = &t.nodes[n];
        let chevron = if node.empty {
            ""
        } else if node.expanded {
            GLYPH_OPEN
        } else {
            GLYPH_CLOSED
        };
        let mut row = line(theme)
            .mx_1()
            .rounded(px(theme.m(Metric::RadiusControl)))
            .pl(px(theme.m(Metric::PadRowX) + f32::from(node.depth) * 16.0))
            .gap_1()
            .child(
                div()
                    .w(px(14.0))
                    .font_family(ICON_FONT)
                    .text_size(px(10.0))
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(chevron)),
            )
            .child(
                div()
                    .font_family(ICON_FONT)
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(GLYPH_FOLDER)),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(node.name.to_string())),
            );
        if k == cur {
            row = row.bg(theme.c(Role::FillCursor));
        }
        list = list.child(row);
    }
    list
}

fn pair(theme: &Theme, key: &str, value: String) -> Div {
    let i18n = urfin_i18n::current();
    line(theme)
        .gap_3()
        .child(
            div()
                .w(px(140.0))
                .flex_none()
                .text_color(theme.c(Role::TextSecondary))
                .child(SharedString::from(i18n.get(key))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(value)),
        )
}

/// «Сведения» о соседней панели: путь, содержимое, выделение, объект под курсором, том.
fn info(s: &AppState, side: Side, theme: &Theme) -> Div {
    let i18n = urfin_i18n::current();
    let t = s.panel(side.other()).tab();
    let bytes = |n: u64| urfin_i18n::fmt::bytes_with(&i18n, n);
    let mut a = FluentArgs::new();
    a.set("dirs", u64::from(t.listing.stats.dirs));
    a.set("files", u64::from(t.listing.stats.files));
    let mut col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .py_1()
        .overflow_hidden()
        .child(pair(
            theme,
            "ui-info-path",
            t.loc.path.display().to_string(),
        ))
        .child(pair(
            theme,
            "ui-info-content",
            i18n.fmt("ui-info-content-value", &a),
        ))
        .child(pair(theme, "ui-info-bytes", bytes(t.listing.stats.bytes)));
    if !t.selection.is_empty() {
        col = col.child(pair(
            theme,
            "ui-info-selected",
            format!("{} · {}", t.selection.len(), bytes(t.selection.bytes())),
        ));
    }
    if let Some(Row::Entry { entry, .. }) = t.current() {
        col = col.child(pair(theme, "ui-info-cursor", entry.name.to_string()));
        if entry.kind == urfin_core::EntryKind::File {
            col = col.child(pair(
                theme,
                "ui-info-size",
                urfin_i18n::fmt::bytes_exact_with(&i18n, entry.size),
            ));
        }
    }
    let i = usize::from(side == Side::Right);
    let vol = s
        .ext
        .get::<UiState>()
        .and_then(|u| u.info[i].as_ref())
        .and_then(|inf| inf.volume.as_ref());
    if let Some(Ok(v)) = vol {
        let media = match v.media {
            Media::Net => format!(" · {}", i18n.get("ui-info-media-net")),
            Media::Usb => format!(" · {}", i18n.get("ui-info-media-usb")),
            _ => String::new(),
        };
        col = col
            .child(pair(
                theme,
                "ui-info-volume",
                format!("{}{media}", volume_name(v, &i18n)),
            ))
            .child(pair(theme, "ui-info-fs", v.fs_name.clone()));
        if let Some(sp) = v.space {
            #[allow(clippy::cast_precision_loss, reason = "доля для полосы")]
            let used = if sp.total > 0 {
                1.0 - sp.avail as f64 / sp.total as f64
            } else {
                0.0
            };
            let mut fa = FluentArgs::new();
            fa.set("free", bytes(sp.avail));
            fa.set("total", bytes(sp.total));
            fa.set("pct", format!("{:.0}", (1.0 - used) * 100.0));
            #[allow(clippy::cast_possible_truncation, reason = "доля 0…1")]
            let used = used as f32;
            col = col
                .child(pair(
                    theme,
                    "ui-info-free",
                    i18n.fmt("ui-info-free-value", &fa),
                ))
                .child(
                    div().px(px(theme.m(Metric::PadRowX))).child(
                        div()
                            .h(px(6.0))
                            .rounded(px(3.0))
                            .bg(theme.c(Role::FillControl))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(used))
                                    .rounded(px(3.0))
                                    .bg(theme.c(Role::AccentFill)),
                            ),
                    ),
                );
        }
    }
    col
}

fn preview_title(s: &AppState, side: Side) -> String {
    match s.panel(side.other()).tab().current() {
        Some(Row::Entry { entry, .. }) => entry.name.to_string(),
        _ => String::new(),
    }
}

/// Быстрый просмотр: текст или hex первой страницы, каталог и ошибка — строкой.
fn preview(p: &PreviewState, theme: &Theme, rows: u32) -> Div {
    let page = usize::try_from(rows.max(1)).unwrap_or(1);
    let mut col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .py_1()
        .font_family("Cascadia Mono")
        .text_size(px(theme.m(Metric::TypeMono)));
    let note = |text: String| {
        line(theme)
            .text_color(theme.c(Role::TextSecondary))
            .child(SharedString::from(text))
    };
    match &p.content {
        None => col = col.child(note("…".to_owned())),
        Some(Preview::Text { text, encoding, .. }) => {
            col = col.child(note(encoding.name().to_owned()));
            for l in text.lines().take(page.saturating_sub(1)) {
                col = col.child(
                    line(theme)
                        .whitespace_nowrap()
                        .overflow_hidden()
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
                col = col.child(line(theme).whitespace_nowrap().child(SharedString::from(
                    format!("{:08X}  {hex:<48} {ascii}", n * HEX_ROW),
                )));
            }
        }
        Some(Preview::Dir | Preview::Nothing) => {}
        Some(Preview::Error(e)) => {
            col = col.child(
                line(theme)
                    .text_color(theme.c(Role::CriticalFg))
                    .child(SharedString::from(e.to_string())),
            );
        }
    }
    col
}

/// Имя тома как в Проводнике: метка или «Локальный диск», плюс корень (`C:`).
fn volume_name(v: &crate::views::VolumeView, i18n: &urfin_i18n::I18n) -> String {
    let root = v.root.display().to_string();
    let root = root.trim_end_matches(['\\', '/']);
    let label = if v.label.is_empty() {
        i18n.get("ui-info-volume-local")
    } else {
        v.label.clone()
    };
    if root.is_empty() {
        label
    } else {
        format!("{label} ({root})")
    }
}
