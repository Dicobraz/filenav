//! Интерфейс на GPUI (ADR-0018). G-02 — каркас: [`CoreHost`] владеет `Core` и качает
//! его `pump` только по сигналу [`Signal`] (воркеры Core, вторые запуски) — без таймеров
//! опроса; в простое кадров нет. Клавиатура — конвейер keymap S3 ([`input`]).

mod chrome;
mod field;
mod input;
mod menu;
mod overlays;
mod panel;
mod theme;
mod views;

pub use theme::Theme;

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, AsyncApp, Bounds, ClickEvent, Context, FocusHandle, IntoElement, KeyUpEvent,
    ModifiersChangedEvent, Render, SharedString, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowOptions, div, point, px, size,
};
use urfin_core::{Command, Core, Side, WindowGeom};

use crate::tokens::{Metric, Role};

/// Бюджет `pump` за один проход (как у egui-окна, S1 §1).
const PUMP_BUDGET: Duration = Duration::from_millis(4);
enum Wake {
    Now,
    After(Duration),
}

/// `urfin_core::Waker` окна GPUI: сигнал в канал, его ждёт задача основного потока.
#[derive(Debug)]
pub struct Signal {
    tx: UnboundedSender<Wake>,
}

/// Приёмник сигналов [`Signal`]: отдаётся в [`run`].
#[derive(Debug)]
pub struct WakeRx(UnboundedReceiver<Wake>);

impl std::fmt::Debug for Wake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wake::Now => f.write_str("Now"),
            Wake::After(d) => write!(f, "After({d:?})"),
        }
    }
}

impl Signal {
    /// Пара «будильник для Core — приёмник для окна».
    pub fn new() -> (Arc<Signal>, WakeRx) {
        let (tx, rx) = unbounded();
        (Arc::new(Signal { tx }), WakeRx(rx))
    }
}

impl urfin_core::Waker for Signal {
    fn wake(&self) {
        // Окно закрыто — приёмника нет, будить некого.
        let _ = self.tx.unbounded_send(Wake::Now);
    }

    fn wake_after(&self, d: Duration) {
        let _ = self.tx.unbounded_send(Wake::After(d));
    }
}

/// Что приложение делает на каждом проходе `pump`: вторые запуски (S1-15). `true` —
/// окно вывести вперёд.
pub type Poll = Box<dyn FnMut(&mut Core) -> bool>;

/// Всё, что окну GPUI нужно от приложения.
pub struct Launch {
    pub core: Core,
    pub wake: WakeRx,
    /// Будильник того же `Core`: повторный проход, если `pump` не успел всё.
    pub signal: Arc<Signal>,
    pub poll: Poll,
    /// Конвейер клавиш keymap (S3).
    pub keys: urfin_keymap::KeyInput,
    /// Тема Fluent (токены, раскраска, формат ячеек).
    pub theme: Theme,
    /// После первого кадра; `true` — выйти (замер старта).
    pub first_frame: Option<Box<dyn FnOnce() -> bool>>,
    /// `--bench`: сценарий замера (G-07).
    pub bench: Option<crate::bench::Bench>,
}

impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launch").finish_non_exhaustive()
    }
}

/// Владелец `Core` в окне GPUI (ADR-0018): `pump` по сигналу, `cx.notify()` — только
/// когда состояние изменилось.
pub struct CoreHost {
    core: Option<Core>,
    signal: Arc<Signal>,
    poll: Poll,
    rev: u64,
    /// Сетка панелей (строк в колонке, колонок), отправленная в Core (`panel.viewport`).
    grid: [(u32, u32); 2],
    theme: Theme,
    keybar: crate::keybar::Keybar,
    registered: Vec<&'static str>,
    /// Зажатые модификаторы — подписи F-клавиш.
    mods: crate::keybar::Mods,
    toasts: Vec<overlays::Toast>,
    first_frame: Option<Box<dyn FnOnce() -> bool>>,
    bench: Option<crate::bench::Bench>,
    /// Открытое поле ввода (черновик, переименование).
    field: Option<field::Field>,
    /// Черновик удаления — сразу насовсем (Shift+F8 или корзины нет).
    ops_permanent: bool,
    menu: menu::Menu,
    /// Остаток прокрутки колесом в строках (по панелям).
    wheel_acc: [f32; 2],
    /// Выбранный вариант видимого вопроса: (id вопроса, индекс).
    choice: (u64, usize),
    focus: FocusHandle,
    input: input::Input,
    out: Vec<Command>,
}

