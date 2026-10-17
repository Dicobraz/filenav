//! Карточка панели: табы, путь, строки, подвал. Строки — временная простая отрисовка
//! (имя и размер), пока нет `FileList` (S2-10); цвета и размеры — из токенов.

use std::time::Duration;

use std::fmt::Write as _;
use std::sync::Arc;

use egui::{CornerRadius, FontId, Galley, Sense, Stroke, StrokeKind};
use filenav_core::{Command, ListingStatus, Row, Side, Tab};
use filenav_i18n::{FluentArgs, I18n};

use crate::cache::{CellKey, CellSpec, RowCache, fingerprint};
use crate::fmt_cell::{ColKind, FmtCfg, cell_text};
use crate::fonts::{Font, FontSet};
use crate::highlight::Highlighter;
use crate::tokens::{Metric, Role, Tokens};

/// Пока новый каталог ничего не прислал, панель показывает прежний кадр — не
/// дольше этого (S2: `Loading` > 150 мс — индикатор загрузки).
pub(crate) const HOLD: Duration = Duration::from_millis(150);

/// Строка списка, готовая к отрисовке: текст — galley из [`RowCache`].
#[derive(Debug, Clone)]
pub(crate) struct Line {
    /// Строка таба (`Tab::row`): по ней клик строит команду.
    pub row: u32,
    pub name: Arc<Galley>,
    /// Размер: galley, сдвиг первой цифры и ширина по слотам.
    pub size: Option<(Arc<Galley>, f32, f32)>,
    pub cursor: bool,
    pub selected: bool,
    /// Группа раскраски ([`Highlighter::classify`]), 0 — без цвета.
    pub group: u8,
    /// Глиф иконки (S2 §1.6) и тон: каталог — `accent.text`, скрытый — `text.tertiary`.
    pub icon: char,
    pub icon_tone: IconTone,
    pub entry: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum IconTone {
    Normal,
    Dir,
    Hidden,
}

/// Состояние карточки между кадрами. Буфер строк переиспользуется; путь и подвал
/// пересчитываются, только когда меняются их входы.
#[derive(Debug, Default)]
pub(crate) struct CardMemo {
    /// Последний сообщённый размер страницы (чтобы не слать `panel.viewport` каждый кадр).
    pub page: u32,
    /// Кадр, каким его увидел пользователь: путь, подвал, строки.
    pub lines: Vec<Line>,
    pub path: String,
    pub footer: String,
    /// Есть что показать при удержании прежнего кадра.
    pub has_frame: bool,
    path_key: Option<(filenav_core::TabId, filenav_core::Location)>,
    footer_key: Option<FooterKey>,
}

/// Входы подвала.
#[derive(Debug, Clone, PartialEq)]
struct FooterKey {
    tab: filenav_core::TabId,
    generation: filenav_core::Gen,
    status: u8,
    dirs: u32,
    files: u32,
    bytes: u64,
    sel: (u32, u64),
    epoch: u64,
}

impl CardMemo {
    /// Путь и подвал таба — пересчёт только при смене входов (без аллокаций в простое).
    pub(crate) fn refresh_texts(&mut self, i18n: &I18n, t: &Tab) {
        if self
            .path_key
            .as_ref()
            .is_none_or(|(id, loc)| *id != t.id || *loc != t.loc)
        {
            self.path = path_text(t);
            self.path_key = Some((t.id, t.loc.clone()));
        }
        let status = match &t.listing.status {
            ListingStatus::Loading { .. } => 0,
            ListingStatus::Failed(_) => 1,
            _ => 2,
        };
        let s = t.listing.stats;
        let key = FooterKey {
            tab: t.id,
            generation: t.listing.generation,
            status,
            dirs: s.dirs,
            files: s.files,
            bytes: s.bytes,
            sel: (t.selection.len(), t.selection.bytes()),
            epoch: filenav_i18n::epoch(),
        };
        if self.footer_key.as_ref() != Some(&key) {
            self.footer = footer(i18n, t);
            self.footer_key = Some(key);
        }
    }
}

/// Аргумент `side` команд.
pub(crate) fn side_arg(side: Side) -> &'static str {
    if side == Side::Left { "left" } else { "right" }
}

/// Пока новый листинг пуст и грузится меньше [`HOLD`] — сколько ещё держать
/// прежний кадр.
pub(crate) fn hold_left(t: &Tab) -> Option<Duration> {
    match t.listing.status {
        ListingStatus::Loading { since } if t.listing.entries.is_empty() => {
            HOLD.checked_sub(since.elapsed()).filter(|d| !d.is_zero())
        }
        _ => None,
    }
}

pub(crate) fn path_text(t: &Tab) -> String {
    if t.loc.is_local() {
        t.loc.path.display().to_string()
    } else {
        t.loc.to_uri()
    }
}

/// Видимые строки таба: `page` строк от `cursor.top`.
/// Ширина колонки размера во временном списке (S2 §1.2, Full: 72).
pub(crate) const SIZE_W: f32 = 72.0;

