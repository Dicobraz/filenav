//! Шрифты окна (S2 §1.1, §5; S0-decisions 2026-10-17 — на Windows только системные).
//!
//! - [`load`] читает файлы (поток `font-probe`, до окна) и [`load_fallbacks`] — после
//!   первого кадра; в кадре I/O нет.
//! - [`install`] ставит семейства: `ui`, `ui-strong`, `ui-caption`, `ui-subtitle` — один
//!   Segoe UI Variable с разными осями `wght`/`opsz` (без осей — Segoe UI + Semibold),
//!   `Monospace` — Cascadia Mono → Consolas, `icons` — Segoe Fluent Icons → MDL2.
//!   В конце каждой цепочки — fallback-шрифты и шрифты egui.
//! - [`FontSet`] — что поставлено; прогрев атласа и проверка tabular-цифр на первом кадре.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::epaint::text::{FontData, FontDefinitions, FontInsert, FontPriority, InsertFontFamily};
use egui::{Color32, FontFamily, FontId};
use urfin_core::ConfigWarning;

use crate::legacy::config::UiCfg;
use crate::legacy::sys::FontFiles;

pub const UI: &str = "ui";
pub const UI_STRONG: &str = "ui-strong";
pub const UI_CAPTION: &str = "ui-caption";
pub const UI_SUBTITLE: &str = "ui-subtitle";
pub const ICONS: &str = "icons";

/// Роль шрифта в интерфейсе (S2 §1.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    Ui,
    Strong,
    Caption,
    Subtitle,
    Mono,
    Icons,
}

/// Файл шрифта в памяти. Байты живут до конца процесса: на них ссылаются все семейства.
#[derive(Clone, Debug)]
struct Face {
    name: String,
    bytes: &'static [u8],
}

impl Face {
    fn read(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let name = path
            .file_name()
            .map_or_else(|| "font".to_owned(), |n| n.to_string_lossy().into_owned());
        Ok(Self {
            name,
            bytes: Box::leak(bytes.into_boxed_slice()),
        })
    }

    fn data(&self, coords: &[(&str, f32)]) -> FontData {
        let mut d = FontData::from_static(self.bytes);
        if !coords.is_empty() {
            d.tweak.coords = egui::epaint::text::VariationCoords::new(coords.iter().copied());
        }
        d
    }
}

/// Прочитанные файлы для [`install`].
#[derive(Clone, Debug, Default)]
pub struct LoadedFonts {
    ui: Option<Face>,
    /// Статический Semibold, если `ui` без осей.
    ui_strong: Option<Face>,
    mono: Option<Face>,
    icons: Option<Face>,
}

impl LoadedFonts {
    /// UI-шрифт вариативный (`fvar`): роли — осями одного файла.
    fn variable(&self) -> bool {
        self.ui
            .as_ref()
            .is_some_and(|f| has_table(f.bytes, *b"fvar"))
    }
}

/// Есть ли таблица в TrueType/OpenType (не коллекции).
fn has_table(font: &[u8], tag: [u8; 4]) -> bool {
    let Some(n) = font
        .get(4..6)
        .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
    else {
        return false;
    };
    (0..n).any(|i| font.get(12 + i * 16..16 + i * 16) == Some(&tag[..]))
}

/// `system` (и прежнее значение по умолчанию `jetbrains-mono`) — системный шрифт.
fn user_path(v: &str) -> Option<PathBuf> {
    let v = v.trim();
    let system = v.is_empty()
        || v.eq_ignore_ascii_case("system")
        || v.eq_ignore_ascii_case("jetbrains-mono");
    (!system).then(|| PathBuf::from(v))
}

/// Чтение файлов: свой путь из `[ui]`, иначе найденные probe. Только I/O, без egui.
pub fn load(files: &FontFiles, cfg: &UiCfg) -> (LoadedFonts, Vec<ConfigWarning>) {
    let _s = tracing::info_span!("startup.fonts.read").entered();
    let mut warnings = Vec::new();
    let mut read = |key: &str, path: &Path| match Face::read(path) {
        Ok(f) => Some(f),
        Err(e) => {
            warnings.push(ConfigWarning {
                key: key.to_owned(),
                msg: format!("{}: {e}", path.display()),
            });
            None
        }
    };
    let ui = user_path(&cfg.font_ui)
        .and_then(|p| read("ui.font_ui", &p))
        .or_else(|| files.ui.first().and_then(|p| read("ui.fonts", p)));
    let mono = user_path(&cfg.font_mono)
        .and_then(|p| read("ui.font_mono", &p))
        .or_else(|| files.mono.as_ref().and_then(|p| read("ui.fonts", p)));
    let ui_strong = files.ui.get(1).and_then(|p| read("ui.fonts", p));
    let icons = if cfg.icons.eq_ignore_ascii_case("none") {
        None
    } else {
        files.icons.as_ref().and_then(|p| read("ui.fonts", p))
    };
    let loaded = LoadedFonts {
        ui,
        ui_strong,
        mono,
        icons,
    };
    (loaded, warnings)
}

