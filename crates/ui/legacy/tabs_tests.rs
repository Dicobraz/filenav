use egui::{Pos2, Rect, pos2, vec2};
use urfin_core::{Command, Location, Panel, Side, Tab, TabId};

use super::*;
use crate::legacy::canvas::RecordCanvas;
use crate::legacy::fonts::FontSet;
use crate::legacy::tokens::Tokens;

fn loc(path: &str) -> Location {
    Location {
        backend: "test".into(),
        path: path.into(),
        outer: None,
    }
}

fn panel(paths: &[&str], active: usize) -> Panel {
    Panel {
        tabs: paths
            .iter()
            .enumerate()
            .map(|(i, p)| Tab::new(TabId(u32::try_from(i).unwrap() + 1), loc(p)))
            .collect(),
        active,
        visible: true,
    }
}

/// Полоса табов 600×40.
const AREA: Rect = Rect {
    min: Pos2 { x: 8.0, y: 0.0 },
    max: Pos2 { x: 608.0, y: 40.0 },
};

struct Fx {
    ctx: egui::Context,
    i18n: urfin_i18n::I18n,
    tokens: Tokens,
    fonts: FontSet,
    memo: TabsMemo,
}

impl Fx {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            i18n: urfin_i18n::Catalog::embedded()
                .build(&"ru".parse().unwrap())
                .0,
            tokens: Tokens::default(),
            fonts: FontSet::default(),
            memo: TabsMemo::default(),
        }
    }

    fn frame(&mut self, p: &Panel, events: Vec<egui::Event>) -> (String, Vec<Command>, Vec<Rect>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let (mut yaml, mut out, mut hit) = (String::new(), Vec::new(), Vec::new());
        let ctx = self.ctx.clone();
        let mut full = ctx.run_ui(input, |root| {
            let mut c = RecordCanvas::new(&self.i18n);
            let mut cx = TabsCx {
                ui: root,
                tokens: &self.tokens,
                fonts: &self.fonts,
                memo: &mut self.memo,
            };
            show(&mut c, &mut cx, p, Side::Left, AREA, &mut out, &mut hit);
            yaml = c.to_yaml();
        });
        full.textures_delta.clear();
        (yaml, out, hit)
    }
}

fn press(at: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn click(at: Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        press(at, button, true),
        press(at, button, false),
    ]
}

fn ids(out: &[Command]) -> Vec<String> {
    out.iter().map(ToString::to_string).collect()
}

#[test]
fn layout_fits_or_scrolls() {
    let l = layout(&[120.0, 80.0, 300.0], 0.0, 600.0, 50.0);
    assert_eq!(l.arrows, None);
    assert_eq!(l.scroll, 0.0, "влезают — прокрутки нет");
    assert_eq!(l.tabs[1].1, TAB_MIN);
    assert_eq!(l.tabs[2].1, TAB_MAX);
    assert!(l.plus.0 > l.tabs[2].0 + l.tabs[2].1);
    let many = layout(&[150.0; 20], 0.0, 600.0, 0.0);
    assert!(many.arrows.is_some());
    assert!(many.max_scroll > 0.0);
    assert_eq!(many.plus.1, 600.0);
    let last = scroll_to(&many, 19);
    let shown = layout(&[150.0; 20], 0.0, 600.0, last);
    let (x, w) = shown.tabs[19];
    assert!(x >= shown.view.0 && x + w <= shown.view.1 + 0.5);
}

/// Табы мокапа §2.1: `urfin` (активный, с крестиком), `docs`, `home`, затем `+`.
#[test]
fn snapshot_tabs() {
    let mut fx = Fx::new();
    let p = panel(&["/work/urfin", "/work/urfin/docs", "/home"], 0);
    fx.frame(&p, Vec::new());
    let (yaml, out, _) = fx.frame(&p, Vec::new());
    assert_eq!(out.len(), 0);
    insta::assert_snapshot!(yaml);
}

