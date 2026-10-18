//! `--screens <dir>` (S2-21): экраны S2 §2 ([`urfin_ui::screens`]) в настоящем
//! окне — DX12 и системные шрифты — по очереди в светлой и тёмной теме, снимок
//! каждого — `dir/<экран>_<тема>.png`, затем окно закрывается. Для скриншотов в
//! PR; состояние Core не трогается и не сохраняется. Только с `--features screens`;
//! без неё — пустая заглушка (флаг отвергается при разборе аргументов).

#[cfg(not(feature = "screens"))]
pub use stub::ScreensRun;

#[cfg(not(feature = "screens"))]
mod stub {
    use std::path::PathBuf;

    use eframe::egui;
    use urfin_ui::UrfinUi;

    pub struct ScreensRun;

    impl ScreensRun {
        pub fn new(_dir: PathBuf) -> ScreensRun {
            ScreensRun
        }

        #[allow(
            clippy::unused_self,
            reason = "заглушка: та же сигнатура, что без feature"
        )]
        pub fn frame(&mut self, _root: &mut egui::Ui, _ui: &mut UrfinUi) -> bool {
            false
        }
    }
}

#[cfg(feature = "screens")]
pub use real::ScreensRun;

#[cfg(feature = "screens")]
mod real {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, PoisonError};

    use eframe::egui;
    use urfin_ui::UrfinUi;
    use urfin_ui::screens::Screen;

    /// Кадров до снимка: раскладка, шрифты и тема устоялись.
    const SETTLE_FRAMES: u32 = 4;
    /// Кадров в начале: окно приняло размер 1280×768.
    const WARMUP_FRAMES: u32 = 10;

    pub struct ScreensRun {
        dir: PathBuf,
        screens: Vec<Screen>,
        /// Экран и тема (0 — светлая, 1 — тёмная).
        i: usize,
        dark: bool,
        frames: u32,
        warmup: u32,
        requested: bool,
        inbox: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    }

    impl ScreensRun {
        pub fn new(dir: PathBuf) -> ScreensRun {
            let mut screens = urfin_ui::screens::all();
            if let Some(s) = screens.first_mut() {
                s.set_theme(false);
            }
            ScreensRun {
                dir,
                screens,
                i: 0,
                dark: false,
                frames: 0,
                warmup: 0,
                requested: false,
                inbox: Arc::default(),
            }
        }

        /// Кадр режима экранов вместо обычного. `false` — экраны кончились.
        pub fn frame(&mut self, root: &mut egui::Ui, ui: &mut UrfinUi) -> bool {
            let ctx = root.ctx().clone();
            ctx.request_repaint();
            if self.warmup < WARMUP_FRAMES {
                if self.warmup == 0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        1280.0, 768.0,
                    )));
                }
                self.warmup += 1;
            }
            let shot = self
                .inbox
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(image) = shot {
                self.save(&image);
                if !self.advance() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return false;
                }
            }
            let Some(sc) = self.screens.get(self.i) else {
                return false;
            };
            let ev = if self.frames == 0 {
                &sc.events[..]
            } else {
                &[]
            };
            let mut out = Vec::new();
            ui.show(root, &sc.state, ev, &mut out);
            self.frames += 1;
            if self.warmup >= WARMUP_FRAMES && self.frames >= SETTLE_FRAMES && !self.requested {
                self.requested = true;
                let inbox = Arc::clone(&self.inbox);
                ctx.request_screenshot(move |image| {
                    *inbox.lock().unwrap_or_else(PoisonError::into_inner) = Some(image);
                });
            }
            true
        }

        fn save(&self, image: &egui::ColorImage) {
            let Some(sc) = self.screens.get(self.i) else {
                return;
            };
            let [w, h] = image.size;
            let mut rgb = Vec::with_capacity(w * h * 3);
            for p in &image.pixels {
                rgb.extend_from_slice(&[p.r(), p.g(), p.b()]);
            }
            let theme = if self.dark { "dark" } else { "light" };
            let path = self.dir.join(format!("{}_{theme}.png", sc.name));
            if let Err(e) = crate::selftest::write_png(&path, w, h, &rgb) {
                tracing::error!(error = %e, path = %path.display(), "screens: cannot write");
            } else {
                tracing::info!(path = %path.display(), w, h, "screens: saved");
            }
        }

        /// Следующая тема или экран; `false` — всё снято.
        fn advance(&mut self) -> bool {
            self.frames = 0;
            self.requested = false;
            if self.dark {
                self.dark = false;
                self.i += 1;
            } else {
                self.dark = true;
            }
            let dark = self.dark;
            match self.screens.get_mut(self.i) {
                Some(sc) => {
                    sc.set_theme(dark);
                    true
                }
                None => false,
            }
        }
    }
}