/// Fallback-шрифты (символы, CJK, `[ui] fallback_fonts`): после первого кадра, в фоне.
pub fn load_fallbacks(files: &FontFiles, cfg: &UiCfg) -> (Vec<FallbackFont>, Vec<ConfigWarning>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let paths = files
        .symbols
        .iter()
        .chain(&files.cjk)
        .cloned()
        .chain(cfg.fallback_fonts.iter().map(PathBuf::from));
    for p in paths {
        match Face::read(&p) {
            Ok(face) => out.push(FallbackFont(face)),
            Err(e) => warnings.push(ConfigWarning {
                key: "ui.fallback_fonts".to_owned(),
                msg: format!("{}: {e}", p.display()),
            }),
        }
    }
    (out, warnings)
}

/// Fallback-шрифт, прочитанный в фоне.
#[derive(Clone, Debug)]
pub struct FallbackFont(Face);

/// Что поставлено в контекст.
#[derive(Debug, Default)]
pub struct FontSet {
    /// Свои семейства (`ui*`, `icons`) есть; иначе — шрифты egui.
    installed: bool,
    /// Иконочный шрифт есть.
    pub icons: bool,
    /// UI-шрифт вариативный.
    pub variable: bool,
    /// Цифры `ui` и `ui-caption` одной ширины (проверка на первом кадре).
    tabular: Cell<Option<[bool; 2]>>,
}

