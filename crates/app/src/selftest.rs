//! Автономная самопроверка окна (`--selftest <dir>`): для платформ, где окно не
//! проверено руками (macOS). Сценарий сам разворачивает, растягивает, сворачивает
//! окно и ходит по каталогам; пишет `selftest.json` (кадры, размеры окна, ошибки
//! поверхности wgpu, метки этапов) и PNG собственных кадров. Только чтение
//! состояния и команды — как у пользователя.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use eframe::egui;
use serde::Serialize;
use urfin_core::{AppState, Command, Core, ListingStatus};

/// Скриншот не пришёл за это время — записать отказ и идти дальше.
const SHOT_TIMEOUT: Duration = Duration::from_secs(3);
/// Скриншоты уменьшаются до этой ширины (Retina — 2–3× пикселей).
const SHOT_MAX_WIDTH: usize = 1280;

static ADAPTER: OnceLock<String> = OnceLock::new();
static SURFACE: Mutex<Vec<(f64, String)>> = Mutex::new(Vec::new());
static T0: OnceLock<Instant> = OnceLock::new();

fn ms_since_start() -> f64 {
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// Выбранный адаптер wgpu (из `pick_adapter`).
pub fn note_adapter(text: String) {
    let _ = ADAPTER.set(text);
}

/// Не-`Success` статус поверхности wgpu (resize, occluded, lost…).
pub fn note_surface(status: &str) {
    SURFACE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((ms_since_start(), status.to_owned()));
}

enum Step {
    Mark(&'static str),
    WaitShown,
    Wait(u64),
    Vp(egui::ViewportCommand),
    Core(Command),
    Shot(&'static str),
    Finish,
}

#[derive(Serialize)]
struct Frame {
    t_ms: f64,
    phase: &'static str,
    ui_ms: f64,
    /// Внутренний размер в точках и масштаб (Retina — 2.0).
    w: f32,
    h: f32,
    ppp: f32,
    /// Активный таб обеих панелей: `loading:<rows>` / `complete:<rows>`.
    tabs: [String; 2],
}

#[derive(Serialize, PartialEq, Clone)]
struct Window {
    t_ms: f64,
    phase: &'static str,
    w: f32,
    h: f32,
    ppp: f32,
    maximized: Option<bool>,
    fullscreen: Option<bool>,
    minimized: Option<bool>,
    focused: Option<bool>,
}

#[derive(Serialize)]
struct Shot {
    name: String,
    t_ms: f64,
    file: Option<String>,
    width: usize,
    height: usize,
    /// Доля почти чёрных пикселей и пикселей фона панели (0..1).
    black: f64,
    background: f64,
    error: Option<String>,
}

#[derive(Serialize)]
struct Report {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    adapter: Option<String>,
    first_frame_tabs: Option<[String; 2]>,
    marks: Vec<(&'static str, f64)>,
    windows: Vec<Window>,
    frames: Vec<Frame>,
    shots: Vec<Shot>,
    surface: Vec<(f64, String)>,
    surface_counts: BTreeMap<String, u32>,
    total_ms: f64,
}

pub struct SelfTest {
    dir: PathBuf,
    steps: VecDeque<Step>,
    wait_until: Option<Instant>,
    phase: &'static str,
    marks: Vec<(&'static str, f64)>,
    frames: Vec<Frame>,
    windows: Vec<Window>,
    shots: Vec<Shot>,
    pending_shot: Option<(&'static str, Instant)>,
    first_frame_tabs: Option<[String; 2]>,
    background: egui::Color32,
}

impl SelfTest {
    pub fn new(dir: PathBuf) -> SelfTest {
        let _ = T0.set(Instant::now());
        SelfTest {
            dir,
            steps: script().into(),
            wait_until: None,
            phase: "startup",
            marks: Vec::new(),
            frames: Vec::new(),
            windows: Vec::new(),
            shots: Vec::new(),
            pending_shot: None,
            first_frame_tabs: None,
            background: egui::Color32::TRANSPARENT,
        }
    }

    /// Шаг сценария; вызывается из `App::logic` каждый кадр.
    pub fn logic(&mut self, ctx: &egui::Context, core: &mut Core, shown: bool) {
        self.track_window(ctx);
        loop {
            if let Some((name, since)) = self.pending_shot {
                if since.elapsed() < SHOT_TIMEOUT {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
                self.shots.push(Shot::failed(name, "no screenshot event"));
                self.pending_shot = None;
            }
            if let Some(t) = self.wait_until {
                let now = Instant::now();
                if now < t {
                    ctx.request_repaint_after(t - now);
                    return;
                }
                self.wait_until = None;
            }
            let Some(step) = self.steps.pop_front() else {
                return;
            };
            match step {
                Step::Mark(name) => {
                    self.phase = name;
                    self.marks.push((name, ms_since_start()));
                }
                Step::WaitShown => {
                    if !shown {
                        self.steps.push_front(Step::WaitShown);
                        ctx.request_repaint_after(Duration::from_millis(20));
                        return;
                    }
                }
                Step::Wait(ms) => {
                    self.wait_until = Some(Instant::now() + Duration::from_millis(ms));
                }
                Step::Vp(c) => ctx.send_viewport_cmd(c),
                Step::Core(c) => core.dispatch(c),
                Step::Shot(name) => {
                    self.pending_shot = Some((name, Instant::now()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                        name,
                    )));
                    ctx.request_repaint();
                    return;
                }
                Step::Finish => {
                    self.write_report();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
    }

    /// Каждый нарисованный кадр (из `App::ui`).
    pub fn frame(&mut self, ui: &egui::Ui, s: &AppState, ui_ms: f64) {
        self.background = ui.visuals().panel_fill;
        let tabs = s.panels.each_ref().map(|p| {
            let t = p.tab();
            let st = if matches!(t.listing.status, ListingStatus::Loading { .. }) {
                "loading"
            } else {
                "complete"
            };
            format!("{st}:{}", t.rows())
        });
        if self.first_frame_tabs.is_none() {
            self.first_frame_tabs = Some(tabs.clone());
        }
        let r = ui.ctx().content_rect();
        self.frames.push(Frame {
            t_ms: ms_since_start(),
            phase: self.phase,
            ui_ms,
            w: r.width(),
            h: r.height(),
            ppp: ui.ctx().pixels_per_point(),
            tabs,
        });
        let shots: Vec<(String, std::sync::Arc<egui::ColorImage>)> = ui.ctx().input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot {
                        user_data, image, ..
                    } => user_data
                        .data
                        .as_ref()
                        .and_then(|d| d.downcast_ref::<&'static str>())
                        .map(|n| ((*n).to_owned(), image.clone())),
                    _ => None,
                })
                .collect()
        });
        for (name, image) in shots {
            let shot = self.save_shot(&name, &image);
            self.shots.push(shot);
            if self.pending_shot.is_some_and(|(n, _)| n == name) {
                self.pending_shot = None;
            }
            ui.ctx().request_repaint();
        }
    }

    fn track_window(&mut self, ctx: &egui::Context) {
        let w = ctx.input(|i| {
            let v = i.viewport();
            let r = v.inner_rect.unwrap_or(egui::Rect::NOTHING);
            Window {
                t_ms: 0.0,
                phase: self.phase,
                w: r.width(),
                h: r.height(),
                ppp: i.pixels_per_point,
                maximized: v.maximized,
                fullscreen: v.fullscreen,
                minimized: v.minimized,
                focused: v.focused,
            }
        });
        let same = self.windows.last().is_some_and(|l| {
            Window {
                t_ms: l.t_ms,
                ..w.clone()
            } == *l
        });
        if !same {
            self.windows.push(Window {
                t_ms: ms_since_start(),
                ..w
            });
        }
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "доли пикселей, точность не важна"
    )]
    fn save_shot(&self, name: &str, image: &egui::ColorImage) -> Shot {
        let [w, h] = image.size;
        let step = w.div_ceil(SHOT_MAX_WIDTH).max(1);
        let (sw, sh) = (w / step, h / step);
        let mut rgb = Vec::with_capacity(sw * sh * 3);
        let (mut black, mut bg) = (0usize, 0usize);
        let b = self.background;
        for y in 0..sh {
            for x in 0..sw {
                let p = image.pixels[(y * step) * w + x * step];
                rgb.extend_from_slice(&[p.r(), p.g(), p.b()]);
                if u16::from(p.r()) + u16::from(p.g()) + u16::from(p.b()) < 30 {
                    black += 1;
                }
                if p.r().abs_diff(b.r()) <= 2
                    && p.g().abs_diff(b.g()) <= 2
                    && p.b().abs_diff(b.b()) <= 2
                {
                    bg += 1;
                }
            }
        }
        let n = (sw * sh).max(1) as f64;
        let file = format!("shot-{name}.png");
        let error = write_png(&self.dir.join(&file), sw, sh, &rgb)
            .err()
            .map(|e| e.to_string());
        Shot {
            name: name.to_owned(),
            t_ms: ms_since_start(),
            file: error.is_none().then_some(file),
            width: w,
            height: h,
            black: black as f64 / n,
            background: bg as f64 / n,
            error,
        }
    }

    fn write_report(&mut self) {
        let surface = std::mem::take(&mut *SURFACE.lock().unwrap_or_else(PoisonError::into_inner));
        let mut counts = BTreeMap::new();
        for (_, s) in &surface {
            *counts.entry(s.clone()).or_insert(0) += 1;
        }
        let report = Report {
            version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            adapter: ADAPTER.get().cloned(),
            first_frame_tabs: self.first_frame_tabs.clone(),
            marks: std::mem::take(&mut self.marks),
            windows: std::mem::take(&mut self.windows),
            frames: std::mem::take(&mut self.frames),
            shots: std::mem::take(&mut self.shots),
            surface,
            surface_counts: counts,
            total_ms: ms_since_start(),
        };
        let path = self.dir.join("selftest.json");
        match serde_json::to_vec_pretty(&report) {
            Ok(b) => {
                if let Err(e) = std::fs::write(&path, b) {
                    tracing::error!(error = %e, file = %path.display(), "selftest report not written");
                }
            }
            Err(e) => tracing::error!(error = %e, "selftest report not serialized"),
        }
        tracing::info!(file = %path.display(), "selftest done");
    }
}

impl Shot {
    fn failed(name: &str, error: &str) -> Shot {
        Shot {
            name: name.to_owned(),
            t_ms: ms_since_start(),
            file: None,
            width: 0,
            height: 0,
            black: 0.0,
            background: 0.0,
            error: Some(error.to_owned()),
        }
    }
}

/// Сценарий: старт → навигация → разворот → полный экран → размеры → свёртывание
/// → простой. Паузы — с запасом на анимации macOS (~0,5–1 с).
fn script() -> Vec<Step> {
    use egui::ViewportCommand as V;
    let mut s = vec![
        Step::Mark("startup"),
        Step::WaitShown,
        Step::Wait(600),
        Step::Shot("start"),
        Step::Mark("navigate"),
        Step::Core(Command::new("panel.cursor.home")),
        Step::Core(Command::new("panel.cursor.down")),
    ];
    for _ in 0..5 {
        s.extend([
            Step::Core(Command::new("panel.enter")),
            Step::Wait(250),
            Step::Core(Command::new("panel.up")),
            Step::Wait(250),
        ]);
    }
    s.extend([
        Step::Shot("navigated"),
        Step::Mark("maximize"),
        Step::Vp(V::Maximized(true)),
        Step::Wait(1500),
        Step::Shot("maximized"),
        Step::Mark("restore"),
        Step::Vp(V::Maximized(false)),
        Step::Wait(1500),
        Step::Shot("restored"),
        Step::Mark("fullscreen"),
        Step::Vp(V::Fullscreen(true)),
        Step::Wait(2500),
        Step::Shot("fullscreen"),
        Step::Mark("fullscreen_exit"),
        Step::Vp(V::Fullscreen(false)),
        Step::Wait(2500),
        Step::Mark("resize"),
    ]);
    for (w, h) in [
        (900.0, 600.0),
        (1000.0, 650.0),
        (1100.0, 700.0),
        (700.0, 500.0),
        (1200.0, 760.0),
    ] {
        s.extend([Step::Vp(V::InnerSize(egui::vec2(w, h))), Step::Wait(300)]);
    }
    s.extend([
        Step::Shot("resized"),
        Step::Mark("minimize"),
        Step::Vp(V::Minimized(true)),
        Step::Wait(1500),
        Step::Mark("unminimize"),
        Step::Vp(V::Minimized(false)),
        Step::Vp(V::Focus),
        Step::Wait(1500),
        Step::Shot("unminimized"),
        // Простой: кадров быть не должно, кроме пробуждения в конце.
        Step::Mark("idle"),
        Step::Wait(10_000),
        Step::Mark("done"),
        Step::Finish,
    ]);
    s
}

/// PNG без сжатия (deflate «stored»): без зависимостей; архив результатов
/// всё равно сжимается.
fn write_png(path: &Path, w: usize, h: usize, rgb: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for row in rgb.chunks(w * 3) {
        raw.push(0); // фильтр None
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(b) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        let len = u16::try_from(b.len()).expect("block ≤ 65535");
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&u32::try_from(w).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(h).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 бит, RGB
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(&b"IHDR"[..], &ihdr[..]), (b"IDAT", &z[..]), (b"IEND", &[])] {
        png.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let start = png.len();
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let crc = crc32(&png[start..]);
        png.extend_from_slice(&crc.to_be_bytes());
    }
    std::fs::write(path, png)
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffff_u32;
    for &x in data {
        c ^= u32::from(x);
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn png_has_valid_structure() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.png");
        write_png(&p, 2, 2, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]).unwrap();
        let b = std::fs::read(p).unwrap();
        assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&b[12..16], b"IHDR");
        assert_eq!(&b[b.len() - 8..b.len() - 4], b"IEND");
    }

    #[test]
    fn script_ends_with_finish() {
        assert!(matches!(script().last(), Some(Step::Finish)));
    }
}
