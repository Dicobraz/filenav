use std::sync::Arc;
use std::time::{Duration, Instant};

use filenav_core::fake::FakeListing;
use filenav_core::{
    Attrs, Boot, Command, Core, Entry, EntryKind, Focus, Gen, Listing, ListingStatus, Location,
    OverlayKind, Services, Tab, TabId, UiEvent, Waker,
};

use super::*;
use crate::panel::{HOLD, hold_left, lines};

fn tab(names: &[&str]) -> Tab {
    // Путь с родителем на любой ОС: строка 0 — `..`.
    let mut t = Tab::new(TabId(1), Location::local(std::env::temp_dir().join("x")));
    let entries: Vec<Entry> = names
        .iter()
        .map(|n| Entry {
            name: (*n).into(),
            size: 2048,
            mtime: 0,
            attrs: Attrs::default(),
            kind: EntryKind::File,
        })
        .collect();
    let order = (0..u32::try_from(entries.len()).unwrap()).collect();
    t.listing = Listing::with_entries(entries, order, Gen(1));
    t
}

fn en() -> I18n {
    filenav_i18n::Catalog::embedded()
        .build(&"en".parse().unwrap())
        .0
}

#[test]
fn holds_only_empty_fresh_loading() {
    let mut t = tab(&[]);
    assert_eq!(hold_left(&t), None, "готовый пустой каталог не держим");
    t.listing.status = ListingStatus::Loading {
        since: Instant::now(),
    };
    assert!(hold_left(&t).is_some_and(|d| d <= HOLD));
    t.listing.status = ListingStatus::Loading {
        since: Instant::now().checked_sub(HOLD).unwrap(),
    };
    assert_eq!(hold_left(&t), None, "дольше 150 мс — показываем загрузку");
    let mut t = tab(&["a"]);
    t.listing.status = ListingStatus::Loading {
        since: Instant::now(),
    };
    assert_eq!(hold_left(&t), None, "первые записи пришли — рисуем их");
}

#[test]
fn lines_from_top_with_cursor() {
    let mut t = tab(&["a", "b", "c"]);
    t.cursor.top = 1;
    t.cursor.row = 2;
    let fmt = crate::fmt_cell::FmtCfg::new(&en(), false, jiff::tz::TimeZone::UTC);
    let v = lines(&fmt, &crate::highlight::Highlighter::default(), &t, 2);
    let got: Vec<_> = v
        .iter()
        .map(|l| (l.name.as_str(), l.size.as_str(), l.cursor))
        .collect();
    assert_eq!(got, [("a", "2.00 KB", false), ("b", "2.00 KB", true)]);
    assert_eq!(v[1].click, Command::new("panel.cursor.to").arg("name", "b"));
}

#[test]
fn footer_texts() {
    let i18n = en();
    let mut t = tab(&["a", "b"]);
    t.listing.status = ListingStatus::Complete;
    t.listing.stats.files = 2;
    t.listing.stats.bytes = 4096;
    assert_eq!(panel::footer(&i18n, &t), "2 items · 4.00 KB");
    t.listing.status = ListingStatus::Loading {
        since: Instant::now(),
    };
    assert_eq!(panel::footer(&i18n, &t), "Loading…");
}

#[test]
fn keybar_labels_and_fallback() {
    let i18n = en();
    assert_eq!(keybar::label(&i18n, "ops.copy"), "Copy");
    // Нет keybar-ui-qsearch — подпись из cmd-ui-qsearch.
    assert_eq!(keybar::label(&i18n, "ui.qsearch"), "Quick search");
    assert_eq!(keybar::label(&i18n, "no.such"), "no.such");
    let mut kb = keybar::Keybar::default();
    kb.refresh(&i18n, 0, &["app.quit"]);
    let f10 = kb.items.iter().find(|i| i.n == 10).unwrap();
    assert_eq!(
        (f10.cmd, f10.label.as_str(), f10.enabled),
        ("app.quit", "Quit", true)
    );
    assert!(kb.items.iter().filter(|i| i.n != 10).all(|i| !i.enabled));
    kb.refresh(&i18n, 1, &["app.quit", "ui.menu"]);
    assert!(
        !kb.items.iter().find(|i| i.n == 9).unwrap().enabled,
        "меню — S2-14"
    );
}

