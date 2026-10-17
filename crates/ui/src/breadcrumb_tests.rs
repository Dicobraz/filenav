use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Rect, pos2, vec2};
use urfin_core::{Command, Location, Side};

use super::*;
use crate::canvas::RecordCanvas;
use crate::fonts::FontSet;
use crate::state::UiState;
use crate::tokens::Tokens;

#[cfg(windows)]
fn labels(loc: &Location) -> Vec<String> {
    segments(loc).iter().map(|c| c.label.to_string()).collect()
}

fn test_loc(path: &str) -> Location {
    Location {
        backend: "test".into(),
        path: path.into(),
        outer: None,
    }
}

#[cfg(windows)]
#[test]
fn drive_and_unc_roots() {
    let c = segments(&Location::local(r"C:\work\urfin"));
    assert_eq!(
        labels(&Location::local(r"C:\work\urfin")),
        ["C:", "work", "urfin"]
    );
    assert_eq!(c[0].loc, Location::local(r"C:\"));
    assert_eq!(c[1].loc, Location::local(r"C:\work"));
    let unc = Location::local(r"\\srv\share\docs\2026");
    assert_eq!(labels(&unc), [r"\\srv\share", "docs", "2026"]);
    assert_eq!(segments(&unc)[0].loc, Location::local(r"\\srv\share\"));
}

#[test]
fn unix_style_root_is_a_segment() {
    let s = segments(&test_loc("/home/anton"));
    assert_eq!(
        s.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        ["/", "home", "anton"]
    );
    assert_eq!(s[1].loc, test_loc("/home"));
}

/// Архив (ADR-0009): имя архива — сегмент, ведёт в корень архива; путь внутри — дальше.
#[test]
fn archive_chain_is_decoded() {
    let outer = test_loc("/data/backup 2026.zip");
    let inner = Location {
        backend: "zip".into(),
        path: PathBuf::from("/photos/june"),
        outer: Some(Arc::new(outer.clone())),
    };
    let s = segments(&inner);
    assert_eq!(
        s.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        ["/", "data", "backup 2026.zip", "photos", "june"]
    );
    // Клик по архиву — корень архива, а не файл архива.
    assert_eq!(s[2].loc.backend, "zip");
    assert_eq!(s[2].loc.outer.as_deref(), Some(&outer));
    assert_eq!(s[2].loc.path, PathBuf::from("/"));
    assert_eq!(s[1].loc, test_loc("/data"));
    assert_eq!(s[4].loc, inner);
}

#[test]
fn fit_collapses_leading_segments() {
    assert_eq!(fit(&[30.0, 40.0, 50.0], 500.0, 16.0), (false, 0));
    let widths = vec![60.0; 40];
    let (collapsed, first) = fit(&widths, 400.0, 16.0);
    assert!(collapsed);
    let shown = 16.0
        + CHEVRON_W
        + widths[first..].iter().sum::<f32>()
        + widths[first + 1..].iter().map(|_| CHEVRON_W).sum::<f32>();
    assert!(shown <= 400.0, "shown {shown}");
    assert_eq!(
        first,
        40 - 4,
        "четыре последних сегмента по 60 + `› …` в 400"
    );
}

struct Fx {
    ctx: egui::Context,
    i18n: urfin_i18n::I18n,
    tokens: Tokens,
    fonts: FontSet,
    st: UiState,
    memo: CrumbMemo,
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
            st: UiState::new(),
            memo: CrumbMemo::default(),
        }
    }

    /// Строка пути 400×32: запись Canvas и команды.
    fn frame(&mut self, loc: &Location, events: Vec<egui::Event>) -> (Vec<String>, Vec<Command>) {
        let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(400.0, 32.0));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut items = Vec::new();
        let mut out = Vec::new();
        let ctx = self.ctx.clone();
        let mut full = ctx.run_ui(input, |root| {
            let mut c = RecordCanvas::new(&self.i18n);
            let mut cx = CrumbCx {
                ui: root,
                tokens: &self.tokens,
                fonts: &self.fonts,
                st: &self.st,
                memo: &mut self.memo,
            };
            show(&mut c, &mut cx, r, loc, Side::Left, true, &mut out);
            items = c.to_yaml().lines().map(str::to_owned).collect();
        });
        full.textures_delta.clear();
        (items, out)
    }
}

/// `[x, y, w, h]` записи Canvas.
fn rect_of(line: &str) -> [f32; 4] {
    let a = line.find('[').unwrap();
    let b = line.find(']').unwrap();
    let v: Vec<f32> = line[a + 1..b]
        .split(',')
        .map(|n| n.trim().parse().unwrap())
        .collect();
    [v[0], v[1], v[2], v[3]]
}

fn click(at: egui::Pos2) -> Vec<egui::Event> {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![egui::Event::PointerMoved(at), press(true), press(false)]
}

/// 40 сегментов: начало свёрнуто в `…`, ничего не вылезает за строку.
#[test]
fn forty_segments_fit_the_row() {
    let path = (0..40).fold(String::new(), |mut s, i| {
        let _ = write!(s, "/dir-{i:02}");
        s
    });
    let loc = test_loc(&path);
    let mut fx = Fx::new();
    let (items, _) = fx.frame(&loc, Vec::new());
    assert!(items.iter().any(|l| l.contains("\"…\"")), "{items:#?}");
    assert!(items.iter().any(|l| l.contains("dir-39")));
    assert!(!items.iter().any(|l| l.contains("dir-00")));
    for l in &items {
        let [x, _, w, _] = rect_of(l);
        assert!(x >= 10.0 && x + w <= 410.0 + 0.5, "{l}");
    }
}

#[test]
fn clicks_go_open_menu_and_edit() {
    let loc = test_loc("/work/urfin");
    let mut fx = Fx::new();
    let (items, _) = fx.frame(&loc, Vec::new());
    let work = items.iter().find(|l| l.contains("\"work\"")).unwrap();
    let [x, y, w, h] = rect_of(work);
    // Клик по сегменту — `panel.go` в него.
    let (_, out) = fx.frame(&loc, click(pos2(x + w / 2.0, y + h / 2.0)));
    let go = out
        .iter()
        .find(|c| c.id.as_str() == "panel.go")
        .expect("panel.go");
    assert_eq!(go.args.loc("path"), Some(test_loc("/work")));
    // Клик по `›` после `work` — меню его подкаталогов.
    let (_, out) = fx.frame(&loc, click(pos2(x + w + CHEVRON_W / 2.0, y + h / 2.0)));
    let menu = out
        .iter()
        .find(|c| c.id.as_str() == "ui.menu")
        .expect("ui.menu");
    assert_eq!(menu.args.str("menu"), Some("subdirs"));
    assert_eq!(menu.args.loc("path"), Some(test_loc("/work")));
    // Пустое место — ввод пути.
    let (_, out) = fx.frame(&loc, click(pos2(380.0, 26.0)));
    assert!(out.iter().any(|c| c.id.as_str() == "ui.path_edit"));
    assert!(!out.iter().any(|c| c.id.as_str() == "panel.go"));
}
