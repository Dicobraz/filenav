//! Бюджет S3-04: `lookup` <300 нс.

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use urfin_core::{AppState, CommandMeta, CommandRegistry, Focus, Location, Panel, Tab, TabId};
use urfin_keymap::{ChordState, FlagRegistry, Keymap, Layer, Os, Stroke};

static METAS: &[CommandMeta] = &[CommandMeta {
    id: "a.one",
    title: "one",
    category: "bench",
    args: &[],
    repeatable: false,
}];

fn bench(c: &mut Criterion) {
    let mut reg = CommandRegistry::new();
    reg.register(METAS).unwrap();
    // Около 200 привязок в нескольких контекстах — порядок пресета Far.
    let mut text = String::from("format = 1\nname = \"p\"\n");
    for ctx in [
        "panel",
        "\"panel && !has_selection\"",
        "cmdline",
        "global",
        "viewer",
    ] {
        let _ = writeln!(text, "[context.{ctx}]");
        for key in ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10"] {
            for m in ["", "Ctrl+", "Shift+", "Alt+"] {
                let _ = writeln!(text, "\"{m}{key}\" = \"a.one\"");
            }
        }
        text.push_str("\"Ctrl+K Ctrl+C\" = \"a.one\"\n");
    }
    let (km, _) = Keymap::from_str_layers(
        &[(&text, Layer::Preset)],
        &reg,
        &FlagRegistry::with_builtin(),
        Os::Windows,
    );
    let loc = Location::local(if cfg!(windows) { r"C:\w" } else { "/w" });
    let mut st = AppState::new(
        Panel::new(Tab::new(TabId(1), loc.clone())),
        Panel::new(Tab::new(TabId(2), loc)),
    );
    st.focus = Focus::Panel;
    let s = |t: &str| Stroke::parse(t, Os::Windows).unwrap();
    let f5 = s("Ctrl+F5");
    let miss = s("Ctrl+Shift+Q");
    let mut pending = ChordState::default();
    pending.strokes.push(s("Ctrl+K"));
    let second = s("Ctrl+C");
    c.bench_function("lookup hit", |b| {
        b.iter(|| black_box(km.lookup(None, black_box(f5), &st)).is_some_hit());
    });
    c.bench_function("lookup miss", |b| {
        b.iter(|| black_box(km.lookup(None, black_box(miss), &st)).is_some_hit());
    });
    c.bench_function("lookup chord", |b| {
        b.iter(|| black_box(km.lookup(Some(&pending), black_box(second), &st)).is_some_hit());
    });
}

trait IsHit {
    fn is_some_hit(&self) -> bool;
}

impl IsHit for urfin_keymap::Lookup<'_> {
    fn is_some_hit(&self) -> bool {
        matches!(self, urfin_keymap::Lookup::Hit(..))
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