impl std::fmt::Debug for CoreHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreHost")
            .field("rev", &self.rev)
            .finish_non_exhaustive()
    }
}

impl CoreHost {
    fn new(launch: Launch, cx: &mut Context<Self>) -> CoreHost {
        let Launch {
            core,
            wake,
            signal,
            poll,
            keys,
            theme,
            first_frame,
            bench,
        } = launch;
        listen(wake, cx);
        let registered = crate::keybar::registered(core.registry());
        let menu = menu::Menu::new(core.registry(), None);
        CoreHost {
            core: Some(core),
            signal,
            poll,
            rev: u64::MAX,
            grid: [(0, 0); 2],
            theme,
            keybar: crate::keybar::Keybar::default(),
            registered,
            mods: crate::keybar::Mods::None,
            toasts: Vec::new(),
            first_frame,
            bench,
            field: None,
            ops_permanent: false,
            menu,
            wheel_acc: [0.0; 2],
            choice: (u64::MAX, 0),
            focus: cx.focus_handle(),
            input: input::Input::new(keys),
            out: Vec::new(),
        }
    }

    /// Какие панели видны и их ширины: Ctrl+O — ни одной; узкое окно (< 720) — только
    /// активная; иначе — по `ui.split` (S2-07).
    fn layout(theme: &Theme, s: &urfin_core::AppState, width: f32) -> [Option<f32>; 2] {
        let st = s.ext.get::<crate::UiState>();
        if st.is_some_and(|u| u.panels_hidden) {
            return [None, None];
        }
        let gap = theme.m(Metric::GapPanels);
        let avail = width - 2.0 * gap;
        let shown = [s.panel(Side::Left).visible, s.panel(Side::Right).visible];
        let narrow = width < crate::layout::SINGLE_PANEL_BELOW;
        match (shown, narrow) {
            ([true, true], false) => {
                let split = st.map_or(0.5, |u| u.split).clamp(0.1, 0.9);
                let l = (avail - gap) * split;
                [Some(l), Some(avail - gap - l)]
            }
            ([true, true], true) => {
                if s.active == Side::Left {
                    [Some(avail), None]
                } else {
                    [None, Some(avail)]
                }
            }
            ([true, false], _) => [Some(avail), None],
            ([false, true], _) => [None, Some(avail)],
            _ => [None, None],
        }
    }

