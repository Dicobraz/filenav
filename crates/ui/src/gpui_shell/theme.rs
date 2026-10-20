//! Тема Fluent для GPUI (№56): токены S2 ([`Tokens`], роли и метрики), раскраска файлов
//! ([`Highlighter`]) и формат ячеек ([`FmtCfg`]) — те же, что у egui-окна.

use egui::Color32;
use gpui_kit::{Hsla, Rgba};

use crate::config::UiCfg;
use crate::fmt_cell::FmtCfg;
use crate::highlight::Highlighter;
use crate::sys::{SysProbe, Zone};
use crate::tokens::{Metric, Role, Tokens};

/// Всё, что нужно видам для цвета, размеров и текста ячеек.
pub struct Theme {
    pub tokens: Tokens,
    pub hl: Highlighter,
    pub fmt: FmtCfg,
}

impl std::fmt::Debug for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Theme").finish_non_exhaustive()
    }
}

impl Theme {
    pub fn new(probe: &SysProbe, cfg: &UiCfg) -> Theme {
        let (tokens, _) = Tokens::build(probe, cfg);
        let (hl, _) = Highlighter::build(&cfg.colors);
        let iso = cfg.date_format.eq_ignore_ascii_case("iso");
        let fmt = FmtCfg::new(&urfin_i18n::current(), iso, Zone::system());
        Theme { tokens, hl, fmt }
    }

    /// Цвет роли.
    pub fn c(&self, r: Role) -> Hsla {
        hsla(self.tokens.c(r))
    }

    /// Метрика в px.
    pub fn m(&self, m: Metric) -> f32 {
        self.tokens.m(m)
    }
}

/// `Color32` (sRGB, премультиплицированный) → цвет gpui.
pub fn hsla(color: Color32) -> Hsla {
    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    Rgba {
        r: f32::from(red) / 255.0,
        g: f32::from(green) / 255.0,
        b: f32::from(blue) / 255.0,
        a: f32::from(alpha) / 255.0,
    }
    .into()
}
