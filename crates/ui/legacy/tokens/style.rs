//! Токены → `egui::Style` (S2 §1.1–1.3): виджеты, обводки, радиусы, выделение, курсор
//! текста, тени, размеры текста, отступы, анимации. Ставится для обеих тем.

#![allow(
    clippy::many_single_char_names,
    reason = "короткие замыкания токенов c, m"
)]

use egui::{
    Color32, FontFamily, FontId, Margin, Shadow, Stroke, Style, TextStyle, Theme, Vec2, Visuals,
};

use super::{Metric, Role, Tokens};

/// Стиль egui для темы.
pub fn style(t: &Tokens, theme: Theme) -> Style {
    let p = t.palette(theme);
    let c = |r: Role| p.get(r);
    let m = |x: Metric| t.m(x);
    let radius = |x: Metric| t.radius(x);
    let thin = m(Metric::StrokeThin);

    let mut style = Style {
        visuals: match theme {
            Theme::Light => Visuals::light(),
            Theme::Dark => Visuals::dark(),
        },
        ..Style::default()
    };
    let v = &mut style.visuals;
    v.override_text_color = None;
    v.weak_text_color = Some(c(Role::TextSecondary));
    v.hyperlink_color = c(Role::AccentText);
    v.panel_fill = c(Role::BgBase);
    v.window_fill = c(Role::LayerOverlay);
    v.window_stroke = Stroke::new(thin, c(Role::StrokeCard));
    v.window_corner_radius = radius(Metric::RadiusOverlay);
    v.menu_corner_radius = radius(Metric::RadiusOverlay);
    v.faint_bg_color = c(Role::FillSubtleHover);
    v.extreme_bg_color = c(Role::LayerPanel);
    v.text_edit_bg_color = Some(c(Role::FillControlInput));
    v.code_bg_color = c(Role::FillControl);
    v.warn_fg_color = c(Role::CautionFg);
    v.error_fg_color = c(Role::CriticalFg);
    let shadow = Shadow {
        offset: [0, 8],
        blur: 16,
        spread: 0,
        color: c(Role::ShadowOverlay),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.selection.bg_fill = c(Role::AccentFill);
    v.selection.stroke = Stroke::new(thin, c(Role::AccentOn));
    v.text_cursor.stroke = Stroke::new(thin.max(1.5), c(Role::AccentFill));
    v.text_cursor.blink = t.animations;

    let control = radius(Metric::RadiusControl);
    let text = c(Role::TextPrimary);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c(Role::LayerPanel);
    w.noninteractive.weak_bg_fill = c(Role::LayerPanel);
    w.noninteractive.bg_stroke = Stroke::new(thin, c(Role::StrokeDivider));
    w.noninteractive.fg_stroke = Stroke::new(thin, text);
    w.noninteractive.corner_radius = control;
    for (wv, fill) in [
        (&mut w.inactive, c(Role::FillControl)),
        (
            &mut w.hovered,
            over_fill(c(Role::FillControl), c(Role::FillSubtleHover)),
        ),
        (
            &mut w.active,
            over_fill(c(Role::FillControl), c(Role::FillSubtlePressed)),
        ),
        (&mut w.open, c(Role::FillControl)),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::new(thin, c(Role::StrokeControl));
        wv.fg_stroke = Stroke::new(thin, text);
        wv.corner_radius = control;
        // Fluent не «раздувает» контрол при наведении.
        wv.expansion = 0.0;
    }

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(m(Metric::SpaceS), m(Metric::SpaceXs));
    s.button_padding = Vec2::new(m(Metric::SpaceM), m(Metric::SpaceXs));
    s.interact_size = Vec2::new(m(Metric::HControl) * 2.0, m(Metric::HControl));
    s.window_margin = Margin::same(m(Metric::SpaceM) as i8);
    s.menu_margin = Margin::same(m(Metric::SpaceXs) as i8);

    let (body, caption, subtitle, mono) = (
        m(Metric::TypeBody),
        m(Metric::TypeCaption),
        m(Metric::TypeSubtitle),
        m(Metric::TypeMono),
    );
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(caption, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(body, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(body, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(subtitle, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(mono, FontFamily::Monospace),
        ),
    ]
    .into();
    style.animation_time = if t.animations { 0.083 } else { 0.0 };
    style
}

/// Полупрозрачная заливка наведения поверх заливки контрола (обе могут быть с альфой):
/// упрощённо — более плотная из двух альф, цвет наведения.
fn over_fill(base: Color32, hover: Color32) -> Color32 {
    if hover.a() == 0 {
        return base;
    }
    let [r, g, b, a] = base.to_srgba_unmultiplied();
    let [hr, hg, hb, ha] = hover.to_srgba_unmultiplied();
    let k = f32::from(ha) / 255.0;
    #[allow(clippy::cast_sign_loss, reason = "значения 0..=255")]
    let mix = |x: u8, y: u8| (f32::from(x) * (1.0 - k) + f32::from(y) * k).round() as u8;
    Color32::from_rgba_unmultiplied(mix(r, hr), mix(g, hg), mix(b, hb), a.max(ha))
}

/// Ставит стиль обеих тем в контекст; размеры текста — в семействах `fonts`.
pub fn apply(ctx: &egui::Context, t: &Tokens, fonts: &crate::legacy::fonts::FontSet) {
    use crate::legacy::fonts::Font;
    for theme in [Theme::Light, Theme::Dark] {
        let mut s = style(t, theme);
        s.text_styles = [
            (
                TextStyle::Small,
                fonts.id(Font::Caption, t.m(Metric::TypeCaption)),
            ),
            (TextStyle::Body, fonts.id(Font::Ui, t.m(Metric::TypeBody))),
            (TextStyle::Button, fonts.id(Font::Ui, t.m(Metric::TypeBody))),
            (
                TextStyle::Heading,
                fonts.id(Font::Subtitle, t.m(Metric::TypeSubtitle)),
            ),
            (
                TextStyle::Monospace,
                fonts.id(Font::Mono, t.m(Metric::TypeMono)),
            ),
        ]
        .into();
        ctx.set_style_of(theme, s);
    }
}
