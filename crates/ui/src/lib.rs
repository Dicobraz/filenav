//! FileNav — UI-фронтенд на egui.
//!
//! Пока здесь только `DebugUi` (S1-13): отладочный вид состояния до настоящего
//! интерфейса S2. Клавиши захардкожены до keymap S3. Подписей нет — только
//! данные (пути, имена, размеры) и символы; тексты баннера, кнопок и тостов
//! приходят из core.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use egui::{Color32, Event, Key, RichText, Sense, TextStyle};
use filenav_core::{AppState, Command, Focus, ListingStatus, Row, Side, Tab, UiEvent, Value};

/// Сколько показывается тост.
const TOAST_TTL: Duration = Duration::from_secs(4);

/// Отладочный UI: две колонки текстом, баннер, запрос, тосты.
#[derive(Debug, Default)]
pub struct DebugUi {
    /// Последний сообщённый размер страницы по сторонам (чтобы не слать
    /// `panel.viewport` каждый кадр).
    page: [u32; 2],
    toasts: VecDeque<(String, Instant)>,
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
        if s.focus == Focus::Panel && s.prompts.active().is_none() {
            keys(&ctx, out);
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
                if ui
                    .selectable_label(i == p.active, tab.title().as_ref())
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
        let path = if t.loc.is_local() {
            t.loc.path.display().to_string()
        } else {
            t.loc.to_uri()
        };
        let path = if active {
            RichText::new(path).strong()
        } else {
            RichText::new(path)
        };
        ui.label(path.monospace());
        ui.monospace(status(t));

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
        if area.hovered() {
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
        let top = t.cursor.top;
        for r in top..(top + page).min(t.rows()) {
            let Some(row) = t.row(r) else { break };
            let text = match row {
                Row::Parent => "  ..".to_owned(),
                Row::Entry { idx, entry } => {
                    let mark = if t.selection.contains(idx) { '*' } else { ' ' };
                    let slash = if entry.kind.is_dir_like() { "/" } else { "" };
                    let bytes = if entry.kind.is_dir_like() {
                        String::new()
                    } else {
                        entry.size.to_string()
                    };
                    format!("{mark} {}{slash}  {bytes}", entry.name)
                }
            };
            let mut rt = RichText::new(text).monospace();
            if matches!(row, Row::Entry { idx, .. } if t.selection.contains(idx)) {
                rt = rt.color(Color32::from_rgb(230, 190, 60));
            }
            let resp = ui.selectable_label(active && r == t.cursor.row, rt);
            if resp.clicked() {
                if !active {
                    out.push(Command::new("panel.focus").arg("side", side_arg));
                }
                out.push(match row {
                    Row::Parent => Command::new("panel.cursor.home"),
                    Row::Entry { entry, .. } => {
                        Command::new("panel.cursor.to").arg("name", entry.name.as_str())
                    }
                });
            }
            if resp.double_clicked() {
                out.push(Command::new("panel.enter"));
            }
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

/// Клавиши до keymap S3: стрелки, страницы, Home/End, Enter, Backspace, Tab, Insert.
fn keys(ctx: &egui::Context, out: &mut Vec<Command>) {
    let pressed: Vec<Key> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                Event::Key {
                    key, pressed: true, ..
                } => Some(*key),
                _ => None,
            })
            .collect()
    });
    for k in pressed {
        let cmd = match k {
            Key::ArrowUp => Command::new("panel.cursor.up"),
            Key::ArrowDown => Command::new("panel.cursor.down"),
            Key::PageUp => Command::new("panel.cursor.page_up"),
            Key::PageDown => Command::new("panel.cursor.page_down"),
            Key::Home => Command::new("panel.cursor.home"),
            Key::End => Command::new("panel.cursor.end"),
            Key::Enter => Command::new("panel.enter"),
            Key::Backspace => Command::new("panel.up"),
            Key::Tab => Command::new("panel.switch"),
            Key::Insert => Command::new("sel.toggle").arg("advance", true),
            _ => continue,
        };
        out.push(cmd);
    }
}

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}
