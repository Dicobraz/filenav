use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use compact_str::CompactString;
use urfin_core::fake::FakeListing;
use urfin_core::{Boot, Command, Core, Entry, ListingStatus, Location, Services, Waker};

use super::*;
use crate::state::UiState;

// ---- кодировки и hex ----

#[test]
fn classify_text_and_binary() {
    let utf8 = classify("Привет\r\nмир".as_bytes(), false);
    assert!(matches!(
        &utf8,
        Preview::Text { encoding: Encoding::Utf8, crlf: true, text, .. } if text.starts_with("Привет")
    ));
    let bom = classify(&[0xEF, 0xBB, 0xBF, b'a'], false);
    assert!(matches!(
        bom,
        Preview::Text {
            encoding: Encoding::Utf8Bom,
            ..
        }
    ));
    let le: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("Да".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    assert!(matches!(
        classify(&le, false),
        Preview::Text { encoding: Encoding::Utf16Le, text, .. } if &*text == "Да"
    ));
    // «Привет» в 1251.
    let cp = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
    assert!(matches!(
        classify(&cp, false),
        Preview::Text { encoding: Encoding::Cp1251, text, .. } if &*text == "Привет"
    ));
    assert!(matches!(
        classify(b"MZ\x90\x00\x03", false),
        Preview::Hex(_)
    ));
    // Обрезано посреди символа — всё ещё UTF-8.
    let cut = &"Ёж".as_bytes()[..3];
    assert!(matches!(
        classify(cut, true),
        Preview::Text {
            encoding: Encoding::Utf8,
            ..
        }
    ));
}

#[test]
fn hex_rows() {
    // 6 байт из 16: недостающие — по три пробела, плюс разрыв посередине.
    let expected = format!("00000010  48 65 6C 6C 6F 00{}Hello.", " ".repeat(33));
    assert_eq!(crate::views_draw::hex_row(16, b"Hello\x00"), expected);
}

// ---- дерево ----

fn loc(p: &str) -> Location {
    Location::local(p)
}

fn names(t: &TreeState) -> Vec<String> {
    t.visible()
        .iter()
        .map(|&i| {
            format!(
                "{}{}",
                "  ".repeat(usize::from(t.nodes[i].depth)),
                t.nodes[i].name
            )
        })
        .collect()
}

/// Дети вставляются под родителя, раскрытая цепочка к курсору сохраняется.
#[test]
fn tree_merge_keeps_chain() {
    let base = if cfg!(windows) { r"C:\" } else { "/" };
    let a = format!("{base}a");
    let ab = format!("{base}a{}b", std::path::MAIN_SEPARATOR);
    let mut t = tree_for(&loc(&ab), 1);
    assert_eq!(t.cursor, loc(&ab));
    let root = t.nodes[0].loc.clone();
    merge_children(&mut t, &root, &["a".into(), "x".into()]);
    merge_children(&mut t, &loc(&a), &["b".into(), "c".into()]);
    let v = names(&t);
    assert_eq!(v[1..], ["  a", "    b", "    c", "  x"], "{v:?}");
    // Свернуть «a» — дети скрыты, но остаются.
    let i = t.index_of(&loc(&a)).unwrap();
    t.nodes[i].expanded = false;
    assert_eq!(names(&t)[1..], ["  a", "  x"]);
    assert_eq!(t.nodes.len(), 5);
    let empty: [CompactString; 0] = [];
    merge_children(&mut t, &loc(&ab), &empty);
    assert!(t.nodes[t.index_of(&loc(&ab)).unwrap()].empty);
}

// ---- интеграция: Core, настоящие файлы ----

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

/// Слева — каталог `dir` (листинг — фейк с теми же именами), справа — он же.
fn core_at(dir: &std::path::Path, entries: Vec<Entry>) -> Core {
    let root = Location::local(dir);
    let fake = FakeListing::new(1 << 12);
    fake.set_dir(root.clone(), entries);
    let mut core = Core::new(
        Boot::new(root.clone(), root),
        Services {
            listing: Arc::new(fake),
            ..Services::null()
        },
        Arc::new(NoWake),
    );
    crate::register(&mut core);
    pump_until(&mut core, |c| {
        c.state().active_tab().listing.status == ListingStatus::Complete
    });
    core
}

fn pump_until(core: &mut Core, f: impl Fn(&Core) -> bool) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        core.pump(Duration::from_millis(5));
        if f(core) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("condition not reached");
}

fn cmd(core: &mut Core, s: &str) {
    core.dispatch(Command::parse(s).unwrap());
    core.pump(Duration::from_millis(5));
}

fn ui(core: &Core) -> &UiState {
    core.state().ext.get::<UiState>().expect("UiState")
}

/// Быстрый просмотр: бинарный — hex, текст — текст; 1000 шагов курсора — одно чтение.
#[test]
fn preview_hex_text_and_one_read() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "hello\nworld").unwrap();
    std::fs::write(tmp.path().join("b.bin"), [0u8, 1, 2, 3, 0xFF]).unwrap();
    let mut entries = vec![
        FakeListing::file("a.txt", 11),
        FakeListing::file("b.bin", 5),
    ];
    entries.extend((0..1000).map(|i| FakeListing::file(&format!("f{i:04}.dat"), 1)));
    let mut c = core_at(tmp.path(), entries);
    cmd(&mut c, "panel.mode mode=qview side=right");
    cmd(&mut c, "panel.cursor.to name=a.txt");
    pump_until(&mut c, |c| {
        matches!(
            ui(c).preview[1].as_ref().and_then(|p| p.content.as_ref()),
            Some(Preview::Text { .. })
        )
    });
    cmd(&mut c, "panel.cursor.to name=b.bin");
    pump_until(&mut c, |c| {
        matches!(
            ui(c).preview[1].as_ref().and_then(|p| p.content.as_ref()),
            Some(Preview::Hex(_))
        )
    });
    // Быстрое движение: чтения отменяются до открытия файла.
    let before = PREVIEW_READS.load(Ordering::Relaxed);
    for _ in 0..1000 {
        c.dispatch(Command::new("panel.cursor.down"));
        c.pump(Duration::from_millis(1));
    }
    std::thread::sleep(PREVIEW_DEBOUNCE * 4);
    pump_until(&mut c, |c| {
        ui(c).preview[1]
            .as_ref()
            .is_some_and(|p| p.content.is_some())
    });
    let reads = PREVIEW_READS.load(Ordering::Relaxed) - before;
    assert!(reads <= 2, "{reads} reads for 1000 cursor moves");
}

