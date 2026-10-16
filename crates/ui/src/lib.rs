//! FileNav — UI-фронтенд на egui.
//!
//! Пока здесь только `DebugUi` (S1-13): отладочный вид состояния до настоящего
//! интерфейса S2. Клавиатуру обрабатывает app (`raw_input_hook`, до keymap S3);
//! здесь — мышь. Подписей нет — только данные (пути, имена, размеры) и символы;
//! тексты баннера, кнопок и тостов приходят из core.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Sense, TextStyle};
use filenav_core::{AppState, Command, ListingStatus, Row, Side, Tab, UiEvent, Value};

/// Сколько показывается тост.
const TOAST_TTL: Duration = Duration::from_secs(4);
/// Пока новый каталог ничего не прислал, панель показывает прежний кадр — не
/// дольше этого (S2: `Loading` > 150 мс — индикатор загрузки).
const HOLD: Duration = Duration::from_millis(150);

/// Отладочный UI: две колонки текстом, баннер, запрос, тосты.
#[derive(Debug, Default)]
pub struct DebugUi {
    /// Последний сообщённый размер страницы по сторонам (чтобы не слать
    /// `panel.viewport` каждый кадр).
    page: [u32; 2],
    toasts: VecDeque<(String, Instant)>,
    /// Последний показанный кадр панели по сторонам.
    shown: [Option<Shown>; 2],
}

impl DebugUi {
    pub fn new() -> Self {
        Self::default()
    }

    /// Кадр: только читает `s`, действия пишет командами в `out`. `root` —
    /// корневой `Ui` окна от eframe (ADR-0011); `Context` — `root.ctx()`.
    pub fn show(
        &mut self,
        root: &mut egui::Ui,
        s: &AppState,
        ev: &[UiEvent],
        out: &mut Vec<Command>,
    ) {
        let ctx = root.ctx().clone();
        let now = Instant::now();
        for e in ev {
            if let UiEvent::Toast { text, .. } = e {
                self.toasts.push_back((text.clone(), now + TOAST_TTL));
            }
        }
        self.toasts.retain(|(_, until)| *until > now);
        if let Some((_, until)) = self.toasts.front() {
            ctx.request_repaint_after(until.saturating_duration_since(now));
        }
        egui::CentralPanel::default().show(root, |ui| {
            if let Some(b) = &s.banner {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&b.text).color(Color32::from_rgb(220, 80, 60)));
                    for (label, cmd) in &b.actions {
                        if ui.button(label.as_str()).clicked() {
                            out.push(cmd.clone());
                        }
                    }
                });
                ui.separator();
            }
            if let Some(p) = s.prompts.active() {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&p.spec.title);
                    ui.label(&p.spec.body);
                    for c in &p.spec.choices {
                        if ui.button(c.label.as_str()).clicked() {
                            out.push(
                                Command::new("prompt.reply")
                                    .arg(
                                        "id",
                                        Value::Int(i64::try_from(p.id.0).unwrap_or(i64::MAX)),
                                    )
                                    .arg("choice", c.id.as_str()),
                            );
                        }
                    }
                });
                ui.separator();
            }
            let toast_h = if self.toasts.is_empty() {
                0.0
            } else {
                ui.text_style_height(&TextStyle::Body)
                    * f32::from(u16::try_from(self.toasts.len()).unwrap_or(u16::MAX))
                    + 8.0
            };
            let h = (ui.available_height() - toast_h).max(0.0);
            ui.columns(2, |cols| {
                for (col, side) in cols.iter_mut().zip([Side::Left, Side::Right]) {
                    col.set_max_height(h);
                    self.panel(col, s, side, out);
                }
            });
            for (text, _) in &self.toasts {
                ui.label(text);
            }
        });
    }

    fn panel(&mut self, ui: &mut egui::Ui, s: &AppState, side: Side, out: &mut Vec<Command>) {
        let p = s.panel(side);
        if !p.visible {
            ui.weak("—");
            return;
        }
        let side_arg = if side == Side::Left { "left" } else { "right" };
        let active = s.active == side;
        let t = p.tab();
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in p.tabs.iter().enumerate() {
                let mut title = RichText::new(tab.title().as_ref());
                if i == p.active {
                    title = title.strong().background_color(ui.visuals().faint_bg_color);
                }
                // Sense::CLICK без FOCUSABLE: в панелях нет фокусируемых виджетов (S0 v3 №13).
                if ui
                    .add(egui::Label::new(title).sense(Sense::CLICK))
                    .clicked()
                {
                    out.push(
                        Command::new("tab.goto")
                            .arg("n", i64::try_from(i + 1).unwrap_or(1))
                            .arg("side", side_arg),
                    );
                }
            }
        });
        // Новый каталог ещё ничего не прислал — показываем прежний кадр панели,
        // а не пустую: иначе каждый переход мигает (S2: индикатор — после 150 мс).
        let held = hold_left(t).and_then(|left| {
            self.shown[side.idx()]
                .take()
                .inspect(|_| ui.ctx().request_repaint_after(left))
        });
        let interactive = held.is_none();
        let (path, status_line) = match &held {
            Some(prev) => (prev.path.clone(), prev.status.clone()),
            None => (path_text(t), status(t)),
        };
        let path_rt = if active {
            RichText::new(path.as_str()).strong()
        } else {
            RichText::new(path.as_str())
        };
        ui.label(path_rt.monospace());
        ui.monospace(status_line.as_str());

        let row_h = ui.text_style_height(&TextStyle::Monospace) + ui.spacing().item_spacing.y;
        let page = rows_fit(ui.available_height(), row_h);
        if self.page[side.idx()] != page {
            self.page[side.idx()] = page;
            out.push(
                Command::new("panel.viewport")
                    .arg("side", side_arg)
                    .arg("rows", i64::from(page))
                    .arg("cols", 1),
            );
        }
        let area = ui.interact(
            ui.available_rect_before_wrap(),
            ui.id().with("rows"),
            Sense::hover(),
        );
        if interactive && area.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            let lines = (-dy / row_h).round() as i64;
            if lines != 0 {
                let top = (i64::from(t.cursor.top) + lines).max(0);
                out.push(
                    Command::new("panel.viewport")
                        .arg("side", side_arg)
                        .arg("rows", i64::from(page))
                        .arg("cols", 1)
                        .arg("top", top),
                );
            }
        }
        let shown = held.unwrap_or_else(|| Shown {
            path,
            status: status_line,
            lines: lines(t, page),
        });
        paint_lines(ui, &shown.lines, active, interactive, row_h, side_arg, out);
        self.shown[side.idx()] = Some(shown);
    }
}