    /// Сетка каждой панели — в Core (`panel.viewport`): по ней он держит курсор в
    /// видимой странице и считает PgUp/PgDn и ←/→ в колонках.
    fn sync_viewport(&mut self, window: &Window) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let size = window.viewport_size();
        let widths = Self::layout(&self.theme, core.state(), f32::from(size.width));
        let h = f32::from(size.height)
            - self.theme.m(Metric::HTitlebar)
            - self.theme.m(Metric::HCmdline)
            - self.theme.m(Metric::HKeybar);
        for (i, side) in [Side::Left, Side::Right].into_iter().enumerate() {
            let mode = core.state().panel(side).tab().view_mode;
            let Some(w) = widths[i] else {
                continue;
            };
            let g = panel::grid(&self.theme, mode, w, h);
            if g == self.grid[i] {
                continue;
            }
            self.grid[i] = g;
            core.dispatch(
                Command::new("panel.viewport")
                    .arg("side", if i == 0 { "left" } else { "right" })
                    .arg("rows", i64::from(g.0))
                    .arg("cols", i64::from(g.1)),
            );
            urfin_core::Waker::wake(&*self.signal);
        }
    }

    /// Проход Core: вторые запуски, `pump`, выход по `quit`.
    fn pump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        if (self.poll)(core) {
            window.activate_window();
        }
        let p = core.pump(PUMP_BUDGET);
        if p.more {
            urfin_core::Waker::wake(&*self.signal);
        }
        if core.state().quit {
            self.shutdown(cx);
            return;
        }
        self.take_events(cx);
        // Режим вида мог смениться: сетка панели другая.
        self.sync_viewport(window);
        let Some(core) = self.core.as_ref() else {
            return;
        };
        let rev = core.state().rev;
        if rev != self.rev {
            self.rev = rev;
            cx.notify();
        }
    }

    fn dispatch(&mut self, cmd: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(core) = self.core.as_mut() {
            core.dispatch(Command::new(cmd));
        }
        self.pump(window, cx);
    }

    /// События UI из Core: тосты (дедлайн — будильником); job-бар ≤ 30 кадров/с.
    fn take_events(&mut self, cx: &mut Context<Self>) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let now = std::time::Instant::now();
        let mut drafts = Vec::new();
        for ev in core.take_ui_events() {
            match ev {
                urfin_core::UiEvent::Toast { level, text } => {
                    self.toasts.push(overlays::Toast::new(level, text, now));
                    cx.notify();
                }
                // Черновик операции (F5/F6/F7/F8): строка с полем или кнопками (S2-15).
                urfin_core::UiEvent::Ext { id, args } if id.as_str() == "ops.draft" => {
                    let command = args.str("command").unwrap_or_default().to_owned();
                    let arg = match command.as_str() {
                        "ops.copy" | "ops.move" => "dest",
                        "ops.mkdir" => "name",
                        _ => "confirmed",
                    };
                    self.ops_permanent = args.bool("permanent").unwrap_or(false);
                    drafts.push(
                        Command::new("ui.draft")
                            .arg("command", command.as_str())
                            .arg("arg", arg),
                    );
                }
                _ => {}
            }
        }
        if !drafts.is_empty() {
            for c in drafts {
                core.dispatch(c);
            }
            urfin_core::Waker::wake(&*self.signal);
            cx.notify();
        }
        let before = self.toasts.len();
        self.toasts.retain(|t| t.until > now);
        if self.toasts.len() != before {
            cx.notify();
        }
        if let Some(next) = self.toasts.iter().map(|t| t.until).min() {
            urfin_core::Waker::wake_after(&*self.signal, next - now);
        }
        if overlays::jobs_running(core.state()) {
            urfin_core::Waker::wake_after(&*self.signal, Duration::from_millis(33));
            cx.notify();
        }
    }

    /// Клавиши видимого вопроса: Enter — выбранный, Esc — отмена, ←/→/Tab — выбор,
    /// буква варианта — ответ.
    fn prompt_key(&mut self, ks: &gpui_kit::Keystroke) -> Option<Command> {
        let p = self.core.as_ref()?.state().prompts.active()?;
        let n = p.spec.choices.len();
        if self.choice.0 != p.id.0 {
            self.choice = (p.id.0, usize::from(p.spec.default).min(n.saturating_sub(1)));
        }
        let sel = &mut self.choice.1;
        match ks.key.as_str() {
            "escape" => return Some(overlays::cancel(p)),
            "enter" => return overlays::reply(p, *sel),
            "left" => *sel = sel.saturating_sub(1),
            "right" | "tab" => *sel = (*sel + 1).min(n.saturating_sub(1)),
            k => {
                let typed = ks.key_char.as_deref().unwrap_or(k).to_lowercase();
                let i = p.spec.choices.iter().position(|c| {
                    let key = c.key.to_lowercase().to_string();
                    key == k || key == typed
                })?;
                return overlays::reply(p, i);
            }
        }
        None
    }

    /// Несколько команд мыши подряд.
    fn run_all(&mut self, cmds: Vec<Command>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(core) = self.core.as_mut() {
            for c in cmds {
                core.dispatch(c.from(urfin_core::Origin::Mouse));
            }
        }
        self.pump(window, cx);
    }

    /// Клик по строке: панель — активной, курсор — на строку; Ctrl — пометка;
    /// двойной — вход (S2 §2.1).
    fn row_click(
        &mut self,
        side: Side,
        name: Option<&str>,
        clicks: usize,
        ctrl: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut cmds = self.focus_and_cursor(side, name);
        if ctrl && name.is_some() {
            cmds.push(Command::new("sel.toggle"));
        }
        if clicks >= 2 {
            cmds.push(Command::new("panel.enter"));
        }
        self.run_all(cmds, window, cx);
    }

    /// ПКМ — пометка строки (как в Far); `..` не помечается.
    fn row_right_click(
        &mut self,
        side: Side,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut cmds = self.focus_and_cursor(side, name);
        if name.is_some() {
            cmds.push(Command::new("sel.toggle"));
        }
        self.run_all(cmds, window, cx);
    }

    fn focus_and_cursor(&self, side: Side, name: Option<&str>) -> Vec<Command> {
        let active = self.core.as_ref().is_some_and(|c| c.state().active == side);
        let mut cmds = Vec::with_capacity(3);
        if !active {
            cmds.push(Command::new("panel.focus").arg("side", chrome::side_name(side)));
        }
        cmds.push(match name {
            Some(n) => Command::new("panel.cursor.to").arg("name", n),
            None => Command::new("panel.cursor.home"),
        });
        cmds
    }

    /// Колесо: строки копятся с дробной частью, сдвиг — `panel.viewport{top}`.
    fn wheel(
        &mut self,
        side: Side,
        ev: &gpui_kit::ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row_h = self.theme.m(Metric::HRow);
        let dy = f32::from(ev.delta.pixel_delta(px(row_h)).y);
        let i = usize::from(side == Side::Right);
        self.wheel_acc[i] -= dy / row_h;
        let whole = self.wheel_acc[i].trunc();
        self.wheel_acc[i] -= whole;
        #[allow(clippy::cast_possible_truncation, reason = "строк за событие немного")]
        let lines = whole as i64;
        let Some(core) = self.core.as_ref() else {
            return;
        };
        if lines == 0 {
            return;
        }
        let t = core.state().panel(side).tab();
        let top = (i64::from(t.cursor.top) + lines).max(0);
        let (rows, cols) = self.grid[i];
        self.run_all(
            vec![
                Command::new("panel.viewport")
                    .arg("side", chrome::side_name(side))
                    .arg("rows", i64::from(rows))
                    .arg("cols", i64::from(cols))
                    .arg("top", top),
            ],
            window,
            cx,
        );
    }

    /// Тема: `ui.theme` или, при «системной», светлая/тёмная ОС (gpui следит за ней).
    fn sync_theme(&mut self, window: &Window) {
        let mode = self
            .core
            .as_ref()
            .and_then(|c| c.state().ext.get::<crate::UiState>())
            .and_then(|u| u.theme)
            .unwrap_or_default();
        let dark = match mode {
            crate::ThemeMode::Dark => true,
            crate::ThemeMode::Light => false,
            crate::ThemeMode::System => matches!(
                window.appearance(),
                gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark
            ),
        };
        let want = if dark {
            crate::color::Theme::Dark
        } else {
            crate::color::Theme::Light
        };
        if self.theme.tokens.theme() != want {
            self.theme.tokens.set_theme(want);
        }
    }

    /// Шаг сценария `--bench` на кадре: команды, размер окна, простой или отчёт и выход.
    fn drive_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.bench.as_mut() else {
            return;
        };
        match b.frame(std::time::Instant::now()) {
            crate::bench::Step::Next => window.request_animation_frame(),
            crate::bench::Step::Cmds(cmds) => {
                if let Some(core) = self.core.as_mut() {
                    for c in cmds {
                        core.dispatch(c);
                    }
                    core.pump(PUMP_BUDGET);
                }
                window.request_animation_frame();
            }
            crate::bench::Step::Resize(w, h) => {
                window.resize(size(px(w), px(h)));
                window.request_animation_frame();
            }
            crate::bench::Step::Idle(d) => {
                // Простой: кадров не просим; разбудит таймер, кадр — по notify.
                let this = cx.entity().downgrade();
                cx.spawn_in(window, async move |_, cx| {
                    cx.background_executor().timer(d).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            crate::bench::Step::Done(report) => {
                println!("bench ui=gpui {report}");
                tracing::info!(%report, "bench");
                self.bench = None;
                self.shutdown(cx);
            }
        }
    }

    /// Команда от мыши (табы, F-клавиши).
    fn run(&mut self, cmd: Command, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(core) = self.core.as_mut() {
            core.dispatch(cmd.from(urfin_core::Origin::Mouse));
        }
        self.pump(window, cx);
    }

    /// Сессия на диск и выход из цикла GPUI.
    fn shutdown(&mut self, cx: &mut Context<Self>) {
        if let Some(core) = self.core.take() {
            let session = core.shutdown(Duration::from_millis(500));
            tracing::info!(window = ?session.window, "session saved on exit");
        }
        cx.quit();
    }

    /// Геометрия окна — в сессию (размер развёрнутого не сохраняется).
    fn track_bounds(&mut self, window: &mut Window) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let last = core.window();
        let g = match window.window_bounds() {
            WindowBounds::Windowed(b) => WindowGeom {
                x: f32::from(b.origin.x),
                y: f32::from(b.origin.y),
                w: f32::from(b.size.width),
                h: f32::from(b.size.height),
                maximized: false,
                split: last.map_or(0.5, |g| g.split),
            },
            WindowBounds::Maximized(_) | WindowBounds::Fullscreen(_) => match last {
                Some(g) => WindowGeom {
                    maximized: true,
                    ..g
                },
                None => return,
            },
        };
        core.set_window(g);
    }

    /// Команды keymap — в Core, проход `pump`; дедлайн аккорда — будильник.
    fn after_input(
        &mut self,
        o: Option<input::Outcome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(core) = self.core.as_mut() {
            for c in self.out.drain(..) {
                core.dispatch(c);
            }
        }
        if let Some(t) = o.and_then(|o| o.wake_at) {
            let d = t.saturating_duration_since(std::time::Instant::now());
            urfin_core::Waker::wake_after(&*self.signal, d);
        }
        self.pump(window, cx);
    }

    /// Нажатие (перехват до привязок gpui): `true` — keymap забрал его.
    fn key_down(
        &mut self,
        ks: &gpui_kit::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(core) = self.core.as_ref() else {
            return false;
        };
        // Поле ввода или черновик: Enter — выполнить, Esc — закрыть; остальное — полю.
        let editing = self.field.is_some()
            || core
                .state()
                .ext
                .get::<crate::UiState>()
                .is_some_and(|u| u.draft.is_some());
        if editing {
            let cmds = match ks.key.as_str() {
                "escape" => vec![Command::new("ui.cancel")],
                "enter" => self.field_commit(ks.modifiers.shift, cx),
                _ => return false,
            };
            self.out
                .extend(cmds.into_iter().map(|c| c.from(urfin_core::Origin::Key)));
            self.after_input(None, window, cx);
            return true;
        }
        // Открытое меню F9 разбирает клавиши само (S2 §2.6).
        if let Some(open) = core
            .state()
            .ext
            .get::<crate::UiState>()
            .and_then(crate::menu::open_id)
            .map(str::to_owned)
        {
            let cmds = self.menu.key(&open, ks);
            self.out
                .extend(cmds.into_iter().map(|c| c.from(urfin_core::Origin::Key)));
            self.after_input(None, window, cx);
            cx.notify();
            return true;
        }
        // Видимый вопрос разбирает клавиши сам (S2-15).
        if core.state().prompts.active().is_some() {
            if let Some(cmd) = self.prompt_key(ks) {
                self.out.push(cmd.from(urfin_core::Origin::Key));
            }
            self.after_input(None, window, cx);
            cx.notify();
            return true;
        }
        let o = self.input.key_down(ks, core.state(), &mut self.out);
        let consumed = o.consumed;
        self.after_input(Some(o), window, cx);
        consumed
    }

    fn key_up(&mut self, ev: &KeyUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(core) = self.core.as_ref() {
            self.input
                .key_up(&ev.keystroke, core.state(), &mut self.out);
        }
        self.after_input(None, window, cx);
    }

    fn modifiers(
        &mut self,
        ev: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = ev.modifiers;
        let mods = match (m.shift, m.control, m.alt) {
            (true, false, false) => crate::keybar::Mods::Shift,
            (false, true, false) => crate::keybar::Mods::Ctrl,
            (false, false, true) => crate::keybar::Mods::Alt,
            _ => crate::keybar::Mods::None,
        };
        if mods != self.mods {
            self.mods = mods;
            cx.notify();
        }
        let o = match self.core.as_ref() {
            Some(core) => self
                .input
                .modifiers(ev.modifiers, core.state(), &mut self.out),
            None => return,
        };
        self.after_input(Some(o), window, cx);
    }

    fn activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_window_active() {
            return;
        }
        if let Some(core) = self.core.as_ref() {
            self.input.focus_lost(core.state(), &mut self.out);
        }
        self.after_input(None, window, cx);
    }
}

