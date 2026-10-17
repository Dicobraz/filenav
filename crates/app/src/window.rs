//! Окно eframe и цикл кадра (S1 §1, S1-13).
//!
//! Кадр: `logic` — pump Core (≤ 4 мс), закрытие окна через `app.quit`, геометрия
//! для сессии; `ui` — фронтенд читает `&AppState`, его команды выполняются после
//! кадра. Перерисовка — только по событию: в простое 0% CPU.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use eframe::egui;
use urfin_core::{Boot, Command, Core, Focus, OverlayKind, Session, WindowGeom};
use urfin_ui::UrfinUi;

use crate::Startup;
use crate::gpu::{self, Gpu};

/// Будит UI из других потоков (S1-13). Контекст меняется, когда окно создаётся
/// заново после потери устройства (Core и его потоки — прежние).
struct EguiWaker(Mutex<egui::Context>);

impl EguiWaker {
    fn new(ctx: egui::Context) -> Self {
        EguiWaker(Mutex::new(ctx))
    }

    fn ctx(&self) -> egui::Context {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, ctx: egui::Context) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = ctx;
    }
}

impl urfin_core::Waker for EguiWaker {
    fn wake(&self) {
        self.ctx().request_repaint();
    }

    fn wake_after(&self, d: Duration) {
        self.ctx().request_repaint_after(d);
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
    // Ctrl+O — панели за терминалом (S0-decisions №27; привязка S3 §3).
    if m.ctrl && !m.shift && !m.alt && key == K::O {
        return Some(Command::new("ui.panels.toggle_all"));
    }
    // Ctrl+1..4 — вид панели, как в Far (S3 §3).
    if m.ctrl && !m.shift && !m.alt {
        let mode = match key {
            K::Num1 => "brief",
            K::Num2 => "medium",
            K::Num3 => "full",
            K::Num4 => "wide",
            _ => return None,
        };
        return Some(Command::new("panel.view_mode").arg("mode", mode));
    }
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
        // F-клавиши пресета Far, у которых уже есть действие (S3 §3); F9 — с меню S2-14.
        K::F9 => Command::new("ui.menu"),
        K::F10 => Command::new("app.quit"),
        K::Enter => Command::new("panel.enter"),
        K::Backspace => Command::new("panel.up"),
        K::Tab => Command::new("panel.switch"),
        // Колонка влево/вправо в Brief и Medium (S3 §3).
        K::ArrowLeft => Command::new("panel.cursor.left"),
        K::ArrowRight => Command::new("panel.cursor.right"),
        // Insert — как в Far; Space — для клавиатур без Insert.
        K::Insert | K::Space => Command::new("sel.toggle").arg("advance", true),
        _ => return None,
    })
}

