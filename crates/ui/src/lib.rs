//! FileNav — UI-фронтенд на egui (S2).
//!
//! [`FileNavUi`] — две карточки панелей, баннер, вопрос, тосты и keybar (каркас S2-02);
//! вид — из токенов Fluent ([`tokens`], S2-05) по теме и акценту ОС ([`sys`], S2-03).
//! Handler `ui` и [`UiState`] в `AppState.ext` — [`register`]. Ввод панели — в app
//! (`raw_input_hook`, до keymap S3); здесь — мышь. Тексты — только через [`text`]
//! (`clippy.toml`: `disallowed-methods`).

pub mod accent;
pub mod config;
mod handler;
pub mod highlight;
pub mod keybar;
mod panel;
pub mod state;
pub mod sys;
pub mod text;
pub mod tokens;

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{Align2, Sense, TextStyle};
use filenav_core::{
    AppState, Command, CommandRegistry, ConfigWarning, Core, Level, Side, UiEvent, Value,
};
use filenav_i18n::I18n;

pub use handler::UiHandler;
pub use state::{Draft, ThemeMode, UiState};

use crate::config::UiCfg;
use crate::highlight::Highlighter;
use crate::panel::{CardMemo, Shown};
use crate::text::Tx;
use crate::tokens::{Metric, Role, Tokens};

/// Сколько показывается тост.
const TOAST_TTL: Duration = Duration::from_secs(4);
/// Не больше стольких тостов сразу (S2 §3).
const TOASTS_MAX: usize = 3;

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
    cfg: UiCfg,
    /// Тема из `[ui] theme`; команда `ui.theme` её перекрывает.
    cfg_theme: ThemeMode,
    tokens: Tokens,
    highlighter: Highlighter,
    /// Применённые настройки вида: тема, масштаб, `look` ОС.
    applied: Option<(ThemeMode, u16, sys::Look)>,
}

impl FileNavUi {
    /// `registry` — команды всех handlers (keybar гасит незарегистрированные);
    /// `probe` — [`sys::probe_system`]; `cfg` — секция `[ui]`. Предупреждения probe и
    /// конфига показываются тостами.
    pub fn new(registry: &CommandRegistry, probe: &sys::SysProbe, cfg: &UiCfg) -> Self {
        let (tokens, mut warnings) = Tokens::build(probe, cfg);
        let (highlighter, more) = Highlighter::build(&cfg.colors);
        warnings.extend(more);
        let cfg_theme = match cfg.theme.trim().to_ascii_lowercase().as_str() {
            "" | "system" => ThemeMode::System,
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            other => {
                warnings.push(ConfigWarning {
                    key: "ui.theme".to_owned(),
                    msg: format!("expected system | light | dark, got {other:?}"),
                });
                ThemeMode::System
            }
        };
        let far = Instant::now() + TOAST_TTL;
        let toasts = probe
            .warnings
            .iter()
            .chain(&warnings)
            .map(|w| (format!("{}: {}", w.key, w.msg), far))
            .collect();
        Self {
            toasts,
            cards: [CardMemo::default(), CardMemo::default()],
            keybar: keybar::Keybar::default(),
            registered: keybar::registered(registry),
            look: probe.look,
            look_rx: None,
            cfg: cfg.clone(),
            cfg_theme,
            tokens,
            highlighter,
            applied: None,
        }
    }

    /// Токены текущего вида (тема — та, что у egui в этом кадре).
    pub fn tokens(&self) -> &Tokens {
        &self.tokens
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
        self.tokens.set_theme(ctx.theme());
        self.take_toasts(&ctx, ev);
        self.keybar
            .refresh(&i18n, filenav_i18n::epoch(), &self.registered);

        let t = &self.tokens;
        let gap = t.m(Metric::GapPanels);
        if self.cfg.keybar {
            egui::Panel::bottom("keybar")
                .resizable(false)
                .exact_size(t.m(Metric::HKeybar) + gap)
                .frame(egui::Frame::new().fill(t.c(Role::BgBase)))
                .show(root, |ui| self.paint_keybar(ui, &i18n, out));
        }
        let central = egui::Frame::new()
            .fill(self.tokens.c(Role::BgBase))
            .inner_margin(egui::Margin::same(gap as i8));
        egui::CentralPanel::default()
            .frame(central)
            .show(root, |ui| {
                top_bars(ui, &i18n, &self.tokens, s, st, out);
                let h = ui.available_height();
                let w = ui.available_width();
                let left_w = ((w - gap) * st.split).max(0.0);
                ui.horizontal_top(|ui| {
                    for side in [Side::Left, Side::Right] {
                        let width = match side {
                            Side::Left => left_w,
                            Side::Right => (w - gap - left_w).max(0.0),
                        };
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(width, h), Sense::hover());
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
        let mode = st.theme.unwrap_or(self.cfg_theme);
        let want = (mode, st.zoom, self.look);
        if self.applied == Some(want) {
            return;
        }
        if self.applied.is_none_or(|(_, _, look)| look != self.look) {
            let probe = sys::SysProbe {
                look: self.look,
                ..sys::SysProbe::default()
            };
            // Предупреждения конфига уже показаны при создании.
            self.tokens = Tokens::build(&probe, &self.cfg).0;
            tokens::style::apply(ctx, &self.tokens);
        }
        self.applied = Some(want);
        ctx.set_theme(match (mode, self.look.theme) {
            (ThemeMode::Light, _) | (ThemeMode::System, Some(egui::Theme::Light)) => {
                egui::ThemePreference::Light
            }
            (ThemeMode::Dark, _) | (ThemeMode::System, Some(egui::Theme::Dark)) => {
                egui::ThemePreference::Dark
            }
            (ThemeMode::System, None) => egui::ThemePreference::System,
        });
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
        let tk = &self.tokens;
        let hl = &self.highlighter;
        let card = panel::card_frame(tk).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            tab_strip(ui, i18n, tk, p, side, out);
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

            let row_h = tk.m(Metric::HRow);
            let footer_h = tk.m(Metric::HPanelFooter);
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
                lines: panel::lines(i18n, hl, t, page),
            });
            let mut list = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
            panel::Rows {
                i18n,
                tokens: tk,
                hl,
                active,
                interactive,
                side,
            }
            .paint(&mut list, &shown.lines, out);
            ui.separator();
            text::label_styled(
                ui,
                i18n,
                &Tx::user(shown.footer.as_str()),
                Some(tk.c(Role::TextSecondary)),
                false,
            );
            memo.shown = Some(shown);
        });
        if active {
            panel::accent_bar(ui, tk, card.response.rect);
        }
    }

