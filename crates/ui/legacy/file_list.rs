//! `FileList`: «Таблица» (Full), Wide, «Список» (Brief) и Medium (S2 §2.1–2.2, S2-10,
//! S2-11). Отрисовка — через
//! [`Canvas`] (снапшоты без GPU), ввод — через `egui::Ui` (клики, колесо, перетаскивание
//! разделителей колонок). Тексты строк — из [`RowCache`].
//!
//! Brief — колонки только с именем, по ширине (не уже 160 px), Medium — две колонки
//! «имя + размер»; строки идут по столбцам: элемент `top + блок·rows + строка`,
//! ← → и `PgDn` считает core по `cols` из `panel.viewport` (S0 v3 №3).
//!
//! Карточка: путь (`h.breadcrumb`; breadcrumb — S2-12) → заголовки колонок
//! (`h.col_header`, клик — `panel.sort`, разделители тянутся) → строки 24 px → подвал
//! (`h.panel_footer`, подсказка — точный размер). Состояния: загрузка дольше 150 мс —
//! полоса 2 px и счётчик, ошибка — значок, текст и «Повторить», пусто — «Папка пуста».

use std::sync::Arc;
use std::time::Duration;

use egui::{Align2, CursorIcon, Pos2, Rect, Sense, pos2, vec2};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use urfin_core::{Command, ListingStatus, Row, Side, SortKey, Tab, TabId, ViewMode};
use urfin_i18n::{FluentArgs, I18n};

use crate::legacy::cache::RowCache;
use crate::legacy::canvas::Canvas;
use crate::legacy::fmt_cell::FmtCfg;
use crate::legacy::fonts::{Font, FontSet};
use crate::legacy::highlight::Highlighter;
use crate::legacy::panel::{
    CardMemo, IconTone, Line, LineCx, fill_lines, hold_left, rows_fit, side_arg,
};
use crate::legacy::text::{self, Tx};
use crate::legacy::tokens::{Metric, Role, Tokens};

/// Ширины колонок по умолчанию (S2 §1.2): метка 12 · иконка 20 · имя · размер 72 · дата 112.
const MARK_W: f32 = 12.0;
const ICON_W: f32 = 20.0;
const SIZE_W: f32 = 72.0;
const MTIME_W: f32 = 112.0;
const NAME_MIN: f32 = 120.0;
/// Зазор между колонками и ширина ручки разделителя.
const COL_GAP: f32 = 8.0;
const HANDLE_W: f32 = 6.0;
/// Строк на деление колеса (как в Windows по умолчанию).
const WHEEL_ROWS: f32 = 3.0;
/// Brief: колонка не уже (S2 §2.1); больше колонок не бывает и на 4K.
const BRIEF_MIN: f32 = 160.0;
const MAX_BLOCKS: usize = 24;
/// Medium: колонок «имя + размер» (S2 §2.2).
const MEDIUM_BLOCKS: usize = 2;

/// Колонки строк по блокам слева направо (без кучи: блоков ≤ [`MAX_BLOCKS`]).
pub type Blocks = SmallVec<[Columns; MAX_BLOCKS]>;

/// Ширины колонок таба в режиме (тянутся мышью; в сессию — S2-22).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ColWidths {
    pub size: f32,
    pub mtime: f32,
}

impl Default for ColWidths {
    fn default() -> Self {
        Self {
            size: SIZE_W,
            mtime: MTIME_W,
        }
    }
}

/// Ширины колонок всех табов.
pub type Widths = FxHashMap<(TabId, ViewMode), ColWidths>;

/// Горизонтальная раскладка строки: колонки в координатах x.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Columns {
    /// Границы блока (строка, подсветка курсора).
    pub block: (f32, f32),
    pub mark: (f32, f32),
    pub icon: Option<(f32, f32)>,
    pub name: (f32, f32),
    pub size: (f32, f32),
    pub mtime: Option<(f32, f32)>,
}

