//! Секция `[ui]` конфига (S2 §4). Читается через `Config::section::<UiCfg>("ui")`;
//! неверная секция целиком — дефолт и предупреждение core, неверные значения токенов
//! и раскраски — предупреждения [`crate::tokens::Tokens::build`] и
//! [`crate::highlight::Highlighter::build`].

use std::collections::BTreeMap;

use serde::Deserialize;

/// Плотность строк и контролов.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    /// 24 px.
    #[default]
    Compact,
    /// 32 px.
    Standard,
}

/// Переопределения цветов `[ui.tokens.light]` / `[ui.tokens.dark]`: роль → `#RRGGBB`.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct TokenOverrides {
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
}

/// Правило раскраски `[[ui.colors]]` (S0-decisions №32; в S2 §4 — `[[ui.highlight]]`).
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct ColorRule {
    /// Маска имени (`*.rs`, `*.rs;*.toml`).
    pub mask: Option<String>,
    /// Все атрибуты должны быть: `hidden`, `system`, `readonly`, `exec`, `offline`, …
    pub attrs: Vec<String>,
    /// `dir`, `file`, `link`, `broken`.
    pub kind: Option<String>,
    /// `#RRGGBB` или роль `@accent.text`.
    pub fg: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct UiCfg {
    /// `system | light | dark`.
    pub theme: String,
    /// `system | "#RRGGBB"`.
    pub accent: String,
    /// `auto | solid` (S2-06).
    pub backdrop: String,
    /// `system | путь` (S2-04).
    pub font_ui: String,
    pub font_ui_size: f32,
    /// `jetbrains-mono | путь` (S2-04).
    pub font_mono: String,
    pub font_mono_size: f32,
    pub fallback_fonts: Vec<String>,
    /// `fluent | none` (S2-04).
    pub icons: String,
    pub density: Density,
    /// `auto | iso` (S2-08).
    pub date_format: String,
    pub keybar: bool,
    pub animations: bool,
    pub tokens: TokenOverrides,
    #[serde(alias = "highlight")]
    pub colors: Vec<ColorRule>,
}

impl Default for UiCfg {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            accent: "system".into(),
            backdrop: "auto".into(),
            font_ui: "system".into(),
            font_ui_size: 14.0,
            font_mono: "jetbrains-mono".into(),
            font_mono_size: 13.0,
            fallback_fonts: Vec::new(),
            icons: "fluent".into(),
            density: Density::Compact,
            date_format: "auto".into(),
            keybar: true,
            animations: false,
            tokens: TokenOverrides::default(),
            colors: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_example() {
        let text = r##"
theme = "dark"
accent = "#E81123"
density = "standard"
animations = true
[tokens.light]
"accent.fill" = "#0067C0"
[[highlight]]
mask = "*.rs"
fg = "@accent.text"
"##;
        let cfg: UiCfg = toml::from_str(text).unwrap();
        assert_eq!(cfg.theme, "dark");
        assert_eq!(cfg.density, Density::Standard);
        assert_eq!(cfg.tokens.light["accent.fill"], "#0067C0");
        assert_eq!(cfg.colors.len(), 1, "`highlight` — синоним `colors`");
        assert_eq!(cfg.font_ui_size, 14.0);
    }
}
