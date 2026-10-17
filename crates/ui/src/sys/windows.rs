//! Windows: реестр (тема, прозрачность, акцент, шрифты), `SPI_GETHIGHCONTRAST`,
//! `RegNotifyChangeKeyValue` для изменений (S2 §1.5).

#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT, COLOR_WINDOW,
    COLOR_WINDOWTEXT, GetSysColor,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET,
    RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW, RegNotifyChangeKeyValue,
    RegOpenKeyExW,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForMultipleObjects};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};

use super::{BackdropCaps, FontFiles, HcColors, Look};
use crate::accent::AccentPalette;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
const DWM: &str = r"Software\Microsoft\Windows\DWM";
const HIGH_CONTRAST: &str = r"Control Panel\Accessibility\HighContrast";
const FONTS: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";
const VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
/// Первая сборка Windows 11 с `DWMWA_SYSTEMBACKDROP_TYPE` (22H2).
const MICA_BUILD: u32 = 22_621;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Значение реестра в буфер; `None` — нет ключа, значения или другой тип.
fn reg_raw(root: HKEY, key: &str, value: &str, kind: u32) -> Option<Vec<u8>> {
    let (key, value) = (wide(key), wide(value));
    let mut len = 0u32;
    // SAFETY: строки с NUL живут до конца вызова; запрос размера без буфера.
    let rc = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            value.as_ptr(),
            kind,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut len,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: буфер `len` байт; функция пишет не больше `len` и обновляет его.
    let rc = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            value.as_ptr(),
            kind,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &raw mut len,
        )
    };
    (rc == ERROR_SUCCESS).then(|| {
        buf.truncate(len as usize);
        buf
    })
}

fn reg_dword(root: HKEY, key: &str, value: &str) -> Option<u32> {
    let b = reg_raw(root, key, value, RRF_RT_REG_DWORD)?;
    Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
}

fn reg_string(root: HKEY, key: &str, value: &str) -> Option<String> {
    let b = reg_raw(root, key, value, RRF_RT_REG_SZ)?;
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16(&units).ok()
}

pub(super) fn look() -> Look {
    let light =
        reg_dword(HKEY_CURRENT_USER, PERSONALIZE, "AppsUseLightTheme").is_none_or(|v| v != 0);
    let transparency =
        reg_dword(HKEY_CURRENT_USER, PERSONALIZE, "EnableTransparency").is_none_or(|v| v != 0);
    let accent = reg_raw(
        HKEY_CURRENT_USER,
        ACCENT,
        "AccentPalette",
        RRF_RT_REG_BINARY,
    )
    .and_then(|b| AccentPalette::from_registry(&b))
    .or_else(|| reg_dword(HKEY_CURRENT_USER, DWM, "AccentColor").map(AccentPalette::from_abgr))
    .unwrap_or_default();
    let high_contrast = high_contrast();
    Look {
        theme: Some(if light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        }),
        accent,
        transparency,
        high_contrast,
        hc: high_contrast.then(hc_colors),
    }
}

fn hc_colors() -> HcColors {
    let c = |index| {
        // SAFETY: без указателей; индекс из констант COLOR_*.
        let v = unsafe { GetSysColor(index) };
        // COLORREF — 0x00BBGGRR.
        let [r, g, b, _] = v.to_le_bytes();
        egui::Color32::from_rgb(r, g, b)
    };
    HcColors {
        window: c(COLOR_WINDOW),
        window_text: c(COLOR_WINDOWTEXT),
        highlight: c(COLOR_HIGHLIGHT),
        highlight_text: c(COLOR_HIGHLIGHTTEXT),
        gray_text: c(COLOR_GRAYTEXT),
        hotlight: c(COLOR_HOTLIGHT),
    }
}

fn high_contrast() -> bool {
    let mut hc = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: структура нужного размера с заполненным `cbSize`; функция только пишет в неё.
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, (&raw mut hc).cast(), 0) };
    ok != 0 && hc.dwFlags & HCF_HIGHCONTRASTON != 0
}

pub(super) fn backdrop() -> BackdropCaps {
    let build = reg_string(HKEY_LOCAL_MACHINE, VERSION, "CurrentBuildNumber")
        .and_then(|b| b.trim().parse::<u32>().ok())
        .unwrap_or(0);
    BackdropCaps {
        mica: build >= MICA_BUILD,
    }
}

fn fonts_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
        .join("Fonts")
}

/// Файл шрифта по имени из реестра `Fonts` (HKLM — относительно `%WINDIR%\Fonts`, HKCU —
/// абсолютный путь пользовательской установки), иначе — файл по умолчанию.
fn font(dir: &Path, name: &str, file: &str) -> Option<PathBuf> {
    let registered = [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
        .into_iter()
        .filter_map(|root| reg_string(root, FONTS, name))
        .map(|f| dir.join(f));
    registered
        .chain(std::iter::once(dir.join(file)))
        .find(|p| p.is_file())
}

/// Язык ОС (`ja-JP`).
fn locale() -> String {
    const LOCALE_NAME_MAX_LENGTH: i32 = 85;
    let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH as usize];
    // SAFETY: буфер на LOCALE_NAME_MAX_LENGTH (85) символов; функция пишет не больше.
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), LOCALE_NAME_MAX_LENGTH) };
    let n = usize::try_from(n).unwrap_or(0).saturating_sub(1);
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