impl Columns {
    /// Колонки для строки `left..right`: Full — имя, размер, дата; Wide и Medium — имя и
    /// размер; Brief — только имя.
    pub fn new(left: f32, right: f32, mode: ViewMode, w: ColWidths, icons: bool, pad: f32) -> Self {
        let mark = (left + 4.0, left + 4.0 + MARK_W);
        let icon = icons.then_some((mark.1, mark.1 + ICON_W));
        let name_left = icon.map_or(mark.1, |i| i.1) + 4.0;
        let full = mode == ViewMode::Full;
        if mode == ViewMode::Brief {
            let name = (name_left, (right - pad).max(name_left));
            return Self {
                block: (left, right),
                mark,
                icon,
                name,
                size: (name.1, name.1),
                mtime: None,
            };
        }
        let avail = right - pad - name_left;
        // Имя — не уже 120: лишняя ширина колонок срезается.
        let (mut size_w, mut mtime_w) = (w.size, if full { w.mtime } else { 0.0 });
        let fixed = size_w + mtime_w + COL_GAP * if full { 2.0 } else { 1.0 };
        if avail - fixed < NAME_MIN {
            let k = ((avail - NAME_MIN - COL_GAP * 2.0) / (size_w + mtime_w)).clamp(0.0, 1.0);
            size_w *= k;
            mtime_w *= k;
        }
        let mtime = full.then_some((right - pad - mtime_w, right - pad));
        let size_right = mtime.map_or(right - pad, |m| m.0 - COL_GAP);
        let size = (size_right - size_w, size_right);
        let name = (name_left, (size.0 - COL_GAP).max(name_left));
        Self {
            block: (left, right),
            mark,
            icon,
            name,
            size,
            mtime,
        }
    }

    /// Есть колонка размера.
    pub fn has_size(&self) -> bool {
        self.size.1 > self.size.0
    }
}

/// Блоки колонок вида `mode` в `left..right`: Brief — по ширине, Medium — два, прочие — один.
pub fn blocks(
    left: f32,
    right: f32,
    mode: ViewMode,
    w: ColWidths,
    icons: bool,
    pad: f32,
) -> Blocks {
    let width = right - left;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "колонок — единицы"
    )]
    let n = match mode {
        ViewMode::Brief => ((width / BRIEF_MIN).floor() as usize).clamp(1, MAX_BLOCKS),
        ViewMode::Medium => MEDIUM_BLOCKS,
        _ => 1,
    };
    // Целые пиксели: разделители колонок — чёткие линии 1 px.
    #[allow(clippy::cast_precision_loss, reason = "колонок — единицы")]
    let bw = (width / n as f32).floor();
    (0..n)
        .map(|i| {
            #[allow(clippy::cast_precision_loss, reason = "колонок — единицы")]
            let l = left + bw * i as f32;
            Columns::new(l, l + bw, mode, w, icons, pad)
        })
        .collect()
}

/// Вид для раскладки: Custom (M2+) пока рисуется таблицей.
fn layout_mode(m: ViewMode) -> ViewMode {
    match m {
        ViewMode::Full | ViewMode::Wide | ViewMode::Brief | ViewMode::Medium => m,
        ViewMode::Custom(_) => ViewMode::Full,
    }
}

/// Входы виджета.
pub(crate) struct ListCx<'a> {
    pub ui: &'a mut egui::Ui,
    pub i18n: &'a I18n,
    pub tokens: &'a Tokens,
    pub fonts: &'a FontSet,
    pub hl: &'a Highlighter,
    pub fmt: &'a FmtCfg,
    pub cache: &'a mut RowCache,
    pub memo: &'a mut CardMemo,
    pub widths: &'a mut Widths,
    pub st: &'a crate::legacy::state::UiState,
    /// Быстрый поиск и начатый аккорд (keymap, ADR-0016).
    pub keys: Option<&'a urfin_keymap::KeymapView>,
    /// Имя `dst` открытого конфликта копирования — строка `caution.bg` (S2 §2.8).
    pub conflict: Option<&'a str>,
}

/// Имя, которое переименовывается в этом табе (`ui.rename`, S2-16).
fn rename_target<'a>(cx: &ListCx<'a>, t: &Tab) -> Option<&'a str> {
    cx.st
        .rename
        .as_ref()
        .filter(|(id, _)| *id == t.id)
        .map(|(_, name)| name.as_str())
}

