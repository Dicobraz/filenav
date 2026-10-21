//! Панель файлов на GPUI (G-04, S2 §2.1–2.2): карточка с путём, шапкой колонок,
//! строками 24 px и подвалом. Рисуется только видимая страница от `cursor.top` —
//! её держит Core по `panel.viewport` (S0 v3 №3).

use gpui_kit::prelude::*;
use std::rc::Rc;

use gpui_kit::{
    App, Bounds, ClickEvent, ContentMask, Context, Div, Font, Hsla, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, Stateful, TextAlign, TextRun, TruncateFrom, WeakEntity, Window, canvas, div,
    point, px, size as size2,
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
    let list_h = h - theme.m(Metric::HBreadcrumb) - head;
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

/// Обработчики событий панели: замыкания над хостом (как `cx.listener`, но панель —
/// своя view и рисуется вне `Context<CoreHost>`).
#[derive(Clone)]
pub(super) struct Listen(pub WeakEntity<CoreHost>);

impl Listen {
    pub fn listener<E: ?Sized>(
        &self,
        f: impl Fn(&mut CoreHost, &E, &mut Window, &mut Context<CoreHost>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let host = self.0.clone();
        move |e, window, cx| {
            let _ = host.update(cx, |h, cx| f(h, e, window, cx));
        }
    }
}

/// Часть панели — своя view: окно кладёт её кэшированной (`cached`), и кадр, где она не
/// менялась, gpui повторяет без раскладки и отрисовки (G-07). При листании меняется
/// только список; путь, шапка колонок и подвал берутся из кэша.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Piece {
    /// Путь и шапка колонок.
    Head,
    /// Строки (или дерево, «Сведения», просмотр).
    List,
    /// Итоги, быстрый поиск, аккорд.
    Foot,
}

pub(super) struct PanelView {
    pub host: WeakEntity<CoreHost>,
    pub side: Side,
    pub piece: Piece,
}

impl Render for PanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(host) = self.host.upgrade() else {
            return div();
        };
        let h = host.read(cx);
        let Some(core) = h.core.as_ref() else {
            return div();
        };
        let s = core.state();
        let side = self.side;
        let t = s.panel(side).tab();
        let theme = &h.theme;
        let l = Listen(self.host.clone());
        let (rows, cols) = h.grid[usize::from(side == Side::Right)];
        let body = div().size_full().flex().flex_col();
        match self.piece {
            Piece::Head => {
                let panel_w = h.panel_w[usize::from(side == Side::Right)];
                let font_px = px(theme.m(Metric::TypeBody));
                let font = window.text_style().font();
                let measure = |text: &str| -> f32 {
                    let run = TextRun {
                        len: text.len(),
                        font: font.clone(),
                        color: Hsla::default(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    f32::from(
                        window
                            .text_system()
                            .shape_line(SharedString::from(text.to_owned()), font_px, &[run], None)
                            .width,
                    )
                };
                let cols = fit_cols(theme, t.view_mode, panel_w);
                body.child(path_bar(
                    t,
                    side,
                    s.active == side,
                    theme,
                    h.field.as_ref(),
                    panel_w,
                    &measure,
                    &l,
                ))
                .when(matches!(t.view_mode, ViewMode::Full), |d| {
                    d.child(col_header(side, theme, cols, &l))
                })
            }
            // Дерево, «Сведения», просмотр (S2 §2.4–2.5).
            Piece::List if t.mode != urfin_core::PanelMode::Files => {
                body.child(super::views::body(s, side, theme, rows))
            }
            Piece::List => body.child(list(
                t,
                side,
                s.active == side,
                theme,
                rows,
                cols,
                h.field.as_ref(),
                h.slide_pos(side),
                &l,
            )),
            Piece::Foot => body.child(footer(s, t, side, s.active == side, theme)),
        }
    }
}

/// Отпечаток всего, что рисует часть панели: изменился — часть рисуется заново, иначе
/// кадр берётся из кэша gpui. Ввод в поле внутри части помечает её сам (предок поля).
pub(super) fn fingerprint(
    s: &AppState,
    side: Side,
    piece: Piece,
    grid: (u32, u32),
    look: u64,
    field: Option<&super::field::Field>,
    anim: u64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    let t = s.panel(side).tab();
    let active = s.active == side;
    (side == Side::Right, look, urfin_i18n::epoch(), t.id.0).hash(&mut h);
    format!("{:?}{:?}", t.view_mode, t.mode).hash(&mut h);
    if let Some(f) = field {
        format!("{:?}{}", f.key, f.input.entity_id()).hash(&mut h);
    }
    match piece {
        Piece::Head => format!("{:?}", t.loc).hash(&mut h),
        Piece::List => {
            (
                active,
                grid,
                t.rev,
                t.cursor.row,
                t.cursor.top,
                t.rows(),
                anim,
            )
                .hash(&mut h);
            (t.selection.len(), t.selection.bytes()).hash(&mut h);
            if t.mode != urfin_core::PanelMode::Files {
                // Дерево, «Сведения», просмотр смотрят и на другую панель.
                s.rev.hash(&mut h);
            }
        }
        Piece::Foot => {
            (active, t.rev, t.selection.len(), t.selection.bytes()).hash(&mut h);
            if let Some(k) = s.ext.get::<urfin_keymap::KeymapView>() {
                if let Some(q) = k.qs.as_ref().filter(|q| q.side == side) {
                    format!("{q:?}").hash(&mut h);
                }
                if active {
                    k.pending.hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// Эксперимент с курсором (`URFIN_CURSOR=solid,slide`, выключен по умолчанию): сплошной
/// акцентный курсор и скольжение полосы между строками. Решение — за владельцем.
pub(super) fn cursor_mode() -> (bool, bool) {
    static M: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    *M.get_or_init(|| {
        let v = std::env::var("URFIN_CURSOR").unwrap_or_default();
        (v.contains("solid"), v.contains("slide"))
    })
}

/// Панель — плоская, фон `panel`, без рамки (D-02): активную видно по подчёркнутому
/// пути. Части кладёт окно.
pub(super) fn card(theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(theme.c(Role::LayerPanel))
}

/// Высота шапки панели: путь и, в Full, строка колонок.
pub(super) fn head_h(theme: &Theme, mode: ViewMode) -> f32 {
    theme.m(Metric::HBreadcrumb)
        + if matches!(mode, ViewMode::Full) {
            theme.m(Metric::HColHeader)
        } else {
            0.0
        }
}

/// Сегменты пути от корня: (подпись, путь до него включительно).
fn crumbs(t: &Tab) -> Vec<(String, std::path::PathBuf)> {
    let mut acc = std::path::PathBuf::new();
    let mut out = Vec::new();
    for c in t.loc.path.components() {
        acc.push(c.as_os_str());
        let label = match c {
            std::path::Component::Prefix(p) => p.as_os_str().to_string_lossy().into_owned(),
            std::path::Component::RootDir => continue,
            other => other.as_os_str().to_string_lossy().into_owned(),
        };
        out.push((label, acc.clone()));
    }
    out
}

/// Путь панели (D-02, S2-12): строка пути `C:\Dev\urfin` — у активной панели цвет
/// `text` и подчёркивание `accent` 2 px, у другой — `secondary`; справа «N объектов».
/// Сегмент — переход к нему, пустое место — правка пути. Не влезает — начальные
/// сегменты сворачиваются в «…» (переход к последнему скрытому), текущая папка видна.
#[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
#[allow(clippy::too_many_lines, reason = "сегменты слева направо")]
fn path_bar(
    t: &Tab,
    side: Side,
    active: bool,
    theme: &Theme,
    field: Option<&super::field::Field>,
    width: f32,
    measure: &dyn Fn(&str) -> f32,
    cx: &Listen,
) -> Div {
    let bar = div()
        .h(px(theme.m(Metric::HBreadcrumb)))
        .flex_none()
        .flex()
        .items_center()
        .px(px(theme.m(Metric::PadRowX)))
        .text_size(px(PATH_TEXT))
        .border_b_1()
        .border_color(theme.c(Role::StrokeDivider));
    if let Some(input) = super::field::path_input(field, side) {
        return bar.child(div().flex_1().child(input));
    }
    let name = super::chrome::side_name(side);
    let color = theme.c(if active {
        Role::TextPrimary
    } else {
        Role::TextSecondary
    });
    let mut segs = div()
        .flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(color)
        .when(active, |d| {
            d.border_b_2()
                .border_color(theme.c(Role::AccentFill))
                .pb(px(3.0))
                .mt(px(5.0))
        });
    let list = if t.loc.is_local() {
        crumbs(t)
    } else {
        Vec::new()
    };
    let first = first_shown(&list, width - 2.0 * theme.m(Metric::PadRowX), measure);
    let sep = || div().child(SharedString::new_static("\\"));
    let seg = |id: String, label: SharedString, path: std::path::PathBuf| {
        let loc = urfin_core::Location::local(path);
        div()
            .id(SharedString::from(id))
            .hover(Styled::underline)
            .child(label)
            .on_click(cx.listener(move |h, _: &ClickEvent, w, cx| {
                h.run(
                    urfin_core::Command::new("panel.go")
                        .arg("side", name)
                        .arg("path", loc.clone()),
                    w,
                    cx,
                );
            }))
    };
    if first > 0 {
        // «…» — к последнему скрытому сегменту.
        let (_, path) = &list[first - 1];
        segs = segs
            .child(seg(
                format!("crumb-{name}-more"),
                SharedString::new_static("…"),
                path.clone(),
            ))
            .child(sep());
    }
    for (k, (label, path)) in list.into_iter().enumerate().skip(first) {
        // У диска разделитель уже в подписи (`C:` + `\`).
        if k > first {
            segs = segs.child(sep());
        }
        segs = segs.child(seg(
            format!("crumb-{name}-{k}"),
            SharedString::from(label),
            path,
        ));
    }
    if !t.loc.is_local() {
        segs = segs.child(SharedString::from(t.loc.to_uri()));
    }
    let mut args = FluentArgs::new();
    let st = &t.listing.stats;
    args.set("count", u64::from(st.files + st.dirs));
    let count = urfin_i18n::current().fmt("ui-panel-count", &args);
    bar.child(segs)
        .child(
            div()
                .id(SharedString::from(format!("crumb-edit-{name}")))
                .flex_1()
                .h_full()
                .on_click(cx.listener(move |h, _: &ClickEvent, w, cx| {
                    h.run(
                        urfin_core::Command::new("ui.path_edit").arg("side", name),
                        w,
                        cx,
                    );
                })),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.c(Role::TextSecondary))
                .child(SharedString::from(count)),
        )
}

/// Текст пути и шапки колонок (D-02), px.
const PATH_TEXT: f32 = 12.5;
const COL_HEAD_TEXT: f32 = 11.5;

/// Ширина сегмента пути с полями и разделитель `\` между сегментами.
const CRUMB_PAD: f32 = 0.0;
const CRUMB_CHEVRON: f32 = 8.0;
/// Место справа от пути под клик «править путь».
const CRUMB_EDIT_MIN: f32 = 32.0;

/// Первый показываемый сегмент: начальные уходят в «…», пока путь не влезет в `avail`.
fn first_shown(
    list: &[(String, std::path::PathBuf)],
    avail: f32,
    measure: &dyn Fn(&str) -> f32,
) -> usize {
    let cost: Vec<f32> = list.iter().map(|(l, _)| measure(l) + CRUMB_PAD).collect();
    let budget = avail - CRUMB_EDIT_MIN;
    #[allow(clippy::cast_precision_loss, reason = "сегментов десятки")]
    let full: f32 = cost.iter().sum::<f32>() + CRUMB_CHEVRON * list.len().saturating_sub(1) as f32;
    if full <= budget || list.len() < 2 {
        return 0;
    }
    let more = measure("…") + CRUMB_PAD + CRUMB_CHEVRON;
    let mut total = full + more;
    let mut first = 0;
    while total > budget && first + 1 < list.len() {
        total -= cost[first] + CRUMB_CHEVRON;
        first += 1;
    }
    first
}

fn col_header(side: Side, theme: &Theme, cols: (f32, f32), cx: &Listen) -> Div {
    let i18n = urfin_i18n::current();
    // Клик по колонке — сортировка по ней (повтор — обратная, S2 §2.1).
    let cell = |key: &str, sort: &'static str, w: Option<f32>| {
        let d = div()
            .id(SharedString::from(format!("col-{sort}-{side:?}")))
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
        .text_size(px(COL_HEAD_TEXT))
        .text_color(theme.c(Role::TextSecondary))
        .border_b_1()
        .border_color(theme.c(Role::StrokeDivider))
        .child(cell("ui-col-name", "name", None))
        .when(cols.0 > 0.0, |d| {
            d.child(cell("ui-col-size", "size", Some(cols.0)))
        })
        .when(cols.1 > 0.0, |d| {
            d.child(cell("ui-col-mtime", "mtime", Some(cols.1)))
        })
}

/// Строка видимой страницы для отрисовки: всё готово, `canvas` только рисует.
struct Line {
    /// Имя для `panel.cursor.to`; у `..` — нет.
    target: Option<SharedString>,
    glyph: SharedString,
    name: SharedString,
    size: Option<SharedString>,
    mtime: Option<SharedString>,
    /// Помечена: фон `accent.subtle`, галочка, имя `accent.text` (S0-decisions №31).
    marked: bool,
    /// Курсор: `Some(true)` — активной панели (заливка и рамка 1 px), `Some(false)` — пассивной.
    cursor: Option<bool>,
    color: Hsla,
    /// Иконка: каталог — акцент, скрытый и системный — третичный, прочее — вторичный.
    icon_color: Hsla,
    /// Строка на сплошном акцентном курсоре (эксперимент `solid`): текст `accent.on`.
    on_accent: bool,
    /// Вместо имени — поле переименования.
    renaming: bool,
}

/// Геометрия строки в колонке: отступы и колонки Full/Wide.
#[derive(Clone, Copy)]
struct Geo {
    row_h: f32,
    pad: f32,
    /// Колонка галочки помеченных (как у egui-панели).
    mark_size: f32,
    stroke: f32,
    icon_w: f32,
    icon_size: f32,
    radius: f32,
    size_w: f32,
    mtime_w: f32,
}

/// То же правило колонок, что [`fit_cols`], для колонки списка шириной `w`.
fn fit_cols_geo(geo: &Geo, w: f32) -> (f32, f32) {
    let avail = w - ROW_MX - geo.pad - geo.name_x();
    let (mut size, mut mtime) = (geo.size_w, geo.mtime_w);
    if avail - size - mtime < NAME_MIN {
        mtime = 0.0;
    }
    if avail - size - mtime < NAME_MIN {
        size = 0.0;
    }
    (size, mtime)
}

impl Geo {
    /// Левый край иконки в колонке.
    fn icon_x(&self) -> f32 {
        ROW_MX + self.pad + MARK_W
    }

    /// Левый край имени в колонке.
    fn name_x(&self) -> f32 {
        self.icon_x() + self.icon_w
    }

    /// Ширина имени в колонке шириной `w`.
    fn name_w(&self, w: f32) -> f32 {
        (w - ROW_MX - self.pad - self.size_w - self.mtime_w - self.name_x()).max(0.0)
    }
}

/// Отступ строки от краёв колонки (`mx_1`).
const ROW_MX: f32 = 4.0;
/// Имя не уже этого: в узкой панели сначала уходит дата, потом размер (как в egui).
const NAME_MIN: f32 = 120.0;

/// Ширины колонок размера и даты для панели шириной `w` (0 — колонки нет).
pub(super) fn fit_cols(theme: &Theme, mode: ViewMode, w: f32) -> (f32, f32) {
    let pad = theme.m(Metric::PadRowX) - ROW_MX;
    let name_x = ROW_MX + pad + MARK_W + theme.m(Metric::WIcon) + 8.0;
    let avail = w - ROW_MX - pad - name_x;
    let mut size = if matches!(mode, ViewMode::Full | ViewMode::Wide) {
        W_SIZE
    } else {
        0.0
    };
    let mut mtime = if matches!(mode, ViewMode::Full) {
        W_MTIME
    } else {
        0.0
    };
    if avail - size - mtime < NAME_MIN {
        mtime = 0.0;
    }
    if avail - size - mtime < NAME_MIN {
        size = 0.0;
    }
    (size, mtime)
}
/// Ширина колонки галочки.
const MARK_W: f32 = 12.0;

/// Цвета строк для `canvas`.
#[derive(Clone, Copy)]
struct Pal {
    /// Эксперимент `URFIN_CURSOR=solid`: курсор активной панели — сплошной акцент.
    solid: bool,
    accent_fill: Hsla,
    on_accent: Hsla,
    secondary: Hsla,
    marked: Hsla,
    cursor: Hsla,
    cursor_passive: Hsla,
    stroke: Hsla,
    accent: Hsla,
}

/// Видимая страница: Full/Wide — одна колонка, Brief/Medium — колонки сверху вниз.
/// Строки рисует один `canvas` (фон, иконка, текст), без дерева `div`: раскладка taffy
/// для сотни узлов с текстом — половина кадра (G-07, профиль). Клики — по координате.
#[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
#[allow(clippy::too_many_lines, reason = "сборка строк и отрисовка рядом")]
fn list(
    t: &Tab,
    side: Side,
    active: bool,
    theme: &Theme,
    rows: u32,
    cols: u32,
    field: Option<&super::field::Field>,
    glide: Option<f32>,
    cx: &Listen,
) -> Stateful<Div> {
    let total = t.rows();
    let top = t.cursor.top;
    let rows = rows.max(1);
    let cols = cols.max(1);
    let full = matches!(t.view_mode, ViewMode::Full | ViewMode::Wide);
    let geo = Geo {
        row_h: theme.m(Metric::HRow),
        pad: theme.m(Metric::PadRowX) - ROW_MX,
        mark_size: theme.m(Metric::TypeCaption),
        stroke: theme.m(Metric::StrokeThin),
        icon_w: theme.m(Metric::WIcon) + 8.0,
        icon_size: theme.m(Metric::WIcon),
        radius: theme.m(Metric::RadiusControl),
        size_w: if full { W_SIZE } else { 0.0 },
        mtime_w: if matches!(t.view_mode, ViewMode::Full) {
            W_MTIME
        } else {
            0.0
        },
    };
    let rename = super::field::rename_input(field, t);
    let solid = cursor_mode().0;
    let mut lines = Vec::with_capacity((rows * cols) as usize);
    let mut cell = Cell::new();
    for slot in 0..rows * cols {
        let ix = top + slot;
        if ix >= total {
            break;
        }
        let cursor = ix == t.cursor.row;
        let (glyph, name, marked, color, entry) = match t.row(ix) {
            Some(Row::Parent) => (GLYPH_UP, SharedString::new_static(".."), false, None, None),
            Some(Row::Entry { idx, entry }) => {
                let icon = crate::icons::glyph(icon_class(&entry.name, entry.kind, entry.attrs));
                let group = theme.hl.classify(entry);
                let color = theme.hl.color(group, &theme.tokens);
                let name = SharedString::from(entry.name.to_string());
                (icon, name, t.selection.contains(idx), color, Some(entry))
            }
            None => break,
        };
        let mut col = |kind: ColKind| {
            entry.map(|e| {
                cell.clear();
                cell_text(kind, e, &theme.fmt, &mut cell);
                SharedString::from(cell.as_str().to_owned())
            })
        };
        let bytes = if full { col(ColKind::Size) } else { None };
        let when = if geo.mtime_w > 0.0 {
            col(ColKind::Mtime)
        } else {
            None
        };
        lines.push(Line {
            target: entry.map(|_| name.clone()),
            glyph: SharedString::from(glyph.to_string()),
            size: bytes,
            mtime: when,
            name,
            marked,
            cursor: cursor.then_some(active),
            on_accent: solid && cursor && active,
            color: if solid && cursor && active {
                theme.c(Role::AccentOn)
            } else if marked {
                theme.c(Role::AccentText)
            } else {
                color.map_or_else(|| theme.c(Role::TextPrimary), super::theme::hsla)
            },
            icon_color: theme.c(match entry {
                _ if solid && cursor && active => Role::AccentOn,
                Some(e)
                    if e.attrs.has(urfin_core::Attrs::HIDDEN)
                        || e.attrs.has(urfin_core::Attrs::SYSTEM) =>
                {
                    Role::TextTertiary
                }
                Some(e) if e.kind.is_dir_like() => Role::AccentText,
                _ => Role::TextSecondary,
            }),
            renaming: cursor && rename.is_some(),
        });
    }
    let lines = Rc::new(lines);
    let bounds = Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
    let pal = Pal {
        solid,
        accent_fill: theme.c(Role::AccentFill),
        on_accent: theme.c(Role::AccentOn),
        secondary: theme.c(Role::TextSecondary),
        marked: theme.c(Role::AccentSubtle),
        cursor: theme.c(Role::FillCursor),
        cursor_passive: theme.c(Role::FillCursorPassive),
        stroke: theme.c(Role::StrokeStrong),
        accent: theme.c(Role::AccentText),
    };
    let paint_lines = lines.clone();
    let paint_bounds = bounds.clone();
    let canvas = canvas(
        move |b, _, _| paint_bounds.set(b),
        move |b, (), window, cx| {
            paint(&paint_lines, b, rows, cols, geo, pal, glide, window, cx);
        },
    )
    .size_full();
    // Строка под точкой → цель для `panel.cursor.to` (`None` снаружи — мимо строк).
    let hit = move |lines: &[Line], p: Point<Pixels>| -> Option<Option<SharedString>> {
        let b = bounds.get();
        if !b.contains(&p) {
            return None;
        }
        #[allow(clippy::cast_precision_loss, reason = "колонок единицы")]
        let col_w = f32::from(b.size.width) / cols as f32;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "не меньше 0"
        )]
        let c = (f32::from(p.x - b.origin.x) / col_w).floor().max(0.0) as u32;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "не меньше 0"
        )]
        let r = (f32::from(p.y - b.origin.y) / geo.row_h).floor().max(0.0) as u32;
        if r >= rows {
            return None;
        }
        lines
            .get((c.min(cols - 1) * rows + r) as usize)
            .map(|l| l.target.clone())
    };
    let click_lines = lines.clone();
    let click_hit = hit.clone();
    let move_lines = lines.clone();
    let move_hit = hit.clone();
    let right_lines = lines.clone();
    let mut d = div()
        .id(SharedString::from(format!("list-{side:?}")))
        .flex_1()
        .min_h_0()
        .relative()
        .overflow_hidden()
        .child(canvas)
        .on_scroll_wheel(cx.listener(move |h, ev: &ScrollWheelEvent, w, cx| {
            h.wheel(side, ev, w, cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |h, ev: &MouseUpEvent, w, cx| {
                if let Some(target) = click_hit(&click_lines, ev.position) {
                    h.row_click(
                        side,
                        target.as_deref(),
                        ev.click_count,
                        ev.modifiers.control,
                        w,
                        cx,
                    );
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |h, ev: &MouseDownEvent, w, cx| {
                if let Some(target) = hit(&right_lines, ev.position) {
                    h.row_right_click(side, target.as_deref(), w, cx);
                }
            }),
        )
        // Протяжка с зажатой ПКМ помечает строки под мышью (Far).
        .on_mouse_move(cx.listener(move |h, ev: &MouseMoveEvent, w, cx| {
            if ev.pressed_button != Some(MouseButton::Right) {
                h.right_drag_end();
            } else if let Some(target) = move_hit(&move_lines, ev.position) {
                h.row_right_drag(side, target.as_deref(), w, cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Right,
            cx.listener(|h, _: &MouseUpEvent, _, _| h.right_drag_end()),
        );
    // Переименование на месте (S2-16): поле поверх имени в строке курсора.
    if let Some(input) = rename
        && let Some(k) = lines.iter().position(|l| l.renaming)
    {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "номер строки на странице"
        )]
        let (top, col, n_cols) = (
            (k as u32 % rows) as f32 * geo.row_h,
            (k as u32 / rows) as f32,
            cols as f32,
        );
        d = d.child(
            div()
                .absolute()
                .top(px(top))
                .h(px(geo.row_h))
                .left(gpui_kit::relative(col / n_cols))
                .w(gpui_kit::relative(1.0 / n_cols))
                .pl(px(geo.name_x()))
                .pr(px(ROW_MX + geo.pad + geo.size_w + geo.mtime_w))
                .flex()
                .items_center()
                .child(div().flex_1().min_w_0().child(input)),
        );
    }
    d
}

/// Рисует строки страницы как egui-панель (S0-decisions №31–32): фон помеченных и галочка,
/// курсор — заливка, у активной панели ещё рамка 1 px; иконка, имя (с «…»), колонки.
#[allow(clippy::too_many_arguments, reason = "плоский вызов отрисовки")]
#[allow(clippy::too_many_lines, reason = "строка слева направо")]
fn paint(
    lines: &[Line],
    b: Bounds<Pixels>,
    rows: u32,
    cols: u32,
    geo: Geo,
    pal: Pal,
    glide: Option<f32>,
    window: &mut Window,
    cx: &mut App,
) {
    let style = window.text_style();
    let size = style.font_size.to_pixels(window.rem_size());
    let font = style.font();
    let check = SharedString::new_static("\u{E8FB}");
    let icons = gpui_kit::font(ICON_FONT);
    let row_h = px(geo.row_h);
    #[allow(clippy::cast_precision_loss, reason = "колонок единицы")]
    let col_w = f32::from(b.size.width) / cols as f32;
    let (size_w, mtime_w) = fit_cols_geo(&geo, col_w);
    let geo = Geo {
        size_w,
        mtime_w,
        ..geo
    };
    let name_w = px(geo.name_w(col_w));
    let run = |len: usize, font: &Font, color: Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shape = |window: &mut Window, s: &SharedString, font: &Font, color: Hsla, sz: Pixels| {
        window
            .text_system()
            .shape_line(s.clone(), sz, &[run(s.len(), font, color)], None)
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "номер строки на странице"
    )]
    let at = |k: usize| {
        (
            b.origin.x + px((k as u32 / rows) as f32 * col_w),
            b.origin.y + row_h * ((k as u32 % rows) as f32),
        )
    };
    let bar = |x0: Pixels, y: Pixels| {
        Bounds::new(
            point(x0 + px(ROW_MX), y),
            size2(px(col_w - 2.0 * ROW_MX), row_h),
        )
    };
    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        // Сначала все фоны, потом содержимое: скользящий курсор не перекрывает текст.
        for (k, l) in lines.iter().enumerate() {
            let (x0, y) = at(k);
            if l.marked {
                window.paint_quad(
                    gpui_kit::fill(bar(x0, y), pal.marked).corner_radii(px(geo.radius)),
                );
            }
            if let Some(active) = l.cursor {
                // Эксперимент `slide`: полоса между прежней и новой строкой.
                let y = match glide {
                    Some(f) => b.origin.y + row_h * f,
                    None => y,
                };
                let q = if active && pal.solid {
                    gpui_kit::fill(bar(x0, y), pal.accent_fill)
                } else {
                    let fill = if active {
                        pal.cursor
                    } else {
                        pal.cursor_passive
                    };
                    let q = gpui_kit::fill(bar(x0, y), fill).corner_radii(px(geo.radius));
                    if active {
                        q.border_widths(px(geo.stroke)).border_color(pal.stroke)
                    } else {
                        q
                    }
                };
                window.paint_quad(q);
            }
        }
        for (k, l) in lines.iter().enumerate() {
            let (x0, y) = at(k);
            let secondary = if l.on_accent {
                pal.on_accent
            } else {
                pal.secondary
            };
            if l.marked {
                let tone = if l.on_accent {
                    pal.on_accent
                } else {
                    pal.accent
                };
                let mark = shape(window, &check, &icons, tone, px(geo.mark_size));
                let _ = mark.paint(
                    point(x0 + px(ROW_MX + geo.pad), y),
                    row_h,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            let icon = shape(window, &l.glyph, &icons, l.icon_color, px(geo.icon_size));
            let _ = icon.paint(
                point(x0 + px(geo.icon_x()), y),
                row_h,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            if !l.renaming {
                let f = &font;
                let mut line = shape(window, &l.name, f, l.color, size);
                if line.width > name_w {
                    let runs = [run(l.name.len(), f, l.color)];
                    let (cut, runs) = cx
                        .text_system()
                        .line_wrapper(f.clone(), size)
                        .truncate_line(l.name.clone(), name_w, "…", &runs, TruncateFrom::End);
                    line = window.text_system().shape_line(cut, size, &runs, None);
                }
                let _ = line.paint(
                    point(x0 + px(geo.name_x()), y),
                    row_h,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            // Колонки прижаты вправо: дата у края, размер левее.
            let mut right = x0 + px(col_w - ROW_MX - geo.pad);
            for (s, w) in [(&l.mtime, geo.mtime_w), (&l.size, geo.size_w)] {
                if w <= 0.0 {
                    continue;
                }
                if let Some(s) = s {
                    let line = shape(window, s, &font, secondary, size);
                    let _ = line.paint(
                        point(right - line.width, y),
                        row_h,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
                right -= px(w);
            }
        }
    });
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
