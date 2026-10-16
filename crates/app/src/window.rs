//! Окно eframe и цикл кадра (S1 §1, S1-13).
//!
//! Кадр: `logic` — pump Core (≤ 4 мс), закрытие окна через `app.quit`, геометрия
//! для сессии; `ui` — фронтенд читает `&AppState`, его команды выполняются после
//! кадра. Перерисовка — только по событию: в простое 0% CPU.

use std::cell::RefCell;
use std::rc::Rc;
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
/// Окно стартует скрытым (DWM cloak) и показывается первым кадром с загруженными
/// табами — без мигания пустыми панелями; но не позже этого срока.
const SHOW_AT_LATEST: Duration = Duration::from_millis(300);

/// Клавиши до keymap S3 (S0 v3 №2: весь ввод — через `raw_input_hook`).
/// Поглощённое событие удаляется из ввода egui: Tab не двигает фокус egui.
fn hard_key(key: egui::Key, m: egui::Modifiers) -> Option<Command> {
    use egui::Key as K;
    let motion = |k| match k {
        K::ArrowUp => Some("up"),
        K::ArrowDown => Some("down"),
        K::PageUp => Some("page_up"),
        K::PageDown => Some("page_down"),
        K::Home => Some("home"),
        K::End => Some("end"),
        _ => None,
    };
    if m.shift && !m.ctrl && !m.alt {
        // Shift+стрелки: пометка с движением (Far).
        return motion(key).map(|mo| Command::new("sel.move").arg("motion", mo));
    }
    if m.any() {
        return None;
    }
    if let Some(mo) = motion(key) {
        return Some(match mo {
            "up" => Command::new("panel.cursor.up"),
            "down" => Command::new("panel.cursor.down"),
            "page_up" => Command::new("panel.cursor.page_up"),
            "page_down" => Command::new("panel.cursor.page_down"),
            "home" => Command::new("panel.cursor.home"),
            _ => Command::new("panel.cursor.end"),
        });
    }
    Some(match key {
        K::Enter => Command::new("panel.enter"),
        K::Backspace => Command::new("panel.up"),
        K::Tab => Command::new("panel.switch"),
        // Insert — как в Far; Space — для клавиатур без Insert.
        K::Insert | K::Space => Command::new("sel.toggle").arg("advance", true),
        _ => return None,
    })
}

/// Забирает из ввода egui клавиши панели и превращает их в команды. Отпускание
/// поглощённой клавиши тоже удаляется; пробел не уходит в egui текстом.
fn take_keys(events: &mut Vec<egui::Event>, out: &mut Vec<Command>) {
    events.retain(|e| match e {
        egui::Event::Key {
            key,
            pressed,
            modifiers,
            ..
        } => match hard_key(*key, *modifiers) {
            Some(c) => {
                if *pressed {
                    out.push(c);
                }
                false
            }
            None => true,
        },
        egui::Event::Text(t) => t != " ",
        _ => true,
    });
}

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

/// Бэкенды wgpu по очереди (ADR-0012): следующий пробуется, только если
/// предыдущий не дал адаптера. Каждый поднятый бэкенд стоит старту десятки мс.
/// Windows — DX12: Vulkan при развороте окна 30–85 мс показывает чёрный кадр
/// (пересоздание swapchain).
#[cfg(windows)]
const BACKENDS: &[wgpu::Backends] = &[wgpu::Backends::DX12, wgpu::Backends::VULKAN];
#[cfg(target_os = "macos")]
const BACKENDS: &[wgpu::Backends] = &[wgpu::Backends::METAL];
#[cfg(all(unix, not(target_os = "macos")))]
const BACKENDS: &[wgpu::Backends] = &[wgpu::Backends::VULKAN, wgpu::Backends::GL];

/// wgpu по ADR-0012: экономный по памяти режим, адаптер — [`adapter_rank`].
/// Present — `AutoVsync` (по умолчанию egui-wgpu).
fn wgpu_options(backends: wgpu::Backends) -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = backends;
    setup.native_adapter_selector = Some(Arc::new(pick_adapter));
    setup.device_descriptor = Arc::new(|_| wgpu::DeviceDescriptor {
        label: Some("filenav"),
        required_limits: wgpu::Limits {
            // Текстура на весь экран — до 8K.
            max_texture_dimension_2d: 8192,
            ..wgpu::Limits::downlevel_defaults()
        },
        // Блоки памяти по умолчанию (Performance) — +200 МБ RAM на встроенной
        // графике, где видеопамять — это RAM процесса.
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    });
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}

