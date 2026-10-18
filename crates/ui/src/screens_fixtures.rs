//! Экраны S2 §2 как готовые состояния (S2-21): одни и те же сценарии для
//! insta-снапшотов (`screens_tests`) и для скриншотов настоящего окна
//! (`urfin --screens <dir>`). Данные фиктивные и детерминированные: каталоги,
//! задачи, вопросы, режимы панелей.
//!
//! §2.3 (Custom columns) — M2+, §2.11 (терминал по Ctrl+O) — S6: экранов ещё нет.
//! Keybar (§2.12) есть в каждом экране.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use smallvec::SmallVec;
use urfin_core::{
    Anchor, AppState, Attrs, Choice, Entry, EntryKind, Gen, JobSpec, Lane, Listing, ListingStatus,
    Location, Panel, PanelMode, PromptDetail, PromptSpec, Runtime, RuntimeConfig, Side, Tab, TabId,
    UiEvent, ViewMode, Waker,
};
use urfin_vfs::Space;
use urfin_vfs::volume::{Media, VolCaps};

use crate::views::{
    Encoding, InfoState, Preview, PreviewKey, PreviewState, VolumeView, merge_children, tree_for,
};
use crate::{ThemeMode, UiState};

/// Готовый экран.
pub struct Screen {
    /// `2_1_main` и т.п. — имя снапшота и файла.
    pub name: &'static str,
    pub state: AppState,
    /// События первого кадра (`ops.draft` и т.п.).
    pub events: Vec<UiEvent>,
    /// Что обязано быть на экране (проверка снапшота).
    pub needles: &'static [&'static str],
    /// Рантайм задач экрана (их `MsgSink`) — живёт вместе с экраном.
    _rt: Option<Runtime>,
}

impl std::fmt::Debug for Screen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Screen")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Screen {
    /// Тема экрана: светлая или тёмная (как команда `ui.theme`).
    pub fn set_theme(&mut self, dark: bool) {
        ui_mut(&mut self.state).theme = Some(if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        });
    }
}

struct NoWake;
impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

/// 14.10.26 09:12 UTC.
const T0: i64 = 1_791_969_120 * 1_000_000_000;
const DAY: i64 = 86_400 * 1_000_000_000;

fn loc(path: &str) -> Location {
    Location {
        backend: "test".into(),
        path: path.into(),
        outer: None,
    }
}

fn e(name: &str, size: u64, kind: EntryKind, age_days: i64, attrs: u32) -> Entry {
    Entry {
        name: name.into(),
        size,
        mtime: T0 - age_days * DAY,
        attrs: Attrs(Attrs::ARCHIVE | attrs),
        kind,
    }
}

/// Каталог проекта: каталоги, исходники, исполняемый, архив, скрытый.
fn project() -> Vec<Entry> {
    use EntryKind::{Dir, File};
    vec![
        e("crates", 0, Dir, 1, 0),
        e("docs", 0, Dir, 2, 0),
        e("target", 0, Dir, 0, 0),
        e(".git", 0, Dir, 0, Attrs::HIDDEN | Attrs::DOTFILE),
        e("Cargo.lock", 98_304, File, 0, 0),
        e("Cargo.toml", 1_229, File, 3, 0),
        e("CLAUDE.md", 4_302, File, 1, 0),
        e("deny.toml", 2_048, File, 30, 0),
        e("LICENSE", 16_725, File, 120, 0),
        e("README.md", 3_891, File, 5, 0),
        e("rust-toolchain.toml", 96, File, 60, 0),
        e("urfin.exe", 14_878_208, File, 0, 0),
        e("urfin-0.1.0.zip", 6_291_456, File, 2, 0),
    ]
}

fn backup() -> Vec<Entry> {
    use EntryKind::{Dir, File};
    let mut v = vec![e("2026-09", 0, Dir, 30, 0), e("2026-10", 0, Dir, 4, 0)];
    for i in 1..=24i64 {
        v.push(e(
            &format!("photo-{i:03}.jpg"),
            2_400_000 + i.unsigned_abs() * 7_919,
            File,
            i,
            0,
        ));
    }
    v
}

fn tab(id: u32, path: &str, entries: Vec<Entry>, mode: ViewMode) -> Tab {
    let mut t = Tab::new(TabId(id), loc(path));
    let n = entries.len() as u32;
    t.listing = Listing::with_entries(entries, (0..n).collect(), Gen(1));
    t.listing.status = ListingStatus::Complete;
    t.view_mode = mode;
    t
}

/// Главный экран: слева — проект (Full), справа — бэкап (Brief); два файла
/// выделены, курсор на `README.md`.
fn state(left: ViewMode, right: ViewMode) -> AppState {
    let mut l = tab(1, "C:\\work\\urfin", project(), left);
    for i in [5u32, 9] {
        let entries = Arc::clone(&l.listing.entries);
        l.selection.set(i, true, &entries[i as usize]);
    }
    l.cursor.row = 10;
    let r = tab(2, "D:\\backup\\photos", backup(), right);
    let mut s = AppState::new(Panel::new(l), Panel::new(r));
    s.active = Side::Left;
    s.ext.insert(UiState::new());
    s
}

