//! FileNav — UI-фронтенд на egui (S2).
//!
//! Каркас S2-02: [`FileNavUi`] — две карточки панелей, баннер, вопрос, тосты и keybar;
//! handler `ui` и [`UiState`] в `AppState.ext` ([`register`]). Ввод панели — в app
//! (`raw_input_hook`, до keymap S3); здесь — мышь. Тексты — только через [`text`]
//! (`clippy.toml`: `disallowed-methods`).

pub mod accent;
mod handler;
pub mod keybar;
mod panel;
pub mod state;
pub mod sys;
pub mod text;

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Sense, TextStyle};
use filenav_core::{AppState, Command, CommandRegistry, Core, Side, UiEvent, Value};
use filenav_i18n::I18n;

pub use handler::UiHandler;
pub use state::{Draft, ThemeMode, UiState};

use crate::panel::{CardMemo, Shown};
use crate::text::Tx;

/// Сколько показывается тост.
const TOAST_TTL: Duration = Duration::from_secs(4);
/// Не больше стольких тостов сразу (S2 §3).
const TOASTS_MAX: usize = 3;
/// Высота keybar.
const KEYBAR_H: f32 = 26.0;

/// Регистрирует handler `ui`; `UiState` появится в `ext` в первом pump.
pub fn register(core: &mut Core) {
    core.register(Box::new(UiHandler));
}

/// Фронтенд окна: только читает `AppState`, действия — командами.
#[derive(Debug)]
pub struct FileNavUi {
    toasts: VecDeque<(String, Instant)>,
    cards: [CardMemo; 2],
    keybar: keybar::Keybar,
    registered: Vec<&'static str>,
    /// Тема, акцент и контрастность ОС (probe, затем `sys::watch`).
    look: sys::Look,
    look_rx: Option<mpsc::Receiver<sys::Look>>,
    /// Применённые настройки вида: команды `ui.theme`, `ui.zoom` и `look` ОС.
    applied: Option<(ThemeMode, u16, sys::Look)>,
}

impl FileNavUi {
    /// `registry` — команды всех handlers (keybar гасит незарегистрированные);
    /// `probe` — [`sys::probe_system`], его предупреждения показываются тостами.
    /// Конфиг `[ui]` добавит S2-05.
    pub fn new(registry: &CommandRegistry, probe: &sys::SysProbe) -> Self {
        let far = Instant::now() + TOAST_TTL;
        let toasts = probe
            .warnings
            .iter()
            .map(|w| (format!("{}: {}", w.key, w.msg), far))
            .collect();
        Self {
            toasts,
            cards: [CardMemo::default(), CardMemo::default()],
            keybar: keybar::Keybar::default(),
            registered: keybar::registered(registry),
            look: probe.look,
            look_rx: None,
            applied: None,
        }
    }

    /// Следить за темой и акцентом ОС ([`sys::watch`]); без этого — только probe.
    pub fn watch_system(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        sys::watch(ctx, tx);
        self.look_rx = Some(rx);
    }

