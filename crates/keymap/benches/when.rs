//! Бюджет S3-02: вычисление when-выражения <50 нс.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use urfin_core::{AppState, Focus, Location, Panel, Tab, TabId};
use urfin_keymap::{FlagRegistry, When};

fn state() -> AppState {
    let loc = Location::local(if cfg!(windows) { r"C:\work" } else { "/work" });
    let mut st = AppState::new(
        Panel::new(Tab::new(TabId(1), loc.clone())),
        Panel::new(Tab::new(TabId(2), loc)),
    );
    st.focus = Focus::Panel;
    st
}

fn bench(c: &mut Criterion) {
    let flags = FlagRegistry::with_builtin();
    let st = state();
    for expr in [
        "global",
        "panel && !qs",
        "(panel || cmdline) && !has_selection && !terminal",
    ] {
        let w = When::parse(expr, &flags).unwrap();
        c.bench_function(&format!("when {expr}"), |b| {
            b.iter(|| black_box(&w).matches(black_box(&st)));
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