fn ui_mut(s: &mut AppState) -> &mut UiState {
    s.ext.get_mut::<UiState>().expect("UiState")
}

fn choice(id: &str, label: &str, key: char) -> Choice {
    Choice {
        id: id.into(),
        label: label.into(),
        key,
    }
}

/// Рантайм ради `MsgSink` задач; живёт до конца теста.
fn runtime() -> Runtime {
    Runtime::new(&RuntimeConfig::default(), Arc::new(NoWake)).0
}

fn job(s: &mut AppState, rt: &Runtime, title: &str, done: u64, total: u64) -> urfin_core::JobId {
    let h = s.jobs.start(
        JobSpec {
            kind: "copy",
            title: title.into(),
            lane: Lane::Bulk,
            priority: 0,
            owner_ns: "ops",
        },
        rt.sink(),
    );
    let p = h.progress();
    p.bytes_done
        .store(done, std::sync::atomic::Ordering::Relaxed);
    p.bytes_total
        .store(total, std::sync::atomic::Ordering::Relaxed);
    p.items_done.store(12, std::sync::atomic::Ordering::Relaxed);
    p.items_total
        .store(37, std::sync::atomic::Ordering::Relaxed);
    p.total_final
        .store(true, std::sync::atomic::Ordering::Relaxed);
    p.set_phase(urfin_core::Phase::Transfer);
    let id = h.id();
    std::mem::forget(h);
    id
}

fn main(v: &mut Vec<Screen>) {
    let s = state(ViewMode::Full, ViewMode::Brief);
    v.push(Screen {
        name: "2_1_main",
        state: s,
        events: Vec::new(),
        needles: &["README.md", "photo-001.jpg", "Cargo.toml"],
        _rt: None,
    });
}

fn medium_wide(v: &mut Vec<Screen>) {
    let s = state(ViewMode::Medium, ViewMode::Wide);
    v.push(Screen {
        name: "2_2_medium_wide",
        state: s,
        events: Vec::new(),
        needles: &["README.md", "photo-001.jpg"],
        _rt: None,
    });
}

fn tree_info(v: &mut Vec<Screen>) {
    let mut s = state(ViewMode::Full, ViewMode::Brief);
    // Активная — правая: дерево слева показывает её каталог, «Сведения» — левую.
    let mut left = tab(1, "C:\\work\\urfin", project(), ViewMode::Full);
    left.mode = PanelMode::Tree;
    s.panels[0] = Panel::new(left);
    let mut right = tab(2, "C:\\work\\urfin", project(), ViewMode::Full);
    right.mode = PanelMode::Info;
    right.cursor.row = 10;
    s.panels[1] = Panel::new(right);
    s.active = Side::Left;
    let here = Location::local(if cfg!(windows) {
        r"C:\work\urfin"
    } else {
        "/work/urfin"
    });
    let mut tree = tree_for(&here, 2);
    let root = tree.nodes[0].loc.clone();
    merge_children(
        &mut tree,
        &root,
        &["Program Files".into(), "Users".into(), "work".into()],
    );
    let work = here.parent().expect("parent");
    merge_children(&mut tree, &work, &["notes".into(), "urfin".into()]);
    merge_children(
        &mut tree,
        &here,
        &["crates".into(), "docs".into(), "target".into()],
    );
    let st = ui_mut(&mut s);
    st.tree[0] = Some(tree);
    st.info[1] = Some(InfoState {
        loc: loc("C:\\work\\urfin"),
        volume: Some(Ok(VolumeView {
            root: PathBuf::from("C:\\"),
            label: "Локальный диск".into(),
            fs_name: "NTFS".into(),
            media: Media::Nvme,
            caps: VolCaps(VolCaps::TRASH | VolCaps::ADS | VolCaps::COMPRESSION),
            space: Some(Space {
                total: 952 << 30,
                free: 412 << 30,
                avail: 412 << 30,
            }),
        })),
    });
    v.push(Screen {
        name: "2_4_tree_info",
        state: s,
        events: Vec::new(),
        needles: &["Program Files", "Сведения", "NTFS · NVMe SSD"],
        _rt: None,
    });
}

