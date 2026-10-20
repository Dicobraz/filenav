//! Системный probe (S2 §1.5): тема, акцент, прозрачность, высокая контрастность, файлы
//! шрифтов. Все чтения — до окна, в `app::boot` (поток `font-probe`); в кадре I/O нет.
//! Изменения темы и акцента приходят из [`watch`] (поток `sys-watch`).
//! `unsafe` разрешён только в платформенных модулях (S0 v4 №1).

use std::path::PathBuf;
use std::sync::mpsc;

use urfin_core::ConfigWarning;

use crate::accent::AccentPalette;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as os;

#[cfg(windows)]
mod chrome_win;

pub mod cpu;

#[cfg(windows)]
#[path = "time_win.rs"]
mod time;

#[cfg(not(windows))]
#[path = "time_other.rs"]
mod time;

pub use time::Zone;

/// Местное время для ячеек дат (S2-08).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
use other as os;

/// То, что меняется на лету: тема, акцент, прозрачность, контрастность.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// `None` — ОС не сказала, тему ведёт winit (`ctx.system_theme()`).
    pub theme: Option<egui::Theme>,
    pub accent: AccentPalette,
    /// Прозрачность окон включена (Mica — S2-06).
    pub transparency: bool,
    pub high_contrast: bool,
    /// Системные цвета при высокой контрастности (Windows, `GetSysColor`).
    pub hc: Option<HcColors>,
}

/// Минимальный набор системных цветов для режима высокой контрастности (S2 §1.5).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HcColors {
    pub window: egui::Color32,
    pub window_text: egui::Color32,
    pub highlight: egui::Color32,
    pub highlight_text: egui::Color32,
    pub gray_text: egui::Color32,
    pub hotlight: egui::Color32,
}

/// Возможности материала окна (S2 §1.4).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct BackdropCaps {
    /// Mica: Windows 11 22H2+ (сборка ≥ 22621).
    pub mica: bool,
}

/// Файлы шрифтов ОС по цепочкам S2 §1.5. Пустая цепочка — встроенные шрифты.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontFiles {
    /// Интерфейс: `SegUIVar.ttf`, иначе `segoeui.ttf` + Semibold.
    pub ui: Vec<PathBuf>,
    /// Иконки: Segoe Fluent Icons, иначе Segoe MDL2 Assets; нет — вшитые (S2-04).
    pub icons: Option<PathBuf>,
    /// Моноширинный: Cascadia Mono, иначе Consolas.
    pub mono: Option<PathBuf>,
    /// Символы вне Segoe UI.
    pub symbols: Option<PathBuf>,
    /// CJK по языку ОС; грузится после первого кадра.
    pub cjk: Option<PathBuf>,
}

/// Итог probe.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SysProbe {
    pub look: Look,
    pub backdrop: BackdropCaps,
    pub fonts: FontFiles,
    /// Например, «нет ни одного шрифта интерфейса» — тогда встроенные шрифты egui.
    pub warnings: Vec<ConfigWarning>,
}

/// Probe ОС. Конфиг `[ui]` (свои шрифты, тема) учтут S2-04/S2-05.
pub fn probe_system() -> SysProbe {
    let _s = tracing::info_span!("startup.probe").entered();
    let started = std::time::Instant::now();
    // Linux/macOS: zoneinfo читает и кэширует jiff — здесь, а не в UI-потоке (S2-08).
    #[cfg(not(windows))]
    let _ = Zone::system();
    let mut probe = SysProbe {
        look: os::look(),
        backdrop: os::backdrop(),
        fonts: os::fonts(),
        warnings: Vec::new(),
    };
    if cfg!(windows) {
        probe.warnings.extend(font_warning(&probe.fonts));
    }
    tracing::info!(target: "startup.probe", elapsed = ?started.elapsed());
    tracing::debug!(?probe, "system probe");
    probe
}

/// Нет ни одного шрифта интерфейса — встроенные шрифты egui и предупреждение.
fn font_warning(fonts: &FontFiles) -> Option<ConfigWarning> {
    fonts.ui.is_empty().then(|| ConfigWarning {
        key: "ui.fonts".to_owned(),
        msg: urfin_i18n::current().get("ui-warn-no-system-font"),
    })
}

/// Своя рамка окна (S2-06): Windows — subclass и DWM, `mica` — подложка Mica за
/// прозрачными частями. Другие ОС и ошибка — `None`: системная рамка.
pub fn install_window_chrome(
    window: &impl raw_window_handle::HasWindowHandle,
    ctx: &egui::Context,
    mica: bool,
) -> Option<crate::chrome::ChromeHandle> {
    #[cfg(windows)]
    {
        let raw = window.window_handle().ok()?.as_raw();
        let raw_window_handle::RawWindowHandle::Win32(h) = raw else {
            return None;
        };
        chrome_win::install(h.hwnd.get(), ctx, mica)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, ctx, mica);
        None
    }
}

/// Mica сейчас: Win11 22H2+, не высокая контрастность и `[ui] backdrop` — `auto` при
/// включённой прозрачности или `mica` (принудительно). Нужна до окна (прозрачный swapchain).
pub fn mica_wanted(backdrop_cfg: &str) -> bool {
    let cfg = backdrop_cfg.trim().to_ascii_lowercase();
    if cfg == "solid" {
        return false;
    }
    let look = os::look();
    os::backdrop().mica && !look.high_contrast && (look.transparency || cfg == "mica")
}

/// Следит за темой и акцентом ОС: новый [`Look`] уходит в `sink`, кадр будится.
/// Windows — `RegNotifyChangeKeyValue` в отдельном потоке (без опроса); остальные ОС —
/// ничего (тему ведёт winit; акцент macOS — при реактивации, docs/macos-notes.md).
pub fn watch(ctx: &egui::Context, sink: mpsc::Sender<Look>) {
    os::watch(ctx.clone(), sink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_font_chain_warns() {
        let w = font_warning(&FontFiles::default()).expect("warning");
        assert_eq!(w.key, "ui.fonts");
        assert!(!w.msg.starts_with('⟨'), "{}", w.msg);
        let found = FontFiles {
            ui: vec!["segoeui.ttf".into()],
            ..FontFiles::default()
        };
        assert_eq!(font_warning(&found), None);
    }

    #[test]
    fn probe_does_not_panic() {
        let p = probe_system();
        if cfg!(windows) {
            assert!(p.look.theme.is_some());
            assert!(!p.fonts.ui.is_empty(), "{:?}", p.fonts);
        }
    }
}
