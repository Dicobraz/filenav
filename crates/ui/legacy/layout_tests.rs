#![allow(clippy::float_cmp, reason = "размеры на сетке 4 px — точные")]

use egui::{Rect, pos2, vec2};
use urfin_core::{
    AppState, Attrs, Banner, Entry, EntryKind, Level, Location, Panel, PromptDetail, PromptSpec,
    Side, Tab, TabId,
};

use super::*;
use crate::legacy::canvas::RecordCanvas;
use crate::legacy::tokens::Tokens;

fn state() -> AppState {
    let loc = Location::local(std::env::temp_dir().join("urfin-layout"));
    AppState::new(
        Panel::new(Tab::new(TabId(1), loc.clone())),
        Panel::new(Tab::new(TabId(2), loc)),
    )
}

fn screen(w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
}

fn record(r: &Regions, s: &AppState, ui: &UiState) -> String {
    let i18n = urfin_i18n::Catalog::embedded()
        .build(&"ru".parse().unwrap())
        .0;
    let mut c = RecordCanvas::new(&i18n);
    crate::legacy::frame::paint(&mut c, r, &Tokens::default(), s, ui, r"C:\work\urfin>");
    c.to_yaml()
}

fn all_rects(r: &Regions) -> Vec<Rect> {
    let mut v = vec![r.titlebar, r.cmdline, r.keybar, r.terminal];
    v.extend(r.banners.iter().copied());
    v.extend(r.panels.iter().flatten().copied());
    v.extend(r.splitter);
    v.extend(r.jobs.iter().copied());
    v
}

fn on_grid(v: f32) -> bool {
    (v / GRID).fract().abs() < 1e-4
}

/// Мокап §2.1 на 1280×768: заголовок, две карточки, cmdline, keybar.
#[test]
fn snapshot_1280x768() {
    let s = state();
    let ui = UiState::new();
    let r = Layout::compute(screen(1280.0, 768.0), &Tokens::default(), &s, &ui);
    insta::assert_snapshot!(record(&r, &s, &ui));
}

/// 1920×1080 при 150 % — 1280×720 логических.
#[test]
fn snapshot_1920x1080_at_150() {
    let s = state();
    let ui = UiState::new();
    let r = Layout::compute(
        screen(1920.0 / 1.5, 1080.0 / 1.5),
        &Tokens::default(),
        &s,
        &ui,
    );
    insta::assert_snapshot!(record(&r, &s, &ui));
}

/// 800×600: две панели (уже 720 — одна).
#[test]
fn snapshot_800x600() {
    let s = state();
    let ui = UiState::new();
    let r = Layout::compute(screen(800.0, 600.0), &Tokens::default(), &s, &ui);
    insta::assert_snapshot!(record(&r, &s, &ui));
}

#[test]
fn everything_on_4px_grid() {
    let mut s = state();
    s.banner = Some(Banner {
        level: Level::Error,
        text: "x".into(),
        actions: smallvec::SmallVec::default(),
    });
    let ui = UiState::new();
    for (w, h) in [
        (1280.0, 768.0),
        (1280.0, 720.0),
        (800.0, 600.0),
        (640.0, 400.0),
    ] {
        for jobs in 0..3 {
            let r = Layout::compute_with(screen(w, h), &Tokens::default(), &s, &ui, jobs);
            for rect in all_rects(&r) {
                for v in [rect.left(), rect.top(), rect.right(), rect.bottom()] {
                    assert!(on_grid(v), "{w}x{h} jobs {jobs}: {rect:?}");
                }
            }
        }
    }
}

#[test]
fn conflict_banner_takes_120_and_jobs_32_each() {
    let t = Tokens::default();
    let ui = UiState::new();
    let base = Layout::compute(screen(1280.0, 768.0), &t, &state(), &ui);
    let h0 = base.panels[0].unwrap().height();

    let mut s = state();
    let e = Entry {
        name: "a".into(),
        size: 1,
        mtime: 0,
        attrs: Attrs::default(),
        kind: EntryKind::File,
    };
    let spec = PromptSpec {
        anchor: urfin_core::Anchor::JobBar,
        title: "t".into(),
        body: "b".into(),
        choices: smallvec::SmallVec::default(),
        default: 0,
        input: None,
        detail: Some(PromptDetail::Conflict {
            src: e.clone(),
            dst: e,
            index: 1,
            total: 3,
        }),
        secret: false,
        allow_all: false,
    };
    let _ = s.prompts.open(spec, None);
    let with_conflict = Layout::compute(screen(1280.0, 768.0), &t, &s, &ui);
    // Баннер 48 + 3×24 над cmdline (S2 §2: job-бар → вопрос → cmdline) и зазор 8.
    let bar = with_conflict.prompt.expect("prompt bar");
    assert_eq!(bar.height(), 120.0);
    assert_eq!(bar.bottom(), with_conflict.cmdline.top() - 8.0);
    assert_eq!(with_conflict.banners.len(), 0);
    assert_eq!(h0 - with_conflict.panels[0].unwrap().height(), 128.0);

    let two_jobs = Layout::compute_with(screen(1280.0, 768.0), &t, &state(), &ui, 2);
    assert_eq!(two_jobs.jobs.len(), 2);
    assert!(two_jobs.jobs.iter().all(|j| j.height() == 32.0));
    assert_eq!(h0 - two_jobs.panels[0].unwrap().height(), 64.0);
}

#[test]
fn ctrl_o_hides_panels_terminal_unchanged() {
    let t = Tokens::default();
    let s = state();
    let mut ui = UiState::new();
    let shown = Layout::compute(screen(1280.0, 768.0), &t, &s, &ui);
    ui.panels_hidden = true;
    let hidden = Layout::compute(screen(1280.0, 768.0), &t, &s, &ui);
    assert_eq!(hidden.panels, [None, None]);
    assert_eq!(hidden.splitter, None);
    assert_eq!(hidden.terminal, shown.terminal, "размер PTY не меняется");
    assert_eq!(hidden.cmdline, shown.cmdline);
}

#[test]
fn narrow_window_shows_active_panel_only() {
    let t = Tokens::default();
    let mut s = state();
    let ui = UiState::new();
    s.active = Side::Right;
    let r = Layout::compute(screen(700.0, 500.0), &t, &s, &ui);
    assert!(r.panels[0].is_none());
    assert_eq!(r.panels[1].unwrap().width(), 700.0 - 16.0);
    // Скрытая `panel.toggle` — вторая на всю ширину.
    let mut s = state();
    s.panel_mut(Side::Left).visible = false;
    let r = Layout::compute(screen(1280.0, 768.0), &t, &s, &ui);
    assert_eq!(r.panels[0], None);
    assert_eq!(r.panels[1].unwrap().width(), 1280.0 - 16.0);
}

#[test]
fn split_follows_ui_state() {
    let t = Tokens::default();
    let s = state();
    let mut ui = UiState::new();
    ui.split = 0.3;
    let r = Layout::compute(screen(1280.0, 768.0), &t, &s, &ui);
    let left = r.panels[0].unwrap().width();
    let right = r.panels[1].unwrap().width();
    assert!(left < right);
    assert!(on_grid(left) && on_grid(right));
}