fn quick_view(v: &mut Vec<Screen>) {
    let mut s = state(ViewMode::Full, ViewMode::Brief);
    s.panels[1].tabs[0].mode = PanelMode::QView;
    ui_mut(&mut s).preview[1] = Some(PreviewState {
        key: Some(PreviewKey {
            tab: TabId(1),
            generation: Gen(1),
            name: "README.md".into(),
        }),
        size: 3_891,
        content: Some(Preview::Text {
            text: "# Urfin\n\nБыстрый двухпанельный файловый менеджер.\n\n## Сборка\n\n\tcargo build --release\n".into(),
            encoding: Encoding::Utf8,
            crlf: false,
            truncated: false,
        }),
    });
    v.push(Screen {
        name: "2_5_quick_view",
        state: s,
        events: Vec::new(),
        needles: &["UTF-8 · LF · 3,8 КБ", "Быстрый двухпанельный"],
        _rt: None,
    });
}

fn menu(v: &mut Vec<Screen>) {
    let mut s = state(ViewMode::Full, ViewMode::Brief);
    let st = ui_mut(&mut s);
    st.menu = Some("files".into());
    st.overlay = Some(urfin_core::OverlayKind::Menu);
    s.focus = urfin_core::Focus::Overlay(urfin_core::OverlayKind::Menu);
    v.push(Screen {
        name: "2_6_menu",
        state: s,
        events: Vec::new(),
        needles: &["Файл"],
        _rt: None,
    });
}

fn delete(v: &mut Vec<Screen>) {
    let mut s = state(ViewMode::Full, ViewMode::Brief);
    ui_mut(&mut s).draft = Some(crate::Draft {
        cmd: "ops.delete".into(),
        arg: "confirmed".into(),
        text: compact_str::CompactString::default(),
    });
    let c = urfin_core::Command::new("ops.draft").arg("command", "ops.delete");
    let ev = [UiEvent::Ext {
        id: c.id,
        args: c.args,
    }];
    v.push(Screen {
        name: "2_7_delete",
        state: s,
        events: ev.to_vec(),
        needles: &["В корзину"],
        _rt: None,
    });
}

fn conflict(v: &mut Vec<Screen>) {
    let rt = runtime();
    let mut s = state(ViewMode::Full, ViewMode::Brief);
    let id = job(
        &mut s,
        &rt,
        "Копирование в D:\\backup",
        600 << 20,
        1_400 << 20,
    );
    let _ = s.prompts.open(
        PromptSpec {
            anchor: Anchor::JobBar,
            title: "Копирование #1".into(),
            body: String::new(),
            choices: SmallVec::from_vec(vec![
                choice("overwrite", "Заменить", 'r'),
                choice("skip", "Пропустить", 's'),
                choice("rename_new", "Переименовать", 'n'),
                choice("abort", "Прервать задачу", 'q'),
            ]),
            default: 0,
            input: None,
            allow_all: true,
            detail: Some(PromptDetail::Conflict {
                src: e("report-q3.xlsx", 2_097_152, EntryKind::File, 0, 0),
                dst: e("report-q3.xlsx", 11_980, EntryKind::File, 1, 0),
                index: 3,
                total: 37,
            }),
            secret: false,
        },
        Some(id),
    );
    v.push(Screen {
        name: "2_8_conflict",
        state: s,
        events: Vec::new(),
        needles: &["report-q3.xlsx", "Заменить"],
        _rt: Some(rt),
    });
}

fn two_jobs(v: &mut Vec<Screen>) {
    let rt = runtime();
    let mut s = state(ViewMode::Full, ViewMode::Brief);
    job(
        &mut s,
        &rt,
        "Копирование в D:\\backup",
        600 << 20,
        1_400 << 20,
    );
    job(
        &mut s,
        &rt,
        "Перемещение в E:\\archive",
        90 << 20,
        300 << 20,
    );
    v.push(Screen {
        name: "2_9_two_jobs",
        state: s,
        events: Vec::new(),
        needles: &["#1", "#2"],
        _rt: Some(rt),
    });
}

fn search_and_rename(v: &mut Vec<Screen>) {
    let mut s = state(ViewMode::Full, ViewMode::Brief);
    s.ext.insert(urfin_keymap::KeymapView {
        qs: Some(urfin_keymap::QuickSearch {
            side: Side::Left,
            mask: "rea".into(),
            matches: 1,
            miss_at: None,
        }),
        pending: None,
    });
    v.push(Screen {
        name: "2_10_quick_search",
        state: s,
        events: Vec::new(),
        needles: &["rea"],
        _rt: None,
    });

    let mut s = state(ViewMode::Full, ViewMode::Brief);
    let st = ui_mut(&mut s);
    st.rename = Some((TabId(1), "README.md".into()));
    v.push(Screen {
        name: "2_10_rename",
        state: s,
        events: Vec::new(),
        needles: &["Enter — переименовать"],
        _rt: None,
    });
}

/// Все экраны S2 §2, по порядку разделов.
pub fn all() -> Vec<Screen> {
    let mut v = Vec::new();
    main(&mut v);
    medium_wide(&mut v);
    tree_info(&mut v);
    quick_view(&mut v);
    menu(&mut v);
    delete(&mut v);
    conflict(&mut v);
    two_jobs(&mut v);
    search_and_rename(&mut v);
    v
}