    /// Кадр (ADR-0011): `root` — корневой `Ui` окна от eframe.
    pub fn show(
        &mut self,
        root: &mut egui::Ui,
        s: &AppState,
        ev: &[UiEvent],
        out: &mut Vec<Command>,
    ) {
        let ctx = root.ctx().clone();
        let i18n = filenav_i18n::current();
        let fallback = UiState::new();
        let st = s.ext.get::<UiState>().unwrap_or(&fallback);
        self.apply_look(&ctx, st);
        self.take_toasts(&ctx, ev);
        self.keybar
            .refresh(&i18n, filenav_i18n::epoch(), &self.registered);

        egui::Panel::bottom("keybar")
            .resizable(false)
            .exact_size(KEYBAR_H)
            .show(root, |ui| self.paint_keybar(ui, &i18n, out));
        egui::CentralPanel::default().show(root, |ui| {
            top_bars(ui, &i18n, s, st, out);
            let h = ui.available_height();
            let w = ui.available_width();
            let gap = ui.spacing().item_spacing.x;
            let left_w = ((w - gap) * st.split).max(0.0);
            ui.horizontal_top(|ui| {
                for side in [Side::Left, Side::Right] {
                    let width = match side {
                        Side::Left => left_w,
                        Side::Right => (w - gap - left_w).max(0.0),
                    };
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), Sense::hover());
                    let mut card = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    self.card(&mut card, &i18n, s, side, out);
                }
            });
        });
        self.paint_toasts(&ctx, &i18n);
    }

    fn apply_look(&mut self, ctx: &egui::Context, st: &UiState) {
        if let Some(rx) = &self.look_rx {
            while let Ok(look) = rx.try_recv() {
                self.look = look;
            }
        }
        let want = (st.theme, st.zoom, self.look);
        if self.applied == Some(want) {
            return;
        }
        self.applied = Some(want);
        ctx.set_theme(match (st.theme, self.look.theme) {
            (ThemeMode::Light, _) | (ThemeMode::System, Some(egui::Theme::Light)) => {
                egui::ThemePreference::Light
            }
            (ThemeMode::Dark, _) | (ThemeMode::System, Some(egui::Theme::Dark)) => {
                egui::ThemePreference::Dark
            }
            (ThemeMode::System, None) => egui::ThemePreference::System,
        });
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let (fill, text, on) = accent_roles(&self.look.accent, theme);
            ctx.style_mut_of(theme, |style| {
                let v = &mut style.visuals;
                v.selection.bg_fill = fill;
                v.selection.stroke.color = on;
                v.hyperlink_color = text;
            });
        }
        ctx.set_zoom_factor(f32::from(st.zoom) / 100.0);
    }

    fn take_toasts(&mut self, ctx: &egui::Context, ev: &[UiEvent]) {
        let now = Instant::now();
        for e in ev {
            if let UiEvent::Toast { text, .. } = e {
                self.toasts.push_back((text.clone(), now + TOAST_TTL));
            }
        }
        while self.toasts.len() > TOASTS_MAX {
            self.toasts.pop_front();
        }
        self.toasts.retain(|(_, until)| *until > now);
        if let Some((_, until)) = self.toasts.front() {
            ctx.request_repaint_after(until.saturating_duration_since(now));
        }
    }

    fn card(
        &mut self,
        ui: &mut egui::Ui,
        i18n: &I18n,
        s: &AppState,
        side: Side,
        out: &mut Vec<Command>,
    ) {
        let p = s.panel(side);
        let active = s.active == side;
        let memo = &mut self.cards[side.idx()];
        if !p.visible {
            return;
        }
        let t = p.tab();
        panel::card_frame(ui, active).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal_wrapped(|ui| {
                for (i, tab) in p.tabs.iter().enumerate() {
                    let resp = text::label_styled(
                        ui,
                        i18n,
                        &Tx::user(tab.title().as_ref()),
                        (i != p.active).then(|| ui.visuals().weak_text_color()),
                        i == p.active,
                    );
                    // Клик без фокуса: в панелях нет фокусируемых виджетов (S0 v3 №13).
                    if ui
                        .interact(resp.rect, resp.id.with("tab"), Sense::CLICK)
                        .clicked()
                    {
                        out.push(
                            Command::new("tab.goto")
                                .arg("n", i64::try_from(i + 1).unwrap_or(1))
                                .arg("side", panel::side_arg(side)),
                        );
                    }
                }
            });
            // Новый каталог ещё ничего не прислал — прежний кадр, а не пустая панель:
            // иначе каждый переход мигает (S2: индикатор — после 150 мс).
            let held = panel::hold_left(t).and_then(|left| {
                memo.shown
                    .take()
                    .inspect(|_| ui.ctx().request_repaint_after(left))
            });
            let interactive = held.is_none();
            let (path, footer) = match &held {
                Some(prev) => (prev.path.clone(), prev.footer.clone()),
                None => (panel::path_text(t), panel::footer(i18n, t)),
            };
            text::label_styled(ui, i18n, &Tx::user(path.as_str()), None, active);
            ui.separator();

            let row_h = ui.text_style_height(&TextStyle::Monospace) + 4.0;
            let footer_h =
                ui.text_style_height(&TextStyle::Body) + ui.spacing().item_spacing.y * 2.0;
            let list_h = (ui.available_height() - footer_h).max(row_h);
            let page = panel::rows_fit(list_h, row_h);
            if memo.page != page {
                memo.page = page;
                out.push(
                    Command::new("panel.viewport")
                        .arg("side", panel::side_arg(side))
                        .arg("rows", i64::from(page))
                        .arg("cols", 1),
                );
            }
            let (list_rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), list_h), Sense::hover());
            let area = ui.interact(list_rect, ui.id().with("rows"), Sense::hover());
            if interactive && area.hovered() {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                let lines = (-dy / row_h).round() as i64;
                if lines != 0 {
                    let top = (i64::from(t.cursor.top) + lines).max(0);
                    out.push(
                        Command::new("panel.viewport")
                            .arg("side", panel::side_arg(side))
                            .arg("rows", i64::from(page))
                            .arg("cols", 1)
                            .arg("top", top),
                    );
                }
            }
            let shown = held.unwrap_or_else(|| Shown {
                path,
                footer,
                lines: panel::lines(i18n, t, page),
            });
            let mut list = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
            panel::paint_lines(
                &mut list,
                i18n,
                &shown.lines,
                active,
                interactive,
                row_h,
                side,
                out,
            );
            ui.separator();
            text::label_styled(
                ui,
                i18n,
                &Tx::user(shown.footer.as_str()),
                Some(ui.visuals().weak_text_color()),
                false,
            );
            memo.shown = Some(shown);
        });
    }

    fn paint_keybar(&self, ui: &mut egui::Ui, i18n: &I18n, out: &mut Vec<Command>) {
        let n = 12.0;
        let full = ui.available_rect_before_wrap();
        let w = full.width() / n;
        let font = TextStyle::Body.resolve(ui.style());
        let v = ui.visuals().clone();
        for item in &self.keybar.items {
            let x = full.left() + w * f32::from(item.n - 1);
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, full.top()), egui::vec2(w, full.height()))
                    .shrink2(egui::vec2(1.0, 2.0));
            let sense = if item.enabled {
                Sense::CLICK
            } else {
                Sense::hover()
            };
            let resp = ui.interact(rect, ui.id().with(("fkey", item.n)), sense);
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 3.0, v.faint_bg_color);
            let num = text::paint(
                &painter,
                i18n,
                rect.left_center() + egui::vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                &Tx::user(item.n.to_string()),
                font.clone(),
                v.weak_text_color(),
            );
            let color = if item.enabled {
                v.text_color()
            } else {
                v.weak_text_color()
            };
            text::paint(
                &painter,
                i18n,
                egui::pos2(num.right() + 4.0, rect.center().y),
                Align2::LEFT_CENTER,
                &Tx::user(item.label.as_str()),
                font.clone(),
                color,
            );
            if resp.clicked() {
                out.push(Command::new(item.cmd));
            }
        }
    }

    fn paint_toasts(&self, ctx: &egui::Context, i18n: &I18n) {
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -(KEYBAR_H + 12.0)))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                for (msg, _) in &self.toasts {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        text::label(ui, i18n, &Tx::user(msg.as_str()));
                    });
                }
            });
    }
}