/// Порядок адаптеров одного бэкенда (меньше — лучше): встроенная графика —
/// дискретная файловому менеджеру не нужна; программный рендер — последним.
fn adapter_rank(device: wgpu::DeviceType) -> u8 {
    use wgpu::DeviceType as D;
    match device {
        D::IntegratedGpu => 0,
        D::DiscreteGpu => 1,
        D::VirtualGpu => 2,
        D::Other => 3,
        D::Cpu => 4,
    }
}

fn pick_adapter(
    adapters: &[wgpu::Adapter],
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    let best = adapters
        .iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| {
            let i = a.get_info();
            adapter_rank(i.device_type)
        })
        .cloned()
        .ok_or_else(|| "no graphics adapter can draw to the window".to_owned())?;
    let i = best.get_info();
    tracing::info!(backend = ?i.backend, device = %i.name, kind = ?i.device_type, driver = %i.driver_info, "graphics adapter");
    Ok(best)
}

/// Окно до выхода. `boot.session` — уже загруженная сессия (геометрия из неё).
pub fn run(boot: Boot, startup: Startup) -> anyhow::Result<()> {
    // `WGPU_BACKEND` (переменная wgpu) заменяет очередь — для диагностики.
    let order = wgpu::Backends::from_env().map_or_else(|| BACKENDS.to_vec(), |b| vec![b]);
    let pending = Rc::new(RefCell::new(Some((boot, startup))));
    let mut last = None;
    for backends in order {
        match run_with(backends, &pending) {
            Ok(()) => return Ok(()),
            // App не создан — значит, не поднялся рендер: пробуем следующий бэкенд.
            Err(e) if pending.borrow().is_some() => {
                tracing::warn!(?backends, error = %e, "graphics backend unavailable");
                last = Some(e);
            }
            Err(e) => return Err(anyhow::anyhow!("window: {e}")),
        }
    }
    Err(anyhow::anyhow!(
        "window: no graphics backend: {}",
        last.map_or_else(String::new, |e| e.to_string())
    ))
}

type Pending = Rc<RefCell<Option<(Boot, Startup)>>>;

fn run_with(backends: wgpu::Backends, pending: &Pending) -> eframe::Result {
    let geom = pending
        .borrow()
        .as_ref()
        .and_then(|(b, _)| b.session.as_ref().and_then(|s| s.window));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("FileNav")
        .with_visible(false)
        .with_min_inner_size([480.0, 320.0])
        .with_inner_size(geom.map_or([1200.0, 760.0], |g| [g.w, g.h]));
    // «Развёрнуто» — не здесь: winit показал бы окно при создании, до первого
    // кадра. Разворачиваем уже скрытое окно (см. создание `App`).
    let maximized = geom.is_some_and(|g| g.maximized);
    if let Some(g) = geom {
        viewport = viewport.with_position([g.x, g.y]);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_options(backends),
        // Хук сообщений Windows (KeyNotes, numpad, Ctrl+Ins/Shift+Del) поставит
        // keymap S3 (S0 v3 №10); winit — ровно версия eframe.
        event_loop_builder: Some(Box::new(|_builder| {})),
        ..Default::default()
    };
    let window_started = Instant::now();
    let pending = Rc::clone(pending);
    eframe::run_native(
        "FileNav",
        options,
        Box::new(move |cc| {
            let (boot, mut startup) = pending.borrow_mut().take().expect("boot is taken once");
            let services = crate::demo_services();
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
            // Окно показывается скрытым от экрана: первые кадры рисуются уже в
            // итоговом размере, на экран выходит только готовый (S1-13).
            crate::sys::cloak(cc, true);
            let bg = cc.egui_ctx.style_of(cc.egui_ctx.theme()).visuals.panel_fill;
            crate::sys::set_background(cc, [bg.r(), bg.g(), bg.b()]);
            crate::sys::no_transitions(cc);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Maximized(maximized));
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            cc.egui_ctx.request_repaint_after(SHOW_AT_LATEST);
            Ok(Box::new(App {
                core: Some(core),
                ui: DebugUi::new(),
                out: Vec::new(),
                startup,
                first_frame_done: false,
                created: Instant::now(),
                reveal: false,
                shown: false,
            }))
        }),
    )
}