/// 20 табов прокручиваются стрелками; активный (последний) виден сразу.
#[test]
fn twenty_tabs_scroll() {
    let names: Vec<String> = (0..20).map(|i| format!("/dir/tab-{i:02}")).collect();
    let paths: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut fx = Fx::new();
    let p = panel(&paths, 19);
    let (yaml, _, _) = fx.frame(&p, Vec::new());
    assert!(yaml.contains("tab-19"), "активный виден: {yaml}");
    assert!(fx.memo.scroll > 0.0);
    let end = fx.memo.scroll;
    // Стрелка «влево» — левый край полосы.
    let back = pos2(AREA.left() + BUTTON_W / 2.0, AREA.bottom() - 16.0);
    fx.frame(&p, click(back, egui::PointerButton::Primary));
    assert!(fx.memo.scroll < end);
}

/// Hit-test рамки: табы и `+` — интерактивные зоны, пустое место заголовка — нет.
#[test]
fn tabs_are_interactive_blank_is_caption() {
    let mut fx = Fx::new();
    let p = panel(&["/a", "/b"], 0);
    let (_, _, hit) = fx.frame(&p, Vec::new());
    let l = layout(
        &fx.memo
            .galleys
            .iter()
            .map(|g| g.size().x + fx.tokens.m(Metric::SpaceM) + CLOSE_W)
            .collect::<Vec<_>>(),
        AREA.left(),
        AREA.right(),
        0.0,
    );
    let y = AREA.bottom() - 16.0;
    for (x, w) in &l.tabs {
        assert!(hit.iter().any(|r| r.contains(pos2(x + w / 2.0, y))));
    }
    assert!(
        hit.iter()
            .any(|r| r.contains(pos2(f32::midpoint(l.plus.0, l.plus.1), y)))
    );
    let free = pos2(f32::midpoint(l.plus.1, AREA.right()), y);
    assert!(!hit.iter().any(|r| r.contains(free)));
}

#[test]
fn click_middle_close_and_plus() {
    let mut fx = Fx::new();
    let p = panel(&["/a", "/b", "/c"], 2);
    fx.frame(&p, Vec::new());
    let tab = |fx: &Fx, i: usize| {
        let l = layout(
            &fx.memo
                .galleys
                .iter()
                .map(|g| g.size().x + fx.tokens.m(Metric::SpaceM) + CLOSE_W)
                .collect::<Vec<_>>(),
            AREA.left(),
            AREA.right(),
            fx.memo.scroll,
        );
        (l.tabs[i], l.plus)
    };
    let ((x, _), plus) = tab(&fx, 0);
    let at = pos2(x + 20.0, AREA.bottom() - 16.0);
    let (_, out, _) = fx.frame(&p, click(at, egui::PointerButton::Primary));
    assert_eq!(ids(&out), ["tab.goto side=left n=1"]);
    // Средняя кнопка по чужому табу: закрыть его и вернуться к активному (сдвинулся на 1).
    let (_, out, _) = fx.frame(&p, click(at, egui::PointerButton::Middle));
    assert_eq!(
        ids(&out),
        [
            "tab.goto side=left n=1",
            "tab.close side=left",
            "tab.goto side=left n=2"
        ]
    );
    let (_, out, _) = fx.frame(
        &p,
        click(
            pos2(f32::midpoint(plus.0, plus.1), AREA.bottom() - 16.0),
            egui::PointerButton::Primary,
        ),
    );
    assert_eq!(ids(&out), ["tab.new side=left"]);
}

/// Перетаскивание активного таба вправо за последний — `tab.move n=3`.
#[test]
fn drag_moves_tab() {
    let mut fx = Fx::new();
    let p = panel(&["/a", "/b", "/c"], 0);
    fx.frame(&p, Vec::new());
    let y = AREA.bottom() - 16.0;
    let from = pos2(AREA.left() + 30.0, y);
    let to = pos2(AREA.left() + 3.0 * TAB_MIN + 20.0, y);
    let primary = egui::PointerButton::Primary;
    fx.frame(
        &p,
        vec![egui::Event::PointerMoved(from), press(from, primary, true)],
    );
    let mut out = Vec::new();
    for k in 1..=6 {
        #[allow(clippy::cast_precision_loss, reason = "шаги теста")]
        let x = from.x + (to.x - from.x) * k as f32 / 6.0;
        out.extend(fx.frame(&p, vec![egui::Event::PointerMoved(pos2(x, y))]).1);
    }
    out.extend(fx.frame(&p, vec![press(to, primary, false)]).1);
    assert_eq!(ids(&out), ["tab.move side=left n=3"]);
}
