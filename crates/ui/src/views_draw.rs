//! Отрисовка режимов панели (S2 §2.4, §2.5, S2-18): та же карточка, что у списка, —
//! шапка, тело, подвал с подсказкой. Данные — `UiState.info/preview/tree`
//! ([`crate::views`]); здесь только чтение и команды.

#![allow(
    clippy::many_single_char_names,
    reason = "геометрия: c, t, k, r, y, a — как в остальных модулях отрисовки"
)]

use egui::{Align2, CornerRadius, Rect, Sense, pos2, vec2};
use urfin_core::{AppState, Attrs, Command, EntryKind, PanelMode, Row, Side, Tab, Value};
use urfin_i18n::{FluentArgs, I18n};
use urfin_vfs::volume::{Media, VolCaps};

use crate::canvas::Canvas;
use crate::fmt_cell::{Cell, FmtCfg};
use crate::fonts::Font;
use crate::icons;
use crate::state::UiState;
use crate::text::Tx;
use crate::tokens::{Metric, Role, Tokens};
use crate::views::{InfoState, Preview, PreviewState, TreeState};

/// Строк hex на экран — по 16 байт.
const HEX_ROW: usize = 16;

pub(crate) struct ViewCx<'a> {
    pub ui: &'a mut egui::Ui,
    pub i18n: &'a I18n,
    pub tokens: &'a Tokens,
    pub fmt: &'a FmtCfg,
}

/// Геометрия карточки: шапка, тело, подвал.
struct Card {
    head: Rect,
    body: Rect,
    foot: Rect,
    pad: f32,
}

fn card(c: &mut dyn Canvas, t: &Tokens, r: Rect) -> Card {
    let inner = r.shrink(t.m(Metric::SpaceXs));
    let head = inner.with_max_y(inner.top() + t.m(Metric::HBreadcrumb));
    let foot = inner.with_min_y(inner.bottom() - t.m(Metric::HPanelFooter));
    let thin = t.m(Metric::StrokeThin);
    for y in [head.bottom(), foot.top()] {
        c.rect(
            Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), thin)),
            Role::StrokeDivider,
            CornerRadius::ZERO,
        );
    }
    Card {
        body: Rect::from_min_max(
            pos2(inner.left(), head.bottom()),
            pos2(inner.right(), foot.top()),
        ),
        head,
        foot,
        pad: t.m(Metric::PadRowX),
    }
}

fn title(c: &mut dyn Canvas, cx: &ViewCx<'_>, k: &Card, tx: &Tx<'_>) {
    c.text(
        k.head.left_center() + vec2(k.pad, 0.0),
        Align2::LEFT_CENTER,
        tx,
        Font::Strong,
        cx.tokens.m(Metric::TypeBody),
        Role::TextPrimary,
    );
}

fn hint(c: &mut dyn Canvas, cx: &ViewCx<'_>, k: &Card, key: &'static str) {
    c.text(
        k.foot.left_center() + vec2(k.pad, 0.0),
        Align2::LEFT_CENTER,
        &Tx::key(key),
        Font::Caption,
        cx.tokens.m(Metric::TypeCaption),
        Role::TextSecondary,
    );
}

/// Карточка панели `side` в режиме, отличном от списка файлов.
#[allow(
    clippy::too_many_arguments,
    reason = "холст, контекст, место, состояние, вывод"
)]
pub(crate) fn show(
    c: &mut dyn Canvas,
    cx: &mut ViewCx<'_>,
    r: Rect,
    s: &AppState,
    st: &UiState,
    side: Side,
    active: bool,
    out: &mut Vec<Command>,
) {
    let i = side.idx();
    let other = s.panel(side.other()).tab();
    match s.panel(side).tab().mode {
        PanelMode::Info => info(c, cx, r, other, st.info[i].as_ref()),
        PanelMode::QView => preview(c, cx, r, st.preview[i].as_ref()),
        PanelMode::Tree => tree(c, cx, r, side, active, st.tree[i].as_ref(), out),
        PanelMode::Files => {}
    }
}