/// Входы разметки строк.
pub(crate) struct LineCx<'a> {
    pub ctx: &'a egui::Context,
    pub fmt: &'a FmtCfg,
    pub hl: &'a Highlighter,
    pub font: FontId,
    /// Ширина имени (логич.) и слот цифры размера (`None` — цифры табличные).
    pub name_w: f32,
    pub digit_slot: Option<f32>,
}

/// Видимые строки таба (`page` строк от `cursor.top`) в `out` (буфер переиспользуется).
pub(crate) fn fill_lines(
    out: &mut Vec<Line>,
    cache: &mut RowCache,
    cx: &LineCx<'_>,
    t: &Tab,
    page: u32,
) {
    out.clear();
    let ppp = cx.ctx.pixels_per_point();
    // Ширина в ключе — физические пиксели, по 4: ресайз колонки не плодит ключи.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "ширина колонки"
    )]
    let width_px = ((cx.name_w * ppp / 4.0).round() * 4.0).clamp(0.0, f32::from(u16::MAX)) as u16;
    let top = t.cursor.top;
    for r in top..(top + page).min(t.rows()) {
        let Some(row) = t.row(r) else { break };
        let base = CellKey {
            tab: t.id,
            generation: t.listing.generation,
            idx: u32::MAX,
            col: ColKind::Name as u8,
            width_px,
        };
        let line = match row {
            Row::Parent => {
                let name = cache.get_or_layout(base, 0, cx.ctx, |c| {
                    let _ = c.write_str("..");
                    spec(&cx.font, cx.name_w, None)
                });
                Line {
                    row: r,
                    name: Arc::clone(&name.galley),
                    size: None,
                    cursor: r == t.cursor.row,
                    selected: false,
                    group: 0,
                    icon: crate::icons::PARENT,
                    icon_tone: IconTone::Normal,
                    entry: false,
                }
            }
            Row::Entry { idx, entry } => {
                let fp = fingerprint(entry);
                let dir = entry.kind.is_dir_like();
                let name = cache.get_or_layout(CellKey { idx, ..base }, fp, cx.ctx, |c| {
                    let _ = c.write_str(&entry.name);
                    if dir {
                        let _ = c.write_char('/');
                    }
                    let icon = crate::icons::glyph(filenav_vfs::icons::icon_class(
                        &entry.name,
                        entry.kind,
                        entry.attrs,
                    ));
                    CellSpec {
                        group: cx.hl.classify(entry),
                        icon: u16::try_from(u32::from(icon)).unwrap_or(0),
                        ..spec(&cx.font, cx.name_w, None)
                    }
                });
                let (galley, group, icon) = (Arc::clone(&name.galley), name.group, name.icon);
                let size = cache.get_or_layout(
                    CellKey {
                        idx,
                        col: ColKind::Size as u8,
                        width_px: 0,
                        ..base
                    },
                    fp,
                    cx.ctx,
                    |c| {
                        cell_text(ColKind::Size, entry, cx.fmt, c);
                        spec(&cx.font, f32::INFINITY, cx.digit_slot)
                    },
                );
                let size = (!size.galley.is_empty())
                    .then(|| (Arc::clone(&size.galley), size.lead, size.width));
                Line {
                    row: r,
                    name: galley,
                    size,
                    cursor: r == t.cursor.row,
                    selected: t.selection.contains(idx),
                    group,
                    icon: char::from_u32(u32::from(icon)).unwrap_or(' '),
                    icon_tone: if entry.attrs.has(filenav_core::Attrs::HIDDEN)
                        || entry.attrs.has(filenav_core::Attrs::SYSTEM)
                    {
                        IconTone::Hidden
                    } else if dir {
                        IconTone::Dir
                    } else {
                        IconTone::Normal
                    },
                    entry: true,
                }
            }
        };
        out.push(line);
    }
}

fn spec(font: &FontId, max_width: f32, digit_slot: Option<f32>) -> CellSpec {
    CellSpec {
        font: font.clone(),
        max_width,
        group: 0,
        icon: 0,
        digit_slot,
    }
}

/// Подвал: загрузка, ошибка, «N объектов · размер» или «N выбрано · размер».
pub(crate) fn footer(i18n: &I18n, t: &Tab) -> String {
    match &t.listing.status {
        ListingStatus::Loading { .. } => i18n.get("ui-loading"),
        ListingStatus::Failed(e) => {
            let mut args = FluentArgs::new();
            args.set("error", e.to_string());
            i18n.fmt("ui-listing-failed", &args)
        }
        _ => {
            let mut args = FluentArgs::new();
            if t.selection.is_empty() {
                let s = t.listing.stats;
                args.set("count", u64::from(s.dirs) + u64::from(s.files));
                args.set("bytes", filenav_i18n::fmt::bytes_with(i18n, s.bytes));
                i18n.fmt("ui-footer-items", &args)
            } else {
                args.set("count", u64::from(t.selection.len()));
                args.set(
                    "bytes",
                    filenav_i18n::fmt::bytes_with(i18n, t.selection.bytes()),
                );
                i18n.fmt("ui-footer-selected", &args)
            }
        }
    }
}