/// Пока новый листинг пуст и грузится меньше [`HOLD`] — сколько ещё держать
/// прежний кадр.
fn hold_left(t: &Tab) -> Option<Duration> {
    match t.listing.status {
        ListingStatus::Loading { since } if t.listing.entries.is_empty() => {
            HOLD.checked_sub(since.elapsed()).filter(|d| !d.is_zero())
        }
        _ => None,
    }
}

fn path_text(t: &Tab) -> String {
    if t.loc.is_local() {
        t.loc.path.display().to_string()
    } else {
        t.loc.to_uri()
    }
}

/// Кадр панели, каким его увидел пользователь: путь, статус, видимые строки.
#[derive(Debug)]
struct Shown {
    path: String,
    status: String,
    lines: Vec<Line>,
}

#[derive(Debug, PartialEq)]
struct Line {
    text: String,
    cursor: bool,
    selected: bool,
    /// Команда курсора на эту строку (клик).
    click: Command,
    entry: bool,
}

/// Видимые строки таба: `page` строк от `cursor.top`.
fn lines(t: &Tab, page: u32) -> Vec<Line> {
    let top = t.cursor.top;
    (top..(top + page).min(t.rows()))
        .map_while(|r| {
            let row = t.row(r)?;
            let selected = matches!(row, Row::Entry { idx, .. } if t.selection.contains(idx));
            let (text, click, entry) = match row {
                Row::Parent => ("  ..".to_owned(), Command::new("panel.cursor.home"), false),
                Row::Entry { entry, .. } => {
                    let mark = if selected { '*' } else { ' ' };
                    let slash = if entry.kind.is_dir_like() { "/" } else { "" };
                    let bytes = if entry.kind.is_dir_like() {
                        String::new()
                    } else {
                        entry.size.to_string()
                    };
                    (
                        format!("{mark} {}{slash}  {bytes}", entry.name),
                        Command::new("panel.cursor.to").arg("name", entry.name.as_str()),
                        true,
                    )
                }
            };
            Some(Line {
                text,
                cursor: r == t.cursor.row,
                selected,
                click,
                entry,
            })
        })
        .collect()
}