/// Каждая F-клавиша заглушки имеет подпись в en и ru (`keybar-*` или `cmd-*`).
#[test]
fn keybar_stub_keys_exist() {
    let catalog = filenav_i18n::Catalog::embedded();
    for tag in ["en", "ru"] {
        let i18n = catalog.build(&tag.parse().unwrap()).0;
        for id in keybar::FAR_STUB.iter().flatten() {
            let part = keybar::key_part(id);
            assert!(
                i18n.has(&format!("keybar-{part}")) || i18n.has(&format!("cmd-{part}")),
                "{tag}: no keybar/cmd key for {id}"
            );
        }
    }
}

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

fn core() -> Core {
    let root = Location::local(std::env::temp_dir().join("filenav-ui-test"));
    let fake = FakeListing::new(64);
    fake.set_dir(
        root.clone(),
        vec![FakeListing::dir("d"), FakeListing::file("f.txt", 10)],
    );
    let mut core = Core::new(
        Boot::new(root.clone(), root),
        Services {
            listing: Arc::new(fake),
            ..Services::null()
        },
        Arc::new(NoWake),
    );
    register(&mut core);
    for _ in 0..20 {
        core.pump(Duration::from_millis(5));
        let loaded = core
            .state()
            .panels
            .iter()
            .all(|p| p.tab().listing.status == ListingStatus::Complete);
        if loaded {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    core
}

fn run(core: &mut Core, cmd: &str) {
    core.dispatch(Command::parse(cmd).unwrap());
    core.pump(Duration::from_millis(5));
}

fn ui_state(core: &Core) -> &UiState {
    core.state().ext.get::<UiState>().expect("UiState in ext")
}

fn errors(core: &mut Core) -> Vec<String> {
    core.take_ui_events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::Toast { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn ui_state_lives_in_ext_after_pump() {
    let core = core();
    assert_eq!(ui_state(&core), &UiState::new());
}

#[test]
fn overlay_mirrors_focus() {
    let mut c = core();
    run(&mut c, "ui.menu menu=file");
    assert_eq!(c.state().focus, Focus::Overlay(OverlayKind::Menu));
    assert_eq!(ui_state(&c).overlay, Some(OverlayKind::Menu));
    assert_eq!(ui_state(&c).menu.as_deref(), Some("file"));
    // Новый оверлей закрывает прежний.
    run(&mut c, "ui.qsearch text=ab");
    assert_eq!(c.state().focus, Focus::Overlay(OverlayKind::QuickSearch));
    assert_eq!(ui_state(&c).menu, None);
    assert_eq!(
        ui_state(&c).qsearch.as_ref().map(|(s, t)| (*s, t.as_str())),
        Some((c.state().active, "ab"))
    );
    run(&mut c, "ui.cancel");
    assert_eq!(c.state().focus, Focus::Panel);
    assert_eq!(ui_state(&c).overlay, None);
    assert_eq!(ui_state(&c).qsearch, None);
    // Фокус увели в панель (клик) — оверлей закрывается в следующем pump.
    run(&mut c, "ui.draft command=ops.mkdir arg=name init=New");
    assert_eq!(
        ui_state(&c).draft.as_ref().map(|d| d.text.as_str()),
        Some("New")
    );
    run(&mut c, "panel.focus");
    c.pump(Duration::from_millis(5));
    assert_eq!(ui_state(&c).overlay, None);
    assert_eq!(ui_state(&c).draft, None);
    assert_eq!(errors(&mut c), Vec::<String>::new());
}

#[test]
fn rename_takes_name_under_cursor() {
    let mut c = core();
    // Строка 0 — `..`: переименовывать нечего.
    run(&mut c, "panel.cursor.home");
    run(&mut c, "ui.rename");
    assert_eq!(ui_state(&c).rename, None);
    assert_eq!(c.state().focus, Focus::Panel);
    run(&mut c, "panel.cursor.to name=f.txt");
    run(&mut c, "ui.rename");
    let tab = c.state().active_tab().id;
    assert_eq!(
        ui_state(&c).rename.as_ref().map(|(t, n)| (*t, n.as_str())),
        Some((tab, "f.txt"))
    );
    assert_eq!(c.state().focus, Focus::Overlay(OverlayKind::InlineEdit));
}

#[test]
fn toggles_theme_zoom_split_columns() {
    let mut c = core();
    run(&mut c, "ui.jobs.toggle");
    assert!(ui_state(&c).jobs_expanded);
    run(&mut c, "ui.theme mode=dark");
    assert_eq!(ui_state(&c).theme, Some(ThemeMode::Dark));
    run(&mut c, "ui.zoom delta=1");
    run(&mut c, "ui.zoom delta=1");
    assert_eq!(ui_state(&c).zoom, 120);
    run(&mut c, "ui.zoom");
    assert_eq!(ui_state(&c).zoom, 100);
    run(&mut c, "ui.split delta=-1");
    assert!((ui_state(&c).split - 0.45).abs() < 1e-6);
    run(&mut c, "ui.split reset=true");
    assert!((ui_state(&c).split - 0.5).abs() < 1e-6);
    run(&mut c, "ui.columns set=photos");
    assert_eq!(ui_state(&c).columns.as_deref(), Some("photos"));
    run(&mut c, "ui.path_edit side=passive");
    assert_eq!(ui_state(&c).path_edit, Some(c.state().active.other()));
    assert_eq!(errors(&mut c), Vec::<String>::new());
    run(&mut c, "ui.theme mode=blue");
    assert_eq!(errors(&mut c).len(), 1, "плохой mode — тост ошибки");
}

#[test]
fn prompt_hide_without_prompt_is_noop() {
    let mut c = core();
    run(&mut c, "ui.prompt.hide");
    assert_eq!(ui_state(&c).prompt_hidden.as_slice(), []);
    run(&mut c, "ui.prompt.hide id=7");
    run(&mut c, "ui.prompt.hide id=7");
    assert_eq!(
        ui_state(&c).prompt_hidden.as_slice(),
        [filenav_core::PromptId(7)]
    );
    run(&mut c, "ui.prompt.show id=7");
    assert_eq!(ui_state(&c).prompt_hidden.as_slice(), []);
}

/// Кадр без окна: карточки, keybar и тосты рисуются, команды — валидные.
#[test]
fn show_headless_frame() {
    let mut c = core();
    run(&mut c, "app.no_such");
    let ev = c.take_ui_events();
    let mut ui = FileNavUi::new(
        c.registry(),
        &sys::SysProbe::default(),
        &UiCfg::default(),
        fonts::FontSet::default(),
    );
    let ctx = egui::Context::default();
    let mut out = Vec::new();
    for frame in 0..2 {
        // События кадра приходят один раз.
        let ev = if frame == 0 { ev.as_slice() } else { &[] };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 760.0),
            )),
            ..Default::default()
        };
        let mut full = ctx.run_ui(input, |root| ui.show(root, c.state(), ev, &mut out));
        full.textures_delta.clear();
    }
    let viewports: Vec<_> = out
        .iter()
        .filter(|c| c.id.as_str() == "panel.viewport")
        .collect();
    assert_eq!(viewports.len(), 2, "по разу на панель: {out:?}");
    for cmd in &out {
        c.registry().validate(cmd).unwrap();
    }
    assert_eq!(ui.toasts.len(), 1, "{ev:?}");
}

/// Линт `commands_i18n` (S2 §7): у каждой команды реестра (core + ui) есть `cmd-<id>` в
/// en и ru. `keybar-<id>` для F-клавиш пресетов — `keybar_stub_keys_exist`, полный
/// набор — с keymap S3.
#[test]
fn commands_i18n() {
    let c = core();
    let catalog = filenav_i18n::Catalog::embedded();
    let mut missing = Vec::new();
    for tag in ["en", "ru"] {
        let i18n = catalog.build(&tag.parse().unwrap()).0;
        for m in c.registry().iter() {
            let key = format!("cmd-{}", keybar::key_part(m.id));
            // Цепочка ru → en скрыла бы пропуск: проверяем бандл языка без фолбэка.
            if !catalog.has_own(&tag.parse().unwrap(), &key) {
                missing.push(format!("{tag}: {key}"));
            }
            assert!(i18n.has(&key));
        }
    }
    assert!(c.registry().iter().count() > 60);
    assert_eq!(missing, Vec::<String>::new());
}
