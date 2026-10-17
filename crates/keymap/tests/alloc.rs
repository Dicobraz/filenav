//! `Keymap::lookup` без кучи (S3 §5, S3-04): считающий аллокатор на потоке теста.

#![allow(unsafe_code, reason = "тестовый GlobalAlloc — обёртка над System")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use urfin_core::{
    AppState, CommandMeta, CommandRegistry, Focus, Location, Panel, Side, Tab, TabId,
};
use urfin_keymap::{ChordState, FlagRegistry, Keymap, Layer, Lookup, Os, Stroke};

struct Counting;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: делегирует System; счётчик — потоковый `Cell` без аллокаций.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ON.with(Cell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        // SAFETY: тот же контракт, что у вызывающего.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: тот же контракт, что у вызывающего.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ON.with(|o| o.set(true));
    f();
    ON.with(|o| o.set(false));
    COUNT.with(Cell::get)
}

static METAS: &[CommandMeta] = &[
    CommandMeta {
        id: "a.one",
        title: "one",
        category: "test",
        args: &[],
        repeatable: false,
    },
    CommandMeta {
        id: "a.two",
        title: "two",
        category: "test",
        args: &[],
        repeatable: false,
    },
];

#[test]
fn lookup_does_not_allocate() {
    let mut reg = CommandRegistry::new();
    reg.register(METAS).unwrap();
    let text = r#"format = 1
name = "p"
[context.panel]
"F5" = "a.one"
"Ctrl+K Ctrl+C" = "a.two"
[context."panel && !has_selection"]
"F6" = "a.two"
[context.global]
"F7" = "a.one"
"#;
    let (km, w) = Keymap::from_str_layers(
        &[(text, Layer::Preset)],
        &reg,
        &FlagRegistry::with_builtin(),
        Os::Windows,
    );
    assert!(w.is_empty(), "{w:?}");
    let loc = Location::local(if cfg!(windows) { r"C:\w" } else { "/w" });
    let mut st = AppState::new(
        Panel::new(Tab::new(TabId(1), loc.clone())),
        Panel::new(Tab::new(TabId(2), loc)),
    );
    st.focus = Focus::Panel;
    st.active = Side::Left;
    let s = |t: &str| Stroke::parse(t, Os::Windows).unwrap();
    let strokes = [s("F5"), s("F6"), s("F7"), s("Ctrl+K"), s("Ctrl+Z")];
    let mut pending = ChordState::default();
    pending.strokes.push(s("Ctrl+K"));
    let second = s("Ctrl+C");
    let mut hits = 0;
    let n = allocations(|| {
        for _ in 0..100 {
            for st_ in &strokes {
                if matches!(km.lookup(None, *st_, &st), Lookup::Hit(..)) {
                    hits += 1;
                }
            }
            if matches!(km.lookup(Some(&pending), second, &st), Lookup::Hit(..)) {
                hits += 1;
            }
        }
    });
    assert_eq!(n, 0, "lookup allocated {n} times");
    assert_eq!(hits, 400);
}
