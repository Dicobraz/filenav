use std::time::Instant;

use egui::{Rect, pos2, vec2};
use urfin_core::{Attrs, Entry, EntryKind, Gen, Listing, ListingStatus, Location, Tab};

use super::*;
use crate::canvas::RecordCanvas;
use crate::sys::Zone;

/// 14.10.26 09:12 UTC.
const T0: i64 = 1_791_969_120 * 1_000_000_000;

fn entry(name: &str, size: u64, attrs: u32, kind: EntryKind, minutes: i64) -> Entry {
    Entry {
        name: name.into(),
        size,
        mtime: T0 - minutes * 60_000_000_000,
        attrs: Attrs(attrs),
        kind,
    }
}

/// Каталог мокапов: не локальный — сегменты breadcrumb одинаковы на всех ОС.
fn mock_dir(path: &str) -> Location {
    Location {
        backend: "test".into(),
        path: path.into(),
        outer: None,
    }
}

/// Левая панель мокапа §2.1: каталоги, затем файлы; курсор на README.md, помечены
/// Cargo.lock, Cargo.toml и deny.toml.
#[allow(
    clippy::many_single_char_names,
    reason = "d, f — каталог и файл мокапа"
)]
fn mockup_tab() -> Tab {
    let d = |n: &str, m| entry(n, 0, 0, EntryKind::Dir, m);
    let f = |n: &str, s, m| entry(n, s, Attrs::ARCHIVE, EntryKind::File, m);
    let entries = vec![
        d(".git", 2),
        d("crates", 17),
        d("docs", 631),
        d("i18n", 632),
        d("target", 1),
        entry(
            ".gitignore",
            118,
            Attrs::HIDDEN | Attrs::ARCHIVE,
            EntryKind::File,
            17_289,
        ),
        f("Cargo.lock", 48_213, 13),
        f("Cargo.toml", 1_229, 14),
        f("clippy.toml", 96, 17_289),
        f("deny.toml", 1_331, 15_952),
        f("LICENSE-APACHE", 10_854, 17_289),
        f("LICENSE-MIT", 1_023, 17_289),
        f("README.md", 3_891, 0),
        f("rust-toolchain.toml", 92, 17_289),
        f("rustfmt.toml", 41, 17_289),
        f("отчёт_по_производству.docx", 2_097_152, 2_312),
        entry(
            "setup.exe",
            14_260_000,
            Attrs::READONLY | Attrs::ARCHIVE,
            EntryKind::File,
            20_350,
        ),
        f("backup.7z", 1_024_000_000, 18_717),
    ];
    let n = u32::try_from(entries.len()).unwrap();
    let mut t = Tab::new(TabId(1), mock_dir("/work/urfin"));
    t.listing = Listing::with_entries(entries, (0..n).collect(), Gen(1));
    t.listing.status = ListingStatus::Complete;
    for name in ["Cargo.lock", "Cargo.toml", "deny.toml"] {
        let idx = t
            .listing
            .entries
            .iter()
            .position(|e| e.name == name)
            .unwrap();
        let e = t.listing.entries[idx].clone();
        t.selection.set(u32::try_from(idx).unwrap(), true, &e);
    }
    // Строка 0 — `..`; README.md — 13-я запись.
    t.cursor.row = 13;
    t
}

/// Правая панель мокапа §2.1 (Brief): `D: › backup`, 30 объектов, курсор на `2024`.
fn backup_tab() -> Tab {
    let d = |n: &str| entry(n, 0, 0, EntryKind::Dir, 60);
    let f = |n: &str, s| entry(n, s, Attrs::ARCHIVE, EntryKind::File, 60);
    let hidden = |n: &str, s| entry(n, s, Attrs::HIDDEN | Attrs::ARCHIVE, EntryKind::File, 60);
    let mut entries: Vec<Entry> = [
        "2024", "2025", "2026", "old", "photos", "projects", "scans", "vm",
    ]
    .into_iter()
    .map(d)
    .collect();
    entries.extend([
        f("archive.7z", 4_200_000_000),
        f("backup-01.zip", 310_000_000),
        f("backup-02.zip", 312_000_000),
        f("backup-03.zip", 315_000_000),
        f("backup-04.zip", 318_000_000),
        f("backup-05.zip", 320_000_000),
        f("db_dump.sql.gz", 88_000_000),
        hidden("desktop.ini", 282),
        f("urfin.log", 51_200),
        f("home.tar.zst", 2_600_000_000),
        f("keys.kdbx", 9_100),
        f("notes.txt", 2_048),
        f("passport.pdf", 1_400_000),
        f("photos-2025.7z", 7_900_000_000),
        f("readme.md", 640),
        f("restore.ps1", 3_300),
        f("snapshot.vhdx", 2_100_000_000),
        f("todo.md", 512),
        f("vacation.mp4", 1_800_000_000),
        f("win11.iso", 6_100_000_000),
        hidden("~$draft.docx", 162),
    ]);
    let n = u32::try_from(entries.len()).unwrap();
    let mut t = Tab::new(TabId(2), mock_dir("/backup"));
    t.listing = Listing::with_entries(entries, (0..n).collect(), Gen(1));
    t.listing.status = ListingStatus::Complete;
    t.view_mode = ViewMode::Brief;
    // Строка 0 — `..`, 1 — `2024`.
    t.cursor.row = 1;
    t
}