/// Сигналы Core → проход `pump` на основном потоке. Очередь сигналов сливается в один
/// проход; `wake_after` — отдельная задача с таймером исполнителя.
fn listen(wake: WakeRx, cx: &mut Context<CoreHost>) {
    let mut rx = wake.0;
    cx.spawn(async move |this: WeakEntity<CoreHost>, cx: &mut AsyncApp| {
        while let Some(first) = rx.next().await {
            let mut now = false;
            let mut later = Vec::new();
            let mut item = Some(first);
            while let Some(w) = item {
                match w {
                    Wake::Now => now = true,
                    Wake::After(d) => later.push(d),
                }
                item = rx.try_recv().ok();
            }
            for d in later {
                let this = this.clone();
                cx.spawn(async move |cx: &mut AsyncApp| {
                    cx.background_executor().timer(d).await;
                    pump_in_window(&this, cx);
                })
                .detach();
            }
            if now && !pump_in_window(&this, cx) {
                break;
            }
        }
    })
    .detach();
}

/// Проход `pump` в окне хоста; `false` — окна или хоста больше нет.
fn pump_in_window(this: &WeakEntity<CoreHost>, cx: &mut AsyncApp) -> bool {
    let Some(entity) = this.upgrade() else {
        return false;
    };
    let Some(handle) = cx.update(|cx| cx.windows().first().copied()) else {
        return false;
    };
    handle
        .update(cx, |_, window, cx| {
            entity.update(cx, |host, cx| host.pump(window, cx));
        })
        .is_ok()
}