/// Поле переименования поверх имени и размера строки `row` (S2 §2.10): выделена основа
/// имени без расширения; Enter — `ops.rename to=`, Esc — keymap (`ui.cancel`).
fn rename_field(
    c: &mut dyn Canvas,
    cx: &mut ListCx<'_>,
    row: Rect,
    name: &str,
    out: &mut Vec<Command>,
) {
    let tk = cx.tokens;
    let opened = cx.memo.rename.as_ref().is_none_or(|(n, _)| n != name);
    if opened {
        cx.memo.rename = Some((name.into(), name.to_owned()));
    }
    let radius = egui::CornerRadius::from(tk.m(Metric::RadiusControl));
    c.rect(row, Role::FillControlInput, radius);
    c.rect(
        row.with_min_y(row.bottom() - 2.0 * tk.m(Metric::StrokeThin)),
        Role::AccentFill,
        egui::CornerRadius::ZERO,
    );
    c.region("rename", row);
    let text = &mut cx.memo.rename.as_mut().expect("set above").1;
    let mut ui = cx
        .ui
        .new_child(egui::UiBuilder::new().max_rect(row.shrink(1.0)));
    let id = ui.scope_id().with("rename-field");
    let resp = ui.add_sized(
        row.shrink(1.0).size(),
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(egui::Frame::NONE)
            .font(cx.fonts.id(Font::Ui, tk.m(Metric::TypeBody))),
    );
    if opened {
        resp.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            let stem = stem_chars(name);
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(stem),
                )));
            state.store(ui.ctx(), id);
        }
    } else if !resp.has_focus() && !resp.lost_focus() {
        resp.request_focus();
    }
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let to = text.trim();
        if !to.is_empty() && to != name {
            out.push(Command::new("ops.rename").arg("to", to));
        }
        out.push(Command::new("ui.cancel"));
    }
}

/// Длина основы имени в символах: до последней точки; у `.gitignore` и имён без точки —
/// всё имя.
fn stem_chars(name: &str) -> usize {
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].chars().count(),
        _ => name.chars().count(),
    }
}

/// Подсветка поля поиска после не принятого символа (S2 §576).
const MISS_FLASH: Duration = Duration::from_millis(300);

/// Подвал: строка быстрого поиска (S2 §576, ADR-0016), начатый аккорд на активной
/// панели или итоги каталога.
fn footer_line(
    c: &mut dyn Canvas,
    cx: &mut ListCx<'_>,
    foot_r: Rect,
    side: Side,
    active: bool,
    pad: f32,
    caption: f32,
) {
    let keys = cx.keys;
    if let Some(q) = keys.and_then(|k| k.qs.as_ref()).filter(|q| q.side == side) {
        let tk = cx.tokens;
        c.rect(foot_r, Role::FillControlInput, egui::CornerRadius::ZERO);
        let flash = q
            .miss_at
            .map(|t| MISS_FLASH.saturating_sub(t.elapsed()))
            .filter(|d| !d.is_zero());
        if let Some(left) = flash {
            cx.ui.ctx().request_repaint_after(left);
        }
        let edge = 2.0 * tk.m(Metric::StrokeThin);
        c.rect(
            foot_r.with_min_y(foot_r.bottom() - edge),
            if flash.is_some() {
                Role::CriticalFg
            } else {
                Role::AccentFill
            },
            egui::CornerRadius::ZERO,
        );
        let icon_x = foot_r.left() + pad + caption * 0.5;
        c.icon(
            pos2(icon_x, foot_r.center().y),
            crate::legacy::icons::SEARCH,
            caption,
            Role::TextSecondary,
        );
        c.text(
            pos2(icon_x + caption, foot_r.center().y),
            Align2::LEFT_CENTER,
            &Tx::user(q.mask.as_str()),
            Font::Ui,
            caption,
            Role::TextPrimary,
        );
        if !q.mask.is_empty() {
            let mut args = FluentArgs::new();
            args.set("count", u64::from(q.matches));
            c.text(
                foot_r.right_center() - vec2(pad, 0.0),
                Align2::RIGHT_CENTER,
                &Tx::Args("ui-qs-matches", args),
                Font::Caption,
                caption,
                Role::TextSecondary,
            );
        }
        return;
    }
    if let Some(chord) = keys.and_then(|k| k.pending.as_deref()).filter(|_| active) {
        let mut args = FluentArgs::new();
        args.set("chord", chord.to_owned());
        c.text(
            foot_r.left_center() + vec2(pad, 0.0),
            Align2::LEFT_CENTER,
            &Tx::Args("ui-chord-pending", args),
            Font::Caption,
            caption,
            Role::TextPrimary,
        );
        return;
    }
    c.text(
        foot_r.left_center() + vec2(pad, 0.0),
        Align2::LEFT_CENTER,
        &Tx::user(cx.memo.footer.as_str()),
        Font::Caption,
        caption,
        Role::TextSecondary,
    );
}