/// Строки рисуются вручную, без фокуса и подсветки при наведении. Удержанный
/// кадр (`interactive = false`) на мышь не отвечает.
fn paint_lines(
    ui: &mut egui::Ui,
    lines: &[Line],
    active: bool,
    interactive: bool,
    row_h: f32,
    side_arg: &str,
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
            // Курсор неактивной панели виден, но бледно.
            ui.painter().rect_stroke(
                rect,
                2.0,
                visuals.widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        if line.selected {
            color = Color32::from_rgb(230, 190, 60);
        }
        ui.painter().text(
            rect.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            line.text.as_str(),
            font.clone(),
            color,
        );
        if !interactive {
            continue;
        }
        if resp.clicked() {
            if !active {
                out.push(Command::new("panel.focus").arg("side", side_arg));
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

/// Сколько строк высотой `row_h` помещается в `height` (не меньше одной).
#[allow(clippy::cast_sign_loss, reason = "значение зажато в 1..=10 000")]
fn rows_fit(height: f32, row_h: f32) -> u32 {
    (height / row_h).floor().clamp(1.0, 10_000.0) as u32
}

/// Строка состояния: загрузка, ошибка или «каталогов + файлов, байт; выделено».
fn status(t: &Tab) -> String {
    match &t.listing.status {
        ListingStatus::Loading { .. } => "…".to_owned(),
        ListingStatus::Failed(e) => format!("! {e}"),
        _ => {
            let s = t.listing.stats;
            let mut line = format!("{} + {}  {}", s.dirs, s.files, s.bytes);
            if !t.selection.is_empty() {
                let _ = write!(line, "   * {}  {}", t.selection.len(), t.selection.bytes());
            }
            line
        }
    }
}

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;
    use filenav_core::{Attrs, Entry, EntryKind, Gen, Listing, Location, TabId};

    fn tab(names: &[&str]) -> Tab {
        let mut t = Tab::new(TabId(1), Location::local("C:\\x"));
        let entries: Vec<Entry> = names
            .iter()
            .map(|n| Entry {
                name: (*n).into(),
                size: 0,
                mtime: 0,
                attrs: Attrs::default(),
                kind: EntryKind::File,
            })
            .collect();
        let order = (0..u32::try_from(entries.len()).unwrap()).collect();
        t.listing = Listing::with_entries(entries, order, Gen(1));
        t
    }

    #[test]
    fn holds_only_empty_fresh_loading() {
        let mut t = tab(&[]);
        assert_eq!(hold_left(&t), None, "готовый пустой каталог не держим");
        t.listing.status = ListingStatus::Loading {
            since: Instant::now(),
        };
        assert!(hold_left(&t).is_some_and(|d| d <= HOLD));
        t.listing.status = ListingStatus::Loading {
            since: Instant::now().checked_sub(HOLD).unwrap(),
        };
        assert_eq!(hold_left(&t), None, "дольше 150 мс — показываем загрузку");
        let mut t = tab(&["a"]);
        t.listing.status = ListingStatus::Loading {
            since: Instant::now(),
        };
        assert_eq!(hold_left(&t), None, "первые записи пришли — рисуем их");
    }

    #[test]
    fn lines_from_top_with_cursor() {
        let mut t = tab(&["a", "b", "c"]);
        t.cursor.top = 1;
        t.cursor.row = 2;
        let v = lines(&t, 2);
        let names: Vec<_> = v.iter().map(|l| (l.text.trim(), l.cursor)).collect();
        assert_eq!(names, [("a  0", false), ("b  0", true)]);
        assert_eq!(v[1].click, Command::new("panel.cursor.to").arg("name", "b"));
    }

    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}
