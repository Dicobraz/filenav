use std::sync::Arc;
use std::time::{Duration, Instant};

use urfin_core::fake::FakeListing;
use urfin_core::{
    Attrs, Boot, Command, Core, Entry, EntryKind, Focus, Gen, Listing, ListingStatus, Location,
    OverlayKind, Services, Tab, TabId, UiEvent, Waker,
};

use super::*;
use crate::panel::{HOLD, hold_left};

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
    urfin_i18n::Catalog::embedded()
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
    let fmt = crate::fmt_cell::FmtCfg::new(&en(), false, crate::sys::Zone::utc());
    let hl = crate::highlight::Highlighter::default();
    let ctx = egui::Context::default();
    let mut cache = crate::cache::RowCache::new();
    let mut v = Vec::new();
    let mut full = ctx.run_ui(egui::RawInput::default(), |_| {
        let cx = crate::panel::LineCx {
            ctx: &ctx,
            fmt: &fmt,
            hl: &hl,
            font: egui::FontId::proportional(14.0),
            name_w: 300.0,
            digit_slot: None,
            size: true,
            mtime: false,
        };
        crate::panel::fill_lines(&mut v, &mut cache, &cx, &t, 2);
    });
    full.textures_delta.clear();
    let got: Vec<_> = v
        .iter()
        .map(|l| {
            let size = l.size.as_ref().map(|s| s.0.text().to_owned());
            (l.name.text().to_owned(), size, l.cursor, l.row)
        })
        .collect();
    assert_eq!(
        got,
        [
            ("a".to_owned(), Some("2.0 KB".to_owned()), false, 1),
            ("b".to_owned(), Some("2.0 KB".to_owned()), true, 2)
        ]
    );
}

#[test]
fn footer_texts() {
    let i18n = en();
    let mut t = tab(&["a", "b"]);
    t.listing.status = ListingStatus::Complete;
    t.listing.stats.files = 2;
    t.listing.stats.bytes = 4096;
    assert_eq!(panel::footer(&i18n, &t), "2 items · 4.0 KB");
    t.listing.status = ListingStatus::Loading {
        since: Instant::now(),
    };
    assert_eq!(panel::footer(&i18n, &t), "Loading…");
}

#[test]
fn keybar_labels_and_fallback() {
    let i18n = en();
    assert_eq!(keybar::label(&i18n, "keybar-ops-copy", "ops.copy"), "Copy");
    // Нет keybar-ui-qsearch — подпись из cmd-ui-qsearch.
    assert_eq!(
        keybar::label(&i18n, "keybar-ui-qsearch", "ui.qsearch"),
        "Quick search"
    );
    assert_eq!(keybar::label(&i18n, "keybar-no-such", "no.such"), "no.such");
    let mut kb = keybar::Keybar::default();
    kb.refresh(&i18n, 0, &["app.quit", "ui.menu"], &keybar::FarStub);
    let row = kb.items(keybar::Mods::None);
    let f10 = row.iter().find(|i| i.n == 10).unwrap();
    assert_eq!(
        (f10.cmd.to_string(), f10.label.as_str(), f10.enabled),
        ("app.quit".to_owned(), "Quit", true)
    );
    assert!(
        row.iter().find(|i| i.n == 9).unwrap().enabled,
        "меню F9 есть"
    );
    assert!(row.iter().filter(|i| i.n < 9).all(|i| !i.enabled));
    let ctrl = kb.items(keybar::Mods::Ctrl);
    let f3 = ctrl.iter().find(|i| i.n == 3).unwrap();
    assert_eq!(
        (f3.cmd.to_string(), f3.label.as_str()),
        ("panel.sort key=name".to_owned(), "Name")
    );
}

/// Каждая F1…F10 заглушки во всех наборах модификаторов имеет подпись в en и ru.
#[test]
fn keybar_stub_keys_exist() {
    use keybar::KeyHints as _;
    let catalog = urfin_i18n::Catalog::embedded();
    for tag in ["en", "ru"] {
        let i18n = catalog.build(&tag.parse().unwrap()).0;
        for mods in [
            keybar::Mods::None,
            keybar::Mods::Shift,
            keybar::Mods::Ctrl,
            keybar::Mods::Alt,
        ] {
            for k in keybar::FarStub.fkeys(mods).iter().take(10).flatten() {
                let part = keybar::key_part(k.cmd.id.as_str());
                assert!(
                    i18n.has(&k.label_key) || i18n.has(&format!("cmd-{part}")),
                    "{tag}: no label for {}",
                    k.cmd
                );
            }
        }
    }
}