/// Карточка панели в прямоугольнике `r` (подложку и полосу акцента рисует `frame`).
#[allow(clippy::too_many_lines, reason = "последовательная раскладка карточки")]
pub(crate) fn show(
    c: &mut dyn Canvas,
    r: Rect,
    t: &Tab,
    side: Side,
    active: bool,
    cx: &mut ListCx<'_>,
    out: &mut Vec<Command>,
) {
    let tk = cx.tokens;
    let inner = r.shrink(tk.m(Metric::SpaceXs));
    let pad = tk.m(Metric::PadRowX);
    let path_r = inner.with_max_y(inner.top() + tk.m(Metric::HBreadcrumb));
    let head_r = Rect::from_min_max(
        pos2(inner.left(), path_r.bottom()),
        pos2(inner.right(), path_r.bottom() + tk.m(Metric::HColHeader)),
    );
    let foot_r = inner.with_min_y(inner.bottom() - tk.m(Metric::HPanelFooter));
    let list_r = Rect::from_min_max(
        pos2(inner.left(), head_r.bottom()),
        pos2(inner.right(), foot_r.top()),
    );
    let row_h = tk.m(Metric::HRow);
    let ctx = cx.ui.ctx().clone();

    // Новый каталог ещё ничего не прислал — прежний кадр (≤ 150 мс), иначе переход мигает.
    let held = hold_left(t).filter(|_| cx.memo.has_frame);
    if let Some(left) = held {
        ctx.request_repaint_after(left);
    } else {
        cx.memo.refresh_texts(cx.i18n, t);
    }
    let interactive = held.is_none();

    // Путь — breadcrumb (S2-12).
    let body = cx.fonts.id(Font::Ui, tk.m(Metric::TypeBody));
    let caption = tk.m(Metric::TypeCaption);
    crate::legacy::breadcrumb::show(
        c,
        &mut crate::legacy::breadcrumb::CrumbCx {
            ui: cx.ui,
            tokens: tk,
            fonts: cx.fonts,
            st: cx.st,
            memo: &mut cx.memo.crumbs,
        },
        path_r,
        &t.loc,
        side,
        active,
        out,
    );

    // Колонки и заголовки.
    let mode = layout_mode(t.view_mode);
    let widths = *cx.widths.entry((t.id, mode)).or_default();
    let blocks = blocks(
        list_r.left(),
        list_r.right(),
        mode,
        widths,
        cx.fonts.icons,
        pad,
    );
    let cols = blocks[0];
    header(c, cx, head_r, &blocks, t, side, active, mode, out);

    // Строк в колонке, колонки и `panel.viewport` — только при изменении (S0 v3 №3).
    let page = rows_fit(list_r.height(), row_h);
    let ncols = u32::try_from(blocks.len()).unwrap_or(1);
    if (cx.memo.page, cx.memo.cols) != (page, ncols) {
        cx.memo.page = page;
        cx.memo.cols = ncols;
        out.push(
            Command::new("panel.viewport")
                .arg("side", side_arg(side))
                .arg("rows", i64::from(page))
                .arg("cols", i64::from(ncols)),
        );
    }

    // Строки.
    if interactive {
        wheel(
            cx.ui,
            list_r,
            &mut cx.memo.wheel_acc,
            t,
            side,
            (page, ncols),
            out,
        );
        let digit_slot = (!cx.fonts.tabular_ok(Font::Ui)).then(|| FontSet::digit_slot(&ctx, &body));
        let lcx = LineCx {
            ctx: &ctx,
            fmt: cx.fmt,
            hl: cx.hl,
            font: body.clone(),
            name_w: cols.name.1 - cols.name.0,
            digit_slot,
            size: cols.has_size(),
            mtime: cols.mtime.is_some(),
        };
        fill_lines(&mut cx.memo.lines, cx.cache, &lcx, t, page * ncols);
        cx.memo.has_frame = true;
    }
    let lines = std::mem::take(&mut cx.memo.lines);
    rows(
        c,
        cx,
        list_r,
        &blocks,
        &lines,
        t,
        side,
        active,
        interactive,
        out,
    );
    cx.memo.lines = lines;
    state(c, cx, list_r, path_r, t, interactive, out);

    // Разделители и подвал.
    let thin = tk.m(Metric::StrokeThin);
    for y in [path_r.bottom(), foot_r.top()] {
        c.rect(
            Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), thin)),
            Role::StrokeDivider,
            egui::CornerRadius::ZERO,
        );
    }
    if rename_target(cx, t).is_some() {
        // Подсказка вместо итогов (S2 §2.10).
        c.text(
            foot_r.left_center() + vec2(pad, 0.0),
            Align2::LEFT_CENTER,
            &Tx::key("ui-rename-hint"),
            Font::Caption,
            caption,
            Role::TextSecondary,
        );
    } else {
        cx.memo.rename = None;
        footer_line(c, cx, foot_r, side, active, pad, caption);
    }
    // Подсказка подвала — точный размер (S2 §2.1).
    let foot = cx.ui.interact(
        foot_r,
        cx.ui.scope_id().with(("footer", side_arg(side))),
        Sense::hover(),
    );
    if foot.hovered() {
        let bytes = if t.selection.is_empty() {
            t.listing.stats.bytes
        } else {
            t.selection.bytes()
        };
        let exact = urfin_i18n::fmt::bytes_exact_with(cx.i18n, bytes);
        text::tooltip(&foot, cx.i18n, &Tx::user(exact));
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "заголовок использует всю раскладку; геометрия: c, r, t, x, y"
)]
fn header(
    c: &mut dyn Canvas,
    cx: &mut ListCx<'_>,
    r: Rect,
    blocks: &[Columns],
    t: &Tab,
    side: Side,
    active: bool,
    mode: ViewMode,
    out: &mut Vec<Command>,
) {
    let tk = cx.tokens;
    let size = tk.m(Metric::TypeCaption);
    let y = r.center().y;
    let arrow = |key: SortKey| match (t.sort.key == key, t.sort.desc) {
        (true, false) => " ˄",
        (true, true) => " ˅",
        (false, _) => "",
    };
    // Brief — один заголовок «Имя» над первой колонкой, Medium — над каждой (S2 §2.1–2.2);
    // стрелка сортировки — только у первой.
    let titled = if mode == ViewMode::Brief {
        1
    } else {
        blocks.len()
    };
    for (b, cols) in blocks.iter().take(titled).enumerate() {
        let titles: [(&str, SortKey, (f32, f32), Align2); 3] = [
            ("ui-col-name", SortKey::Name, cols.name, Align2::LEFT_CENTER),
            (
                "ui-col-size",
                SortKey::Size,
                cols.size,
                Align2::RIGHT_CENTER,
            ),
            (
                "ui-col-mtime",
                SortKey::Mtime,
                cols.mtime.unwrap_or((0.0, 0.0)),
                Align2::LEFT_CENTER,
            ),
        ];
        for (key, sort, (x0, x1), align) in titles {
            if x1 <= x0 {
                continue;
            }
            let title = if b == 0 {
                format!("{}{}", cx.i18n.get(key), arrow(sort))
            } else {
                cx.i18n.get(key)
            };
            let pos = if align == Align2::RIGHT_CENTER {
                pos2(x1, y)
            } else {
                pos2(x0, y)
            };
            c.text(
                pos,
                align,
                &Tx::user(title),
                Font::Caption,
                size,
                Role::TextSecondary,
            );
            let cell = Rect::from_min_max(pos2(x0, r.top()), pos2(x1, r.bottom()));
            let resp = cx.ui.interact(
                cell,
                cx.ui.scope_id().with(("col", side_arg(side), b, key)),
                Sense::click(),
            );
            if resp.clicked() {
                if !active {
                    out.push(Command::new("panel.focus").arg("side", side_arg(side)));
                }
                let k = match sort {
                    SortKey::Name => "name",
                    SortKey::Size => "size",
                    _ => "mtime",
                };
                out.push(Command::new("panel.sort").arg("key", k));
            }
        }
    }
    // Ручки ширины — только в одноколоночных видах (Full, Wide).
    if let [cols] = blocks {
        resize_handles(c, cx, r, cols, t, side, mode);
    }
}

