//! Бюджеты шины (S1-07): `dispatch(panel.cursor.down)` < 1 мкс (настоящий handler core),
//! `pump` 10k пустых `Msg` < 2 мс.

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use filenav_core::{
    Boot, CmdError, Command, CommandMeta, Core, Cx, Handler, Location, RuntimeConfig, Services,
    Waker, msg,
};

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

const N_MSGS: u32 = 10_000;

static BENCH: [CommandMeta; 1] = [CommandMeta {
    id: "bench.fill",
    title: "Fill",
    category: "bench",
    args: &[],
    repeatable: false,
}];

/// `bench.fill` кладёт `N_MSGS` пустых `Msg`.
struct Bench;

impl Handler for Bench {
    fn namespace(&self) -> &'static str {
        "bench"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &BENCH
    }

    fn handle(&mut self, _cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let sink = cx.rt.sink();
        for _ in 0..N_MSGS {
            sink.try_send(msg("empty", |_| {}))
                .map_err(|_| CmdError::Rejected("full".into()))?;
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
    c.register(Box::new(Bench));
    // Стартовая загрузка NullListing: дождаться, чтобы не мешала замерам.
    std::thread::sleep(Duration::from_millis(50));
    c.pump(Duration::from_secs(1));
    c
}

fn bench(c: &mut Criterion) {
    let mut core = core();
    let down = Command::new("panel.cursor.down");
    c.bench_function("dispatch panel.cursor.down", |b| {
        b.iter(|| core.dispatch(black_box(down.clone())));
    });

    let fill = Command::new("bench.fill");
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
