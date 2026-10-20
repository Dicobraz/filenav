//! Токены Fluent (S2 §1.2–1.3): цвета ролей для светлой и тёмной темы, метрики,
//! переопределения `[ui.tokens.*]`, режим высокой контрастности. Стиль egui из токенов —
//! [`style`].

#![allow(
    clippy::many_single_char_names,
    reason = "цветовые формулы: r, g, b, a"
)]

mod fluent;

use std::fmt::Write as _;

use crate::color::{Color32, Theme};
use urfin_core::ConfigWarning;

use crate::accent::AccentPalette;
use crate::config::{Density, UiCfg};
use crate::sys::SysProbe;

macro_rules! roles {
    ($($role:ident = $name:literal,)+) => {
        /// Цветовая роль (S2 §1.3). Имя — ключ `[ui.tokens.light|dark]` и `@роль` в раскраске.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Role { $($role,)+ }

        impl Role {
            pub const ALL: &[Role] = &[$(Role::$role,)+];
            pub const COUNT: usize = Role::ALL.len();

            pub fn name(self) -> &'static str {
                match self { $(Role::$role => $name,)+ }
            }

            pub fn from_name(name: &str) -> Option<Role> {
                match name { $($name => Some(Role::$role),)+ _ => None }
            }
        }
    };
}

roles! {
    AccentFill = "accent.fill",
    AccentFillHover = "accent.fill.hover",
    AccentFillPressed = "accent.fill.pressed",
    AccentText = "accent.text",
    AccentOn = "accent.on",
    AccentSubtle = "accent.subtle",
    TextPrimary = "text.primary",
    TextSecondary = "text.secondary",
    TextTertiary = "text.tertiary",
    TextDisabled = "text.disabled",
    BgBase = "bg.base",
    BgBaseAlt = "bg.base.alt",
    LayerPanel = "layer.panel",
    LayerOverlay = "layer.overlay",
    FillSubtleHover = "fill.subtle.hover",
    FillSubtlePressed = "fill.subtle.pressed",
    FillCursor = "fill.cursor",
    FillCursorPassive = "fill.cursor.passive",
    FillControl = "fill.control",
    FillControlInput = "fill.control.input",
    StrokeCard = "stroke.card",
    StrokeControl = "stroke.control",
    StrokeControlBottom = "stroke.control.bottom",
    StrokeStrong = "stroke.strong",
    StrokeDivider = "stroke.divider",
    ShadowOverlay = "shadow.overlay",
    CriticalFg = "critical.fg",
    CriticalBg = "critical.bg",
    CautionFg = "caution.fg",
    CautionBg = "caution.bg",
    SuccessFg = "success.fg",
    SuccessBg = "success.bg",
    InfoBg = "info.bg",
    TypeArchive = "type.archive",
    TypeExec = "type.exec",
    TypeTemp = "type.temp",
    TypeMuted = "type.muted",
}

macro_rules! metrics {
    ($($m:ident = $name:literal : $v:expr,)+) => {
        /// Размер в логических px (S2 §1.1–1.2).
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Metric { $($m,)+ }

        impl Metric {
            pub const ALL: &[Metric] = &[$(Metric::$m,)+];
            pub const COUNT: usize = Metric::ALL.len();

            pub fn name(self) -> &'static str {
                match self { $(Metric::$m => $name,)+ }
            }

            /// Значение по умолчанию (плотность compact).
            const fn default_value(self) -> f32 {
                match self { $(Metric::$m => $v,)+ }
            }
        }
    };
}

metrics! {
    SpaceXxs = "space.xxs": 2.0,
    SpaceXs = "space.xs": 4.0,
    SpaceS = "space.s": 8.0,
    SpaceM = "space.m": 12.0,
    SpaceL = "space.l": 16.0,
    SpaceXl = "space.xl": 20.0,
    SpaceXxl = "space.xxl": 24.0,
    RadiusControl = "radius.control": 4.0,
    RadiusOverlay = "radius.overlay": 8.0,
    StrokeThin = "stroke.thin": 1.0,
    FocusOuter = "focus.outer": 2.0,
    FocusInner = "focus.inner": 1.0,
    HTitlebar = "h.titlebar": 40.0,
    HTab = "h.tab": 32.0,
    HBreadcrumb = "h.breadcrumb": 32.0,
    HColHeader = "h.col_header": 28.0,
    HRow = "h.row": 24.0,
    HPanelFooter = "h.panel_footer": 28.0,
    HJobRow = "h.job_row": 32.0,
    HBanner = "h.banner": 48.0,
    HCmdline = "h.cmdline": 32.0,
    HKeybar = "h.keybar": 24.0,
    HControl = "h.control": 24.0,
    HMenuItem = "h.menu_item": 28.0,
    WIcon = "w.icon": 16.0,
    WPill = "w.pill": 3.0,
    WAccentBar = "w.accent_bar": 3.0,
    GapPanels = "gap.panels": 8.0,
    PadRowX = "pad.row_x": 12.0,
    TypeCaption = "type.caption": 12.0,
    TypeBody = "type.body": 14.0,
    TypeSubtitle = "type.subtitle": 20.0,
    TypeMono = "type.mono": 13.0,
}