/// Разделители заголовка: левая граница «Размера» и «Изменён» тянется, двойной клик —
/// ширины по умолчанию.
#[allow(
    clippy::too_many_arguments,
    clippy::many_single_char_names,
    reason = "раскладка заголовка; геометрия: c, r, t, x, y"
)]
fn resize_handles(
    c: &mut dyn Canvas,
    cx: &mut ListCx<'_>,
    r: Rect,
    cols: &Columns,
    t: &Tab,
    side: Side,
    mode: ViewMode,
) {
    let y = r.center().y;
    let handles = [
        (cols.size.0, 0usize),
        (cols.mtime.map_or(f32::NAN, |m| m.0), 1),
    ];
    for (x, which) in handles {
        if !x.is_finite() {
            continue;
        }
        let hr = Rect::from_center_size(pos2(x - COL_GAP / 2.0, y), vec2(HANDLE_W, r.height()));
        c.rect(
            Rect::from_center_size(hr.center(), vec2(1.0, r.height() * 0.5)),
            Role::StrokeDivider,
            egui::CornerRadius::ZERO,
        );
        let resp = cx.ui.interact(
            hr,
            cx.ui.scope_id().with(("divider", side_arg(side), which)),
            Sense::click_and_drag(),
        );
        if resp.hovered() || resp.dragged() {
            cx.ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        let w = cx.widths.entry((t.id, mode)).or_default();
        if resp.double_clicked() {
            *w = ColWidths::default();
        } else if resp.dragged() {
            let dx = resp.drag_delta().x;
            let v = if which == 0 {
                &mut w.size
            } else {
                &mut w.mtime
            };
            // Колонка справа от разделителя: тянем влево — шире. Кратно 4 px (S2 §6.4).
            *v = (((*v - dx) / 4.0).round() * 4.0).clamp(40.0, 400.0);
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "строки используют всю раскладку; геометрия: c, t, i, g, x"
)]
fn rows(
    c: &mut dyn Canvas,
    cx: &mut ListCx<'_>,
    list: Rect,
    blocks: &[Columns],
    lines: &[Line],
    t: &Tab,
    side: Side,
    active: bool,
    interactive: bool,
    out: &mut Vec<Command>,
) {
    let tk = cx.tokens;
    let row_h = tk.m(Metric::HRow);
    let radius = tk.radius(Metric::RadiusControl);
    let ctx = cx.ui.ctx().clone();
    let icon_font = cx.fonts.id(Font::Icons, tk.m(Metric::WIcon));
    let mark_font = cx.fonts.id(Font::Icons, tk.m(Metric::TypeCaption));
    let ctrl = cx.ui.input(|i| i.modifiers.command);
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let per_col = rows_fit(list.height(), row_h).max(1) as usize;
    col_dividers(c, tk, list, blocks);
    for (i, line) in lines.iter().enumerate() {
        let Some(cols) = blocks.get(i / per_col) else {
            break;
        };
        #[allow(clippy::cast_precision_loss, reason = "строк на странице — сотни")]
        let top = list.top() + row_h * (i % per_col) as f32;
        let row = Rect::from_min_max(pos2(cols.block.0, top), pos2(cols.block.1, top + row_h));
        if row.bottom() > list.bottom() + 0.5 {
            continue;
        }
        let mid = row.center().y;
        let mut color = cx
            .hl
            .color(line.group, tk)
            .unwrap_or_else(|| tk.c(Role::TextPrimary));
        if let Some(name) = cx.conflict
            && matches!(t.row(line.row), Some(Row::Entry { entry, .. }) if entry.name == name)
        {
            c.rect(row, Role::CautionBg, radius);
        }
        // Помеченные — `accent.subtle` и `accent.text`, курсор — рамка 1 px (S0-decisions №31).
        if line.selected {
            c.rect(row, Role::AccentSubtle, radius);
            color = tk.c(Role::AccentText);
        }
        if line.cursor && active {
            c.rect(row, Role::FillCursor, radius);
            c.stroke(row, Role::StrokeStrong, radius, tk.m(Metric::StrokeThin));
        } else if line.cursor {
            c.rect(row, Role::FillCursorPassive, radius);
        }
        if line.selected {
            let g = cx
                .cache
                .icon(&ctx, crate::legacy::icons::ACCEPT, &mark_font);
            c.galley(centered(cols.mark, mid, &g), g, Role::AccentText);
        }
        if let Some(ic) = cols.icon {
            let role = match line.icon_tone {
                IconTone::Dir => Role::AccentText,
                IconTone::Hidden => Role::TextTertiary,
                IconTone::Normal => Role::TextSecondary,
            };
            let g = cx.cache.icon(&ctx, line.icon, &icon_font);
            c.galley(centered(ic, mid, &g), g, role);
        }
        if let Some(name) = rename_target(cx, t)
            && matches!(t.row(line.row), Some(Row::Entry { entry, .. }) if entry.name == name)
        {
            let field = Rect::from_min_max(
                pos2(cols.name.0 - 2.0, row.top() + 1.0),
                pos2(cols.size.1.max(cols.name.1), row.bottom() - 1.0),
            );
            rename_field(c, cx, field, name, out);
        } else {
            c.tinted(
                pos2(cols.name.0, mid - line.name.size().y / 2.0),
                Arc::clone(&line.name),
                color,
            );
        }
        let renaming = rename_target(cx, t).is_some_and(
            |name| matches!(t.row(line.row), Some(Row::Entry { entry, .. }) if entry.name == name),
        );
        if let Some((g, lead, width)) = line.size.as_ref().filter(|_| !renaming) {
            let x = cols.size.1 - width + lead;
            c.galley(
                pos2(x, mid - g.size().y / 2.0),
                Arc::clone(g),
                Role::TextSecondary,
            );
        }
        if let (Some(m), Some(g)) = (cols.mtime, &line.mtime) {
            c.galley(
                pos2(m.0, mid - g.size().y / 2.0),
                Arc::clone(g),
                Role::TextSecondary,
            );
        }
        if !interactive {
            continue;
        }
        let resp = cx.ui.interact(
            row,
            cx.ui.scope_id().with(("row", side_arg(side), i)),
            sense,
        );
        let to_row = || match t.row(line.row) {
            Some(Row::Entry { entry, .. }) => {
                Command::new("panel.cursor.to").arg("name", entry.name.as_str())
            }
            _ => Command::new("panel.cursor.home"),
        };
        if resp.clicked() || resp.secondary_clicked() {
            if !active {
                out.push(Command::new("panel.focus").arg("side", side_arg(side)));
            }
            out.push(to_row());
            // Ctrl+клик и ПКМ — пометка (ПКМ — как в Far); `..` не помечается.
            if (resp.secondary_clicked() || ctrl) && line.entry {
                out.push(Command::new("sel.toggle"));
            }
        }
        if resp.double_clicked() {
            out.push(Command::new("panel.enter"));
        }
    }
}

/// Разделители колонок многоколоночных видов (Brief, Medium).
fn col_dividers(c: &mut dyn Canvas, tk: &Tokens, list: Rect, blocks: &[Columns]) {
    for b in blocks.iter().skip(1) {
        c.rect(
            Rect::from_min_size(
                pos2(b.block.0, list.top()),
                vec2(tk.m(Metric::StrokeThin), list.height()),
            ),
            Role::StrokeDivider,
            egui::CornerRadius::ZERO,
        );
    }
}

/// Загрузка дольше 150 мс, ошибка, пустая папка.
fn state(
    c: &mut dyn Canvas,
    cx: &mut ListCx<'_>,
    list: Rect,
    path: Rect,
    t: &Tab,
    interactive: bool,
    out: &mut Vec<Command>,
) {
    let tk = cx.tokens;
    let center = list.center();
    match &t.listing.status {
        ListingStatus::Loading { since }
            if interactive && since.elapsed() >= crate::legacy::panel::HOLD =>
        {
            // Индетерминированная полоса 2 px под путём (S2 §2.1).
            let time = cx.ui.input(|i| i.time) as f32;
            let w = list.width() * 0.3;
            let x = list.left() + (time * 0.8).fract() * (list.width() + w) - w;
            let bar = Rect::from_min_max(
                pos2(x.max(list.left()), path.bottom()),
                pos2((x + w).min(list.right()), path.bottom() + 2.0),
            );
            c.rect(bar, Role::AccentFill, egui::CornerRadius::same(1));
            cx.ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        ListingStatus::Failed(e) => {
            let g = cx.cache.icon(
                cx.ui.ctx(),
                crate::legacy::icons::WARNING,
                &cx.fonts.id(Font::Icons, 32.0),
            );
            c.galley(
                centered((center.x - 16.0, center.x + 16.0), center.y - 40.0, &g),
                g,
                Role::CautionFg,
            );
            let mut args = FluentArgs::new();
            args.set("error", e.to_string());
            c.text(
                center,
                Align2::CENTER_CENTER,
                &Tx::Args("ui-listing-failed", args),
                Font::Ui,
                tk.m(Metric::TypeBody),
                Role::TextSecondary,
            );
            let btn = Rect::from_center_size(
                center + vec2(0.0, 40.0),
                vec2(120.0, tk.m(Metric::HControl) + 8.0),
            );
            let mut ui = cx.ui.new_child(egui::UiBuilder::new().max_rect(btn));
            if text::button(&mut ui, cx.i18n, &Tx::key("ui-retry")).clicked() {
                out.push(Command::new("panel.refresh"));
            }
        }
        ListingStatus::Loading { .. } => {}
        _ if t.listing.order.is_empty() => {
            c.text(
                center,
                Align2::CENTER_CENTER,
                &Tx::key("ui-folder-empty"),
                Font::Ui,
                tk.m(Metric::TypeBody),
                Role::TextTertiary,
            );
        }
        _ => {}
    }
}

fn centered(col: (f32, f32), mid: f32, g: &egui::Galley) -> Pos2 {
    pos2((col.0 + col.1 - g.size().x) / 2.0, mid - g.size().y / 2.0)
}

/// Колесо над списком — `panel.viewport{top}`: сырые события `MouseWheel` с остатком
/// (сглаженная дельта egui приходит долями строки).
fn wheel(
    ui: &egui::Ui,
    list: Rect,
    acc: &mut f32,
    t: &Tab,
    side: Side,
    (page, cols): (u32, u32),
    out: &mut Vec<Command>,
) {
    let over = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| list.contains(p));
    if !over {
        *acc = 0.0;
        return;
    }
    #[allow(clippy::cast_precision_loss, reason = "страница — сотни строк")]
    let page_f = page as f32;
    let rows: f32 = ui.input(|i| {
        i.raw
            .events
            .iter()
            .map(|e| match e {
                egui::Event::MouseWheel { unit, delta, .. } => match unit {
                    egui::MouseWheelUnit::Line => -delta.y * WHEEL_ROWS,
                    egui::MouseWheelUnit::Point => -delta.y / 24.0,
                    egui::MouseWheelUnit::Page => -delta.y * page_f,
                },
                _ => 0.0,
            })
            .sum()
    });
    *acc += rows;
    let whole = acc.trunc();
    *acc -= whole;
    #[allow(clippy::cast_possible_truncation, reason = "строк за кадр немного")]
    let lines = whole as i64;
    if lines != 0 {
        let top = (i64::from(t.cursor.top) + lines).max(0);
        out.push(
            Command::new("panel.viewport")
                .arg("side", side_arg(side))
                .arg("rows", i64::from(page))
                .arg("cols", i64::from(cols))
                .arg("top", top),
        );
    }
}

#[cfg(test)]
#[path = "file_list_tests.rs"]
mod tests;