/// Тексты, нарисованные кадром (для проверок без Canvas).
fn frame_texts(
    ctx: &egui::Context,
    ui: &mut UrfinUi,
    c: &Core,
    input: egui::RawInput,
) -> (Vec<String>, Vec<Command>) {
    let mut out = Vec::new();
    let mut full = ctx.run_ui(input, |root| ui.show(root, c.state(), &[], &mut out));
    full.textures_delta.clear();
    let mut texts = Vec::new();
    for cs in &full.shapes {
        collect_texts(&cs.shape, &mut texts);
    }
    (texts, out)
}

fn collect_texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect_texts(s, out)),
        _ => {}
    }
}

fn screen(events: Vec<egui::Event>, modifiers: egui::Modifiers) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 760.0),
        )),
        events: std::iter::once(egui::Event::ModifiersChanged(modifiers))
            .chain(events)
            .collect(),
        ..Default::default()
    }
}

/// Зажатый Shift меняет подписи keybar в том же кадре (S2-14).
#[test]
fn shift_switches_keybar_labels_in_one_frame() {
    let c = core();
    let mut ui = new_ui(&c);
    let ctx = egui::Context::default();
    let set = |c: &mut Vec<String>| c.retain(|t| t.len() < 40);
    let (mut plain, _) = frame_texts(&ctx, &mut ui, &c, screen(Vec::new(), egui::Modifiers::NONE));
    set(&mut plain);
    let (shifted, _) = frame_texts(
        &ctx,
        &mut ui,
        &c,
        screen(Vec::new(), egui::Modifiers::SHIFT),
    );
    let i18n = urfin_i18n::current();
    let pack = i18n.get("keybar-arc-add");
    let help = i18n.get("keybar-ui-help");
    assert!(plain.contains(&help) && !plain.contains(&pack));
    assert!(shifted.contains(&pack), "{shifted:?}");
}

/// Меню F9 открыто: клик по панели закрывает меню и не уходит в панель (S2-14).
#[test]
fn click_outside_menu_closes_and_stays_out_of_panel() {
    let mut c = core();
    run(&mut c, "ui.menu");
    assert_eq!(ui_state(&c).menu.as_deref(), Some("left"));
    let mut ui = new_ui(&c);
    let ctx = egui::Context::default();
    frame_texts(&ctx, &mut ui, &c, screen(Vec::new(), egui::Modifiers::NONE));
    // Точка в правой карточке, далеко от выпадающего списка левого меню.
    let at = egui::pos2(900.0, 400.0);
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame_texts(
        &ctx,
        &mut ui,
        &c,
        screen(vec![egui::Event::PointerMoved(at)], egui::Modifiers::NONE),
    );
    let events = vec![press(true), press(false)];
    let (_, out) = frame_texts(&ctx, &mut ui, &c, screen(events, egui::Modifiers::NONE));
    let ids: Vec<String> = out.iter().map(|c| c.id.as_str().to_owned()).collect();
    assert!(ids.contains(&"ui.cancel".to_owned()), "{ids:?}");
    assert!(
        !ids.iter()
            .any(|i| i.starts_with("panel.") || i.starts_with("sel.")),
        "{ids:?}"
    );
}

/// Реестр команд core и ui, как в приложении (тесты меню и реестра).
pub(crate) fn registry() -> urfin_core::CommandRegistry {
    menu::copy_registry(core().registry())
}

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

fn core() -> Core {
    core_with(vec![FakeListing::dir("d"), FakeListing::file("f.txt", 10)])
}

fn core_with(entries: Vec<Entry>) -> Core {
    let root = Location::local(std::env::temp_dir().join("urfin-ui-test"));
    let fake = FakeListing::new(64);
    fake.set_dir(root.clone(), entries);
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
        [urfin_core::PromptId(7)]
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
    let mut ui = UrfinUi::new(
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
    let catalog = urfin_i18n::Catalog::embedded();
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

fn headless_frame(
    ctx: &egui::Context,
    ui: &mut UrfinUi,
    c: &Core,
    events: Vec<egui::Event>,
    out: &mut Vec<Command>,
) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 760.0),
        )),
        events,
        ..Default::default()
    };
    let mut full = ctx.run_ui(input, |root| ui.show(root, c.state(), &[], out));
    full.textures_delta.clear();
}

