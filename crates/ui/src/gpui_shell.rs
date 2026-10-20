//! Интерфейс на GPUI (ADR-0018). G-02 — каркас: [`CoreHost`] владеет `Core` и качает
//! его `pump` только по сигналу [`Signal`] (воркеры Core, вторые запуски) — без таймеров
//! опроса; в простое кадров нет. Вид пока отладочный: две панели текстом (как `DebugUi`
//! S1-13), клавиши навигации захардкожены до G-03 (keymap).

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, AsyncApp, Bounds, Context, FocusHandle, IntoElement, KeyDownEvent, Render, SharedString,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions, div, point, px, rgb, size,
};
use urfin_core::{Command, Core, Row, Side, WindowGeom};

// Tab: `gpui_kit::open_window` оборачивает вид в свой `Root`, а тот вешает на Tab
// переход фокуса (контекст `Root`). Свой контекст глубже — его привязка главнее.
gpui_kit::actions!(urfin, [SwitchPanel]);
const KEY_CONTEXT: &str = "Urfin";

/// Бюджет `pump` за один проход (как у egui-окна, S1 §1).
const PUMP_BUDGET: Duration = Duration::from_millis(4);
/// Сколько строк таба выводит отладочный вид.
const DEBUG_ROWS: u32 = 40;

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
    focus: FocusHandle,
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
        } = launch;
        listen(wake, cx);
        CoreHost {
            core: Some(core),
            signal,
            poll,
            rev: u64::MAX,
            focus: cx.focus_handle(),
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

    /// Навигация до keymap (G-03): стрелки, страницы, Enter, Backspace, Insert; Tab —
    /// действием [`SwitchPanel`].
    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let cmd = match ev.keystroke.key.as_str() {
            "up" => "panel.cursor.up",
            "down" => "panel.cursor.down",
            "pageup" => "panel.cursor.page_up",
            "pagedown" => "panel.cursor.page_down",
            "home" => "panel.cursor.home",
            "end" => "panel.cursor.end",
            "enter" => "panel.enter",
            "backspace" => "panel.up",
            "insert" => "sel.toggle",
            _ => return,
        };
        cx.stop_propagation();
        self.dispatch(cmd, window, cx);
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
        let mut root = div()
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|h, _: &SwitchPanel, window, cx| {
                h.dispatch("panel.switch", window, cx);
            }))
            .size_full()
            .flex()
            .flex_row()
            .gap_2()
            .p_2()
            .bg(rgb(0x00f3_f3f3))
            .text_color(rgb(0x001b_1b1b))
            .text_sm();
        if let Some(core) = &self.core {
            let s = core.state();
            for side in [Side::Left, Side::Right] {
                root = root.child(panel_debug(s, side));
            }
        }
        root
    }
}

/// Отладочный вывод таба: путь, состояние, строки с курсором и выделением.
fn panel_debug(s: &urfin_core::AppState, side: Side) -> impl IntoElement {
    let active = s.active == side;
    let t = s.panel(side).tab();
    let head: SharedString = format!("{}  [{:?}]", t.loc.path.display(), t.listing.status).into();
    let mut col = div()
        .flex_1()
        .flex()
        .flex_col()
        .overflow_hidden()
        .p_2()
        .bg(rgb(0x00ff_ffff))
        .border_1()
        .border_color(rgb(if active { 0x0000_78d4 } else { 0x00e0_e0e0 }))
        .child(div().pb_1().child(head));
    let top = t.cursor.top;
    for r in top..(top + DEBUG_ROWS).min(t.rows()) {
        let (name, marked) = match t.row(r) {
            Some(Row::Parent) => ("..".to_owned(), false),
            Some(Row::Entry { idx, entry }) => (entry.name.to_string(), t.selection.contains(idx)),
            None => (String::new(), false),
        };
        let mut line = div().px_1().child(SharedString::from(name));
        if marked {
            line = line.text_color(rgb(0x0000_78d4));
        }
        if r == t.cursor.row {
            line = line.bg(rgb(if active { 0x00cc_e4f7 } else { 0x00e8_e8e8 }));
        }
        col = col.child(line);
    }
    col
}

/// Окно GPUI с Core; возвращается после выхода из цикла (сессия уже сохранена).
pub fn run(launch: Launch, geom: Option<WindowGeom>) {
    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "tab",
            SwitchPanel,
            Some(KEY_CONTEXT),
        )]);
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
                cx.observe_window_bounds(window, |h, window, _| h.track_bounds(window))
                    .detach();
                h.pump(window, cx);
            });
            host
        });
        if let Err(e) = opened {
            tracing::error!(error = %e, "gpui window");
            cx.quit();
        }
        cx.activate(true);
    });
}