/// sRGBA без премультипликации: `Color32` хранит премультиплицированные значения, и
/// полупрозрачные токены WinUI при обратном чтении теряли бы точность.
pub type Rgba = [u8; 4];

/// Цвета всех ролей одной темы.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette([Rgba; Role::COUNT]);

impl Palette {
    pub fn get(&self, r: Role) -> Color32 {
        let [red, g, b, a] = self.0[r as usize];
        Color32::from_rgba_unmultiplied(red, g, b, a)
    }

    /// Значение как задано (для таблиц и сверки).
    pub fn raw(&self, r: Role) -> Rgba {
        self.0[r as usize]
    }

    fn set(&mut self, r: Role, c: Rgba) {
        self.0[r as usize] = c;
    }
}

/// Токены окна: палитры обеих тем и метрики.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    palettes: [Palette; 2],
    metrics: [f32; Metric::COUNT],
    theme: Theme,
    pub high_contrast: bool,
    /// Анимации egui (`ui.animations`).
    pub animations: bool,
}

/// Минимальный контраст текста (WCAG AA).
pub const MIN_CONTRAST: f32 = 4.5;

impl Tokens {
    /// Токены из probe ОС и `[ui]`. Ошибки конфига — предупреждения, берётся дефолт.
    pub fn build(probe: &SysProbe, cfg: &UiCfg) -> (Tokens, Vec<ConfigWarning>) {
        let mut warnings = Vec::new();
        let accent = match cfg.accent.trim() {
            a if a.is_empty() || a.eq_ignore_ascii_case("system") => probe.look.accent,
            a => {
                if let Some(c) = parse_color(a) {
                    AccentPalette::from_base(c.to_opaque())
                } else {
                    warnings.push(warn("ui.accent", format!("bad color {a:?}")));
                    probe.look.accent
                }
            }
        };
        let hc = probe.look.high_contrast.then_some(probe.look.hc).flatten();
        let palettes = match hc {
            Some(hc) => {
                let p = fluent::high_contrast(&hc);
                [p.clone(), p]
            }
            None => [
                palette(Theme::Light, &accent, &cfg.tokens.light, &mut warnings),
                palette(Theme::Dark, &accent, &cfg.tokens.dark, &mut warnings),
            ],
        };
        let mut metrics: [f32; Metric::COUNT] =
            std::array::from_fn(|i| Metric::ALL[i].default_value());
        if cfg.density == Density::Standard {
            metrics[Metric::HRow as usize] = 32.0;
            metrics[Metric::HControl as usize] = 32.0;
        }
        let body = cfg.font_ui_size.clamp(8.0, 32.0);
        metrics[Metric::TypeBody as usize] = body;
        metrics[Metric::TypeCaption as usize] = body - 2.0;
        metrics[Metric::TypeSubtitle as usize] = body + 6.0;
        metrics[Metric::TypeMono as usize] = cfg.font_mono_size.clamp(8.0, 32.0);
        let tokens = Tokens {
            palettes,
            metrics,
            theme: probe.look.theme.unwrap_or(Theme::Light),
            high_contrast: hc.is_some(),
            animations: cfg.animations,
        };
        (tokens, warnings)
    }

    /// Цвет роли в текущей теме.
    pub fn c(&self, r: Role) -> Color32 {
        self.palette(self.theme).get(r)
    }

    pub fn m(&self, m: Metric) -> f32 {
        self.metrics[m as usize]
    }

    /// Радиус скругления из метрики (0…255 px).
    #[allow(clippy::cast_sign_loss, reason = "значение зажато в 0..=255")]
    pub fn palette(&self, theme: Theme) -> &Palette {
        &self.palettes[usize::from(theme == Theme::Dark)]
    }

