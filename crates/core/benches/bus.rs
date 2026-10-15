//! Бюджеты шины (S1-07): `dispatch(panel.cursor.down)` < 1 мкс, `pump` 10k пустых `Msg` < 2 мс.

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use filenav_core::{
    ArgSpec, Boot, CmdError, Command, CommandMeta, Core, Cx, Handler, Location, RuntimeConfig,
    Services, ValueKind, Waker, msg,
};

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

const N_MSGS: u32 = 10_000;

static PANEL: [CommandMeta; 2] = [
    CommandMeta {
        id: "panel.cursor.down",
        title: "Cursor down",
        category: "panel",
        args: &[ArgSpec {
            name: "n",
            kind: ValueKind::Int,
            required: false,
        }],
        repeatable: true,
    },
    CommandMeta {
        id: "panel.fill",
        title: "Fill",
        category: "bench",
        args: &[],
        repeatable: false,
    },
];

/// `panel.cursor.down` двигает курсор; `panel.fill` кладёт `N_MSGS` пустых `Msg`.
struct Panel;

impl Handler for Panel {
    fn namespace(&self) -> &'static str {
        "panel"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &PANEL
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        if cmd.id.as_str() == "panel.fill" {
            let sink = cx.rt.sink();
            for _ in 0..N_MSGS {
                sink.try_send(msg("empty", |_| {}))
                    .map_err(|_| CmdError::Rejected("full".into()))?;
            }
        } else {
            let tab = cx.state.panel_mut(cx.state.active).tab_mut();
            tab.cursor.row = tab.cursor.row.wrapping_add(1);
        }
        cx.keep_rev();
        Ok(())
    }
}

fn core() -> Core {
    let mut boot = Boot::new(Location::local("/"), Location::local("/"));
    boot.runtime = RuntimeConfig {
        msg_capacity: N_MSGS * 2,
        ..RuntimeConfig::default()
    };
    let mut c = Core::new(boot, Services::null(), Arc::new(NoWake));
    c.register(Box::new(Panel));
    c
}

fn bench(c: &mut Criterion) {
    let mut core = core();
    let down = Command::new("panel.cursor.down");
    c.bench_function("dispatch panel.cursor.down", |b| {
        b.iter(|| core.dispatch(black_box(down.clone())));
    });

    let fill = Command::new("panel.fill");
    c.bench_function("pump 10k empty msgs", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                core.dispatch(fill.clone());
                let t0 = Instant::now();
                let p = core.pump(Duration::from_secs(10));
                total += t0.elapsed();
                assert!(!p.more);
            }
            total
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