struct Fx {
    ctx: egui::Context,
    i18n: urfin_i18n::I18n,
    tokens: Tokens,
    fonts: FontSet,
    hl: Highlighter,
    fmt: FmtCfg,
    cache: RowCache,
    memo: CardMemo,
    widths: Widths,
    st: crate::state::UiState,
    keys: Option<urfin_keymap::KeymapView>,
}

impl Fx {
    fn new() -> Self {
        let i18n = urfin_i18n::Catalog::embedded()
            .build(&"ru".parse().unwrap())
            .0;
        let fmt = FmtCfg::new(&i18n, false, Zone::utc());
        Self {
            ctx: egui::Context::default(),
            i18n,
            tokens: Tokens::default(),
            fonts: FontSet::default(),
            hl: Highlighter::default(),
            fmt,
            cache: RowCache::new(),
            memo: CardMemo::default(),
            widths: Widths::default(),
            st: crate::state::UiState::new(),
            keys: None,
        }
    }

    /// Кадр карточки 600×560: запись Canvas (YAML) и команды.
    fn frame(&mut self, t: &Tab, active: bool, events: Vec<egui::Event>) -> (String, Vec<Command>) {
        let card = Rect::from_min_size(pos2(8.0, 40.0), vec2(600.0, 560.0));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 768.0))),
            events,
            ..Default::default()
        };
        let mut yaml = String::new();
        let mut out = Vec::new();
        let ctx = self.ctx.clone();
        let mut full = ctx.run_ui(input, |root| {
            let mut c = RecordCanvas::new(&self.i18n);
            let mut cx = ListCx {
                ui: root,
                i18n: &self.i18n,
                tokens: &self.tokens,
                fonts: &self.fonts,
                hl: &self.hl,
                fmt: &self.fmt,
                cache: &mut self.cache,
                memo: &mut self.memo,
                widths: &mut self.widths,
                st: &self.st,
                keys: self.keys.as_ref(),
                conflict: None,
            };
            show(&mut c, card, t, Side::Left, active, &mut cx, &mut out);
            yaml = c.to_yaml();
        });
        full.textures_delta.clear();
        self.cache.end_frame();
        (yaml, out)
    }
}

/// Левая панель §2.1 (Full): заголовки, строки, метки, размеры, даты, подвал.
#[test]
fn snapshot_full_mockup() {
    let mut fx = Fx::new();
    let t = mockup_tab();
    fx.frame(&t, true, Vec::new());
    let (yaml, _) = fx.frame(&t, true, Vec::new());
    insta::assert_snapshot!(yaml);
}

/// Wide §2.2: имя на всю ширину и размер, без даты.
#[test]
fn snapshot_wide() {
    let mut fx = Fx::new();
    let mut t = mockup_tab();
    t.view_mode = ViewMode::Wide;
    fx.frame(&t, false, Vec::new());
    let (yaml, _) = fx.frame(&t, false, Vec::new());
    assert!(!yaml.contains("Изменён"));
    insta::assert_snapshot!(yaml);
}

/// Brief §2.1 (правая панель): колонки только с именем, не уже 160 px, по столбцам;
/// заголовок «Имя» — один.
#[test]
fn snapshot_brief() {
    let mut fx = Fx::new();
    let t = backup_tab();
    fx.frame(&t, false, Vec::new());
    let (yaml, out) = fx.frame(&t, false, Vec::new());
    assert!(!yaml.contains("Размер") && !yaml.contains("Изменён"));
    assert_eq!(yaml.matches("Имя").count(), 1);
    assert_eq!(out.len(), 0, "viewport only on the first frame");
    insta::assert_snapshot!(yaml);
}