fn new_ui(c: &Core) -> UrfinUi {
    UrfinUi::new(
        c.registry(),
        &sys::SysProbe::default(),
        &UiCfg::default(),
        fonts::FontSet::default(),
    )
}

/// Деление колеса над левым списком — `panel.viewport top=3` (ручная проверка: колесо
/// не работало — сглаженная дельта округлялась в ноль).
#[test]
fn mouse_wheel_scrolls_by_three_rows() {
    let c = core();
    let mut ui = new_ui(&c);
    let ctx = egui::Context::default();
    let mut out = Vec::new();
    headless_frame(&ctx, &mut ui, &c, Vec::new(), &mut out);
    out.clear();
    let events = vec![
        egui::Event::PointerMoved(egui::pos2(300.0, 300.0)),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        },
    ];
    headless_frame(&ctx, &mut ui, &c, events, &mut out);
    let scroll: Vec<_> = out
        .iter()
        .filter(|c| c.id.as_str() == "panel.viewport" && c.args.int("top").is_some())
        .collect();
    assert_eq!(scroll.len(), 1, "{out:?}");
    assert_eq!(scroll[0].args.int("top"), Some(3));
    assert_eq!(scroll[0].args.str("side"), Some("left"));
}

/// Пересоздание атласа шрифтов (смена шрифтов, переполнение) — `RowCache` размечает
/// заново (ручная проверка: кракозябры при старте до разворота окна).
#[test]
fn font_atlas_recreate_resets_row_cache() {
    let c = core();
    let mut ui = new_ui(&c);
    let ctx = egui::Context::default();
    let mut out = Vec::new();
    for _ in 0..3 {
        headless_frame(&ctx, &mut ui, &c, Vec::new(), &mut out);
    }
    // Как прогрев при старте: атлас занят многими строками глифов.
    let mut full = ctx.run_ui(egui::RawInput::default(), |ui| {
        let text: String = ('!'..='~').chain('А'..='я').collect();
        for size in [12.0, 20.0, 32.0, 48.0] {
            let _ = ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(
                    text.clone(),
                    egui::FontId::proportional(size),
                    egui::Color32::WHITE,
                )
            });
        }
    });
    full.textures_delta.clear();
    headless_frame(&ctx, &mut ui, &c, Vec::new(), &mut out);
    let (steady, _) = ui.row_cache_stats();
    headless_frame(&ctx, &mut ui, &c, Vec::new(), &mut out);
    assert_eq!(
        ui.row_cache_stats().0.misses,
        steady.misses,
        "устойчивый кадр — попадания"
    );
    // Другие определения (те же egui пропускает): новый атлас.
    let mut defs = egui::FontDefinitions::default();
    if let Some(list) = defs.families.get_mut(&egui::FontFamily::Proportional) {
        list.reverse();
    }
    ctx.set_fonts(defs);
    for _ in 0..2 {
        headless_frame(&ctx, &mut ui, &c, Vec::new(), &mut out);
    }
    assert!(
        ui.row_cache_stats().0.misses > steady.misses,
        "после новых шрифтов ячейки размечены заново"
    );
}

/// Brief (S2-11): UI сообщает строки в колонке и колонки; ← → прыгает на колонку
/// (`rows`), `PgDn` — на страницу (`rows × cols`).
#[test]
fn brief_columns_drive_left_right_and_page_down() {
    let files = (0..300)
        .map(|i| FakeListing::file(&format!("f{i:03}.txt"), 10))
        .collect();
    let mut c = core_with(files);
    run(&mut c, "panel.view_mode mode=brief");
    let mut ui = new_ui(&c);
    let ctx = egui::Context::default();
    let mut out = Vec::new();
    headless_frame(&ctx, &mut ui, &c, Vec::new(), &mut out);
    let vp = out
        .iter()
        .find(|cmd| cmd.id.as_str() == "panel.viewport" && cmd.args.str("side") == Some("left"))
        .expect("panel.viewport for the left panel");
    let (rows, cols) = (vp.args.int("rows").unwrap(), vp.args.int("cols").unwrap());
    assert!(rows > 10 && cols >= 2, "rows {rows}, cols {cols}");
    for cmd in out.drain(..) {
        c.dispatch(cmd);
    }
    c.pump(Duration::from_millis(5));
    let row = |c: &Core| i64::from(c.state().panel(Side::Left).tab().cursor.row);
    let start = row(&c);
    run(&mut c, "panel.cursor.right");
    assert_eq!(row(&c), start + rows);
    run(&mut c, "panel.cursor.left");
    assert_eq!(row(&c), start);
    run(&mut c, "panel.cursor.page_down");
    assert_eq!(row(&c), start + rows * cols);
}