/// Забирает из ввода egui клавиши панели и превращает их в команды. Клавиша — по
/// позиции (`physical_key`): буквенные сочетания работают на любой раскладке (S0 v4 №5).
/// Отпускание
/// поглощённой клавиши тоже удаляется; пробел не уходит в egui текстом.
fn take_keys(events: &mut Vec<egui::Event>, out: &mut Vec<Command>) {
    events.retain(|e| match e {
        egui::Event::Key {
            key,
            physical_key,
            pressed,
            modifiers,
            ..
        } => match hard_key(physical_key.unwrap_or(*key), *modifiers) {
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

/// Ввод в открытом оверлее: Esc → `ui.cancel`.
fn take_overlay_keys(events: &mut Vec<egui::Event>, out: &mut Vec<Command>) {
    events.retain(|e| match e {
        egui::Event::Key {
            key: egui::Key::Escape,
            pressed,
            modifiers,
            ..
        } if !modifiers.any() => {
            if *pressed {
                out.push(Command::new("ui.cancel"));
            }
            false
        }
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

/// Окно до выхода. `boot.session` — уже загруженная сессия (геометрия из неё).
/// Всё, что окну нужно от `main`.
pub struct Launch {
    pub boot: Boot,
    pub startup: Startup,
    /// Запросы вторых запусков (S1-15); `None` — отдельный экземпляр.
    pub ipc: Option<crate::ipc::Inbox>,
    /// Пути из своей командной строки: новые табы после восстановления сессии.
    pub open: Vec<Command>,
    /// `--selftest <dir>`: сценарий самопроверки и отчёт в `dir`.
    pub selftest: Option<std::path::PathBuf>,
}

pub fn run(launch: Launch) -> anyhow::Result<()> {
    let (ui_cfg, ui_cfg_warning) = launch.boot.config.section::<urfin_ui::config::UiCfg>("ui");
    if let Some(w) = ui_cfg_warning {
        tracing::warn!(key = %w.key, msg = %w.msg, "[ui] section ignored");
    }
    // `WGPU_BACKEND` (переменная wgpu) заменяет очередь — для диагностики.
    let (plan, plan_warning) = match wgpu::Backends::from_env() {
        Some(b) => (vec![Gpu::Any(b)], None),
        None => gpu::plan(&ui_cfg.renderer),
    };
    if let Some(msg) = &plan_warning {
        tracing::warn!(key = "ui.renderer", %msg, "config value ignored");
    }
    // Mica (Win11, прозрачность включена) решается до окна: от неё зависит swapchain.
    let mica = urfin_ui::sys::mica_wanted(&ui_cfg.backdrop);
    let config_dir = launch.boot.paths.config_dir.clone();
    let pending: Pending = Rc::new(RefCell::new(Some(Start {
        from: Origin::Launch(Box::new(launch)),
        probe: spawn_probe(&ui_cfg, &config_dir)?,
        ui_cfg: ui_cfg.clone(),
        warnings: plan_warning.into_iter().collect(),
    })));
    let restart: Restart = Rc::default();
    let mut queue: VecDeque<Gpu> = plan.iter().copied().collect();
    let mut last = None;
    while let Some(gpu) = queue.pop_front() {
        let attempt = Attempt {
            gpu,
            mica: mica && gpu.mica_capable(),
            // Не то, что выбрано настройкой, — откат: тост (S0-decisions №50).
            fallback: gpu != plan[0],
        };
        match run_with(attempt, &pending, &restart) {
            Ok(()) => {
                let Some(resume) = restart.borrow_mut().take() else {
                    return Ok(());
                };
                // Устройство потеряно: окно заново на следующей попытке, Core тот же.
                queue = gpu::after(&plan, gpu).into();
                if queue.is_empty() {
                    let session = resume.core.shutdown(Duration::from_millis(200));
                    tracing::info!(window = ?session.window, "session saved on exit");
                    return Err(anyhow::anyhow!("window: graphics device lost on {gpu:?}"));
                }
                tracing::warn!(?gpu, next = ?queue[0], "restarting the window after device loss");
                *pending.borrow_mut() = Some(Start {
                    from: Origin::Resume(Box::new(resume)),
                    probe: spawn_probe(&ui_cfg, &config_dir)?,
                    ui_cfg: ui_cfg.clone(),
                    warnings: Vec::new(),
                });
            }
            // App не создан — значит, не поднялся рендер: следующая попытка.
            Err(e) if pending.borrow().is_some() => {
                tracing::warn!(?gpu, error = %e, "graphics backend unavailable");
                last = Some(e);
            }
            Err(e) => return Err(anyhow::anyhow!("window: {e}")),
        }
    }
    // Core после потери устройства ждал окна, а рендера больше нет: сессию — на диск.
    if let Some(Start {
        from: Origin::Resume(r),
        ..
    }) = pending.borrow_mut().take()
    {
        r.core.shutdown(Duration::from_millis(200));
    }
    Err(anyhow::anyhow!(
        "window: no graphics backend: {}",
        last.map_or_else(String::new, |e| e.to_string())
    ))
}

/// Probe ОС (тема, акцент, файлы шрифтов), чтение шрифтов и `menu.toml` каталога
/// конфига — параллельно созданию окна и `Core::new`.
fn spawn_probe(
    cfg: &urfin_ui::config::UiCfg,
    config_dir: &std::path::Path,
) -> std::io::Result<Probe> {
    let cfg = cfg.clone();
    let menu = config_dir.join("menu.toml");
    std::thread::Builder::new()
        .name("font-probe".into())
        .spawn(move || {
            let mut probe = urfin_ui::sys::probe_system();
            let (fonts, warnings) = urfin_ui::fonts::load(&probe.fonts, &cfg);
            probe.warnings.extend(warnings);
            // Своё меню F9 (S2-14): нет файла — вшитое.
            let menu = std::fs::read_to_string(&menu).ok();
            (probe, fonts, menu)
        })
}

type Probe = std::thread::JoinHandle<(
    urfin_ui::sys::SysProbe,
    urfin_ui::fonts::LoadedFonts,
    Option<String>,
)>;
type Pending = Rc<RefCell<Option<Start>>>;
/// Сюда `App` кладёт своё состояние при потере устройства (см. [`run`]).
type Restart = Rc<RefCell<Option<Resume>>>;

/// Что нужно окну для запуска.
struct Start {
    from: Origin,
    probe: Probe,
    ui_cfg: urfin_ui::config::UiCfg,
    /// Предупреждения настроек окна — тостами в первом кадре.
    warnings: Vec<String>,
}

enum Origin {
    /// Первый запуск окна.
    Launch(Box<Launch>),
    /// Окно заново после потери устройства: Core и вторые запуски — прежние.
    Resume(Box<Resume>),
}

struct Resume {
    core: Core,
    waker: Arc<EguiWaker>,
    startup: Startup,
    ipc: Option<crate::ipc::Inbox>,
    selftest: Option<crate::selftest::SelfTest>,
}

#[derive(Clone, Copy)]
struct Attempt {
    gpu: Gpu,
    mica: bool,
    fallback: bool,
}

fn run_with(attempt: Attempt, pending: &Pending, restart: &Restart) -> eframe::Result {
    let Attempt { gpu, mica, .. } = attempt;
    let geom = pending.borrow().as_ref().and_then(|s| match &s.from {
        Origin::Launch(l) => l.boot.session.as_ref().and_then(|s| s.window),
        Origin::Resume(r) => r.core.window(),
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Urfin")
        .with_visible(false)
        .with_min_inner_size(urfin_ui::layout::MIN_WINDOW)
        .with_transparent(mica)
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
        wgpu_options: gpu::wgpu_options(gpu, mica),
        // Хук сообщений Windows (KeyNotes, numpad, Ctrl+Ins/Shift+Del) поставит
        // keymap S3 (S0 v3 №10); winit — ровно версия eframe.
        event_loop_builder: Some(Box::new(|_builder| {})),
        // Запасные шрифты — свои (`ui::fonts`); провайдер eframe без фичи
        // `system_fonts` только пишет в лог предупреждение.
        system_font_fallback: false,
        ..Default::default()
    };
    let window_started = Instant::now();
    let pending = Rc::clone(pending);
    let restart = Rc::clone(restart);
    eframe::run_native(
        "Urfin",
        options,
        Box::new(move |cc| {
            let start = pending.borrow_mut().take().expect("start is taken once");
            Ok(Box::new(create_app(
                cc,
                start,
                attempt,
                window_started,
                maximized,
                restart,
            )))
        }),
    )
}

/// `App` для созданного окна: Core (новый или прежний), шрифты, рамка, показ.
#[allow(clippy::too_many_lines, reason = "шаги создания окна по порядку")]
fn create_app(
    cc: &eframe::CreationContext<'_>,
    start: Start,
    attempt: Attempt,
    window_started: Instant,
    maximized: bool,
    restart: Restart,
) -> App {
    let Start {
        from,
        probe,
        ui_cfg,
        warnings,
    } = start;
    let Attempt {
        gpu,
        mica,
        fallback,
    } = attempt;
    let ctx = cc.egui_ctx.clone();
    // Масштаб — только командой `ui.zoom` (S0 v3 №13).
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    let resumed = matches!(from, Origin::Resume(_));
    let (core, waker, startup, ipc, selftest) = match from {
        Origin::Launch(launch) => {
            let Launch {
                boot,
                mut startup,
                ipc,
                open,
                selftest,
            } = *launch;
            let services = crate::vfs_services(&boot.config);
            startup.window = window_started.elapsed();
            let waker = Arc::new(EguiWaker::new(ctx.clone()));
            let mut core = {
                let _s = tracing::info_span!("startup.core").entered();
                let t = Instant::now();
                let mut core = Core::new(boot, services, waker.clone());
                urfin_ui::register(&mut core);
                startup.core = t.elapsed();
                core
            };
            for c in open {
                core.dispatch(c);
            }
            let selftest = selftest.map(crate::selftest::SelfTest::new);
            (core, waker, startup, ipc, selftest)
        }
        Origin::Resume(r) => {
            r.waker.set(ctx.clone());
            (r.core, r.waker, r.startup, r.ipc, r.selftest)
        }
    };
    if let Some(inbox) = &ipc {
        let ctx = cc.egui_ctx.clone();
        inbox.set_waker(move || ctx.request_repaint());
    }
    let lost = Arc::new(AtomicBool::new(false));
    if let Some(rs) = cc.wgpu_render_state.as_ref() {
        let i = rs.adapter.get_info();
        tracing::info!(
            target: "startup.renderer",
            ?gpu,
            backend = ?i.backend,
            device = %i.name,
            kind = ?i.device_type,
            fallback,
            resumed,
            "renderer"
        );
        gpu::watch_device(&rs.device, &cc.egui_ctx, Arc::clone(&lost));
    }
    let (probe, loaded, menu) = probe.join().unwrap_or_else(|_| {
        tracing::error!("system probe panicked; using defaults");
        Default::default()
    });
    // Шрифты — до первого кадра: он рисуется уже ими.
    let fonts = urfin_ui::fonts::install(&cc.egui_ctx, &loaded);
    let theme = probe.look.theme.unwrap_or_else(|| cc.egui_ctx.theme());
    let mut ui = UrfinUi::new(core.registry(), &probe, &ui_cfg, fonts);
    if let Some(text) = menu {
        ui.set_menu(&text);
    }
    for w in warnings {
        ui.toast(format!("ui.renderer: {w}"));
    }
    if fallback {
        ui.toast(urfin_i18n::current().get("ui-renderer-fallback"));
    }
    ui.watch_system(&cc.egui_ctx);
    // Окно показывается скрытым от экрана: первые кадры рисуются уже в
    // итоговом размере, на экран выходит только готовый (S1-13).
    crate::sys::cloak(cc, true);
    let bg = ui
        .tokens()
        .palette(theme)
        .get(urfin_ui::tokens::Role::BgBase);
    // При Mica под визуалом DirectComposition видна поверхность окна: чёрные
    // пиксели в продлённой рамке DWM прозрачны, цветная кисть закрыла бы Mica.
    let bg = if mica { egui::Color32::BLACK } else { bg };
    crate::sys::set_background(cc, [bg.r(), bg.g(), bg.b()]);
    crate::sys::no_transitions(cc);
    // Своя рамка (заголовок с табами, Snap Layouts) — пока окно скрыто.
    ui.set_chrome(urfin_ui::sys::install_window_chrome(cc, &cc.egui_ctx, mica));
    cc.egui_ctx
        .send_viewport_cmd(egui::ViewportCommand::Maximized(maximized));
    cc.egui_ctx
        .send_viewport_cmd(egui::ViewportCommand::Visible(true));
    cc.egui_ctx.request_repaint_after(SHOW_AT_LATEST);
    App {
        core: Some(core),
        waker,
        ui,
        out: Vec::new(),
        startup,
        first_frame_done: false,
        created: Instant::now(),
        reveal: false,
        shown: false,
        ipc,
        activate: 0,
        selftest,
        lost,
        restart,
        #[cfg(debug_assertions)]
        lose_after: (!resumed).then(force_device_lost).flatten(),
    }
}

/// `URFIN_FORCE_DEVICE_LOST=<секунды>` (только debug): эмуляция потери устройства
/// через столько секунд после создания окна — проверка перезапуска на WARP.
#[cfg(debug_assertions)]
fn force_device_lost() -> Option<Duration> {
    let s = std::env::var("URFIN_FORCE_DEVICE_LOST").ok()?;
    s.trim().parse::<f64>().ok().map(Duration::from_secs_f64)
}

struct App {
    core: Option<Core>,
    waker: Arc<EguiWaker>,
    ui: UrfinUi,
    out: Vec<Command>,
    startup: Startup,
    first_frame_done: bool,
    created: Instant,
    /// Кадр для показа нарисован: следующий `logic` снимает cloak.
    reveal: bool,
    shown: bool,
    ipc: Option<crate::ipc::Inbox>,
    /// Сколько кадров ещё выводить окно вперёд после запроса второго запуска.
    activate: u8,
    selftest: Option<crate::selftest::SelfTest>,
    /// Устройство потеряно (`gpu::watch_device`): окно — заново, на следующей попытке.
    lost: Arc<AtomicBool>,
    restart: Restart,
    #[cfg(debug_assertions)]
    lose_after: Option<Duration>,
}

impl App {
    /// Отдать Core и вторые запуски в [`run`] и закрыть окно: оно откроется заново
    /// на следующей попытке рендера. `on_exit` без Core сессию не трогает.
    fn hand_over(&mut self, ctx: &egui::Context) {
        let Some(core) = self.core.take() else {
            return;
        };
        *self.restart.borrow_mut() = Some(Resume {
            core,
            waker: Arc::clone(&self.waker),
            startup: self.startup,
            ipc: self.ipc.take(),
            selftest: self.selftest.take(),
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

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
        #[cfg(debug_assertions)]
        if self.lose_after.is_some_and(|d| self.created.elapsed() >= d) {
            self.lose_after = None;
            tracing::warn!("URFIN_FORCE_DEVICE_LOST: emulating device loss");
            self.lost.store(true, Ordering::Release);
        }
        #[cfg(debug_assertions)]
        if let Some(d) = self.lose_after {
            ctx.request_repaint_after(d.saturating_sub(self.created.elapsed()));
        }
        if self.lost.load(Ordering::Acquire) {
            self.hand_over(ctx);
            return;
        }
        let Some(core) = self.core.as_mut() else {
            return;
        };
        // Команды клавиатуры из raw_input_hook — до pump, видны в этом же кадре.
        for c in self.out.drain(..) {
            core.dispatch(c);
        }
        // Второй запуск (S1-15): пути — новыми табами, окно — вперёд.
        if let Some(inbox) = &self.ipc {
            for req in inbox.try_iter() {
                for c in req.commands() {
                    core.dispatch(c);
                }
                self.activate = 2;
            }
        }
        if let Some(st) = &mut self.selftest {
            st.logic(ctx, core, self.shown);
        }
        if self.activate > 0 && self.shown {
            // winit не фокусирует свёрнутое окно: сначала развернуть, фокус —
            // и в этом кадре, и в следующем, когда окно уже не свёрнуто.
            if self.activate == 2 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.activate -= 1;
            ctx.request_repaint();
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
                        urfin_core::ListingStatus::Loading { .. } => "loading",
                        urfin_core::ListingStatus::Complete => "complete",
                        _ => "other",
                    };
                    format!("{st}:{}:{}", t.rows(), t.listing.order.len())
                })
                .collect();
            tracing::trace!(target: "frame", rev = s.rev, ?tabs, causes = ?ui.ctx().repaint_causes(), "frame");
        }
        let ev = core.take_ui_events();
        let t_ui = Instant::now();
        self.ui.show(ui, core.state(), &ev, &mut self.out);
        if let Some(st) = &mut self.selftest {
            st.frame(ui, core.state(), t_ui.elapsed().as_secs_f64() * 1000.0);
        }
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
                urfin_core::ListingStatus::Loading { .. }
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
        match s.focus {
            // Оверлеи S2 пока без виджетов: Esc закрывает (до keymap S3).
            Focus::Overlay(_) => take_overlay_keys(&mut raw.events, &mut self.out),
            Focus::Panel if s.prompts.active().is_none() => {
                take_keys(&mut raw.events, &mut self.out);
            }
            _ => {}
        }
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
    fn ctrl_o_by_position_on_any_layout() {
        let ctrl = egui::Modifiers::CTRL;
        // Русская раскладка: логической клавиши O нет, позиция — O.
        let ev = egui::Event::Key {
            key: egui::Key::Num0,
            physical_key: Some(egui::Key::O),
            pressed: true,
            repeat: false,
            modifiers: ctrl,
        };
        let (rest, cmds) = run(vec![ev]);
        assert_eq!(rest.len(), 0);
        assert_eq!(cmds, ["ui.panels.toggle_all"]);
    }

    #[test]
    fn view_mode_and_column_keys() {
        let none = egui::Modifiers::NONE;
        let ctrl = egui::Modifiers::CTRL;
        let (left, cmds) = run(vec![
            key(egui::Key::Num1, ctrl, true),
            key(egui::Key::Num2, ctrl, true),
            key(egui::Key::Num4, ctrl, true),
            key(egui::Key::ArrowRight, none, true),
            key(egui::Key::ArrowLeft, none, true),
            key(egui::Key::F9, none, true),
        ]);
        assert_eq!(left.len(), 0);
        assert_eq!(
            cmds,
            [
                "panel.view_mode mode=brief",
                "panel.view_mode mode=medium",
                "panel.view_mode mode=wide",
                "panel.cursor.right",
                "panel.cursor.left",
                "ui.menu",
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