/// Medium §2.2: две колонки «имя + размер», заголовок над каждой, стрелка — у первой.
#[test]
fn snapshot_medium() {
    let mut fx = Fx::new();
    let mut t = mockup_tab();
    t.view_mode = ViewMode::Medium;
    fx.frame(&t, true, Vec::new());
    let (yaml, _) = fx.frame(&t, true, Vec::new());
    assert_eq!(yaml.matches("Размер").count(), 2);
    assert_eq!(yaml.matches("Имя ˄").count(), 1);
    assert!(!yaml.contains("Изменён"));
    insta::assert_snapshot!(yaml);
}

/// `panel.viewport`: строки в колонке и колонки вида (Brief — по ширине, Medium — 2).
#[test]
fn viewport_reports_columns() {
    let cols = |mode| {
        let mut fx = Fx::new();
        let mut t = mockup_tab();
        t.view_mode = mode;
        let (_, out) = fx.frame(&t, true, Vec::new());
        let vp = out
            .iter()
            .find(|c| c.id.as_str() == "panel.viewport")
            .expect("viewport");
        (vp.args.int("rows").unwrap(), vp.args.int("cols").unwrap())
    };
    let (rows, full) = cols(ViewMode::Full);
    assert_eq!(full, 1);
    // Карточка 600 px: в Brief — 3 колонки по ≥ 160 px.
    assert_eq!(cols(ViewMode::Brief), (rows, 3));
    assert_eq!(cols(ViewMode::Medium), (rows, 2));
    assert_eq!(cols(ViewMode::Wide), (rows, 1));
}

#[test]
fn viewport_only_on_change() {
    let mut fx = Fx::new();
    let t = mockup_tab();
    let (_, first) = fx.frame(&t, true, Vec::new());
    let (_, second) = fx.frame(&t, true, Vec::new());
    let vp = |o: &[Command]| {
        o.iter()
            .filter(|c| c.id.as_str() == "panel.viewport")
            .count()
    };
    assert_eq!(vp(&first), 1);
    assert_eq!(vp(&second), 0);
}

fn click(at: egui::Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: at,
            button,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

/// Первая строка списка (`..`) и заголовки — координаты карточки 600×560 у (8, 40).
const LIST_TOP: f32 = 40.0 + 4.0 + 32.0 + 28.0;

#[test]
fn parent_row_is_not_marked() {
    let mut fx = Fx::new();
    let t = mockup_tab();
    fx.frame(&t, true, Vec::new());
    let (_, out) = fx.frame(
        &t,
        true,
        click(pos2(200.0, LIST_TOP + 12.0), egui::PointerButton::Secondary),
    );
    let ids: Vec<_> = out.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["panel.cursor.home"], "ПКМ на `..` — только курсор");
    // ПКМ на файле — курсор и пометка.
    let (_, out) = fx.frame(
        &t,
        true,
        click(
            pos2(200.0, LIST_TOP + 24.0 * 7.0 + 12.0),
            egui::PointerButton::Secondary,
        ),
    );
    let ids: Vec<_> = out.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["panel.cursor.to", "sel.toggle"]);
}

#[test]
fn header_click_sorts_and_focuses() {
    let mut fx = Fx::new();
    let t = mockup_tab();
    fx.frame(&t, false, Vec::new());
    // «Размер» — колонка правее имени; клик в неактивной панели — фокус, затем сортировка.
    let cols = Columns::new(
        12.0,
        604.0,
        ViewMode::Full,
        ColWidths::default(),
        false,
        12.0,
    );
    let x = f32::midpoint(cols.size.0, cols.size.1);
    let (_, out) = fx.frame(
        &t,
        false,
        click(pos2(x, LIST_TOP - 14.0), egui::PointerButton::Primary),
    );
    let cmds: Vec<String> = out.iter().map(ToString::to_string).collect();
    assert_eq!(cmds, ["panel.focus side=left", "panel.sort key=size"]);
}

