//! Снапшоты целых экранов S2 §2 (S2-21): `UrfinUi::show` целиком, 1280×768,
//! светлая и тёмная тема, сценарии — [`crate::legacy::screens`]. Кадр egui сводится в
//! YAML: тексты с цветом и позицией, заливки прямоугольников с цветом (в порядке
//! рисования). Шрифты — встроенные egui, язык — ru, пояс — UTC: снапшот одинаков
//! на всех машинах.

use std::fmt::Write as _;

use egui::epaint::{ClippedShape, Shape};
use egui::{Color32, Rect, pos2, vec2};
use urfin_core::{AppState, UiEvent};

use crate::legacy::{FontSet, UiCfg, UrfinUi, screens, sys};

/// Цвет как `#rrggbbaa`.
fn hex(c: Color32) -> String {
    let [red, green, blue, alpha] = c.to_srgba_unmultiplied();
    format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
}

fn rect(r: Rect) -> String {
    format!(
        "[{:.0}, {:.0}, {:.0}, {:.0}]",
        r.min.x, r.min.y, r.max.x, r.max.y
    )
}

fn dump(shape: &Shape, out: &mut String) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| dump(s, out)),
        Shape::Text(t) => {
            let text = t.galley.text();
            if text.trim().is_empty() {
                return;
            }
            let color = t
                .galley
                .job
                .sections
                .first()
                .map_or(t.fallback_color, |s| s.format.color);
            let color = match t.override_text_color {
                Some(c) => c,
                None if color == Color32::PLACEHOLDER => t.fallback_color,
                None => color,
            };
            let _ = writeln!(
                out,
                "- text {:?} at ({:.0}, {:.0}) {}",
                text,
                t.pos.x,
                t.pos.y,
                hex(color)
            );
        }
        Shape::Rect(r) if r.fill != Color32::TRANSPARENT => {
            let _ = writeln!(out, "- fill {} {}", rect(r.rect), hex(r.fill));
        }
        Shape::Rect(r) if r.stroke.width > 0.0 => {
            let _ = writeln!(
                out,
                "- stroke {} {} {:.0}",
                rect(r.rect),
                hex(r.stroke.color),
                r.stroke.width
            );
        }
        Shape::Mesh(m) => {
            let _ = writeln!(out, "- mesh {}", rect(m.calc_bounds()));
        }
        Shape::Circle(c) if c.fill != Color32::TRANSPARENT => {
            let _ = writeln!(
                out,
                "- circle ({:.0}, {:.0}) r{:.0} {}",
                c.center.x,
                c.center.y,
                c.radius,
                hex(c.fill)
            );
        }
        _ => {}
    }
}

/// Кадр `UrfinUi::show` → YAML. Рисуется дважды (раскладка ячеек и шрифтов
/// установится); `events` — в первый кадр.
fn render(s: &AppState, events: &[UiEvent]) -> String {
    let _ = urfin_i18n::set_language("ru");
    let cfg = UiCfg::default();
    let reg = crate::legacy::tests::registry();
    let mut ui = UrfinUi::new(&reg, &sys::SysProbe::default(), &cfg, FontSet::default());
    ui.set_zone(sys::Zone::utc());
    let ctx = egui::Context::default();
    let mut out = Vec::new();
    let mut shapes: Vec<ClippedShape> = Vec::new();
    for frame in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 768.0))),
            ..Default::default()
        };
        let ev = if frame == 0 { events } else { &[] };
        let mut full = ctx.run_ui(input, |root| ui.show(root, s, ev, &mut out));
        full.textures_delta.clear();
        shapes = full.shapes;
    }
    let mut y = String::new();
    for c in &shapes {
        dump(&c.shape, &mut y);
    }
    y
}

/// Все экраны §2 в обеих темах. Только Windows: пути экранов (`C:\…` в
/// breadcrumb и дереве) разбираются по правилам ОС; на Linux/macOS — свои
/// снапшоты компонентов с путями без букв дисков.
#[test]
fn screens_match_snapshots() {
    for mut sc in screens::all() {
        for dark in [false, true] {
            sc.set_theme(dark);
            let y = render(&sc.state, &sc.events);
            let theme = if dark { "dark" } else { "light" };
            for n in sc.needles {
                assert!(y.contains(n), "{}/{theme}: no {n:?}\n{y}", sc.name);
            }
            insta::assert_snapshot!(format!("{}_{theme}", sc.name), y);
        }
    }
}