impl Render for CoreHost {
    #[allow(clippy::too_many_lines, reason = "сборка зон окна")]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_field(window, cx);
        self.sync_theme(window);
        self.drive_bench(window, cx);
        if let Some(f) = self.first_frame.take() {
            // Кадр уже на экране к следующему циклу: замер и, для `--bench-startup`, выход.
            cx.on_next_frame(window, move |h, _, cx| {
                if f() {
                    h.shutdown(cx);
                }
            });
        }
        let gap = px(self.theme.m(Metric::GapPanels));
        let i18n = urfin_i18n::current();
        self.keybar.refresh(
            &i18n,
            urfin_i18n::epoch(),
            &self.registered,
            &crate::keybar::FarStub,
        );
        let mut root = div()
            .track_focus(&self.focus)
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers))
            .size_full()
            .flex()
            .flex_col()
            .bg(self.theme.c(Role::BgBase))
            .text_color(self.theme.c(Role::TextPrimary))
            .text_size(px(self.theme.m(Metric::TypeBody)));
        let Some(core) = &self.core else {
            return root;
        };
        let s = core.state();
        let click = |cmd: Command| -> overlays::Handler {
            Box::new(cx.listener(move |h: &mut CoreHost, _: &ClickEvent, w, cx| {
                h.run(cmd.clone(), w, cx);
            }))
        };
        let click_all = |cmds: Vec<Command>| -> overlays::Handler {
            Box::new(cx.listener(move |h: &mut CoreHost, _: &ClickEvent, w, cx| {
                h.run_all(cmds.clone(), w, cx);
            }))
        };
        let open = s
            .ext
            .get::<crate::UiState>()
            .and_then(crate::menu::open_id)
            .map(str::to_owned);
        let content = match &open {
            Some(id) => self.menu.bar(id, &self.theme, &click),
            None => chrome::panel_tabs(s, &self.theme, &click),
        };
        root = root.child(chrome::titlebar(
            content,
            &self.theme,
            window.is_maximized(),
        ));
        let widths = Self::layout(&self.theme, s, f32::from(window.viewport_size().width));
        let mut panels = div().flex_1().min_h_0().flex().flex_row().px(gap);
        for (i, side) in [Side::Left, Side::Right].into_iter().enumerate() {
            let Some(w) = widths[i] else {
                continue;
            };
            if i == 1 && widths[0].is_some() {
                // Зазор между карточками: курсор ↔, двойной клик — 50/50 (S2-07).
                panels = panels.child(
                    div()
                        .id("splitter")
                        .w(gap)
                        .flex_none()
                        .h_full()
                        .cursor(gpui_kit::CursorStyle::ResizeLeftRight)
                        .on_click(cx.listener(|h, ev: &ClickEvent, w, cx| {
                            if ev.click_count() >= 2 {
                                h.run(Command::new("ui.split").arg("reset", true), w, cx);
                            }
                        })),
                );
            }
            let (rows, cols) = self.grid[i];
            panels = panels.child(
                div()
                    .w(px(w))
                    .flex_none()
                    .h_full()
                    .flex()
                    .child(panel::panel(
                        s,
                        side,
                        &self.theme,
                        rows,
                        cols,
                        self.field.as_ref(),
                        cx,
                    )),
            );
        }
        let prompt = s.prompts.active().map(|p| {
            let sel = if self.choice.0 == p.id.0 {
                self.choice.1
            } else {
                usize::from(p.spec.default)
            };
            overlays::prompt_bar(p, sel, &self.theme, &click)
        });
        root.child(panels)
            .children(overlays::job_bar(s, &self.theme))
            .children(field::draft_bar(
                s,
                self.field.as_ref(),
                self.ops_permanent,
                &self.theme,
                &click_all,
            ))
            .children(prompt)
            .child(chrome::cmdline(s, &self.theme))
            .child(chrome::keybar(
                self.keybar.items(self.mods),
                self.mods,
                &self.theme,
                &click,
            ))
            .children(open.as_ref().map(|_| {
                // Щит под меню: клик мимо закрывает его и не уходит в панель.
                div()
                    .id("menu-shield")
                    .absolute()
                    .top(px(self.theme.m(Metric::HTitlebar)))
                    .left_0()
                    .size_full()
                    .on_click(cx.listener(|h, _: &ClickEvent, w, cx| {
                        h.run(Command::new("ui.cancel"), w, cx);
                    }))
            }))
            .children(open.as_ref().and_then(|id| {
                self.menu
                    .flyout(id, &crate::keybar::FarStub, &self.theme, &click_all)
            }))
            .children(overlays::toasts(&self.toasts, &self.theme))
    }
}

