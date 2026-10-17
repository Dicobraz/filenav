//! Карточка панели: табы, путь, строки, подвал. Строки — временная простая отрисовка
//! (имя и размер), пока нет `FileList` (S2-10); цвета и размеры — из токенов.

use std::time::Duration;

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, StrokeKind};
use filenav_core::{Command, ListingStatus, Row, Side, Tab};
use filenav_i18n::{FluentArgs, I18n};

use crate::fonts::{Font, FontSet};
use crate::highlight::Highlighter;
use crate::text::{self, Tx};
use crate::tokens::{Metric, Role, Tokens};

/// Пока новый каталог ничего не прислал, панель показывает прежний кадр — не
/// дольше этого (S2: `Loading` > 150 мс — индикатор загрузки).
pub(crate) const HOLD: Duration = Duration::from_millis(150);

/// Кадр панели, каким его увидел пользователь: путь, подвал, видимые строки.
#[derive(Debug)]
pub(crate) struct Shown {
    pub path: String,
    pub footer: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Line {
    pub name: String,
    pub size: String,
    pub cursor: bool,
    pub selected: bool,
    /// Группа раскраски ([`Highlighter::classify`]), 0 — без цвета.
    pub group: u8,
    /// Глиф иконки (S2 §1.6) и тон: каталог — `accent.text`, скрытый — `text.tertiary`.
    pub icon: char,
    pub icon_tone: IconTone,
    /// Команда курсора на эту строку (клик).
    pub click: Command,
    pub entry: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum IconTone {
    Normal,
    Dir,
    Hidden,
}

/// Состояние карточки между кадрами.
#[derive(Debug, Default)]
pub(crate) struct CardMemo {
    /// Последний сообщённый размер страницы (чтобы не слать `panel.viewport` каждый кадр).
    pub page: u32,
    pub shown: Option<Shown>,
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
pub(crate) fn lines(i18n: &I18n, hl: &Highlighter, t: &Tab, page: u32) -> Vec<Line> {
    let top = t.cursor.top;
    (top..(top + page).min(t.rows()))
        .map_while(|r| {
            let row = t.row(r)?;
            let selected = matches!(row, Row::Entry { idx, .. } if t.selection.contains(idx));
            let line = match row {
                Row::Parent => Line {
                    name: "..".to_owned(),
                    size: String::new(),
                    cursor: false,
                    selected,
                    group: 0,
                    icon: crate::icons::PARENT,
                    icon_tone: IconTone::Normal,
                    click: Command::new("panel.cursor.home"),
                    entry: false,
                },
                Row::Entry { entry, .. } => {
                    let dir = entry.kind.is_dir_like();
                    Line {
                        name: if dir {
                            format!("{}/", entry.name)
                        } else {
                            entry.name.to_string()
                        },
                        size: if dir {
                            String::new()
                        } else {
                            filenav_i18n::fmt::bytes_with(i18n, entry.size)
                        },
                        cursor: false,
                        selected,
                        group: hl.classify(entry),
                        icon: crate::icons::glyph(filenav_vfs::icons::icon_class(
                            &entry.name,
                            entry.kind,
                            entry.attrs,
                        )),
                        icon_tone: if entry.attrs.has(filenav_core::Attrs::HIDDEN)
                            || entry.attrs.has(filenav_core::Attrs::SYSTEM)
                        {
                            IconTone::Hidden
                        } else if dir {
                            IconTone::Dir
                        } else {
                            IconTone::Normal
                        },
                        click: Command::new("panel.cursor.to").arg("name", entry.name.as_str()),
                        entry: true,
                    }
                }
            };
            Some(Line {
                cursor: r == t.cursor.row,
                ..line
            })
        })
        .collect()
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
    pub i18n: &'a I18n,
    pub tokens: &'a Tokens,
    pub hl: &'a Highlighter,
    pub fonts: &'a FontSet,
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
    /// (S0-decisions №31).
    #[allow(
        clippy::too_many_lines,
        reason = "временная отрисовка; заменит FileList (S2-10)"
    )]
    pub(crate) fn paint(&self, ui: &mut egui::Ui, lines: &[Line], out: &mut Vec<Command>) {
        let t = self.tokens;
        let row_h = t.m(Metric::HRow);
        let pad = t.m(Metric::PadRowX);
        let font = self.fonts.id(Font::Ui, t.m(Metric::TypeBody));
        let icon_font = self.fonts.id(Font::Icons, t.m(Metric::WIcon));
        let tabular = self.fonts.tabular_ok(Font::Ui);
        let slot = if tabular {
            0.0
        } else {
            FontSet::digit_slot(ui.ctx(), &font)
        };
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
        for line in lines {
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), sense);
            let painter = ui.painter();
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
            let clip = ui.painter_at(rect);
            let size_pos = rect.right_center() - egui::vec2(pad, 0.0);
            let size_rect = if tabular {
                text::paint(
                    &clip,
                    self.i18n,
                    size_pos,
                    Align2::RIGHT_CENTER,
                    &Tx::user(line.size.as_str()),
                    font.clone(),
                    t.c(Role::TextSecondary),
                )
            } else {
                text::paint_tabular(
                    &clip,
                    size_pos,
                    &line.size,
                    &font,
                    t.c(Role::TextSecondary),
                    slot,
                )
            };
            let mut x = rect.left() + pad;
            if self.icons {
                let tone = match line.icon_tone {
                    IconTone::Dir => t.c(Role::AccentText),
                    IconTone::Hidden => t.c(Role::TextTertiary),
                    IconTone::Normal => t.c(Role::TextSecondary),
                };
                let mut buf = [0u8; 4];
                text::paint(
                    &clip,
                    self.i18n,
                    egui::pos2(x + icon_slot / 2.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    &Tx::user(&*line.icon.encode_utf8(&mut buf)),
                    icon_font.clone(),
                    tone,
                );
                x += icon_slot + t.m(Metric::SpaceS);
            }
            // Имя не залезает на размер: лишнее — многоточием.
            let max_w = size_rect.left() - t.m(Metric::SpaceS) - x;
            text::paint_clipped(
                &clip,
                self.i18n,
                egui::pos2(x, rect.center().y),
                max_w,
                &Tx::user(line.name.as_str()),
                font.clone(),
                color,
            );
            if !self.interactive {
                continue;
            }
            if resp.clicked() {
                if !self.active {
                    out.push(Command::new("panel.focus").arg("side", side_arg(self.side)));
                }
                out.push(line.click.clone());
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

/// Рамка карточки (S2 §1.2–1.3): `layer.panel`, `stroke.card`, `radius.overlay`.
pub(crate) fn card_frame(t: &Tokens) -> egui::Frame {
    let pad = t.m(Metric::SpaceS) as i8;
    egui::Frame::new()
        .fill(t.c(Role::LayerPanel))
        .stroke(Stroke::new(t.m(Metric::StrokeThin), t.c(Role::StrokeCard)))
        .corner_radius(radius(t, Metric::RadiusOverlay))
        .inner_margin(egui::Margin::same(pad))
}

/// Акцентная полоса активной панели — верх карточки (`w.accent_bar`).
pub(crate) fn accent_bar(ui: &egui::Ui, t: &Tokens, card: Rect) {
    let r = t.m(Metric::RadiusOverlay);
    let bar = Rect::from_min_size(
        card.min + egui::vec2(r, 0.0),
        egui::vec2((card.width() - 2.0 * r).max(0.0), t.m(Metric::WAccentBar)),
    );
    let rr = CornerRadius::same(1);
    ui.painter().rect_filled(bar, rr, t.c(Role::AccentFill));
}