pub(super) fn fonts() -> FontFiles {
    let dir = fonts_dir();
    let ui = match font(&dir, "Segoe UI Variable (TrueType)", "SegUIVar.ttf") {
        Some(var) => vec![var],
        None => [
            ("Segoe UI (TrueType)", "segoeui.ttf"),
            ("Segoe UI Semibold (TrueType)", "seguisb.ttf"),
        ]
        .into_iter()
        .filter_map(|(name, file)| font(&dir, name, file))
        .collect(),
    };
    let icons = font(&dir, "Segoe Fluent Icons (TrueType)", "SegoeIcons.ttf")
        .or_else(|| font(&dir, "Segoe MDL2 Assets (TrueType)", "segmdl2.ttf"));
    let symbols = font(&dir, "Segoe UI Symbol (TrueType)", "seguisym.ttf");
    let locale = locale();
    let cjk = match locale.split('-').next().unwrap_or_default() {
        "ja" => Some("YuGothM.ttc"),
        "zh" => Some("msyh.ttc"),
        "ko" => Some("malgun.ttf"),
        _ => None,
    }
    .map(|f| dir.join(f))
    .filter(|p| p.is_file());
    FontFiles {
        ui,
        icons,
        symbols,
        cjk,
    }
}

/// Ключ реестра с событием изменения.
struct Watched {
    key: HKEY,
    event: HANDLE,
}

impl Watched {
    fn open(path: &str) -> Option<Self> {
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: путь с NUL; `key` — выходной параметр.
        let rc = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                KEY_NOTIFY,
                &raw mut key,
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        // SAFETY: безымянное событие с автосбросом, без атрибутов безопасности.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if event.is_null() {
            return None;
        }
        let w = Self { key, event };
        w.arm().then_some(w)
    }

    /// Подписка на одно следующее изменение значений ключа.
    fn arm(&self) -> bool {
        // SAFETY: ключ открыт с KEY_NOTIFY, событие живо; асинхронный режим.
        let rc = unsafe {
            RegNotifyChangeKeyValue(self.key, 0, REG_NOTIFY_CHANGE_LAST_SET, self.event, 1)
        };
        rc == ERROR_SUCCESS
    }
}

/// Поток `sys-watch`: ждёт изменений ключей темы, акцента, DWM и контрастности.
/// Ключи и события живут до выхода процесса.
pub(super) fn watch(ctx: egui::Context, sink: mpsc::Sender<Look>) {
    let spawned = std::thread::Builder::new()
        .name("sys-watch".into())
        .spawn(move || {
            let mut last = look();
            wait_changes(&[PERSONALIZE, ACCENT, DWM, HIGH_CONTRAST], || {
                let now = look();
                if now == last {
                    return true;
                }
                last = now;
                tracing::info!(?now, "system look changed");
                let alive = sink.send(now).is_ok();
                ctx.request_repaint();
                alive
            });
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "sys-watch thread");
    }
}

/// Ждёт изменений значений ключей HKCU и зовёт `changed` после каждого (с паузой 50 мс:
/// «Параметры» пишут несколько значений подряд). `changed` → `false` — выход.
fn wait_changes(keys: &[&str], mut changed: impl FnMut() -> bool) {
    let watched: Vec<Watched> = keys.iter().filter_map(|k| Watched::open(k)).collect();
    if watched.is_empty() {
        return;
    }
    let events: Vec<HANDLE> = watched.iter().map(|w| w.event).collect();
    let count = u32::try_from(events.len()).expect("few keys");
    loop {
        // SAFETY: массив живых событий; ждём любое без таймаута.
        let rc = unsafe { WaitForMultipleObjects(count, events.as_ptr(), 0, INFINITE) };
        let Some(w) = rc
            .checked_sub(WAIT_OBJECT_0)
            .and_then(|i| watched.get(i as usize))
        else {
            tracing::warn!(rc, "sys-watch: wait failed");
            return;
        };
        w.arm();
        std::thread::sleep(Duration::from_millis(50));
        if !changed() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_falls_back_to_default_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.ttf"), b"").unwrap();
        assert_eq!(
            font(dir.path(), "No Such Font (TrueType)", "x.ttf"),
            Some(dir.path().join("x.ttf"))
        );
        assert_eq!(font(dir.path(), "No Such Font (TrueType)", "y.ttf"), None);
    }

    /// Подписка на изменения — на своём ключе, настройки ОС не трогаются.
    #[test]
    fn notices_registry_change() {
        const KEY: &str = r"Software\FileNavTest\Watch";
        let reg = |args: &[&str]| {
            std::process::Command::new("reg")
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        };
        let full = format!(r"HKCU\{KEY}");
        assert!(reg(&[
            "add",
            &full,
            "/v",
            "x",
            "/t",
            "REG_DWORD",
            "/d",
            "0",
            "/f"
        ]));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            wait_changes(&[KEY], || tx.send(()).is_ok());
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(reg(&[
            "add",
            &full,
            "/v",
            "x",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f"
        ]));
        let got = rx.recv_timeout(Duration::from_secs(1));
        reg(&["delete", r"HKCU\Software\FileNavTest", "/f"]);
        assert!(got.is_ok(), "no change notification within 1 s");
    }

    #[test]
    fn registry_reads() {
        assert!(reg_string(HKEY_LOCAL_MACHINE, VERSION, "CurrentBuildNumber").is_some());
        assert_eq!(
            reg_dword(HKEY_CURRENT_USER, PERSONALIZE, "NoSuchValue"),
            None
        );
        // Segoe UI есть в любой Windows 10/11 и Server.
        assert!(font(&fonts_dir(), "Segoe UI (TrueType)", "segoeui.ttf").is_some());
    }
}
