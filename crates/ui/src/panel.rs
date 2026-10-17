//! Карточка панели: табы, путь, строки, подвал. Строки — временная простая отрисовка
//! из `DebugUi` (S1-13), пока нет `FileList` (S2-10): имя и размер, курсор, пометка.

use std::time::Duration;

use egui::{Align2, Color32, CornerRadius, Sense, Stroke, StrokeKind, TextStyle};
use filenav_core::{Command, ListingStatus, Row, Side, Tab};
use filenav_i18n::{FluentArgs, I18n};

use crate::text::{self, Tx};

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
    /// Команда курсора на эту строку (клик).
    pub click: Command,
    pub entry: bool,
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
pub(crate) fn lines(i18n: &I18n, t: &Tab, page: u32) -> Vec<Line> {
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

/// Строки рисуются вручную, без фокуса и подсветки при наведении (S0 v3 №13).
/// Удержанный кадр (`interactive = false`) на мышь не отвечает.
#[allow(clippy::too_many_arguments, reason = "временная отрисовка до FileList")]
pub(crate) fn paint_lines(
    ui: &mut egui::Ui,
    i18n: &I18n,
    lines: &[Line],
    active: bool,
    interactive: bool,
    row_h: f32,
    side: Side,
    out: &mut Vec<Command>,
) {
    let font = TextStyle::Monospace.resolve(ui.style());
    let visuals = ui.visuals().clone();
    let ctrl = ui.input(|i| i.modifiers.command);
    let sense = if interactive {
        Sense::CLICK
    } else {
        Sense::hover()
    };
    for line in lines {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), sense);
        let mut color = visuals.text_color();
        if line.cursor && active {
            ui.painter()
                .rect_filled(rect, 2.0, visuals.selection.bg_fill);
            color = visuals.selection.stroke.color;
        } else if line.cursor {
            // Курсор неактивной панели виден рамкой.
            ui.painter().rect_stroke(
                rect,
                2.0,
                visuals.widgets.noninteractive.bg_stroke,
                StrokeKind::Inside,
            );
        }
        if line.selected {
            color = Color32::from_rgb(230, 190, 60);
        }
        let painter = ui.painter_at(rect);
        let size_rect = text::paint(
            &painter,
            i18n,
            rect.right_center() - egui::vec2(6.0, 0.0),
            Align2::RIGHT_CENTER,
            &Tx::user(line.size.as_str()),
            font.clone(),
            color,
        );
        // Имя не залезает на размер.
        let name_painter = painter.with_clip_rect(rect.with_max_x(size_rect.left() - 8.0));
        text::paint(
            &name_painter,
            i18n,
            rect.left_center() + egui::vec2(6.0, 0.0),
            Align2::LEFT_CENTER,
            &Tx::user(line.name.as_str()),
            font.clone(),
            color,
        );
        if !interactive {
            continue;
        }
        if resp.clicked() {
            if !active {
                out.push(Command::new("panel.focus").arg("side", side_arg(side)));
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

/// Рамка карточки: активная — цветом выделения.
pub(crate) fn card_frame(ui: &egui::Ui, active: bool) -> egui::Frame {
    let v = ui.visuals();
    let stroke = if active {
        Stroke::new(1.0, v.selection.bg_fill)
    } else {
        v.widgets.noninteractive.bg_stroke
    };
    egui::Frame::new()
        .fill(v.extreme_bg_color)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(6))
}