/// «Инфо»: том каталога соседней панели читается в фоне.
#[test]
fn info_reads_volume() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = core_at(tmp.path(), vec![FakeListing::file("a.txt", 1)]);
    cmd(&mut c, "panel.mode mode=info side=right");
    pump_until(&mut c, |c| {
        ui(c).info[1].as_ref().is_some_and(|i| i.volume.is_some())
    });
    let v = ui(&c).info[1].as_ref().unwrap().volume.clone().unwrap();
    assert!(v.is_ok(), "{v:?}");
    // Режим выключен — данные убраны.
    cmd(&mut c, "panel.mode mode=info side=right");
    c.pump(Duration::from_millis(5));
    assert!(ui(&c).info[1].is_none());
}

/// Дерево: цепочка до каталога соседней панели, подкаталоги — лениво; курсор дерева
/// ведёт соседнюю панель через паузу, Enter — сразу.
#[test]
fn tree_follows_and_leads() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("sub/inner")).unwrap();
    std::fs::create_dir(tmp.path().join("zz")).unwrap();
    let mut c = core_at(
        tmp.path(),
        vec![FakeListing::dir("sub"), FakeListing::dir("zz")],
    );
    cmd(&mut c, "panel.mode mode=tree side=left");
    cmd(&mut c, "panel.focus side=left");
    let here = Location::local(tmp.path());
    pump_until(&mut c, |c| {
        ui(c).tree[0]
            .as_ref()
            .is_some_and(|t| t.index_of(&here).is_some_and(|i| t.nodes[i].loaded))
    });
    let t = ui(&c).tree[0].clone().unwrap();
    assert_eq!(t.cursor, here);
    let names: Vec<&str> = t.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"sub") && names.contains(&"zz"), "{names:?}");
    // Вниз — на «sub»; соседняя панель идёт туда после паузы.
    cmd(&mut c, "ui.tree.down");
    let sub = Location::local(tmp.path().join("sub"));
    assert_eq!(ui(&c).tree[0].as_ref().unwrap().cursor, sub);
    pump_until(&mut c, |c| c.state().panels[1].tab().loc == sub);
    // Раскрыть «sub» — подгрузится «inner»; Enter — сразу и с фокусом.
    cmd(&mut c, "ui.tree.expand");
    pump_until(&mut c, |c| {
        ui(c).tree[0]
            .as_ref()
            .is_some_and(|t| t.nodes.iter().any(|n| n.name == "inner"))
    });
    cmd(&mut c, "ui.tree.down");
    cmd(&mut c, "ui.tree.enter");
    let inner = Location::local(tmp.path().join("sub").join("inner"));
    pump_until(&mut c, |c| c.state().panels[1].tab().loc == inner);
    assert_eq!(c.state().active, urfin_core::Side::Right);
}
