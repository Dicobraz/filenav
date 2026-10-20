use egui::{Color32, Theme};

use super::*;
use crate::legacy::sys::{HcColors, Look};

fn probe(accent: AccentPalette) -> SysProbe {
    SysProbe {
        look: Look {
            accent,
            ..Look::default()
        },
        ..SysProbe::default()
    }
}

/// Акценты Windows из «Параметров» (верхний ряд по тонам + серый): синий по умолчанию,
/// жёлтое золото, оранжевый, красный, фиолетовый, бирюзовый, зелёный, серый.
const ACCENTS: [u32; 8] = [
    0x00_78D4, 0xFF_B900, 0xF7_630C, 0xE8_1123, 0x88_1798, 0x00_99BC, 0x10_893E, 0x76_7676,
];

fn rgb(v: u32) -> Color32 {
    let [_, r, g, b] = v.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

#[test]
fn every_role_resolves_in_both_themes() {
    let t = Tokens::default();
    for theme in [Theme::Light, Theme::Dark] {
        let p = t.palette(theme);
        for r in Role::ALL {
            assert_ne!(p.raw(*r)[3], 0, "{theme:?} {}", r.name());
        }
    }
    for r in Role::ALL {
        assert_eq!(Role::from_name(r.name()), Some(*r));
    }
}

#[test]
fn contrast_for_system_accents() {
    for v in ACCENTS {
        let (t, _) = Tokens::build(&probe(AccentPalette::from_base(rgb(v))), &UiCfg::default());
        for theme in [Theme::Light, Theme::Dark] {
            let p = t.palette(theme);
            let panel = p.get(Role::LayerPanel);
            let fill = p.get(Role::AccentFill).to_opaque();
            let checks = [
                ("accent.on/accent.fill", p.get(Role::AccentOn), fill),
                (
                    "text.primary/layer.panel",
                    over(p.get(Role::TextPrimary), panel),
                    panel,
                ),
                (
                    "accent.text/accent.subtle",
                    p.get(Role::AccentText),
                    over(p.get(Role::AccentSubtle), panel),
                ),
                (
                    "text.primary/bg.base",
                    over(p.get(Role::TextPrimary), p.get(Role::BgBase)),
                    p.get(Role::BgBase),
                ),
            ];
            for (what, fg, bg) in checks {
                let k = contrast(fg, bg);
                assert!(k >= MIN_CONTRAST, "#{v:06X} {theme:?} {what}: {k:.2}");
            }
        }
    }
}

#[test]
fn overrides_and_bad_values() {
    let mut cfg = UiCfg::default();
    cfg.tokens
        .light
        .insert("accent.fill".into(), "#0067C0".into());
    cfg.tokens
        .light
        .insert("layer.panel".into(), "#80FFFFFF".into());
    cfg.tokens.dark.insert("no.such".into(), "#000000".into());
    cfg.tokens
        .dark
        .insert("text.primary".into(), "white".into());
    cfg.accent = "#E81123".into();
    let (t, warnings) = Tokens::build(&SysProbe::default(), &cfg);
    let light = t.palette(Theme::Light);
    assert_eq!(
        light.get(Role::AccentFill),
        Color32::from_rgb(0x00, 0x67, 0xC0)
    );
    assert_eq!(
        light.get(Role::LayerPanel),
        Color32::from_rgba_unmultiplied(255, 255, 255, 0x80)
    );
    // Своя база акцента: остальные акцентные роли — от неё.
    assert_ne!(
        light.get(Role::AccentText),
        Tokens::default()
            .palette(Theme::Light)
            .get(Role::AccentText)
    );
    let keys: Vec<_> = warnings
        .iter()
        .map(|w| (w.key.as_str(), w.msg.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            ("ui.tokens.dark", "unknown token \"no.such\""),
            ("ui.tokens.dark", "text.primary: bad color \"white\""),
        ]
    );
    cfg.accent = "blue".into();
    let (_, warnings) = Tokens::build(&SysProbe::default(), &cfg);
    assert_eq!(warnings[0].key, "ui.accent");
}

#[test]
fn density_and_font_sizes() {
    let cfg = UiCfg {
        density: crate::legacy::config::Density::Standard,
        font_ui_size: 16.0,
        ..UiCfg::default()
    };
    let (t, _) = Tokens::build(&SysProbe::default(), &cfg);
    assert_eq!(t.m(Metric::HRow), 32.0);
    assert_eq!(t.m(Metric::HControl), 32.0);
    assert_eq!(t.m(Metric::TypeBody), 16.0);
    assert_eq!(t.m(Metric::TypeCaption), 14.0);
    assert_eq!(Tokens::default().m(Metric::HRow), 24.0);
}

#[test]
fn high_contrast_uses_system_colors() {
    let hc = HcColors {
        window: Color32::BLACK,
        window_text: Color32::WHITE,
        highlight: Color32::from_rgb(0x1A, 0xEB, 0xFF),
        highlight_text: Color32::BLACK,
        gray_text: Color32::from_rgb(0x3F, 0xF2, 0x3F),
        hotlight: Color32::from_rgb(0xFF, 0xFF, 0x00),
    };
    let mut p = SysProbe::default();
    p.look.high_contrast = true;
    p.look.hc = Some(hc);
    let mut cfg = UiCfg::default();
    cfg.tokens
        .light
        .insert("text.primary".into(), "#123456".into());
    let (t, _) = Tokens::build(&p, &cfg);
    assert!(t.high_contrast);
    for theme in [Theme::Light, Theme::Dark] {
        let pal = t.palette(theme);
        assert_eq!(
            pal.get(Role::TextPrimary),
            Color32::WHITE,
            "переопределения не действуют"
        );
        assert_eq!(pal.get(Role::LayerPanel), Color32::BLACK);
        assert_eq!(pal.get(Role::AccentFill), hc.highlight);
        assert_eq!(pal.get(Role::AccentOn), Color32::BLACK);
        for r in Role::ALL {
            let a = pal.get(*r).a();
            assert!(a == 0 || a == 255, "без полупрозрачности: {}", r.name());
        }
    }
}

/// Таблица токенов для акцента `#0078D4` (S2-05: сверка с WinUI фиксируется снимком).
#[test]
fn token_table_snapshot() {
    let (t, _) = Tokens::build(
        &probe(AccentPalette::from_base(rgb(0x00_78D4))),
        &UiCfg::default(),
    );
    insta::assert_snapshot!(table(&t));
}

#[test]
fn style_maps_tokens() {
    let t = Tokens::default();
    for theme in [Theme::Light, Theme::Dark] {
        let s = style::style(&t, theme);
        let p = t.palette(theme);
        assert_eq!(s.visuals.selection.bg_fill, p.get(Role::AccentFill));
        assert_eq!(s.visuals.panel_fill, p.get(Role::BgBase));
        assert_eq!(s.visuals.window_corner_radius, egui::CornerRadius::same(8));
        assert_eq!(
            s.visuals.widgets.inactive.corner_radius,
            egui::CornerRadius::same(4)
        );
        assert_eq!(s.visuals.dark_mode, theme == Theme::Dark);
        assert_eq!(s.animation_time, 0.0);
        assert_eq!(s.text_styles[&egui::TextStyle::Body].size, 14.0);
    }
}