#[test]
fn column_divider_drags_on_4px_grid_and_resets() {
    let mut fx = Fx::new();
    let t = mockup_tab();
    fx.frame(&t, true, Vec::new());
    let cols = Columns::new(
        12.0,
        604.0,
        ViewMode::Full,
        ColWidths::default(),
        false,
        12.0,
    );
    let x = cols.size.0 - COL_GAP / 2.0;
    let y = LIST_TOP - 14.0;
    let press = |p: egui::Pos2, pressed| egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    fx.frame(
        &t,
        true,
        vec![
            egui::Event::PointerMoved(pos2(x, y)),
            press(pos2(x, y), true),
        ],
    );
    for dx in [-10.0, -20.0, -31.0] {
        fx.frame(&t, true, vec![egui::Event::PointerMoved(pos2(x + dx, y))]);
    }
    fx.frame(&t, true, vec![press(pos2(x - 31.0, y), false)]);
    let w = fx.widths[&(t.id, ViewMode::Full)];
    assert!(w.size > SIZE_W, "{w:?}");
    assert_eq!(w.size % 4.0, 0.0);
    // Двойной клик по разделителю (он сдвинулся) — ширины по умолчанию.
    let moved = Columns::new(12.0, 604.0, ViewMode::Full, w, false, 12.0);
    let hx = moved.size.0 - COL_GAP / 2.0;
    let mut dbl = click(pos2(hx, y), egui::PointerButton::Primary);
    dbl.extend(click(pos2(hx, y), egui::PointerButton::Primary));
    fx.frame(&t, true, dbl);
    fx.frame(&t, true, Vec::new());
    assert_eq!(fx.widths[&(t.id, ViewMode::Full)], ColWidths::default());
}

#[test]
fn empty_and_failed_states() {
    let mut fx = Fx::new();
    let mut t = mockup_tab();
    t.listing = Listing::with_entries(Vec::new(), Vec::new(), Gen(2));
    t.listing.status = ListingStatus::Complete;
    let (yaml, _) = fx.frame(&t, true, Vec::new());
    assert!(yaml.contains("Папка пуста"), "{yaml}");
    t.listing.status = ListingStatus::Failed("нет доступа".into());
    let (yaml, _) = fx.frame(&t, true, Vec::new());
    assert!(yaml.contains("нет доступа"), "{yaml}");
}

/// 100k строк, «удержание ↓»: на кадр — разметка только новых ячеек, кадр без
/// зависимости от размера каталога (debug-сборка — с запасом).
#[test]
fn hold_down_on_100k_is_incremental() {
    let mut fx = Fx::new();
    let n = 100_000u32;
    let entries: Vec<Entry> = (0..n)
        .map(|i| {
            entry(
                &format!("file_{i:06}.txt"),
                u64::from(i) * 977,
                Attrs::ARCHIVE,
                EntryKind::File,
                i64::from(i),
            )
        })
        .collect();
    let mut t = Tab::new(TabId(9), Location::local(std::env::temp_dir().join("big")));
    t.listing = Listing::with_entries(entries, (0..n).collect(), Gen(1));
    t.listing.status = ListingStatus::Complete;
    fx.frame(&t, true, Vec::new());
    let page = fx.memo.page;
    let mut worst = std::time::Duration::ZERO;
    for step in 0..120u32 {
        t.cursor.row = page + step;
        t.cursor.top = t.cursor.row + 1 - page;
        let before = fx.cache.stats.misses;
        let start = Instant::now();
        fx.frame(&t, true, Vec::new());
        worst = worst.max(start.elapsed());
        assert!(
            fx.cache.stats.misses - before <= 3,
            "новая строка: имя, размер, дата"
        );
    }
    assert!(worst.as_millis() < 50, "кадр {worst:?}");
}

/// Быстрый поиск (S3-06, ADR-0016): поле вместо подвала — маска и число совпадений;
/// на другой панели — обычный подвал. Начатый аккорд — в подвале активной панели.
#[test]
fn quick_search_footer() {
    let mut fx = Fx::new();
    let t = mockup_tab();
    fx.keys = Some(urfin_keymap::KeymapView {
        qs: Some(urfin_keymap::QuickSearch {
            side: Side::Left,
            mask: "doc".into(),
            matches: 3,
            miss_at: None,
        }),
        pending: None,
    });
    let (yaml, _) = fx.frame(&t, true, Vec::new());
    assert!(yaml.contains("doc"), "{yaml}");
    assert!(yaml.contains("Найдено: 3"), "{yaml}");
    assert!(yaml.contains("fill.control.input"), "{yaml}");
    // Не на этой панели — подвал как обычно.
    if let Some(q) = fx.keys.as_mut().and_then(|k| k.qs.as_mut()) {
        q.side = Side::Right;
    }
    let (yaml, _) = fx.frame(&t, true, Vec::new());
    assert!(!yaml.contains("Найдено"), "{yaml}");
    fx.keys = Some(urfin_keymap::KeymapView {
        qs: None,
        pending: Some("Ctrl+K".into()),
    });
    let (yaml, _) = fx.frame(&t, true, Vec::new());
    assert!(yaml.contains("Ctrl+K …"), "{yaml}");
    let (yaml, _) = fx.frame(&t, false, Vec::new());
    assert!(!yaml.contains("Ctrl+K"), "only on the active panel");
}