/// Окно GPUI с Core; возвращается после выхода из цикла (сессия уже сохранена).
pub fn run(launch: Launch, geom: Option<WindowGeom>) {
    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        let bounds = match geom {
            Some(g) => Bounds {
                origin: point(px(g.x), px(g.y)),
                size: size(px(g.w), px(g.h)),
            },
            None => Bounds::centered(None, size(px(1200.), px(760.)), cx),
        };
        let window_bounds = if geom.is_some_and(|g| g.maximized) {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        };
        let options = WindowOptions {
            window_bounds: Some(window_bounds),
            // Свой заголовок с табами (S2 §1.4): системный скрыт.
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::new_static("Urfin")),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            let host = cx.new(|cx| CoreHost::new(launch, cx));
            host.update(cx, |h, cx| {
                window.focus(&h.focus, cx);
                // Крестик окна — через `app.quit`: handlers могут наложить вето (S0 v2 №13).
                let weak = cx.weak_entity();
                window.on_window_should_close(cx, move |window, cx| {
                    weak.update(cx, |h, cx| {
                        if h.core.is_none() {
                            return true;
                        }
                        // Перемещение окна bounds-наблюдатель не видит: позиция — на выходе.
                        h.track_bounds(window);
                        h.dispatch("app.quit", window, cx);
                        h.core.is_none()
                    })
                    .unwrap_or(true)
                });
                cx.observe_window_activation(window, CoreHost::activation)
                    .detach();
                cx.observe_window_appearance(window, |_, _, cx| cx.notify())
                    .detach();
                cx.observe_window_bounds(window, |h, window, cx| {
                    h.track_bounds(window);
                    h.sync_viewport(window);
                    h.pump(window, cx);
                })
                .detach();
                h.sync_viewport(window);
                h.pump(window, cx);
            });
            // Клавиши — до привязок gpui-kit (его `Root` забирает Tab на переход фокуса).
            let weak = host.downgrade();
            cx.intercept_keystrokes(move |ev, window, cx| {
                let consumed = weak
                    .update(cx, |h, cx| h.key_down(&ev.keystroke, window, cx))
                    .unwrap_or(false);
                if consumed {
                    cx.stop_propagation();
                }
            })
            .detach();
            host
        });
        if let Err(e) = opened {
            tracing::error!(error = %e, "gpui window");
            cx.quit();
        }
        cx.activate(true);
    });
}
