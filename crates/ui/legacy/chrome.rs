//! Своя рамка окна (S2 §0, §1.4; S2-06): заголовок 40 px с табами и кнопками caption
//! рисует UI, а ОС узнаёт о нём через hit-test. Здесь — платформонезависимая часть:
//! раскладка в физических пикселях клиентской области и [`hit_test`]. Subclass окна
//! и DWM — `sys::windows::chrome`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// Прямоугольник в физических пикселях клиентской области.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PxRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PxRect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Из логического прямоугольника egui.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "координаты окна в пределах i32"
    )]
    pub fn from_egui(r: egui::Rect, ppp: f32) -> Self {
        Self {
            left: (r.left() * ppp).round() as i32,
            top: (r.top() * ppp).round() as i32,
            right: (r.right() * ppp).round() as i32,
            bottom: (r.bottom() * ppp).round() as i32,
        }
    }
}

/// Раскладка заголовка, которую UI сообщает ОС каждый кадр.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleLayout {
    /// Высота заголовка.
    pub caption_h: i32,
    /// Ширина клиентской области.
    pub width: i32,
    /// Толщина рамки ресайза сверху (у развёрнутого окна — 0).
    pub resize: i32,
    /// Кнопка «развернуть» — `HTMAXBUTTON` (Snap Layouts Win11).
    pub max_button: PxRect,
    /// Кликабельное в заголовке: табы, `+`, «свернуть», «закрыть».
    pub interactive: Vec<PxRect>,
}

/// Что под курсором с точки зрения ОС.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Обычная клиентская область — события получает egui.
    Client,
    /// Пустой заголовок: перетаскивание, двойной клик, Aero Snap.
    Caption,
    /// Кнопка «развернуть»: меню Snap Layouts по наведению.
    MaxButton,
    Top,
    TopLeft,
    TopRight,
}

/// Hit-test точки `(x, y)` клиентской области.
pub fn hit_test(l: &TitleLayout, x: i32, y: i32) -> Hit {
    if y < l.resize {
        // Углы — шире: как у системной рамки.
        let corner = l.resize * 2;
        return if x < corner {
            Hit::TopLeft
        } else if x >= l.width - corner {
            Hit::TopRight
        } else {
            Hit::Top
        };
    }
    if y >= l.caption_h {
        return Hit::Client;
    }
    if l.max_button.contains(x, y) {
        return Hit::MaxButton;
    }
    if l.interactive.iter().any(|r| r.contains(x, y)) {
        Hit::Client
    } else {
        Hit::Caption
    }
}

/// Общее состояние рамки: UI пишет раскладку, оконная процедура читает её и сообщает
/// наведение и нажатие кнопки «развернуть» (её события идут как неклиентские).
#[derive(Debug, Default)]
pub struct ChromeShared {
    layout: Mutex<TitleLayout>,
    pub max_hover: AtomicBool,
    pub max_pressed: AtomicBool,
    /// Окно развёрнуто (обновляет оконная процедура и UI).
    pub maximized: AtomicBool,
}

impl ChromeShared {
    pub fn layout(&self) -> TitleLayout {
        self.layout
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_layout(&self, l: TitleLayout) {
        let mut cur = self.layout.lock().unwrap_or_else(PoisonError::into_inner);
        if *cur != l {
            tracing::debug!(layout = ?l, "title layout");
            *cur = l;
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> Hit {
        let l = self.layout.lock().unwrap_or_else(PoisonError::into_inner);
        hit_test(&l, x, y)
    }
}

/// Своя рамка установлена (Windows); без неё — системная рамка ОС.
#[derive(Clone)]
pub struct ChromeHandle {
    pub shared: Arc<ChromeShared>,
    /// Mica за прозрачными частями (заголовок, зазоры, keybar).
    pub mica: bool,
    /// Поставить тёмную тему рамки DWM (`DWMWA_USE_IMMERSIVE_DARK_MODE`).
    set_dark: Arc<dyn Fn(bool) + Send + Sync>,
    dark: Arc<AtomicBool>,
}

impl ChromeHandle {
    pub fn new(
        shared: Arc<ChromeShared>,
        mica: bool,
        set_dark: Arc<dyn Fn(bool) + Send + Sync>,
    ) -> Self {
        Self {
            shared,
            mica,
            set_dark,
            dark: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Тема рамки DWM; вызывается каждый кадр, ОС трогается только при смене.
    pub fn sync_dark(&self, dark: bool) {
        if self.dark.swap(dark, Ordering::Relaxed) != dark {
            (self.set_dark)(dark);
        }
    }
}

impl std::fmt::Debug for ChromeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChromeHandle")
            .field("mica", &self.mica)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> TitleLayout {
        TitleLayout {
            caption_h: 40,
            width: 1000,
            resize: 8,
            max_button: PxRect {
                left: 908,
                top: 0,
                right: 954,
                bottom: 40,
            },
            interactive: vec![
                // Таб.
                PxRect {
                    left: 8,
                    top: 8,
                    right: 150,
                    bottom: 40,
                },
                // «Закрыть».
                PxRect {
                    left: 954,
                    top: 0,
                    right: 1000,
                    bottom: 40,
                },
            ],
        }
    }

    #[test]
    fn hit_test_layout() {
        let l = layout();
        assert_eq!(hit_test(&l, 500, 2), Hit::Top);
        assert_eq!(hit_test(&l, 3, 2), Hit::TopLeft);
        assert_eq!(hit_test(&l, 995, 2), Hit::TopRight);
        assert_eq!(hit_test(&l, 500, 20), Hit::Caption);
        assert_eq!(hit_test(&l, 50, 20), Hit::Client, "таб");
        assert_eq!(hit_test(&l, 930, 20), Hit::MaxButton);
        assert_eq!(hit_test(&l, 970, 20), Hit::Client, "«закрыть» — egui");
        assert_eq!(hit_test(&l, 500, 40), Hit::Client, "ниже заголовка");
        // У развёрнутого окна ресайза сверху нет: верх кнопки — тоже кнопка.
        let max = TitleLayout { resize: 0, ..l };
        assert_eq!(hit_test(&max, 930, 0), Hit::MaxButton);
        assert_eq!(hit_test(&max, 500, 0), Hit::Caption);
    }

    #[test]
    fn rect_from_egui_scales() {
        let r = PxRect::from_egui(
            egui::Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(56.0, 40.0)),
            1.5,
        );
        assert_eq!(
            r,
            PxRect {
                left: 15,
                top: 0,
                right: 84,
                bottom: 60
            }
        );
    }
}
