use std::sync::Arc;
use std::time::Duration;

use urfin_core::fake::FakeListing;
use urfin_core::{
    Boot, Command, Core, Entry, Focus, ListingStatus, Location, OverlayKind, Row, Services, Waker,
};

use super::{KeymapView, register};

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

pub(crate) fn core_with(entries: Vec<Entry>) -> Core {
    let root = Location::local(std::env::temp_dir().join("urfin-keymap-qs"));
    let fake = FakeListing::new(1 << 16);
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
    for _ in 0..200 {
        core.pump(Duration::from_millis(5));
        if core.state().active_tab().listing.status == ListingStatus::Complete {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    core
}

fn core() -> Core {
    core_with(
        [
            "alpha.txt",
            "beta.rs",
            "Bravo.toml",
            "beta2.txt",
            "gamma, delta.md",
        ]
        .iter()
        .map(|n| FakeListing::file(n, 1))
        .collect(),
    )
}

fn cmd(core: &mut Core, s: &str) {
    core.dispatch(Command::parse(s).unwrap());
}

fn cur(core: &Core) -> String {
    match core.state().active_tab().current() {
        Some(Row::Entry { entry, .. }) => entry.name.to_string(),
        Some(Row::Parent) => "..".into(),
        None => String::new(),
    }
}

fn view(core: &Core) -> KeymapView {
    core.state()
        .ext
        .get::<KeymapView>()
        .cloned()
        .unwrap_or_default()
}

fn mask(core: &Core) -> (String, u32) {
    let q = view(core).qs.expect("quick search is open");
    (q.mask.to_string(), q.matches)
}

#[test]
fn start_types_and_moves_cursor() {
    let mut c = core();
    cmd(&mut c, "keymap.qs.start text=b");
    assert_eq!(c.state().focus, Focus::Overlay(OverlayKind::QuickSearch));
    // Регистр не важен (Windows/macOS), курсор — на первом совпадении.
    let (m, n) = mask(&c);
    assert_eq!(m, "b");
    assert!(n >= 2, "{n}");
    assert!(cur(&c).to_lowercase().starts_with('b'), "{}", cur(&c));
    cmd(&mut c, "keymap.qs.input text=et");
    assert_eq!(mask(&c), ("bet".into(), 2));
    assert_eq!(cur(&c), "beta.rs");
}

/// Нет совпадения — символ не принят, курсор на месте, поле подсвечено.
#[test]
fn rejected_char_is_dropped() {
    let mut c = core();
    cmd(&mut c, "keymap.qs.start text=al");
    let before = cur(&c);
    cmd(&mut c, "keymap.qs.input text=z");
    assert_eq!(mask(&c).0, "al");
    assert_eq!(cur(&c), before);
    assert!(view(&c).qs.unwrap().miss_at.is_some());
    // Вставка: принимается, пока есть совпадения.
    cmd(&mut c, "keymap.qs.input text=phaX");
    assert_eq!(mask(&c).0, "alpha");
    // Нет совпадения и на старте — поиск открыт с пустой маской.
    cmd(&mut c, "keymap.qs.cancel");
    cmd(&mut c, "keymap.qs.start text=q");
    assert_eq!(mask(&c), (String::new(), 0));
    // Ни тоста, ни ошибки.
    assert!(
        !c.take_ui_events()
            .iter()
            .any(|e| matches!(e, urfin_core::UiEvent::Toast { .. }))
    );
}

#[test]
fn next_prev_backspace_cancel() {
    let mut c = core();
    cmd(&mut c, "keymap.qs.start text=b");
    let first = cur(&c);
    cmd(&mut c, "keymap.qs.next");
    let second = cur(&c);
    assert_ne!(first, second);
    cmd(&mut c, "keymap.qs.prev");
    assert_eq!(cur(&c), first);
    cmd(&mut c, "keymap.qs.backspace");
    assert_eq!(mask(&c), (String::new(), 0));
    cmd(&mut c, "keymap.qs.cancel");
    assert!(view(&c).qs.is_none());
    assert_eq!(c.state().focus, Focus::Panel);
}

/// Маски: `*`, `?`, `>ext` (DN), запятая в имени не делит маску.
#[test]
fn mask_syntax() {
    let mut c = core();
    cmd(&mut c, "keymap.qs.start text=*.t");
    assert_eq!(mask(&c).1, 3, "alpha.txt, Bravo.toml, beta2.txt");
    cmd(&mut c, "keymap.qs.cancel");
    cmd(&mut c, "keymap.qs.start text=b?ta");
    assert_eq!(mask(&c).1, 2);
    cmd(&mut c, "keymap.qs.cancel");
    cmd(&mut c, r#"keymap.qs.start text=">tx""#);
    assert_eq!(mask(&c).1, 2);
    cmd(&mut c, "keymap.qs.cancel");
    cmd(&mut c, r#"keymap.qs.start text="gamma,""#);
    assert_eq!(mask(&c), ("gamma,".into(), 1));
}

/// Фокус ушёл (меню, клик) — поиск закрывается.
#[test]
fn closes_when_focus_leaves() {
    let mut c = core();
    cmd(&mut c, "keymap.qs.start text=a");
    cmd(&mut c, "panel.focus");
    c.pump(Duration::from_millis(5));
    assert!(view(&c).qs.is_none());
}

#[test]
fn pending_chord() {
    let mut c = core();
    cmd(&mut c, "keymap.pending chord=Ctrl+K");
    assert_eq!(view(&c).pending.as_deref(), Some("Ctrl+K"));
    cmd(&mut c, "keymap.pending");
    assert_eq!(view(&c).pending, None);
}