/// Сколько строк высотой `row_h` помещается в `height` (не меньше одной).
#[allow(clippy::cast_sign_loss, reason = "значение зажато в 1..=10 000")]
pub(crate) fn rows_fit(height: f32, row_h: f32) -> u32 {
    (height / row_h).floor().clamp(1.0, 10_000.0) as u32
}

pub(crate) fn radius(t: &Tokens, m: Metric) -> CornerRadius {
    t.radius(m)
}

/// Отрисовка строк списка.
pub(crate) struct Rows<'a> {
    pub tokens: &'a Tokens,
    pub hl: &'a Highlighter,
    pub fonts: &'a FontSet,
    pub tab: &'a Tab,
    /// Колонка иконок (есть иконочный шрифт и `ui.icons != "none"`).
    pub icons: bool,
    pub active: bool,
    /// Удержанный кадр (`false`) на мышь не отвечает.
    pub interactive: bool,
    pub side: Side,
}

impl Rows<'_> {
    /// Строки рисуются вручную, без фокуса и подсветки при наведении (S0 v3 №13).
    /// Курсор — рамка 1 px `stroke.strong`, помеченные — `accent.subtle` и `accent.text`
    /// (S0-decisions №31). Тексты — galley из кэша с подменой цвета.
    pub(crate) fn paint(
        &self,
        ui: &mut egui::Ui,
        cache: &mut RowCache,
        lines: &[Line],
        out: &mut Vec<Command>,
    ) {
        let t = self.tokens;
        let row_h = t.m(Metric::HRow);
        let pad = t.m(Metric::PadRowX);
        let icon_font = self.fonts.id(Font::Icons, t.m(Metric::WIcon));
        // Иконка 16 px в слоте 20 (S2 §1.2).
        let icon_slot = if self.icons {
            t.m(Metric::WIcon) + 4.0
        } else {
            0.0
        };
        let ctrl = ui.input(|i| i.modifiers.command);
        let sense = if self.interactive {
            Sense::CLICK
        } else {
            Sense::hover()
        };
        let r = radius(t, Metric::RadiusControl);
        let ctx = ui.ctx().clone();
        for line in lines {
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), sense);
            let painter = ui.painter_at(rect);
            let mut color = self
                .hl
                .color(line.group, t)
                .unwrap_or_else(|| t.c(Role::TextPrimary));
            if line.selected {
                painter.rect_filled(rect, r, t.c(Role::AccentSubtle));
                color = t.c(Role::AccentText);
            }
            if line.cursor && self.active {
                painter.rect_filled(rect, r, t.c(Role::FillCursor));
                painter.rect_stroke(
                    rect,
                    r,
                    Stroke::new(t.m(Metric::StrokeThin), t.c(Role::StrokeStrong)),
                    StrokeKind::Inside,
                );
            } else if line.cursor {
                painter.rect_filled(rect, r, t.c(Role::FillCursorPassive));
            }
            let mid = rect.center().y;
            if let Some((g, lead, width)) = &line.size {
                let x = rect.right() - pad - width + lead;
                let pos = egui::pos2(x, mid - g.size().y / 2.0);
                painter.galley_with_override_text_color(
                    pos,
                    Arc::clone(g),
                    t.c(Role::TextSecondary),
                );
            }
            let mut x = rect.left() + pad;
            if self.icons {
                let tone = match line.icon_tone {
                    IconTone::Dir => t.c(Role::AccentText),
                    IconTone::Hidden => t.c(Role::TextTertiary),
                    IconTone::Normal => t.c(Role::TextSecondary),
                };
                let g = cache.icon(&ctx, line.icon, &icon_font);
                let pos = egui::pos2(x + (icon_slot - g.size().x) / 2.0, mid - g.size().y / 2.0);
                painter.galley_with_override_text_color(pos, g, tone);
                x += icon_slot + t.m(Metric::SpaceS);
            }
            let pos = egui::pos2(x, mid - line.name.size().y / 2.0);
            painter.galley_with_override_text_color(pos, Arc::clone(&line.name), color);
            if !self.interactive {
                continue;
            }
            if resp.clicked() {
                if !self.active {
                    out.push(Command::new("panel.focus").arg("side", side_arg(self.side)));
                }
                out.push(match self.tab.row(line.row) {
                    Some(Row::Entry { entry, .. }) => {
                        Command::new("panel.cursor.to").arg("name", entry.name.as_str())
                    }
                    _ => Command::new("panel.cursor.home"),
                });
                if ctrl && line.entry {
                    out.push(Command::new("sel.toggle"));
                }
            }
            if resp.double_clicked() {
                out.push(Command::new("panel.enter"));
            }
        }
    }
}
