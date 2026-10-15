//! Окно eframe и цикл кадра (S1 §1, S1-13).
//!
//! Кадр: `logic` — pump Core (≤ 4 мс), закрытие окна через `app.quit`, геометрия
//! для сессии; `ui` — фронтенд читает `&AppState`, его команды выполняются после
//! кадра. Перерисовка — только по событию: в простое 0% CPU.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use filenav_core::{Boot, Command, Core, Focus, OverlayKind, Session, WindowGeom};
use filenav_ui::DebugUi;

use crate::Startup;

/// Будит UI из других потоков (S1-13).
struct EguiWaker(egui::Context);

impl filenav_core::Waker for EguiWaker {
    fn wake(&self) {
        self.0.request_repaint();
    }

    fn wake_after(&self, d: Duration) {
        self.0.request_repaint_after(d);
    }
}

/// Бюджет pump на кадр (S1 §1).
const PUMP_BUDGET: Duration = Duration::from_millis(4);
/// Пока идут задачи — перерисовка не чаще 30 раз/с (S1 §0).
const JOBS_REPAINT: Duration = Duration::from_millis(33);

/// IME: в полях ввода (cmdline, терминал, оверлеи с вводом, редактор) включён,
/// в панелях, вьювере и запросах — выключен (S0 v3 №13, v4 №2, v5 №2).
fn ime_allowed(f: Focus) -> bool {
    match f {
        Focus::Cmdline | Focus::Terminal | Focus::Editor => true,
        // Все оверлеи с вводом текста; меню — без ввода.
        Focus::Overlay(k) => k != OverlayKind::Menu,
        Focus::Panel | Focus::Viewer | Focus::Prompt(_) => false,
    }
}

/// Окно до выхода. `boot.session` — уже загруженная сессия (геометрия из неё).
pub fn run(boot: Boot, startup: Startup) -> anyhow::Result<()> {
    let geom = boot.session.as_ref().and_then(|s| s.window);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("FileNav")
        .with_min_inner_size([480.0, 320.0])
        .with_inner_size(geom.map_or([1200.0, 760.0], |g| [g.w, g.h]));
    if let Some(g) = geom {
        viewport = viewport
            .with_position([g.x, g.y])
            .with_maximized(g.maximized);
    }
    let mut options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        // Хук сообщений Windows (KeyNotes, numpad, Ctrl+Ins/Shift+Del) поставит
        // keymap S3 (S0 v3 №10); winit — ровно версия eframe.
        event_loop_builder: Some(Box::new(|_builder| {})),
        ..Default::default()
    };
    options.glow_options.vsync = true;
    let services = crate::demo_services();
    let window_started = Instant::now();
    eframe::run_native(
        "FileNav",
        options,
        Box::new(move |cc| {
            let mut startup = startup;
            startup.window = window_started.elapsed();
            let ctx = cc.egui_ctx.clone();
            {
                let _s = tracing::info_span!("startup.fonts").entered();
                // Шрифты Segoe / JetBrains Mono — S2; пока шрифты egui по умолчанию.
                ctx.options_mut(|o| o.zoom_with_keyboard = false);
            }
            let core = {
                let _s = tracing::info_span!("startup.core").entered();
                let t = Instant::now();
                let core = Core::new(boot, services, Arc::new(EguiWaker(ctx)));
                startup.core = t.elapsed();
                core
            };
            Ok(Box::new(App {
                core: Some(core),
                ui: DebugUi::new(),
                out: Vec::new(),
                startup,
                first_frame_done: false,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("window: {e}"))
}

struct App {
    core: Option<Core>,
    ui: DebugUi,
    out: Vec<Command>,
    startup: Startup,
    first_frame_done: bool,
}

impl App {
    fn geometry(ctx: &egui::Context, last: Option<WindowGeom>) -> Option<WindowGeom> {
        ctx.input(|i| {
            let v = i.viewport();
            let maximized = v.maximized.unwrap_or(false);
            if maximized {
                // Размер развёрнутого окна не сохраняем: после «восстановить» окно
                // вернётся к прежнему.
                return last.map(|g| WindowGeom { maximized, ..g });
            }
            let outer = v.outer_rect?;
            let inner = v.inner_rect?;
            Some(WindowGeom {
                x: outer.min.x,
                y: outer.min.y,
                w: inner.width(),
                h: inner.height(),
                maximized,
                split: last.map_or(0.5, |g| g.split),
            })
        })
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        // Крестик окна — через app.quit: handlers могут наложить вето (S0 v2 №13).
        if ctx.input(|i| i.viewport().close_requested()) && !core.state().quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            core.dispatch(Command::new("app.quit"));
        }
        let p = core.pump(PUMP_BUDGET);
        if p.more {
            ctx.request_repaint();
        }
        let last = core.window();
        if let Some(g) = Self::geometry(ctx, last) {
            core.set_window(g);
        }
        if core.state().quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let _frame_span = tracing::trace_span!("frame").entered();
        tracing::trace!(target: "frame", rev = core.state().rev, causes = ?ui.ctx().repaint_causes(), "frame");
        let ev = core.take_ui_events();
        self.ui.show(ui, core.state(), &ev, &mut self.out);
        let rev = core.state().rev;
        for c in self.out.drain(..) {
            core.dispatch(c);
        }
        let ctx = ui.ctx();
        if core.state().rev != rev {
            ctx.request_repaint();
        }
        if core.state().jobs.any_active() {
            ctx.request_repaint_after(JOBS_REPAINT);
        }
        if !ime_allowed(core.state().focus) {
            ctx.output_mut(|o| o.ime = None);
        }
        if !self.first_frame_done {
            self.first_frame_done = true;
            self.startup.first_frame = self.startup.process.elapsed();
            tracing::info!(
                target: "startup.first_frame",
                elapsed = ?self.startup.first_frame,
                "first frame"
            );
            if self.startup.bench {
                println!("{}", self.startup.json());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(core) = self.core.take() {
            let session: Session = core.shutdown(Duration::from_millis(200));
            tracing::info!(window = ?session.window, "session saved on exit");
        }
    }
}