    fn paint_keybar(&self, ui: &mut egui::Ui, i18n: &I18n, out: &mut Vec<Command>) {
        let t = &self.tokens;
        let n = 12.0;
        let gap = t.m(Metric::GapPanels);
        let strip = ui
            .available_rect_before_wrap()
            .shrink2(egui::vec2(gap, 0.0))
            .with_max_y(ui.available_rect_before_wrap().bottom() - gap);
        let w = strip.width() / n;
        let font = TextStyle::Small.resolve(ui.style());
        let chip = panel::radius(t, Metric::RadiusControl);
        for item in &self.keybar.items {
            let x = strip.left() + w * f32::from(item.n - 1);
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, strip.top()),
                egui::vec2(w, strip.height()),
            )
            .shrink2(egui::vec2(t.m(Metric::SpaceXxs), 0.0));
            let sense = if item.enabled {
                Sense::CLICK
            } else {
                Sense::hover()
            };
            let resp = ui.interact(rect, ui.id().with(("fkey", item.n)), sense);
            let painter = ui.painter_at(rect);
            let fill = if item.enabled && resp.hovered() {
                t.c(Role::FillSubtleHover)
            } else {
                t.c(Role::FillControl)
            };
            painter.rect_filled(rect, chip, fill);
            let num = text::paint(
                &painter,
                i18n,
                rect.left_center() + egui::vec2(t.m(Metric::SpaceS), 0.0),
                Align2::LEFT_CENTER,
                &Tx::user(item.n.to_string()),
                font.clone(),
                t.c(Role::TextSecondary),
            );
            let color = if item.enabled {
                t.c(Role::TextPrimary)
            } else {
                t.c(Role::TextDisabled)
            };
            text::paint(
                &painter,
                i18n,
                egui::pos2(num.right() + t.m(Metric::SpaceXs), rect.center().y),
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
            .anchor(
                Align2::RIGHT_BOTTOM,
                egui::vec2(
                    -self.tokens.m(Metric::SpaceM),
                    -(self.tokens.m(Metric::HKeybar) + self.tokens.m(Metric::SpaceL)),
                ),
            )
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

/// Табы панели строкой над путём (в заголовок окна переедут в S2-13).
fn tab_strip(
    ui: &mut egui::Ui,
    i18n: &I18n,
    tk: &Tokens,
    p: &filenav_core::Panel,
    side: Side,
    out: &mut Vec<Command>,
) {
    ui.horizontal_wrapped(|ui| {
        for (i, tab) in p.tabs.iter().enumerate() {
            let current = i == p.active;
            let color = tk.c(if current {
                Role::TextPrimary
            } else {
                Role::TextSecondary
            });
            let resp = text::label_styled(
                ui,
                i18n,
                &Tx::user(tab.title().as_ref()),
                Some(color),
                current,
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
}

/// Баннер core и активный вопрос над карточками. Тексты и подписи уже переведены core.
fn top_bars(
    ui: &mut egui::Ui,
    i18n: &I18n,
    t: &Tokens,
    s: &AppState,
    st: &UiState,
    out: &mut Vec<Command>,
) {
    if let Some(b) = &s.banner {
        let fg = match b.level {
            Level::Error => Role::CriticalFg,
            Level::Warn => Role::CautionFg,
            Level::Info => Role::TextPrimary,
        };
        ui.horizontal_wrapped(|ui| {
            text::label_styled(ui, i18n, &Tx::user(b.text.as_str()), Some(t.c(fg)), false);
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