struct App {
    core: Option<Core>,
    ui: DebugUi,
    out: Vec<Command>,
    startup: Startup,
    first_frame_done: bool,
    created: Instant,
    /// Кадр для показа нарисован: следующий `logic` снимает cloak.
    reveal: bool,
    shown: bool,
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
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        // Команды клавиатуры из raw_input_hook — до pump, видны в этом же кадре.
        for c in self.out.drain(..) {
            core.dispatch(c);
        }
        if self.reveal && !self.shown {
            self.shown = true;
            crate::sys::cloak(frame, false);
        }
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
        if tracing::enabled!(target: "frame", tracing::Level::TRACE) {
            let s = core.state();
            // Что видно в кадре: состояние и число строк активного таба каждой панели.
            let tabs: Vec<_> = s
                .panels
                .iter()
                .map(|p| {
                    let t = p.tab();
                    let st = match t.listing.status {
                        filenav_core::ListingStatus::Loading { .. } => "loading",
                        filenav_core::ListingStatus::Complete => "complete",
                        _ => "other",
                    };
                    format!("{st}:{}:{}", t.rows(), t.listing.order.len())
                })
                .collect();
            tracing::trace!(target: "frame", rev = s.rev, ?tabs, causes = ?ui.ctx().repaint_causes(), "frame");
        }
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
        let loading = core.state().panels.iter().any(|p| {
            matches!(
                p.tab().listing.status,
                filenav_core::ListingStatus::Loading { .. }
            )
        });
        // Первый кадр — тот, что увидит пользователь: с загруженными табами.
        if !self.first_frame_done && (!loading || self.created.elapsed() >= SHOW_AT_LATEST) {
            self.first_frame_done = true;
            self.reveal = true;
            ctx.request_repaint();
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

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Some(core) = self.core.as_ref() else {
            return;
        };
        let s = core.state();
        if s.focus != Focus::Panel || s.prompts.active().is_some() {
            return;
        }
        take_keys(&mut raw.events, &mut self.out);
    }

    /// Фон окна до первой отрисовки — как у панели: без белой вспышки.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self) {
        if let Some(core) = self.core.take() {
            let session: Session = core.shutdown(Duration::from_millis(200));
            tracing::info!(window = ?session.window, "session saved on exit");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: egui::Key, modifiers: egui::Modifiers, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        }
    }

    fn run(events: Vec<egui::Event>) -> (Vec<egui::Event>, Vec<String>) {
        let mut events = events;
        let mut out = Vec::new();
        take_keys(&mut events, &mut out);
        (events, out.iter().map(ToString::to_string).collect())
    }

    #[test]
    fn adapter_order() {
        use wgpu::DeviceType as D;
        let mut v = [
            D::Cpu,
            D::DiscreteGpu,
            D::Other,
            D::IntegratedGpu,
            D::VirtualGpu,
        ];
        v.sort_by_key(|&d| adapter_rank(d));
        assert_eq!(
            v,
            [
                D::IntegratedGpu,
                D::DiscreteGpu,
                D::VirtualGpu,
                D::Other,
                D::Cpu
            ]
        );
    }

    #[test]
    fn tab_switches_panel_and_never_reaches_egui() {
        let none = egui::Modifiers::NONE;
        let (left, cmds) = run(vec![
            key(egui::Key::Tab, none, true),
            key(egui::Key::Tab, none, false),
        ]);
        assert!(left.is_empty(), "egui не должен видеть Tab: {left:?}");
        assert_eq!(cmds, ["panel.switch"]);
    }

    #[test]
    fn selection_keys() {
        let none = egui::Modifiers::NONE;
        let shift = egui::Modifiers::SHIFT;
        let (left, cmds) = run(vec![
            key(egui::Key::ArrowDown, shift, true),
            key(egui::Key::End, shift, true),
            key(egui::Key::Insert, none, true),
            key(egui::Key::Space, none, true),
            egui::Event::Text(" ".into()),
        ]);
        assert!(left.is_empty(), "{left:?}");
        assert_eq!(
            cmds,
            [
                "sel.move motion=down",
                "sel.move motion=end",
                "sel.toggle advance=true",
                "sel.toggle advance=true",
            ]
        );
    }

    #[test]
    fn foreign_keys_and_text_pass_through() {
        let ctrl = egui::Modifiers::CTRL;
        let events = vec![
            key(egui::Key::Q, ctrl, true),
            key(egui::Key::ArrowDown, ctrl, true),
            key(egui::Key::F5, egui::Modifiers::NONE, true),
            egui::Event::Text("a".into()),
        ];
        let (left, cmds) = run(events.clone());
        assert_eq!(left, events);
        assert!(cmds.is_empty(), "{cmds:?}");
    }
}