// ---- Инфо ----

/// Пары «ключ — значение» под заголовком секции; возвращает низ.
fn section(
    c: &mut dyn Canvas,
    cx: &ViewCx<'_>,
    k: &Card,
    y: f32,
    head: &Tx<'_>,
    rows: &[(&'static str, String)],
) -> f32 {
    let t = cx.tokens;
    let row = t.m(Metric::HRow);
    let body = t.m(Metric::TypeBody);
    let caption = t.m(Metric::TypeCaption);
    c.text(
        pos2(k.body.left() + k.pad, y + row / 2.0),
        Align2::LEFT_CENTER,
        head,
        Font::Strong,
        body,
        Role::TextPrimary,
    );
    let key_w = 140.0_f32.min(k.body.width() * 0.4);
    let mut yy = y + row;
    for (key, value) in rows {
        let mid = yy + row / 2.0;
        c.text(
            pos2(k.body.left() + k.pad * 2.0, mid),
            Align2::LEFT_CENTER,
            &Tx::key(key),
            Font::Caption,
            caption,
            Role::TextSecondary,
        );
        let x = k.body.left() + k.pad * 2.0 + key_w;
        c.text_clipped(
            pos2(x, mid),
            (k.body.right() - k.pad - x).max(0.0),
            &Tx::user(value.as_str()),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
        yy += row;
    }
    yy + row / 2.0
}

fn media_name(i18n: &I18n, m: Media) -> Option<String> {
    let key = match m {
        Media::Nvme => return Some("NVMe SSD".into()),
        Media::Ssd => return Some("SSD".into()),
        Media::Hdd => return Some("HDD".into()),
        Media::Net => "ui-info-media-net",
        Media::Usb => "ui-info-media-usb",
        Media::Unknown => return None,
    };
    Some(i18n.get(key))
}

fn caps_text(i18n: &I18n, caps: VolCaps) -> String {
    let names: Vec<String> = [
        (VolCaps::TRASH, "ui-info-cap-trash"),
        (VolCaps::ADS, "ui-info-cap-ads"),
        (VolCaps::COMPRESSION, "ui-info-cap-compression"),
        (VolCaps::CLONE, "ui-info-cap-clone"),
        (VolCaps::CASE_SENSITIVE, "ui-info-cap-case"),
    ]
    .into_iter()
    .filter(|(f, _)| caps.has(*f))
    .map(|(_, k)| i18n.get(k))
    .collect();
    if names.is_empty() {
        "—".into()
    } else {
        names.join(", ")
    }
}

fn attrs_text(a: Attrs) -> String {
    let s: String = [
        (Attrs::READONLY, 'R'),
        (Attrs::HIDDEN, 'H'),
        (Attrs::SYSTEM, 'S'),
        (Attrs::ARCHIVE, 'A'),
        (Attrs::REPARSE, 'L'),
        (Attrs::OFFLINE, 'O'),
    ]
    .into_iter()
    .filter(|(f, _)| a.has(*f))
    .map(|(_, ch)| ch)
    .collect();
    if s.is_empty() { "—".into() } else { s }
}

#[allow(clippy::too_many_lines, reason = "три секции сверху вниз")]
fn info(c: &mut dyn Canvas, cx: &mut ViewCx<'_>, r: Rect, other: &Tab, st: Option<&InfoState>) {
    let t = cx.tokens;
    let i18n = cx.i18n;
    let k = card(c, t, r);
    title(c, cx, &k, &Tx::key("ui-info-title"));
    hint(c, cx, &k, "ui-info-hint");
    let mut y = k.body.top() + t.m(Metric::SpaceS);
    // Диск.
    match st.and_then(|s| s.volume.as_ref()) {
        Some(Ok(v)) => {
            let name = if v.label.is_empty() {
                v.root.display().to_string()
            } else {
                format!("{} ({})", v.label, v.root.display())
            };
            let fs = match media_name(i18n, v.media) {
                Some(m) => format!("{} · {m}", v.fs_name),
                None => v.fs_name.clone(),
            };
            let mut rows = vec![
                ("ui-info-fs", fs),
                ("ui-info-caps", caps_text(i18n, v.caps)),
            ];
            if let Some(sp) = v.space {
                let mut a = FluentArgs::new();
                a.set("free", urfin_i18n::fmt::bytes_with(i18n, sp.avail));
                a.set("total", urfin_i18n::fmt::bytes_with(i18n, sp.total));
                #[allow(clippy::cast_precision_loss, reason = "проценты")]
                let pct = if sp.total > 0 {
                    (sp.avail as f64 * 100.0 / sp.total as f64).round()
                } else {
                    0.0
                };
                a.set("pct", pct);
                rows.push(("ui-info-free", i18n.fmt("ui-info-free-value", &a)));
            }
            let bottom = section(c, cx, &k, y, &Tx::user(name), &rows);
            if let Some(sp) = v.space.filter(|sp| sp.total > 0) {
                // Полоса занятости 4 px под строкой «Свободно».
                let bar = Rect::from_min_max(
                    pos2(
                        k.body.left() + k.pad * 2.0,
                        bottom - t.m(Metric::SpaceS) - 4.0,
                    ),
                    pos2(k.body.right() - k.pad, bottom - t.m(Metric::SpaceS)),
                );
                let radius = CornerRadius::from(2.0);
                c.rect(bar, Role::StrokeDivider, radius);
                #[allow(clippy::cast_precision_loss, reason = "доля полосы")]
                let used = (sp.total - sp.avail.min(sp.total)) as f32 / sp.total as f32;
                c.rect(
                    bar.with_max_x(bar.left() + bar.width() * used),
                    Role::AccentFill,
                    radius,
                );
            }
            y = bottom + t.m(Metric::SpaceS);
        }
        Some(Err(e)) => {
            y = section(
                c,
                cx,
                &k,
                y,
                &Tx::key("ui-info-volume"),
                &[("ui-info-fs", e.to_string())],
            );
        }
        None => {
            y = section(
                c,
                cx,
                &k,
                y,
                &Tx::key("ui-info-volume"),
                &[("ui-info-fs", "…".into())],
            );
        }
    }
    // Папка.
    let s = other.listing.stats;
    let mut a = FluentArgs::new();
    a.set("dirs", s.dirs);
    a.set("files", s.files);
    let mut rows = vec![
        ("ui-info-path", crate::panel::path_text(other)),
        ("ui-info-content", i18n.fmt("ui-info-content-value", &a)),
        (
            "ui-info-bytes",
            urfin_i18n::fmt::bytes_exact_with(i18n, s.bytes),
        ),
    ];
    if !other.selection.is_empty() {
        rows.push((
            "ui-info-selected",
            format!(
                "{} · {}",
                other.selection.len(),
                urfin_i18n::fmt::bytes_with(i18n, other.selection.bytes())
            ),
        ));
    }
    let folder = other.loc.path.file_name().map_or_else(
        || other.loc.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    y = section(c, cx, &k, y, &Tx::user(folder), &rows) + t.m(Metric::SpaceS);
    // Под курсором.
    if let Some(Row::Entry { entry, .. }) = other.current() {
        let kind = match entry.kind {
            EntryKind::Dir => "ui-info-kind-dir",
            EntryKind::File => "ui-info-kind-file",
            EntryKind::LinkDir | EntryKind::LinkFile | EntryKind::LinkBroken => "ui-info-kind-link",
            EntryKind::Other => "ui-info-kind-other",
        };
        let size = if entry.kind.is_dir_like() && !entry.attrs.has(Attrs::SIZE_KNOWN) {
            "—".to_owned()
        } else {
            urfin_i18n::fmt::bytes_exact_with(i18n, entry.size)
        };
        let mut date = Cell::new();
        cx.fmt.write_datetime(&mut date, entry.mtime);
        let rows = [
            ("ui-info-name", entry.name.to_string()),
            ("ui-info-type", i18n.get(kind)),
            ("ui-info-size", size),
            ("ui-info-mtime", date.as_str().to_owned()),
            ("ui-info-attrs", attrs_text(entry.attrs)),
        ];
        section(c, cx, &k, y, &Tx::key("ui-info-cursor"), &rows);
    }
}

// ---- Просмотр ----

fn preview(c: &mut dyn Canvas, cx: &mut ViewCx<'_>, r: Rect, st: Option<&PreviewState>) {
    let t = cx.tokens;
    let k = card(c, t, r);
    let caption = t.m(Metric::TypeCaption);
    let mono = t.m(Metric::TypeBody);
    let name = st
        .and_then(|p| p.key.as_ref())
        .map_or_else(String::new, |k| k.name.to_string());
    c.icon(
        pos2(k.head.left() + k.pad + caption / 2.0, k.head.center().y),
        icons::glyph(urfin_vfs::icons::IconClass::File),
        caption,
        Role::TextSecondary,
    );
    c.text_clipped(
        pos2(k.head.left() + k.pad + caption * 1.5, k.head.center().y),
        k.head.width() * 0.55,
        &Tx::user(name),
        Font::Strong,
        t.m(Metric::TypeBody),
        Role::TextPrimary,
    );
    hint(c, cx, &k, "ui-qview-hint");
    let Some(p) = st else {
        return;
    };
    // Справа в шапке: кодировка · концы строк · размер.
    let size = urfin_i18n::fmt::bytes_with(cx.i18n, p.size);
    let meta = match &p.content {
        Some(Preview::Text { encoding, crlf, .. }) => format!(
            "{} · {} · {size}",
            encoding.name(),
            if *crlf { "CRLF" } else { "LF" }
        ),
        Some(Preview::Hex(_)) => format!("{} · {size}", cx.i18n.get("ui-qview-binary")),
        _ => String::new(),
    };
    c.text(
        k.head.right_center() - vec2(k.pad, 0.0),
        Align2::RIGHT_CENTER,
        &Tx::user(meta),
        Font::Caption,
        caption,
        Role::TextSecondary,
    );
    let line_h = mono * 1.4;
    let left = k.body.left() + k.pad;
    let width = k.body.width() - 2.0 * k.pad;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "строк на экран"
    )]
    let fit = ((k.body.height() - k.pad) / line_h).max(0.0) as usize;
    let line = |c: &mut dyn Canvas, i: usize, text: &str, role: Role| {
        #[allow(clippy::cast_precision_loss, reason = "строк на экран")]
        let y = k.body.top() + k.pad / 2.0 + line_h * (i as f32 + 0.5);
        c.text_clipped(
            pos2(left, y),
            width,
            &Tx::user(text),
            Font::Mono,
            mono,
            role,
        );
    };
    match &p.content {
        None => line(c, 0, "…", Role::TextSecondary),
        Some(Preview::Text { text, .. }) => {
            for (i, l) in text.lines().take(fit).enumerate() {
                line(c, i, &l.replace('\t', "    "), Role::TextPrimary);
            }
        }
        Some(Preview::Hex(bytes)) => {
            for (i, chunk) in bytes.chunks(HEX_ROW).take(fit).enumerate() {
                line(c, i, &hex_row(i * HEX_ROW, chunk), Role::TextPrimary);
            }
        }
        Some(Preview::Dir) => line(c, 0, &cx.i18n.get("ui-qview-dir"), Role::TextSecondary),
        Some(Preview::Nothing) => {}
        Some(Preview::Error(e)) => line(c, 0, e, Role::CriticalFg),
    }
}

/// `00000010  48 65 6C 6C …  Hello…`.
pub(crate) fn hex_row(offset: usize, b: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = format!("{offset:08X} ");
    for i in 0..HEX_ROW {
        if i == HEX_ROW / 2 {
            s.push(' ');
        }
        match b.get(i) {
            Some(x) => {
                let _ = write!(s, " {x:02X}");
            }
            None => s.push_str("   "),
        }
    }
    s.push_str("  ");
    s.extend(b.iter().map(|&x| {
        if x.is_ascii_graphic() || x == b' ' {
            char::from(x)
        } else {
            '.'
        }
    }));
    s
}

// ---- Дерево ----

#[allow(
    clippy::too_many_arguments,
    reason = "холст, контекст, место, состояние, вывод"
)]
fn tree(
    c: &mut dyn Canvas,
    cx: &mut ViewCx<'_>,
    r: Rect,
    side: Side,
    active: bool,
    st: Option<&TreeState>,
    out: &mut Vec<Command>,
) {
    let t = cx.tokens;
    let k = card(c, t, r);
    hint(c, cx, &k, "ui-tree-hint");
    let Some(tree) = st else {
        title(c, cx, &k, &Tx::key("ui-tree-title-empty"));
        return;
    };
    let root = tree
        .nodes
        .first()
        .map_or_else(String::new, |n| n.loc.path.display().to_string());
    let mut a = FluentArgs::new();
    a.set("root", root);
    title(c, cx, &k, &Tx::Args("ui-tree-title", a));
    let row = t.m(Metric::HRow);
    let body = t.m(Metric::TypeBody);
    let caption = t.m(Metric::TypeCaption);
    let vis = tree.visible();
    let cur = tree
        .index_of(&tree.cursor)
        .and_then(|i| vis.iter().position(|v| *v == i))
        .unwrap_or(0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "строк на экран"
    )]
    let fit = (k.body.height() / row).max(1.0) as usize;
    // Курсор всегда виден: прокрутка — чтобы он был в пределах экрана.
    let top = cur.saturating_sub(fit.saturating_sub(1));
    let radius = CornerRadius::from(t.m(Metric::RadiusControl));
    for (n, &i) in vis.iter().enumerate().skip(top).take(fit) {
        let node = &tree.nodes[i];
        #[allow(clippy::cast_precision_loss, reason = "строк на экран")]
        let y0 = k.body.top() + row * (n - top) as f32;
        let rr = Rect::from_min_max(pos2(k.body.left(), y0), pos2(k.body.right(), y0 + row));
        if n == cur {
            if active {
                c.rect(rr, Role::FillCursor, radius);
                c.stroke(rr, Role::StrokeStrong, radius, t.m(Metric::StrokeThin));
            } else {
                c.rect(rr, Role::FillCursorPassive, radius);
            }
        }
        let x = k.body.left() + k.pad + 12.0 * f32::from(node.depth);
        let mid = rr.center().y;
        if !(node.loaded && node.empty) {
            c.icon(
                pos2(x + caption / 2.0, mid),
                if node.expanded {
                    icons::CHEVRON_DOWN
                } else {
                    icons::CHEVRON_RIGHT
                },
                caption * 0.8,
                Role::TextSecondary,
            );
        }
        let icon = if node.depth == 0 {
            icons::glyph(urfin_vfs::icons::IconClass::Disk)
        } else {
            icons::FOLDER
        };
        c.icon(
            pos2(x + caption * 1.75, mid),
            icon,
            caption,
            Role::AccentText,
        );
        let tx = x + caption * 2.75;
        c.text_clipped(
            pos2(tx, mid),
            (rr.right() - k.pad - tx).max(0.0),
            &Tx::user(node.name.as_str()),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
        let id = cx.ui.scope_id().with(("tree-row", side.idx(), i));
        let resp = cx.ui.interact(rr, id, Sense::click());
        if resp.clicked() {
            if !active {
                out.push(Command::new("panel.focus").arg("side", crate::panel::side_arg(side)));
            }
            out.push(
                Command::new("ui.tree.select").arg("n", Value::Int(i64::try_from(n).unwrap_or(0))),
            );
        }
        if resp.double_clicked() {
            out.push(Command::new("ui.tree.enter"));
        }
    }
}

#[cfg(test)]
#[path = "views_draw_tests.rs"]
mod tests;