/// Pump, пока `done` не вернёт `true` (фоновые задачи `fs_tasks`), не дольше 3 с.
fn pump_until(core: &mut Core, mut done: impl FnMut(&mut Core) -> bool) -> bool {
    for _ in 0..300 {
        core.pump(Duration::from_millis(5));
        if done(core) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Ввод несуществующего пути (S2-12, ADR-0014): тост, путь панели прежний, поле открыто.
#[test]
fn path_input_rejects_missing_dir() {
    let mut c = core();
    let before = c.state().panel(Side::Left).tab().loc.clone();
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nope");
    run(&mut c, "ui.path_edit side=left");
    c.dispatch(
        Command::new("ui.path_edit")
            .arg("side", "left")
            .arg("path", missing.display().to_string()),
    );
    let mut toasts = Vec::new();
    assert!(pump_until(&mut c, |c| {
        toasts.extend(errors(c));
        !toasts.is_empty()
    }));
    assert!(toasts[0].contains("nope"), "{toasts:?}");
    assert_eq!(c.state().panel(Side::Left).tab().loc, before);
    assert_eq!(ui_state(&c).path_edit, Some(Side::Left));
    assert_eq!(c.state().focus, Focus::Overlay(OverlayKind::InlineEdit));
}

/// Существующий каталог — `panel.go`, поле закрыто; относительный путь — от каталога таба.
#[test]
fn path_input_goes_to_existing_dir() {
    let mut c = core();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    let dir = Location::local(tmp.path());
    run(&mut c, "ui.path_edit side=left");
    c.dispatch(
        Command::new("ui.path_edit")
            .arg("side", "left")
            .arg("path", tmp.path().display().to_string()),
    );
    assert!(pump_until(&mut c, |c| c
        .state()
        .panel(Side::Left)
        .tab()
        .loc
        == dir));
    assert_eq!(ui_state(&c).path_edit, None);
    assert_eq!(c.state().focus, Focus::Panel);
    c.dispatch(
        Command::new("ui.path_edit")
            .arg("side", "left")
            .arg("path", "sub"),
    );
    let sub = Location::local(tmp.path().join("sub"));
    assert!(pump_until(&mut c, |c| c
        .state()
        .panel(Side::Left)
        .tab()
        .loc
        == sub));
}

/// Меню `›`: только каталоги, натуральный порядок.
#[test]
fn subdirs_menu_lists_dirs_only() {
    let mut c = core();
    let tmp = tempfile::tempdir().unwrap();
    for d in ["b10", "a", "b9"] {
        std::fs::create_dir(tmp.path().join(d)).unwrap();
    }
    std::fs::write(tmp.path().join("file.txt"), b"x").unwrap();
    c.dispatch(
        Command::new("ui.menu")
            .arg("menu", "subdirs")
            .arg("side", "left")
            .arg("path", Location::local(tmp.path())),
    );
    assert!(pump_until(&mut c, |c| ui_state(c)
        .subdirs
        .as_ref()
        .is_some_and(|s| s.items.is_some())));
    let s = ui_state(&c).subdirs.clone().unwrap();
    let names: Vec<String> = s
        .items
        .unwrap()
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["a", "b9", "b10"]);
    assert_eq!(ui_state(&c).menu_side, Some(Side::Left));
    run(&mut c, "ui.cancel");
    assert_eq!(ui_state(&c).subdirs, None);
}

/// `docs/COMMANDS.md` совпадает с реестром команд (core + ui) и вшитым меню.
/// Обновить: `URFIN_UPDATE_DOCS=1 cargo nextest run -p urfin-ui commands_doc`.
#[test]
fn commands_doc_is_current() {
    let reg = registry();
    let catalog = urfin_i18n::Catalog::embedded();
    let en = catalog.build(&"en".parse().unwrap()).0;
    let ru = catalog.build(&"ru".parse().unwrap()).0;
    let (menus, _) = menu::parse(menu::MENU_TOML, &reg).unwrap();
    let doc = commands_doc::render(&reg, &en, &ru, &menus);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/COMMANDS.md");
    if std::env::var_os("URFIN_UPDATE_DOCS").is_some() {
        std::fs::write(&path, &doc).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        on_disk == doc,
        "docs/COMMANDS.md устарел: URFIN_UPDATE_DOCS=1 cargo nextest run -p urfin-ui commands_doc"
    );
}
