//! Интерфейс на GPUI (ADR-0018). G-02 — каркас: [`CoreHost`] владеет `Core` и качает
//! его `pump` только по сигналу [`Signal`] (воркеры Core, вторые запуски) — без таймеров
//! опроса; в простое кадров нет. Клавиатура — конвейер keymap S3 ([`input`]).

mod input;
mod panel;
mod theme;

pub use theme::Theme;

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, AsyncApp, Bounds, Context, FocusHandle, IntoElement, KeyUpEvent, ModifiersChangedEvent,
    Render, SharedString, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions, div,
    point, px, size,
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
        } = launch;
        listen(wake, cx);
        CoreHost {
            core: Some(core),
            signal,
            poll,
            rev: u64::MAX,
            grid: [(0, 0); 2],
            theme,
            focus: cx.focus_handle(),
            input: input::Input::new(keys),
            out: Vec::new(),
        }
    }

    /// Сетка каждой панели — в Core (`panel.viewport`): по ней он держит курсор в
    /// видимой странице и считает PgUp/PgDn и ←/→ в колонках.
    fn sync_viewport(&mut self, window: &Window) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let size = window.viewport_size();
        let gap = self.theme.m(Metric::GapPanels);
        let w = (f32::from(size.width) - 3.0 * gap) / 2.0;
        let h = f32::from(size.height) - 2.0 * gap;
        for (i, side) in [Side::Left, Side::Right].into_iter().enumerate() {
            let mode = core.state().panel(side).tab().view_mode;
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
        // Видимый вопрос разбирает клавиши сам (S2-15).
        if crate::prompt_takes_keys(core.state()) {
            return false;
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let gap = px(self.theme.m(Metric::GapPanels));
        let mut root = div()
            .track_focus(&self.focus)
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers))
            .size_full()
            .flex()
            .flex_row()
            .gap(gap)
            .p(gap)
            .bg(self.theme.c(Role::BgBase))
            .text_size(px(self.theme.m(Metric::TypeBody)));
        if let Some(core) = &self.core {
            let s = core.state();
            for (i, side) in [Side::Left, Side::Right].into_iter().enumerate() {
                let (rows, cols) = self.grid[i];
                root = root.child(panel::panel(s, side, &self.theme, rows, cols));
            }
        }
        root
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
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::new_static("Urfin")),
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