/// Семейство, оси `wght`/`opsz` вариативного шрифта, нужен ли Semibold без осей (S2 §1.1).
type RoleFace = (&'static str, &'static [(&'static str, f32)], bool);

const ROLES: &[RoleFace] = &[
    (UI, &[("wght", 400.0), ("opsz", 10.5)], false),
    (UI_STRONG, &[("wght", 600.0), ("opsz", 10.5)], true),
    (UI_CAPTION, &[("wght", 400.0), ("opsz", 8.0)], false),
    (UI_SUBTITLE, &[("wght", 600.0), ("opsz", 20.0)], true),
];

/// Ставит семейства в контекст; вступают в силу со следующего кадра (до первого кадра —
/// с первого).
pub fn install(ctx: &egui::Context, loaded: &LoadedFonts) -> FontSet {
    let _s = tracing::info_span!("startup.fonts").entered();
    let mut defs = FontDefinitions::default();
    let base_prop = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let variable = loaded.variable();
    let add = |defs: &mut FontDefinitions, key: &str, data: FontData| {
        defs.font_data.insert(key.to_owned(), Arc::new(data));
    };
    for &(family, coords, strong) in ROLES {
        let face = match (&loaded.ui, &loaded.ui_strong) {
            (Some(ui), _) if variable => Some(ui.data(coords)),
            (_, Some(semi)) if strong => Some(semi.data(&[])),
            (Some(ui), _) => Some(ui.data(&[])),
            (None, _) => None,
        };
        let mut chain = Vec::new();
        if let Some(data) = face {
            let key = format!("urfin-{family}");
            add(&mut defs, &key, data);
            chain.push(key);
        }
        chain.extend(base_prop.iter().cloned());
        defs.families.insert(FontFamily::Name(family.into()), chain);
    }
    if loaded.ui.is_some() {
        let ui = defs.families[&FontFamily::Name(UI.into())].clone();
        defs.families.insert(FontFamily::Proportional, ui);
    }
    if let Some(mono) = &loaded.mono {
        add(&mut defs, "urfin-mono", mono.data(&[]));
        defs.families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "urfin-mono".to_owned());
    }
    let mut icons = Vec::new();
    if let Some(face) = &loaded.icons {
        add(&mut defs, "urfin-icons", face.data(&[]));
        icons.push("urfin-icons".to_owned());
    }
    icons.extend(base_prop.iter().cloned());
    defs.families.insert(FontFamily::Name(ICONS.into()), icons);
    ctx.set_fonts(defs);
    tracing::info!(
        ui = ?loaded.ui.as_ref().map(|f| &f.name),
        mono = ?loaded.mono.as_ref().map(|f| &f.name),
        icons = ?loaded.icons.as_ref().map(|f| &f.name),
        variable,
        "fonts installed"
    );
    FontSet {
        installed: true,
        icons: loaded.icons.is_some(),
        variable,
        tabular: Cell::new(None),
    }
}

/// Добавить fallback-шрифты в конец всех цепочек (кроме иконок).
pub fn add_fallbacks(ctx: &egui::Context, fonts: &[FallbackFont]) {
    let families = [
        FontFamily::Proportional,
        FontFamily::Monospace,
        FontFamily::Name(UI.into()),
        FontFamily::Name(UI_STRONG.into()),
        FontFamily::Name(UI_CAPTION.into()),
        FontFamily::Name(UI_SUBTITLE.into()),
    ];
    for FallbackFont(face) in fonts {
        ctx.add_font(FontInsert::new(
            &format!("urfin-fallback-{}", face.name),
            face.data(&[]),
            families
                .iter()
                .map(|f| InsertFontFamily {
                    family: f.clone(),
                    priority: FontPriority::Lowest,
                })
                .collect(),
        ));
    }
}

/// Набор для прогрева атласа (S2 §5): ASCII, кириллица, цифры, знаки интерфейса.
const WARM_TEXT: &str = concat!(
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
    "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдеёжзийклмнопрстуфхцчшщъыьэюя",
    "…›˄˅·×—«»№ "
);

impl FontSet {
    /// `FontId` роли; без своих семейств — шрифты egui.
    pub fn id(&self, font: Font, size: f32) -> FontId {
        let family = match (self.installed, font) {
            (_, Font::Mono) => FontFamily::Monospace,
            (false, _) => FontFamily::Proportional,
            (true, Font::Ui) => FontFamily::Name(UI.into()),
            (true, Font::Strong) => FontFamily::Name(UI_STRONG.into()),
            (true, Font::Caption) => FontFamily::Name(UI_CAPTION.into()),
            (true, Font::Subtitle) => FontFamily::Name(UI_SUBTITLE.into()),
            (true, Font::Icons) => FontFamily::Name(ICONS.into()),
        };
        FontId::new(size, family)
    }

    /// Свои семейства поставлены.
    pub fn installed(&self) -> bool {
        self.installed
    }

    /// Первый кадр: прогрев атласа всех ролей и проверка tabular-цифр.
    pub fn warm_up(&self, ctx: &egui::Context, sizes: &[(Font, f32)], icons: &str) {
        if self.tabular.get().is_some() {
            return;
        }
        let _s = tracing::info_span!("startup.fonts.warm").entered();
        ctx.fonts_mut(|f| {
            for &(font, size) in sizes {
                let text = if font == Font::Icons {
                    icons
                } else {
                    WARM_TEXT
                };
                let _ = f.layout_no_wrap(text.to_owned(), self.id(font, size), Color32::WHITE);
            }
        });
        let check = |font| {
            let id = self.id(font, 14.0);
            ctx.fonts_mut(|f| {
                let w: Vec<f32> = ('0'..='9').map(|c| f.glyph_width(&id, c)).collect();
                w.iter().all(|x| (x - w[0]).abs() < 0.01)
            })
        };
        let tab = [check(Font::Ui), check(Font::Caption)];
        tracing::info!(ui = tab[0], caption = tab[1], "tabular digits");
        self.tabular.set(Some(tab));
    }

    /// Цифры шрифта одной ширины (после [`FontSet::warm_up`]; до — `true`).
    pub fn tabular_ok(&self, font: Font) -> bool {
        let [ui, caption] = self.tabular.get().unwrap_or([true, true]);
        if font == Font::Caption { caption } else { ui }
    }

    /// Ширина слота цифры для эмуляции tabular: самая широкая цифра.
    pub fn digit_slot(ctx: &egui::Context, id: &FontId) -> f32 {
        ctx.fonts_mut(|f| {
            ('0'..='9')
                .map(|c| f.glyph_width(id, c))
                .fold(0.0, f32::max)
        })
    }
}

/// Позиции символов при эмуляции tabular: каждая цифра — по центру слота `slot`,
/// остальные — своей ширины. Возвращает смещения начала символов и общую ширину.
pub fn tabular_layout(
    text: &str,
    slot: f32,
    width: impl Fn(char) -> f32,
) -> (Vec<(char, f32)>, f32) {
    let mut x = 0.0;
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        let w = width(c);
        if c.is_ascii_digit() {
            out.push((c, x + (slot - w) / 2.0));
            x += slot;
        } else {
            out.push((c, x));
            x += w;
        }
    }
    (out, x)
}

#[cfg(test)]
mod tests;
