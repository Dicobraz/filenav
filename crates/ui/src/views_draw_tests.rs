use std::path::PathBuf;

use egui::{Rect, pos2, vec2};
use urfin_core::{
    AppState, Attrs, Entry, EntryKind, Gen, Listing, ListingStatus, Location, Panel, PanelMode,
    Tab, TabId,
};
use urfin_vfs::Space;
use urfin_vfs::volume::{Media, VolCaps};

use super::*;
use crate::canvas::RecordCanvas;
use crate::sys::Zone;
use crate::views::{
    InfoState, Preview, PreviewKey, PreviewState, VolumeView, merge_children, tree_for,
};

/// 14.10.26 09:12 UTC.
const T0: i64 = 1_791_969_120 * 1_000_000_000;

fn mock_dir(path: &str) -> Location {
    Location {
        backend: "test".into(),
        path: path.into(),
        outer: None,
    }
}

fn entry(name: &str, size: u64, kind: EntryKind) -> Entry {
    Entry {
        name: name.into(),
        size,
        mtime: T0,
        attrs: Attrs(Attrs::ARCHIVE),
        kind,
    }
}

/// Слева — список `/work/urfin` (курсор на `README.md`), справа — режим `mode`.
fn state(mode: PanelMode) -> AppState {
    let entries = vec![
        entry("crates", 0, EntryKind::Dir),
        entry("docs", 0, EntryKind::Dir),
        entry("Cargo.toml", 1_229, EntryKind::File),
        entry("README.md", 3_891, EntryKind::File),
    ];
    let mut left = Tab::new(TabId(1), mock_dir("/work/urfin"));
    left.listing = Listing::with_entries(entries, (0..4).collect(), Gen(1));
    left.listing.status = ListingStatus::Complete;
    left.listing.stats.dirs = 2;
    left.listing.stats.files = 2;
    left.listing.stats.bytes = 5_120;
    left.cursor.row = 4;
    let mut right = Tab::new(TabId(2), mock_dir("/backup"));
    right.mode = mode;
    let mut s = AppState::new(Panel::new(left), Panel::new(right));
    s.active = urfin_core::Side::Left;
    s
}

fn frame(s: &AppState, st: &UiState) -> String {
    let i18n = urfin_i18n::Catalog::embedded()
        .build(&"ru".parse().unwrap())
        .0;
    let tokens = Tokens::default();
    let fmt = FmtCfg::new(&i18n, false, Zone::utc());
    let ctx = egui::Context::default();
    let mut yaml = String::new();
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 768.0))),
        ..Default::default()
    };
    let mut full = ctx.run_ui(input, |root| {
        let mut c = RecordCanvas::new(&i18n);
        let mut out = Vec::new();
        show(
            &mut c,
            &mut ViewCx {
                ui: root,
                i18n: &i18n,
                tokens: &tokens,
                fmt: &fmt,
            },
            Rect::from_min_size(pos2(640.0, 40.0), vec2(600.0, 560.0)),
            s,
            st,
            urfin_core::Side::Right,
            false,
            &mut out,
        );
        yaml = c.to_yaml();
    });
    full.textures_delta.clear();
    yaml
}

/// §2.4 справа: «Сведения» — том, папка, под курсором (точные байты).
#[test]
fn snapshot_info() {
    let s = state(PanelMode::Info);
    let mut st = UiState::new();
    st.info[1] = Some(InfoState {
        loc: mock_dir("/work/urfin"),
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
    let yaml = frame(&s, &st);
    for needle in [
        "Сведения",
        "NTFS · NVMe SSD",
        "корзина, ADS, сжатие",
        "412 ГБ из 952 ГБ (43 %)",
        "папок: 2 · файлов: 2",
        "README.md",
        // Группировка — неразрывный пробел (в YAML — `\u{a0}`).
        r"3\u{a0}891 байт",
    ] {
        assert!(yaml.contains(needle), "{needle}: {yaml}");
    }
    insta::assert_snapshot!(yaml);
}

/// §2.5: просмотр текста — шапка с кодировкой и размером, строки моноширинным.
#[test]
fn snapshot_quick_view() {
    let s = state(PanelMode::QView);
    let mut st = UiState::new();
    st.preview[1] = Some(PreviewState {
        key: Some(PreviewKey {
            tab: TabId(1),
            generation: Gen(1),
            name: "README.md".into(),
        }),
        size: 3_891,
        content: Some(Preview::Text {
            text: "# Urfin\n\nБыстрый файловый менеджер.\n\tотступ".into(),
            encoding: crate::views::Encoding::Utf8,
            crlf: false,
            truncated: false,
        }),
    });
    let yaml = frame(&s, &st);
    assert!(yaml.contains("UTF-8 · LF · 3,8 КБ"), "{yaml}");
    assert!(yaml.contains("    отступ"), "tabs → spaces: {yaml}");
    insta::assert_snapshot!(yaml);
}

/// §2.4 слева (здесь — справа): дерево с раскрытой цепочкой и курсором.
#[test]
fn snapshot_tree() {
    let s = state(PanelMode::Tree);
    let base = if cfg!(windows) { r"C:\" } else { "/" };
    let here = Location::local(format!("{base}work"));
    let mut tree = tree_for(&here, 1);
    let root = tree.nodes[0].loc.clone();
    merge_children(
        &mut tree,
        &root,
        &["Program Files".into(), "Users".into(), "work".into()],
    );
    merge_children(&mut tree, &here, &["dumps".into(), "urfin".into()]);
    let mut st = UiState::new();
    st.tree[1] = Some(tree);
    let yaml = frame(&s, &st);
    assert!(yaml.contains("Дерево · "), "{yaml}");
    assert!(yaml.contains("fill.cursor.passive"), "{yaml}");
    for n in ["Program Files", "Users", "work", "dumps", "urfin"] {
        assert!(yaml.contains(n), "{n}: {yaml}");
    }
}