/// Роли акцента S2 §1.3: `accent.fill`, `accent.text`, `accent.on`. Остальные токены —
/// S2-05.
fn accent_roles(a: &accent::AccentPalette, theme: egui::Theme) -> (Color32, Color32, Color32) {
    match theme {
        egui::Theme::Light => (a.dark1, a.dark2, Color32::WHITE),
        egui::Theme::Dark => (a.light2, a.light3, Color32::BLACK),
    }
}

/// Баннер core и активный вопрос над карточками. Тексты и подписи уже переведены core.
fn top_bars(ui: &mut egui::Ui, i18n: &I18n, s: &AppState, st: &UiState, out: &mut Vec<Command>) {
    if let Some(b) = &s.banner {
        ui.horizontal_wrapped(|ui| {
            text::label_styled(
                ui,
                i18n,
                &Tx::user(b.text.as_str()),
                Some(Color32::from_rgb(220, 80, 60)),
                false,
            );
            for (label, cmd) in &b.actions {
                if text::button(ui, i18n, &Tx::user(label.as_str())).clicked() {
                    out.push(cmd.clone());
                }
            }
        });
        ui.add_space(4.0);
    }
    if let Some(p) = s
        .prompts
        .active()
        .filter(|p| !st.prompt_hidden.contains(&p.id))
    {
        ui.horizontal_wrapped(|ui| {
            text::label_styled(ui, i18n, &Tx::user(p.spec.title.as_str()), None, true);
            text::label(ui, i18n, &Tx::user(p.spec.body.as_str()));
            for c in &p.spec.choices {
                if text::button(ui, i18n, &Tx::user(c.label.as_str())).clicked() {
                    out.push(
                        Command::new("prompt.reply")
                            .arg("id", Value::Int(i64::try_from(p.id.0).unwrap_or(i64::MAX)))
                            .arg("choice", c.id.as_str()),
                    );
                }
            }
        });
        ui.add_space(4.0);
    }
}

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests;