    /// Mica за базовым слоем (S2 §1.4): `bg.base` и `bg.base.alt` прозрачные — сквозь
    /// них виден материал окна; карточки остаются непрозрачными (S0-decisions №33).
    pub fn set_mica(&mut self, on: bool) {
        if !on || self.high_contrast {
            return;
        }
        for p in &mut self.palettes {
            p.set(Role::BgBase, [0; 4]);
            p.set(Role::BgBaseAlt, [0; 4]);
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Self::build(&SysProbe::default(), &UiCfg::default()).0
    }
}

fn warn(key: &str, msg: String) -> ConfigWarning {
    ConfigWarning {
        key: key.to_owned(),
        msg,
    }
}

/// Палитра темы: нейтральные WinUI, акцент с гарантией контраста, переопределения.
fn palette(
    theme: Theme,
    accent: &AccentPalette,
    overrides: &std::collections::BTreeMap<String, String>,
    warnings: &mut Vec<ConfigWarning>,
) -> Palette {
    let mut p = fluent::neutral(theme);
    let dark = theme == Theme::Dark;
    let (fill, text) = if dark {
        (accent.light2, accent.light3)
    } else {
        (accent.dark1, accent.dark2)
    };
    let subtle = if dark {
        with_alpha(accent.light2, 0.16)
    } else {
        with_alpha(accent.base, 0.12)
    };
    p.set(Role::AccentFill, rgba(fill));
    p.set(Role::AccentFillHover, with_alpha(fill, 0.9));
    p.set(Role::AccentFillPressed, with_alpha(fill, 0.8));
    p.set(Role::AccentOn, rgba(best_on(fill)));
    p.set(Role::AccentSubtle, subtle);
    // Текст-акцент помеченных строк — на `accent.subtle` поверх карточки (S0-decisions №31).
    let marked_bg = over(p.get(Role::AccentSubtle), p.get(Role::LayerPanel));
    p.set(
        Role::AccentText,
        rgba(ensure_contrast(text, marked_bg, dark)),
    );
    let section = if dark {
        "ui.tokens.dark"
    } else {
        "ui.tokens.light"
    };
    for (name, value) in overrides {
        let Some(role) = Role::from_name(name) else {
            warnings.push(warn(section, format!("unknown token {name:?}")));
            continue;
        };
        match parse_rgba(value) {
            Some(c) => p.set(role, c),
            None => warnings.push(warn(section, format!("{name}: bad color {value:?}"))),
        }
    }
    p
}

/// `#RRGGBB` или `#AARRGGBB`.
pub fn parse_color(s: &str) -> Option<Color32> {
    parse_rgba(s).map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
}

fn parse_rgba(s: &str) -> Option<Rgba> {
    let hex = s.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(argb(0xFF00_0000 | v)),
        8 => Some(argb(v)),
        _ => None,
    }
}

/// `0xAARRGGBB` → sRGBA.
pub(crate) const fn argb(v: u32) -> Rgba {
    let [a, r, g, b] = v.to_be_bytes();
    [r, g, b, a]
}

/// Непрозрачный цвет → sRGBA.
fn rgba(c: Color32) -> Rgba {
    c.to_srgba_unmultiplied()
}

/// Непрозрачный цвет с долей непрозрачности.
#[allow(clippy::cast_sign_loss, reason = "доля зажата в 0..=1")]
fn with_alpha(c: Color32, alpha: f32) -> Rgba {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    [r, g, b, (255.0 * alpha.clamp(0.0, 1.0)).round() as u8]
}

/// `fg` поверх непрозрачного `bg` (альфа-смешение в sRGB, как у композитора).
#[allow(clippy::cast_sign_loss, reason = "значения 0..=255")]
pub fn over(fg: Color32, bg: Color32) -> Color32 {
    let [r, g, b, a] = fg.to_srgba_unmultiplied();
    let k = f32::from(a) / 255.0;
    let mix = |f: u8, b: u8| (f32::from(f) * k + f32::from(b) * (1.0 - k)).round() as u8;
    Color32::from_rgb(mix(r, bg.r()), mix(g, bg.g()), mix(b, bg.b()))
}

/// Относительная яркость WCAG.
fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Контраст WCAG двух непрозрачных цветов.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Текст на акценте: белый или чёрный — где контраст выше (минимум ≈ 4,58 для любого цвета).
fn best_on(fill: Color32) -> Color32 {
    let fill = fill.to_opaque();
    if contrast(Color32::WHITE, fill) >= contrast(Color32::BLACK, fill) {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

/// Сдвигает `fg` к тёмному (светлая тема) или светлому (тёмная), пока контраст с `bg`
/// не достигнет [`MIN_CONTRAST`].
fn ensure_contrast(fg: Color32, bg: Color32, dark: bool) -> Color32 {
    let target = if dark { Color32::WHITE } else { Color32::BLACK };
    let mut c = fg;
    for step in 1..=20u8 {
        if contrast(c, bg) >= MIN_CONTRAST {
            break;
        }
        c = lerp(fg, target, f32::from(step) / 20.0);
    }
    c
}

#[allow(clippy::cast_sign_loss, reason = "значения 0..=255")]
fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Таблица `роль  light  dark` (insta, документация).
pub fn table(t: &Tokens) -> String {
    let hex = |[r, g, b, a]: Rgba| {
        if a == 255 {
            format!("#{r:02X}{g:02X}{b:02X}")
        } else {
            format!("#{a:02X}{r:02X}{g:02X}{b:02X}")
        }
    };
    let mut out = String::new();
    for r in Role::ALL {
        let _ = writeln!(
            out,
            "{:<22} {:<10} {}",
            r.name(),
            hex(t.palette(Theme::Light).raw(*r)),
            hex(t.palette(Theme::Dark).raw(*r))
        );
    }
    for m in Metric::ALL {
        let _ = writeln!(out, "{:<22} {}", m.name(), t.m(*m));
    }
    out
}

#[cfg(test)]
mod tests;
